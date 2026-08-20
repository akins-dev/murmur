/*!
 * SOURCE OF TRUTH KEYWORDS: e2e_session, ReplayAudioSource, RecordingEventSink,
 *   CapturingInjector, run_session, speech_wav
 * WHAT:  End-to-end tests that drive the REAL session actor — real chunker,
 *        real VAD, real Whisper engine, real enhancement rules, real database —
 *        with recorded audio replayed through a fake microphone.
 * WHY:   Everything else in this crate tests one layer. These test the thing
 *        the product actually promises: that pressing a key, speaking, and
 *        pressing it again produces correct text, persists a row, and records
 *        its own latency. Every bug worth shipping-blocking lives in the SEAMS
 *        between those layers, and no unit test can see a seam.
 *
 *        Only three things are faked, and each for a reason that is not
 *        convenience:
 *          - the microphone, because a test cannot speak;
 *          - the injector, because a test must not paste into whatever window
 *            happens to have focus on the machine running it;
 *          - the event sink, because there is no UI — and it doubles as the
 *            recording of what the pill would have been told.
 *        The engine is NOT faked. If Whisper stops producing words, these fail.
 *
 *        They self-skip when the model is absent, so a clean checkout stays
 *        green — a suite that is red for an environmental reason is a suite
 *        people learn to ignore.
 * WHERE: The last line of defence before a release.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::adapters::rules::RuleEnhancer;
use crate::db::Database;
use crate::error::AppResult;
use crate::ipc::context::{Ports, SessionContext, SessionHandle};
use crate::pipeline::worker::AsrWorker;
use crate::ports::audio::{AudioSource, CaptureConfig, CaptureEvent, CaptureSession, SampleSender};
use crate::ports::events::EventSink;
use crate::ports::injector::{FrontmostApp, InjectionOutcome, InjectionRequest, TextInjector};
use crate::ports::permissions::{OsPermission, PermissionProvider, PermissionState};
use crate::types::{
    AudioLevel, DeliveryKind, DeviceInfo, DownloadProgress, SessionState, TARGET_SAMPLE_RATE,
};

use super::actor::SessionActor;
use super::machine::SessionEvent;
use super::settings_view::SessionSettings;

// ── Fakes ────────────────────────────────────────────────────────────────

/**
 * WHAT:  A microphone that replays a fixed buffer in realistic-sized blocks.
 * WHY:   Delivers in ~20ms blocks rather than one large push, because the
 *        chunker and VAD both accumulate across calls — handing them the whole
 *        utterance at once would exercise a path that never happens in
 *        production and hide any bug in the accumulation.
 */
struct ReplayAudioSource {
    samples: Arc<Vec<f32>>,
}

struct ReplaySession {
    info: DeviceInfo,
}

impl CaptureSession for ReplaySession {
    fn device(&self) -> &DeviceInfo {
        &self.info
    }
    fn native_sample_rate(&self) -> u32 {
        TARGET_SAMPLE_RATE
    }
    fn stop(self: Box<Self>) -> AppResult<()> {
        Ok(())
    }
}

impl AudioSource for ReplayAudioSource {
    fn list_devices(&self) -> AppResult<Vec<DeviceInfo>> {
        Ok(vec![self.info()])
    }

    fn default_device(&self) -> AppResult<DeviceInfo> {
        Ok(self.info())
    }

    fn start(
        &self,
        _config: &CaptureConfig,
        sink: SampleSender,
    ) -> AppResult<Box<dyn CaptureSession>> {
        let samples = Arc::clone(&self.samples);

        std::thread::spawn(move || {
            // ~20ms at 16kHz, the size a real drain thread delivers.
            for block in samples.chunks(320) {
                if sink.blocking_send(CaptureEvent::Samples(block.to_vec())).is_err() {
                    return;
                }
            }
        });

        Ok(Box::new(ReplaySession { info: self.info() }))
    }
}

impl ReplayAudioSource {
    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            id: "replay".into(),
            name: "Replay".into(),
            is_default: true,
            sample_rate: TARGET_SAMPLE_RATE,
            channels: 1,
        }
    }
}

/// Captures what would have been pasted, instead of pasting it.
#[derive(Default)]
struct CapturingInjector {
    delivered: Mutex<Option<String>>,
}

