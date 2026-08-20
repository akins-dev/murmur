/*!
 * SOURCE OF TRUTH KEYWORDS: SessionActor, run, apply_effect, handle_capture,
 *   handle_decode, finalize, EVENT_QUEUE_DEPTH, TICK_INTERVAL
 * WHAT:  The single task that owns the recording FSM and performs its effects.
 *        Everything else sends it messages.
 * WHY:   One owner, reached only by a bounded channel — never an
 *        `Arc<Mutex<SessionState>>` shared between the audio drain, the ASR
 *        worker, the hotkey handler and the IPC layer. That shape is how you
 *        get a deadlock which reproduces once a week at 2am and cannot be
 *        reproduced on demand. Here the state has exactly one owner, so the
 *        deadlock is structurally impossible rather than merely avoided.
 *
 *        The FSM itself stays pure: it returns effects as DATA and this file is
 *        the only thing that performs them, in the order returned. That
 *        separation is what lets the ordering guarantee — persist the row
 *        before the microphone opens — be asserted by a unit test rather than
 *        hoped for.
 * WHERE: Spawned once during setup in lib.rs. Driven by ipc/commands/session.rs,
 *        the hotkey handler, and the capture and decode channels.
 */

use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::error::{AppError, ErrorCode};
use crate::ipc::context::{AppState, SessionContext};
use crate::pipeline::assembler::Assembler;
use crate::pipeline::worker::{AsrWorker, DecodeResult};
use crate::pipeline::Chunker;
use crate::ports::audio::{CaptureConfig, CaptureEvent, CaptureSession};
use crate::ports::engine::TranscribeRequest;
use crate::ports::permissions::{OsPermission, PermissionState};
use crate::services;
use crate::telemetry::{now_ms, LatencyRecorder};
use crate::types::{
    ChunkKind, LanguageCode, LanguageHint, LatencyStage, SessionId, SessionState,
};

use super::delivery::{FinishedRecording, PendingDelivery};
use super::machine::{Effect, SessionEvent, SessionMachine};
use super::settings_view::SessionSettings;

/// Bounded, so a wedged actor is a visible failure rather than growing memory.
pub const EVENT_QUEUE_DEPTH: usize = 64;

/**
 * SOURCE OF TRUTH KEYWORDS: DELIVERY_QUEUE_DEPTH
 * WHAT:  How many finished recordings can be waiting to be pasted.
 * WHY:   Small on purpose. This is a queue of things the user is expecting to
 *        appear in front of them, and a deep one would mean text arriving long
 *        after it stopped making sense — by then they have moved to a different
 *        app and the paste lands in the wrong place. Four is more than anyone
 *        dictating at human speed will ever fill; filling it means decode has
 *        fallen far enough behind that the honest thing is back-pressure on the
 *        actor rather than a longer silent queue.
 */
const DELIVERY_QUEUE_DEPTH: usize = 4;
/// How often elapsed time is pushed to the pill while recording.
const TICK_INTERVAL: Duration = Duration::from_millis(100);

/// How long the pill lingers after a successful delivery. docs/04 §7.
/// How long a failure stays on screen, so the reason can actually be read.
/// Longer than a success on purpose: a reason nobody had time to read is not
/// a reason. docs/04 §7.
const FAILED_PERSIST: Duration = Duration::from_millis(3_000);

pub struct SessionActor {
    ctx: SessionContext,
    machine: SessionMachine,
    chunker: Chunker,
    assembler: Assembler,
    worker: AsrWorker,
    latency: Arc<LatencyRecorder>,

    /// Live microphone stream, present only while capturing.
    capture: Option<Box<dyn CaptureSession>>,
    capture_rx: Option<mpsc::Receiver<CaptureEvent>>,
    decode_rx: mpsc::Receiver<DecodeResult>,

    /// Wall clock for the current session.
    started_at: Option<Instant>,
    /// When the cancel countdown expires.
    cancel_deadline: Option<Instant>,

    /// Whether the last emitted state was a capturing one, so the stop chime
    /// can fire on the EDGE out of capture rather than on arrival at Idle.
    was_capturing: bool,

    /**
     * SOURCE OF TRUTH KEYWORDS: finalize_started, TotalFinalize
     * WHAT:  When the user pressed stop, held until the text is on screen.
     * WHY:   `TotalFinalize` is labelled "Stop to pasted" and is the headline
     *        number this product promises. It has to span from the KEYPRESS to
     *        the end of delivery, which crosses a decode hop and several
     *        events, so no single scope can time it with a StageTimer.
     * WHERE: Set in dispatch from the handle's stamp; consumed by deliver.
     */
    finalize_started: Option<Instant>,

    /**
     * The language detected on the first chunk, pinned for the rest of the
     * session. This is the single largest latency lever in the app: with
     * `Auto`, whisper.cpp runs its detection encoder pass BEFORE assigning the
     * reduced audio_ctx, so it reuses the previous call's full 1500 context and
     * the tail decode goes from ~54ms to ~333ms — missing the budget on its own.
     * Detect once, then pin. See docs/03 §9.
     */
    detected_language: Option<LanguageCode>,
    settings: SessionSettings,

    /**
     * SOURCE OF TRUTH KEYWORDS: pending_deliveries, route_by_session
     * WHAT:  Recordings that have stopped capturing and are still waiting on
     *        their trailing decode, keyed by session id.
     * WHY:   Decode results all arrive on one channel, and once recordings can
     *        overlap, a result may belong to a recording that ended two
     *        recordings ago. Routing by id is what keeps one person's sentence
     *        out of another's — without it, a late interior chunk from the
     *        previous recording would be appended to the current one's
     *        transcript, which is a data-corruption bug that would look like
     *        the model mishearing.
     * WHERE: Filled by hand_off_to_delivery; drained by handle_decode.
     */
    pending: std::collections::HashMap<SessionId, PendingDelivery>,

    /// Finished recordings, in the order they finished. One consumer, so the
    /// pastes land in the order the words were spoken. See session/delivery.rs.
    deliveries: mpsc::Sender<FinishedRecording>,

    /// Taken by `run`, which spawns the worker. See the note on `new` for why
    /// it is not spawned at construction.
    delivery_rx: Option<mpsc::Receiver<FinishedRecording>>,
}