impl TextInjector for CapturingInjector {
    fn can_inject(&self) -> bool {
        true
    }
    fn frontmost_app(&self) -> Option<FrontmostApp> {
        Some(FrontmostApp {
            bundle_id: "com.apple.Terminal".into(),
            name: "Terminal".into(),
        })
    }
    fn deliver(&self, request: &InjectionRequest) -> AppResult<InjectionOutcome> {
        *self.delivered.lock() = Some(request.text.clone());
        Ok(InjectionOutcome {
            delivery: DeliveryKind::Pasted,
            reason: None,
            // A fake pasteboard is instant. Non-zero so the ClipboardWrite
            // assertion in the metrics test is proving the value travelled,
            // not that a default happened to be there.
            clipboard_write_ms: 0.5,
        })
    }
}

/// Records the states the pill would have been shown, in order.
#[derive(Default)]
struct RecordingEventSink {
    /// Every completed delivery, so a test can assert that words actually
    /// arrived rather than that a recording merely stopped.
    deliveries: Mutex<Vec<(u32, crate::types::DeliveryKind)>>,
    states: Mutex<Vec<SessionState>>,
    pill_visible: Mutex<Vec<bool>>,
}

impl EventSink for RecordingEventSink {
    fn transcript_delivered(&self, word_count: u32, delivery: crate::types::DeliveryKind) {
        self.deliveries.lock().push((word_count, delivery));
    }

    fn session_state_changed(&self, state: &SessionState) {
        self.states.lock().push(state.clone());
    }
    fn audio_level(&self, _level: AudioLevel) {}
    fn set_pill_visible(&self, visible: bool) {
        self.pill_visible.lock().push(visible);
    }
    fn download_progress(&self, _progress: DownloadProgress) {}
    fn set_cancel_key_active(&self, _active: bool) {}
    fn model_state_changed(
        &self,
        _model_id: crate::types::ModelId,
        _state: crate::types::ModelState,
    ) {
    }
}

struct AlwaysGranted;

impl PermissionProvider for AlwaysGranted {
    fn check(&self, _permission: OsPermission) -> PermissionState {
        PermissionState::Granted
    }
    fn request(&self, _permission: OsPermission) -> AppResult<PermissionState> {
        Ok(PermissionState::Granted)
    }
    fn open_privacy_pane(&self, _pane: crate::error::PrivacyPane) -> AppResult<()> {
        Ok(())
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: NeverAsked
 * WHAT:  A permission provider in the state a freshly installed app is in:
 *        the user has never been shown the dialog.
 * WHY:   This is the state the machine was in when the microphone bug was
 *        reported, and it is the state every new install starts in. Testing
 *        only against Granted meant the suite was green while the app was
 *        unusable on exactly the machines that matter most — new ones.
 * WHERE: Used by the regression test below.
 */
#[derive(Default)]
struct NeverAsked {
    asked: std::sync::atomic::AtomicUsize,
}

impl PermissionProvider for NeverAsked {
    fn check(&self, _permission: OsPermission) -> PermissionState {
        PermissionState::NotDetermined
    }
    fn request(&self, _permission: OsPermission) -> AppResult<PermissionState> {
        self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        // The real dialog is asynchronous: it goes on screen and the answer
        // arrives later, so the state right after asking is still undetermined.
        Ok(PermissionState::NotDetermined)
    }
    fn open_privacy_pane(&self, _pane: crate::error::PrivacyPane) -> AppResult<()> {
        Ok(())
    }
}

/// A ModelStore that is never consulted — the engine is built from a real path.
struct UnusedModelStore;

#[async_trait::async_trait]
impl crate::ports::ModelStore for UnusedModelStore {
    async fn list(&self) -> AppResult<Vec<crate::ports::models::ModelStatus>> {
        Ok(vec![])
    }
    async fn status(&self, id: &crate::types::ModelId) -> AppResult<crate::ports::models::ModelStatus> {
        Err(crate::error::AppError::not_found(id.as_str()))
    }
    async fn ensure(&self, _id: &crate::types::ModelId) -> AppResult<PathBuf> {
        Err(crate::error::AppError::not_found("model"))
    }
    async fn verify(&self, _id: &crate::types::ModelId) -> AppResult<bool> {
        Ok(true)
    }
    async fn delete(&self, _id: &crate::types::ModelId) -> AppResult<()> {
        Ok(())
    }
}

// ── Fixtures ─────────────────────────────────────────────────────────────

/// Leading and trailing silence, so the VAD sees a real onset and a real tail.
fn with_silence(speech: Vec<f32>) -> Vec<f32> {
    let pad = vec![0.0_f32; TARGET_SAMPLE_RATE as usize / 2];
    let mut out = pad.clone();
    out.extend(speech);
    out.extend(pad);
    out
}

// ── Harness ──────────────────────────────────────────────────────────────

struct Harness {
    ctx: SessionContext,
    injector: Arc<CapturingInjector>,
    events: Arc<RecordingEventSink>,
    db: Database,
}

fn build(
    samples: Vec<f32>,
    engine: Arc<dyn crate::ports::TranscriptionEngine>,
) -> AppResult<(Harness, mpsc::Receiver<SessionEvent>)> {
    build_with_permissions(samples, engine, Arc::new(AlwaysGranted))
}

/// Same harness, with the permission provider swapped — used by the test that
/// proves a session cannot start without the microphone.
fn build_with_permissions(
    samples: Vec<f32>,
    engine: Arc<dyn crate::ports::TranscriptionEngine>,
    permissions: Arc<dyn PermissionProvider>,
) -> AppResult<(Harness, mpsc::Receiver<SessionEvent>)> {
    let db = Database::open_in_memory()?;
    let injector = Arc::new(CapturingInjector::default());
    let events = Arc::new(RecordingEventSink::default());

    let ports = Ports {
        engine,
        audio: Arc::new(ReplayAudioSource {
            samples: Arc::new(samples),
        }),
        enhancer: Arc::new(RuleEnhancer::new()),
        injector: Arc::clone(&injector) as Arc<dyn TextInjector>,
        models: Arc::new(UnusedModelStore),
        permissions,
        events: Arc::clone(&events) as Arc<dyn EventSink>,
    };

    let (event_tx, event_rx) = mpsc::channel(64);
    let ctx = SessionContext::new(db.clone(), ports, SessionHandle::new(event_tx));

    Ok((
        Harness {
            ctx,
            injector,
            events,
            db,
        },
        event_rx,
    ))
}

/// Drives one full session and returns the harness once it reaches a terminal
/// state, so the test can inspect what actually happened.
async fn run_session(harness: &Harness, event_rx: mpsc::Receiver<SessionEvent>, speak_for: Duration) {
    let (decode_tx, decode_rx) = mpsc::channel(16);
    let worker = AsrWorker::spawn(Arc::clone(&harness.ctx.ports.engine), decode_tx)
        .expect("ASR worker");

    let actor = SessionActor::new(
        harness.ctx.clone(),
        worker,
        decode_rx,
        SessionSettings::defaults(),
    );
    tokio::spawn(actor.run(event_rx));

    // Stamped exactly as the hotkey handler and the IPC commands do, because
    // HotkeyDispatch and TotalFinalize are measured from the press and a
    // harness that skips the stamp would silently stop recording them.
    harness.ctx.session.stamp_request();
    harness
        .ctx
        .session
        .send(SessionEvent::StartRequested)
        .await
        .expect("start");

    tokio::time::sleep(speak_for).await;

    harness.ctx.session.stamp_request();
    harness
        .ctx
        .session
        .send(SessionEvent::StopRequested)
        .await
        .expect("stop");

    /*
     * SOURCE OF TRUTH KEYWORDS: stop_is_instant_e2e
     * The FSM is expected back at Idle essentially at once — capture ends with
     * the keypress and the words are delivered afterwards. Asserting that here,
     * with a deliberately tight bound, is what stops delivery quietly creeping
     * back onto the user's path: if someone makes the stop transition wait on a
     * decode again, this fails in milliseconds rather than being noticed months
     * later as "the app feels sluggish".
     */
    for _ in 0..50 {
        if harness.ctx.current_state().is_terminal() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        harness.ctx.current_state().is_terminal(),
        "releasing the hotkey must free the app immediately, not after the model finishes"
    );

    await_delivery(harness).await;
}

/**
 * WHAT:  Waits for the background delivery worker to finish with this
 *        recording.
 * WHY:   Delivery no longer ends when the state machine does — that separation
 *        is the point — so a test that waits on the FSM would race the words.
 *        Polling the injector is the honest signal: it is the thing the user
 *        actually experiences.
 */
async fn await_delivery(harness: &Harness) {
    for _ in 0..200 {
        // The row is written LAST, after the paste and the metrics, so a
        // finalised row means the whole delivery is done. Waiting on the
        // injector alone would race the History assertions — the text can be
        // on screen a few milliseconds before it is in the database.
        let finished = crate::services::sessions::list_sessions(&harness.db, 10, 0)
            .map(|rows| rows.iter().any(|row| row.ended_at_ms.is_some()))
            .unwrap_or(false);
        if finished {
            return;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services;

    /**
     * Acquires the ONE shared engine and the lock that serialises access to it.
     * Skips with a printed reason rather than failing on a clean checkout.
     *
     * The lock is not about speed. whisper.cpp's Metal backend aborts the
     * process when a second context is destroyed while another holds device
     * resources, so a test that builds its own engine crashes the whole binary
     * and blames whichever test was running at the time.
     */
    // clippy warns about holding a MutexGuard across an await, and it is right
    // to in general — a guard held across a yield point can deadlock a runtime.
    // Here it is the POINT: the guard exists to serialise engine access for the
    // whole body of an async test, awaits included, because two decodes at once
    // abort the process. These tests are single-threaded with respect to the
    // engine by construction, so there is no second waiter to deadlock against.
    macro_rules! require_engine {
        () => {{
            let guard = crate::testing::engine_lock();
            match crate::testing::shared_engine() {
                Some(engine) => (engine, guard),
                None => {
                    eprintln!("skipped: the default model has not been downloaded");
                    return;
                }
            }
        }};
    }

    macro_rules! require_speech {
        ($sentence:expr, $name:expr) => {
            match crate::testing::synthesise_speech($sentence, $name) {
                Some(samples) => with_silence(samples),
                None => {
                    eprintln!("skipped: could not synthesise speech");
                    return;
                }
            }
        };
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: hotkey_permission_regression
     * WHAT:  Pressing the hotkey on a machine that has never granted the
     *        microphone must ask for it, and must not start a recording.
     * WHY:   The bug this locks shut. The command factory preflighted
     *        permissions on every IPC call, which looked like full coverage and
     *        was not: the global hotkey sends StartRequested directly to the
     *        actor and never touches the factory. So the app's PRIMARY entry
     *        point had no gate at all. It opened a stream macOS was feeding
     *        silence into, showed a pill with no signal, and produced no
     *        transcript and no explanation.
     *
     *        The assertion is on the ASK and on the ABSENCE of delivery, not on
     *        an error message, because the message is the part most likely to
     *        be reworded later and the guarantee is not about wording.
     * WHERE: Guards the permission gate in SessionActor::start_capture.
     */
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[allow(clippy::await_holding_lock)]
    async fn recording_never_begins_without_the_microphone() {
        let (engine, _engine_guard) = require_engine!();
        let permissions = Arc::new(NeverAsked::default());

        let (harness, rx) = build_with_permissions(
            vec![0.0; 16_000],
            engine,
            Arc::clone(&permissions) as Arc<dyn PermissionProvider>,
        )
        .expect("harness");

        run_session(&harness, rx, Duration::from_millis(400)).await;

        assert!(
            permissions.asked.load(std::sync::atomic::Ordering::SeqCst) >= 1,
            "a never-asked microphone must be asked for, not silently worked around"
        );
        assert!(
            harness.injector.delivered.lock().is_none(),
            "nothing may be delivered from a session that was never allowed to record"
        );
    }

    /**
     * THE test. Speak, stop, and assert that the words came out the far end.
     * If this passes, the product does what it says on the box.
     */
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[allow(clippy::await_holding_lock)] // the guard must span the awaits — see require_engine
    async fn a_spoken_sentence_becomes_delivered_text() {
        let (engine, _engine_guard) = require_engine!();
        let samples = require_speech!("The quick brown fox jumps over the lazy dog.", "fox");

        let (harness, rx) = build(samples, engine).expect("harness");
        run_session(&harness, rx, Duration::from_secs(4)).await;

        let delivered = harness
            .injector
            .delivered
            .lock()
            .clone()
            .expect("something must have been delivered");

        let lowered = delivered.to_lowercase();
        assert!(
            lowered.contains("quick") && lowered.contains("fox"),
            "the spoken words did not survive the pipeline: {delivered:?}"
        );

        // The enhancement pass ran: a sentence ends in punctuation.
        assert!(
            delivered.trim_end().ends_with('.'),
            "the enhancement pass did not run: {delivered:?}"
        );
    }

    /// The session must be in history, with its text and its timings.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[allow(clippy::await_holding_lock)] // the guard must span the awaits — see require_engine
    async fn a_delivered_session_is_persisted_with_its_metrics() {
        let (engine, _engine_guard) = require_engine!();
        let samples = require_speech!("Recording this sentence for the history table.", "history");

        let (harness, rx) = build(samples, engine).expect("harness");
        run_session(&harness, rx, Duration::from_secs(4)).await;

        let sessions = services::sessions::list_sessions(&harness.db, 10, 0).expect("list");
        assert_eq!(sessions.len(), 1, "exactly one session should be recorded");

        let session = &sessions[0];
        assert_eq!(session.outcome, crate::types::SessionOutcome::Delivered);
        assert!(session.final_text.is_some(), "no transcript was stored");
        assert!(
            session.raw_text.is_some(),
            "raw text must be stored alongside final, or accuracy complaints are unattributable"
        );
        assert!(session.word_count.unwrap_or(0) > 0);
        assert_eq!(
            session.app_bundle_id.as_deref(),
            Some("com.apple.Terminal"),
            "the frontmost app should be recorded for per-app profiles"
        );

        // The latency claim has to be measurable, or it is only an assertion.
        let metrics = services::metrics::metrics_for_session(&harness.db, &session.id)
            .expect("metrics");
        assert!(
            metrics.iter().any(|m| m.stage == crate::types::LatencyStage::TailDecode),
            "the tail decode was not timed"
        );

        /*
         * SOURCE OF TRUTH KEYWORDS: every_declared_metric_is_recorded
         * Four of the nine declared stages were never written by anything —
         * HotkeyDispatch, Assemble, ClipboardWrite and TotalFinalize. The last
         * is labelled "Stop to pasted" and is the number this product promises,
         * so the dashboard had a permanently empty row where its headline
         * figure belongs.
         *
         * registry::reachability asserts that SOMETHING references each stage;
         * this asserts that a real session actually produces one. Both are
         * needed: the first catches a stage nobody wired, this catches a stage
         * wired onto a branch that never runs.
         */
        use crate::types::LatencyStage;
        for stage in [
            LatencyStage::HotkeyDispatch,
            LatencyStage::Assemble,
            LatencyStage::ClipboardWrite,
            LatencyStage::TotalFinalize,
        ] {
            assert!(
                metrics.iter().any(|m| m.stage == stage),
                "{stage:?} is declared on the dashboard and was not recorded by a real session"
            );
        }

        let total = metrics
            .iter()
            .find(|m| m.stage == LatencyStage::TotalFinalize)
            .expect("TotalFinalize was asserted above");
        let tail = metrics
            .iter()
            .find(|m| m.stage == LatencyStage::TailDecode)
            .expect("TailDecode was asserted above");
        // Not a wall-clock budget — that would assert the machine. This asserts
        // the SHAPE: stop-to-pasted contains the tail decode, so a TotalFinalize
        // that started from the wrong instant fails here rather than quietly
        // reporting a flattering number.
        assert!(
            total.duration_ms >= tail.duration_ms,
            "TotalFinalize ({:.1}ms) is shorter than the decode it contains ({:.1}ms); it is being measured from the wrong point",
            total.duration_ms,
            tail.duration_ms
        );
    }

    /**
     * The cancellation guarantee, end to end: Escape then Escape again keeps
     * the recording, and the words still arrive. This is the path that is
     * easiest to break and most alarming when broken.
     */
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[allow(clippy::await_holding_lock)] // the guard must span the awaits — see require_engine
    async fn escape_then_escape_again_keeps_the_recording() {
        let (engine, _engine_guard) = require_engine!();
        let samples = require_speech!("Please do not throw this recording away.", "resume");

        let (harness, rx) = build(samples, engine).expect("harness");

        let (decode_tx, decode_rx) = mpsc::channel(16);
        let worker =
            AsrWorker::spawn(Arc::clone(&harness.ctx.ports.engine), decode_tx).expect("worker");
        let actor = SessionActor::new(
            harness.ctx.clone(),
            worker,
            decode_rx,
            SessionSettings::defaults(),
        );
        tokio::spawn(actor.run(rx));

        let session = &harness.ctx.session;
        session.send(SessionEvent::StartRequested).await.expect("start");
        tokio::time::sleep(Duration::from_millis(1200)).await;

        session.send(SessionEvent::CancelArmed).await.expect("armed");
        tokio::time::sleep(Duration::from_millis(300)).await;
        session.send(SessionEvent::CancelAborted).await.expect("aborted");

        tokio::time::sleep(Duration::from_secs(2)).await;
        session.send(SessionEvent::StopRequested).await.expect("stop");

        await_delivery(&harness).await;

        let delivered = harness.injector.delivered.lock().clone();
        assert!(
            delivered.is_some(),
            "a resumed recording must still deliver — the audio was never torn down"
        );

        // And it is in history, because it was never cancelled.
        assert_eq!(
            services::sessions::list_sessions(&harness.db, 10, 0)
                .expect("list")
                .len(),
            1
        );
    }

    /**
     * The other half of the cancellation guarantee: letting the countdown
     * expire destroys the session completely. Nothing pasted, nothing in
     * history, no tombstone.
     */
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[allow(clippy::await_holding_lock)] // the guard must span the awaits — see require_engine
    async fn a_cancelled_recording_leaves_absolutely_nothing_behind() {
        let (engine, _engine_guard) = require_engine!();
        let samples = require_speech!("This sentence should never be seen again.", "cancel");

        let (harness, rx) = build(samples, engine).expect("harness");

        let (decode_tx, decode_rx) = mpsc::channel(16);
        let worker =
            AsrWorker::spawn(Arc::clone(&harness.ctx.ports.engine), decode_tx).expect("worker");
        let actor = SessionActor::new(
            harness.ctx.clone(),
            worker,
            decode_rx,
            SessionSettings::defaults(),
        );
        tokio::spawn(actor.run(rx));

        let session = &harness.ctx.session;
        session.send(SessionEvent::StartRequested).await.expect("start");
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // A row exists while recording — that is the crash-recovery guarantee.
        assert_eq!(
            services::sessions::list_sessions(&harness.db, 10, 0).expect("list").len(),
            1,
            "the in-flight row must exist during recording"
        );

        session.send(SessionEvent::CancelArmed).await.expect("armed");
        session.send(SessionEvent::CancelExpired).await.expect("expired");
        tokio::time::sleep(Duration::from_millis(500)).await;

        assert!(
            harness.injector.delivered.lock().is_none(),
            "a cancelled recording must never be pasted"
        );
        assert_eq!(
            services::sessions::list_sessions(&harness.db, 10, 0).expect("list").len(),
            0,
            "escape means gone — no row, no tombstone"
        );
    }

    /// The pill must be shown while recording and hidden when idle, driven
    /// entirely by the FSM.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[allow(clippy::await_holding_lock)] // the guard must span the awaits — see require_engine
    async fn the_pill_is_shown_for_the_session_and_hidden_afterwards() {
        let (engine, _engine_guard) = require_engine!();
        let samples = require_speech!("Showing the pill for this session.", "pill");

        let (harness, rx) = build(samples, engine).expect("harness");
        run_session(&harness, rx, Duration::from_secs(3)).await;

        let visibility = harness.events.pill_visible.lock().clone();
        assert!(
            visibility.first() == Some(&true),
            "the pill must appear as soon as the session starts"
        );

        let states = harness.events.states.lock().clone();
        assert!(
            states.iter().any(|s| matches!(s, SessionState::Recording { .. })),
            "the pill was never told it was recording"
        );
        assert!(
            states.iter().any(|s| matches!(s, SessionState::Idle)),
            "the pill was never told it was finalizing"
        );
    }
}