impl SessionActor {
    /**
     * WHAT:  Builds the actor and the channel its delivery worker will read.
     * WHY:   The receiver is HELD rather than spawned here, and that is a
     *        correction worth writing down. The first version spawned the
     *        delivery worker in this constructor, reasoning that the two should
     *        be impossible to construct apart — which is the right instinct and
     *        was the wrong place for it. `new` is called from bootstrap, inside
     *        Tauri's did_finish_launching, where there is NO tokio runtime; the
     *        spawn panicked, and because that callback is `extern "C"` and
     *        cannot unwind, the panic aborted the whole app on launch.
     *
     *        Every test passed, because `#[tokio::test]` provides a runtime.
     *        The lesson is the general one: a constructor must not require an
     *        ambient runtime, because the place it is constructed is not the
     *        place it runs. `run` spawns it instead — `run` is only ever
     *        reached from inside a runtime, and it still cannot be forgotten
     *        because there is no other way to start the actor.
     */
    pub fn new(
        ctx: SessionContext,
        worker: AsrWorker,
        decode_rx: mpsc::Receiver<DecodeResult>,
        settings: SessionSettings,
    ) -> Self {
        let (deliveries, delivery_rx) = mpsc::channel(DELIVERY_QUEUE_DEPTH);

        Self {
            delivery_rx: Some(delivery_rx),
            pending: std::collections::HashMap::new(),
            deliveries,
            machine: SessionMachine::new(settings.cancel_countdown_ms),
            chunker: Chunker::new(),
            assembler: Assembler::new(),
            latency: Arc::new(LatencyRecorder::new()),
            capture: None,
            capture_rx: None,
            decode_rx,
            started_at: None,
            cancel_deadline: None,
            was_capturing: false,
            finalize_started: None,
            detected_language: None,
            settings,
            worker,
            ctx,
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: run
     * WHAT:  The actor loop. Selects over commands, audio, decodes and timers.
     * WHY:   A single select means every input is serialised through one place,
     *        so there is no interleaving to reason about and no lock to take.
     * WHERE: Spawned once in lib.rs setup.
     */
    pub async fn run(mut self, mut events: mpsc::Receiver<SessionEvent>) {
        // Spawned here rather than in `new` because this is the first point
        // guaranteed to be inside a runtime. See the note on `new`.
        if let Some(delivery_rx) = self.delivery_rx.take() {
            tokio::spawn(crate::session::delivery::run_delivery_worker(
                self.ctx.clone(),
                delivery_rx,
            ));
        }

        let mut ticker = tokio::time::interval(TICK_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            // Only select on capture when a stream is actually open, so an idle
            // app is not woken by a channel that will never produce.
            let capture_ready = self.capture_rx.is_some();

            tokio::select! {
                Some(event) = events.recv() => {
                    self.dispatch(event).await;
                }

                Some(capture) = async {
                    match self.capture_rx.as_mut() {
                        Some(rx) => rx.recv().await,
                        None => None,
                    }
                }, if capture_ready => {
                    self.handle_capture(capture).await;
                }

                Some(decode) = self.decode_rx.recv() => {
                    self.handle_decode(decode).await;
                }

                _ = ticker.tick() => {
                    self.on_tick().await;
                }

                else => break,
            }
        }

        tracing::info!("session actor stopped");
    }

    async fn dispatch(&mut self, event: SessionEvent) {
        /*
         * SOURCE OF TRUTH KEYWORDS: delivery_timeout, partial_rescue
         * Intercepted before the FSM, because it is not about the FSM. By the
         * time this fires the machine has long since returned to Idle and may
         * be recording something else entirely; the only thing that still cares
         * is the pending map.
         *
         * Delivering what decoded rather than failing outright is the important
         * part. The interior chunks are usually fine and only the trailing
         * fragment hung, so someone who spoke for two minutes gets their two
         * minutes instead of nothing — which is what a bare timeout would have
         * given them.
         */
        if let SessionEvent::DeliveryTimedOut(session_id) = &event {
            if let Some(pending) = self.pending.remove(session_id) {
                tracing::warn!(
                    session_id = %session_id,
                    "the trailing decode never returned; delivering what did"
                );
                self.finish(pending).await;
            }
            return;
        }

        // Taken BEFORE the transition, because handle() consumes the event, and
        // only recorded if the transition is legal — a stamp from a press the
        // FSM rejected is not a latency anyone experienced.
        let requested_at = match event {
            SessionEvent::StartRequested | SessionEvent::StopRequested => {
                self.ctx.session.take_request_stamp()
            }
            _ => None,
        };
        let was_stop = matches!(event, SessionEvent::StopRequested);

        match self.machine.handle(event) {
            Ok(transition) => {
                if let Some(pressed) = requested_at {
                    if was_stop {
                        // Held open across the decode; closed in deliver.
                        self.finalize_started = Some(pressed);
                    } else {
                        self.latency.record(
                            LatencyStage::HotkeyDispatch,
                            pressed.elapsed().as_secs_f64() * 1000.0,
                        );
                    }
                }
                for effect in transition.effects {
                    self.apply_effect(effect).await;
                }
            }
            // An illegal transition means two parts of the app disagree. Log it
            // and carry on — refusing an Escape because the FSM was mid-change
            // would be worse than ignoring it.
            Err(err) => tracing::debug!(%err, "ignoring an illegal transition"),
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: apply_effect
     * WHAT:  Performs one effect the machine asked for.
     * WHY:   Effects run in the order the machine returned them, and that order
     *        is load-bearing — PersistRow before StartCapture is the crash
     *        recovery guarantee.
     * WHERE: Called for every transition.
     */
    async fn apply_effect(&mut self, effect: Effect) {
        match effect {
            Effect::PersistRow { session_id } => self.persist_row(session_id),
            Effect::StartCapture => self.start_capture().await,
            Effect::StopCapture => self.stop_capture(),
            Effect::HandOffToDelivery { session_id } => {
                self.hand_off_to_delivery(session_id).await
            }
            Effect::StartCountdown { duration_ms } => {
                self.cancel_deadline = Some(Instant::now() + Duration::from_millis(duration_ms));
            }
            Effect::AbortCountdown => self.cancel_deadline = None,
            Effect::DestroySession { session_id } => self.destroy(session_id),
            Effect::PersistOutcome { session_id } => {
                // Only capture-side failures reach here now; a delivered
                // recording writes its own row from the delivery worker.
                if let SessionState::Failed { code, message } = self.machine.state() {
                    crate::session::delivery::persist_capture_failure(
                        &self.ctx,
                        &session_id,
                        &self.latency,
                        *code,
                        message.clone(),
                        self.started_at.take(),
                    );
                }
            }
            Effect::EmitState => self.emit_state(),
        }
    }

    fn persist_row(&mut self, session_id: SessionId) {
        self.started_at = Some(Instant::now());
        // A stop-press stamp that never reached delivery belongs to a session
        // that ended some other way. Cleared here so it cannot be attributed to
        // the session starting now.
        self.finalize_started = None;
        self.assembler.clear();
        self.chunker.clear();
        self.detected_language = None;

        let app_bundle_id = self
            .ctx
            .ports
            .injector
            .frontmost_app()
            .map(|app| app.bundle_id);

        // Settings are re-read per session so a rebind or a per-app profile
        // takes effect on the NEXT recording rather than requiring a restart —
        // and so the app that had focus when the hotkey fired is the one whose
        // profile applies.
        self.settings = SessionSettings::load_for_app(&self.ctx.db, app_bundle_id.as_deref());
        self.machine.set_cancel_countdown_ms(self.settings.cancel_countdown_ms);
        self.machine
            .set_discard_on_escape(self.settings.discard_on_escape);

        let session = services::sessions::NewSession {
            id: session_id,
            started_at: now_ms(),
            engine_id: self.ctx.ports.engine.capabilities().id.as_str().to_string(),
            model_id: self.settings.model_id.clone(),
            app_bundle_id,
        };

        if let Err(err) = services::sessions::create_session(&self.ctx.db, &session) {
            tracing::error!(error = %err, "could not persist the session row");
        }
    }

    async fn start_capture(&mut self) {
        /*
         * SOURCE OF TRUTH KEYWORDS: microphone_gate, every_path_converges_here
         * WHY THIS CHECK IS HERE AND NOT IN THE HOTKEY HANDLER:
         *
         * The command factory preflights permissions for every IPC call, and
         * that was believed to cover the app. It did not. The global hotkey —
         * the way the app is actually used — sends StartRequested straight to
         * this actor and never touches the factory, so the primary path had no
         * permission gate at all. Recording began, macOS fed the stream pure
         * silence, and the user got an empty pill and no transcript with
         * nothing anywhere saying why.
         *
         * Gating in the hotkey handler would have fixed one of two doors. This
         * function is where EVERY path that opens a microphone converges —
         * hotkey, IPC command, tray — so it is the only place the guarantee can
         * actually be made. Same argument as the engine-readiness check below,
         * which is here for the same reason and against the same symptom.
         *
         * `ensure` rather than `check`: never-asked means ask. See
         * PermissionProvider::ensure.
         */
        let microphone = self.ctx.ports.permissions.ensure(OsPermission::Microphone);
        if !microphone.is_granted() {
            tracing::warn!(state = ?microphone, "refusing to record without the microphone");
            let err = match microphone {
                PermissionState::NotDetermined => AppError::microphone_pending(),
                _ => AppError::microphone_denied(),
            };
            let _ = self.ctx.session.send(SessionEvent::ArmingFailed(err)).await;
            return;
        }

        // The engine warms on a background thread at launch, and that takes a
        // few seconds — model load plus a hash of a 574MB file. Someone who
        // launches Murmur and immediately presses the hotkey would otherwise
        // record happily, decode into nothing, and be handed silence with no
        // explanation. Failing fast with a sentence they can act on is far
        // better than a recording that quietly produces no words.
        if !self.ctx.ports.engine.is_ready() {
            let err = AppError::new(
                ErrorCode::EngineNotReady,
                "Murmur is still starting up. Try again in a moment.",
            )
            .recoverable()
            .with_action(crate::error::ErrorAction::Retry);

            let _ = self.ctx.session.send(SessionEvent::ArmingFailed(err)).await;
            return;
        }

        let timer = self.latency.stage_timer(LatencyStage::DeviceOpen);
        let (tx, rx) = mpsc::channel(EVENT_QUEUE_DEPTH);

        let config = CaptureConfig {
            device_id: self.settings.input_device.clone(),
            mode: self.settings.capture_mode,
        };

        match self.ctx.ports.audio.start(&config, tx) {
            Ok(session) => {
                drop(timer);
                self.capture = Some(session);
                self.capture_rx = Some(rx);
                let _ = self.ctx.session.send(SessionEvent::ArmingComplete).await;
            }
            Err(err) => {
                drop(timer);
                tracing::warn!(error = %err, "could not open the microphone");
                let _ = self
                    .ctx
                    .session
                    .send(SessionEvent::ArmingFailed(err))
                    .await;
            }
        }
    }

    fn stop_capture(&mut self) {
        self.capture_rx = None;
        if let Some(session) = self.capture.take() {
            if let Err(err) = session.stop() {
                tracing::warn!(error = %err, "could not stop the microphone cleanly");
            }
        }
    }

    /// Escape means gone: the row, the audio and every decoded segment go in
    /// the same step. No tombstone, no purge job to trust.
    fn destroy(&mut self, session_id: SessionId) {
        self.chunker.clear();
        self.assembler.clear();
        self.started_at = None;
        self.cancel_deadline = None;
        let _ = self.latency.take_samples();

        if let Err(err) = services::sessions::delete_session(&self.ctx.db, &session_id) {
            tracing::error!(error = %err, "could not destroy a cancelled session");
        }
    }

    async fn handle_capture(&mut self, event: CaptureEvent) {
        match event {
            CaptureEvent::Samples(samples) => {
                if let Some(chunk) = self.chunker.push(&samples) {
                    let request = self.transcribe_request();
                    if let Some(session_id) = self.machine.session_id().cloned() {
                        self.worker.submit(chunk, request, session_id);
                    }
                }
            }
            CaptureEvent::Level(level) => {
                // Droppable by design: a missed meter frame is invisible.
                self.ctx.ports.events.audio_level(level);
            }
            CaptureEvent::Lost(err) => {
                tracing::warn!(error = %err, "capture device lost");
                let _ = self.ctx.session.send(SessionEvent::DeviceLost(err)).await;
            }
        }
    }

    async fn handle_decode(&mut self, decode: DecodeResult) {
        /*
         * SOURCE OF TRUTH KEYWORDS: decode_timing_follows_its_session
         * Record against the recorder that OWNS this decode, not the actor's
         * current one. The actor swaps in a fresh recorder at hand-off, so by
         * the time a tail comes back `self.latency` already belongs to the next
         * recording — and timing the old tail against it would file one
         * session's decode time on another session's row. Wrong numbers rather
         * than missing ones, which is worse: they still look like measurements.
         */
        match self.pending.get(&decode.session_id) {
            Some(pending) => pending.latency.record(decode.stage, decode.elapsed_ms),
            None => self.latency.record(decode.stage, decode.elapsed_ms),
        }

        if let Some(error) = &decode.error {
            tracing::warn!(error, "a chunk failed to decode");
        }

        // Pin the language from the first chunk that identified one — see the
        // field comment. Only for the recording still being captured: a pending
        // delivery has already had its language moved across with it, and
        // pinning from a finished recording would set the NEXT one's language
        // from the last one's speech.
        if self.detected_language.is_none() && Some(&decode.session_id) == self.machine.session_id()
        {
            if let Some(language) = decode.segments.iter().find_map(|s| s.language.clone()) {
                tracing::debug!(language = %language, "pinning language for this session");
                self.detected_language = Some(language);
            }
        }

        /*
         * SOURCE OF TRUTH KEYWORDS: route_decode, wrong_transcript
         * Route by session id, never to "the current transcript". Once
         * recordings can overlap, a chunk submitted just before a stop can
         * decode after the next recording has already begun — and appending it
         * to whatever is current would silently graft one recording's words
         * onto another's. The user would read that as the model mishearing, and
         * no amount of looking at the model would find it.
         */
        match self.pending.get_mut(&decode.session_id) {
            Some(pending) => {
                let _timer = pending.latency.stage_timer(LatencyStage::Assemble);
                pending.assembler.push_segments(&decode.segments);
            }
            None if Some(&decode.session_id) == self.machine.session_id() => {
                let _timer = self.latency.stage_timer(LatencyStage::Assemble);
                self.assembler.push_segments(&decode.segments);
            }
            None => {
                // Belongs to a recording that was cancelled, or one already
                // delivered by the timeout path. Dropping it is correct —
                // Escape means gone — but it is logged, because silently
                // discarding decoded speech is not something to do quietly.
                tracing::debug!(
                    session_id = %decode.session_id,
                    segments = decode.segments.len(),
                    "dropping a decode for a session that is no longer collecting"
                );
                return;
            }
        }

        if matches!(decode.kind, ChunkKind::Tail) {
            if let Some(pending) = self.pending.remove(&decode.session_id) {
                self.finish(pending).await;
            }
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: hand_off_to_delivery, capture_is_over
     * WHAT:  Detaches everything this recording captured and submits its tail,
     *        leaving the actor clean for the next one.
     * WHY:   This is the moment the split happens. Every piece of per-session
     *        state moves OUT of the actor here — the assembler, the settings
     *        snapshot, the pinned language, the latency recorder, the clocks —
     *        so that the next recording starts from nothing rather than from
     *        the remains of the last one. `mem::take` rather than clone,
     *        because two owners of a half-finished transcript is exactly the
     *        bug this is meant to prevent.
     *
     *        The FSM is ALREADY Idle by the time this runs: it moved there in
     *        the same transition that emitted this effect. So nothing below is
     *        on the user's path, and a slow submit delays only the words, never
     *        the microphone.
     * WHERE: The HandOffToDelivery effect, emitted by every stop path.
     */
    async fn hand_off_to_delivery(&mut self, session_id: SessionId) {
        let heard_nothing_at_all = self.chunker.heard_nothing_at_all();
        let peak_amplitude = self.chunker.peak_amplitude();
        let tail = self.chunker.close_tail();

        let pending = PendingDelivery {
            session_id: session_id.clone(),
            assembler: std::mem::take(&mut self.assembler),
            settings: self.settings.clone(),
            detected_language: self.detected_language.take(),
            latency: Arc::clone(&self.latency),
            started_at: self.started_at.take(),
            finalize_started: self.finalize_started.take(),
            heard_nothing_at_all,
            peak_amplitude,
        };

        // A fresh recorder for the next recording. Sharing one across
        // overlapping deliveries would file the next session's decode times
        // against this session's row — metrics that are wrong rather than
        // missing, which is worse because they still look like measurements.
        self.latency = Arc::new(LatencyRecorder::new());

        let Some(tail) = tail else {
            // Nothing left to decode: either silence, or everything already
            // went out as interior chunks. It is finished as it stands.
            self.finish(pending).await;
            return;
        };

        let request = self.transcribe_request();
        let deadline = Duration::from_millis(pending.settings.finalize_timeout_ms);
        self.pending.insert(session_id.clone(), pending);

        if !self.worker.submit_tail(tail, request, deadline, session_id.clone()) {
            tracing::warn!("the tail could not be queued; delivering what we have");
            if let Some(pending) = self.pending.remove(&session_id) {
                self.finish(pending).await;
            }
            return;
        }

        /*
         * The tail may never come back — a decode can hang, and this queue
         * entry would sit there holding the user's words forever with nothing
         * to prompt it. The timer is the only thing that guarantees every
         * recording reaches a terminal state, so it is armed unconditionally
         * and fires into the actor's own inbox, which by then is free.
         */
        let session = self.ctx.session.clone();
        tokio::spawn(async move {
            tokio::time::sleep(deadline).await;
            let _ = session.send(SessionEvent::DeliveryTimedOut(session_id)).await;
        });
    }

    /// Queues a finished recording for its turn to be pasted.
    async fn finish(&mut self, pending: PendingDelivery) {
        if self.deliveries.send(FinishedRecording { pending }).await.is_err() {
            tracing::error!("the delivery worker is gone; a transcript was lost");
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: emit_state, auto_reset
     * WHAT:  Publishes the new state, shows or hides the pill, and schedules
     *        the return to Idle from a terminal state.
     * WHY:   The pill's visibility is derived from the FSM rather than
     *        commanded separately, so there is no way for the window to be on
     *        screen in a state that has ended. The window is shown and hidden,
     *        never created and destroyed — recreating it would pay the
     *        webview's parse cost on every hotkey press.
     *
     *        Terminal states dismiss themselves on a timer because the pill is
     *        an indicator with no controls: there is nothing for the user to
     *        press, so something has to put it away. Failed lingers longer than
     *        Delivered because a reason nobody had time to read is not a reason.
     * WHERE: The EmitState effect, which every transition emits.
     */
    fn emit_state(&mut self) {
        let state = self.machine.state().clone();
        let was_capturing = self.was_capturing;
        self.was_capturing = state.is_capturing();

        // Publish and emit in the same step, so the read cache and the event
        // stream can never disagree.
        self.ctx.publish_state(state.clone());
        self.ctx.ports.events.session_state_changed(&state);
        self.ctx
            .ports
            .events
            .set_pill_visible(!matches!(state, SessionState::Idle));

        // Audible confirmation. Murmur has no window at the moment the hotkey
        // fires, so for that instant this is the ONLY feedback that it worked.
        if self.settings.audio_feedback {
            use crate::adapters::macos::{play_feedback, FeedbackSound};
            match &state {
                SessionState::Recording { elapsed_ms: 0 } => play_feedback(FeedbackSound::Start),
                /*
                 * SOURCE OF TRUTH KEYWORDS: stop_chime_at_the_stop
                 * A failure has its own sound and must never also get the stop
                 * chime — it is the only non-visual signal that a dictation
                 * produced nothing.
                 */
                SessionState::Failed { .. } => play_feedback(FeedbackSound::Failed),
                /*
                 * Every other way capture ends: the hotkey, a cancel, a lost
                 * device. Keyed on LEAVING a capturing state rather than on
                 * arriving at Idle, because Idle is also the resting state and
                 * the state after a terminal reset — chiming on those would
                 * mean a sound with no gesture behind it.
                 *
                 * This is where the operator asked for it and it is NOT in the
                 * delivery worker, which is where it used to be: delivery
                 * happens after the decode, so the "off" sound arrived a second
                 * or two after he had already let go of the key.
                 */
                _ if was_capturing && !state.is_capturing() => {
                    play_feedback(FeedbackSound::Stop)
                }
                _ => {}
            }
        }

        // Escape is ours only while there is something to cancel.
        self.ctx
            .ports
            .events
            .set_cancel_key_active(state.is_capturing());

        // Terminal states put themselves away. These two values are the ONLY
        // place the pill's dismiss timing is decided — the frontend does not
        // schedule its own, because a window whose visibility is owned by the
        // FSM and whose dismissal is owned by the view would eventually
        // disagree. Both come from docs/04 §7; if you change one, change the
        // doc in the same edit or the design source quietly stops being true.
        // Only Failed lingers now. Idle IS the end of a successful recording —
        // the pill goes the instant the key is released, and nothing is held on
        // screen while the words are still being worked out.
        let linger = match &state {
            SessionState::Failed { .. } => Some(FAILED_PERSIST),
            _ => None,
        };

        if let Some(linger) = linger {
            let session = self.ctx.session.clone();
            tokio::spawn(async move {
                tokio::time::sleep(linger).await;
                let _ = session.send(SessionEvent::Reset).await;
            });
        }
    }

    async fn on_tick(&mut self) {
        if !self.machine.state().is_capturing() {
            return;
        }

        let elapsed_ms = self
            .started_at
            .map(|start| start.elapsed().as_millis() as u64)
            .unwrap_or_default();

        if let Some(deadline) = self.cancel_deadline {
            let now = Instant::now();
            if now >= deadline {
                self.cancel_deadline = None;
                let _ = self.ctx.session.send(SessionEvent::CancelExpired).await;
                return;
            }
            let remaining_ms = (deadline - now).as_millis() as u64;
            let _ = self
                .ctx
                .session
                .send(SessionEvent::Tick {
                    elapsed_ms,
                    remaining_ms,
                })
                .await;
            return;
        }

        let _ = self
            .ctx
            .session
            .send(SessionEvent::Tick {
                elapsed_ms,
                remaining_ms: 0,
            })
            .await;
    }

    /// Builds the per-chunk engine request, pinning the language once known.
    fn transcribe_request(&self) -> TranscribeRequest {
        let language = match (&self.settings.language, &self.detected_language) {
            // An explicit choice always wins.
            (Some(pinned), _) => LanguageHint::Pinned {
                language: pinned.clone(),
            },
            // Otherwise pin whatever the first chunk identified.
            (None, Some(detected)) => LanguageHint::Pinned {
                language: detected.clone(),
            },
            (None, None) => LanguageHint::Auto,
        };

        TranscribeRequest {
            language,
            prompt: self.prompt(),
        }
    }

    /**
     * WHAT:  The vocabulary prompt, most-important term first.
     * WHY:   Ordering is a contract with the engine adapter: over whisper's
     *        ~224-token budget it keeps the LEADING terms and drops from the
     *        tail. Sorted most-recently-used first, so the terms the user
     *        actually says survive truncation.
     * WHERE: Attached to every TranscribeRequest.
     */
    fn prompt(&self) -> Option<String> {
        let terms = services::dictionary::recent_terms(&self.ctx.db, 64).ok()?;
        if terms.is_empty() {
            return None;
        }
        Some(terms.join(", "))
    }
}

/// Reads the settings a session needs, once, at start.
pub fn load_settings(state: &AppState) -> SessionSettings {
    SessionSettings::load(&state.db)
}
