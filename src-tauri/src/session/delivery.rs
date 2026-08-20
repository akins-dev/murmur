/*!
 * SOURCE OF TRUTH KEYWORDS: DeliveryWorker, PendingDelivery, FinishedRecording,
 *   run_delivery_worker, ordered_delivery, background_delivery
 * WHAT:  Everything that happens to a recording AFTER the user has stopped
 *        talking: the trailing decode, enhancement, the paste, the History row.
 * WHY:   This module exists so that none of it happens while the user is
 *        waiting. Capture and delivery used to be one state machine, which
 *        meant the person was held at the machine's pace — the pill stayed on
 *        screen for as long as the model took, and the next recording was
 *        refused until the previous one had finished pasting. Splitting them at
 *        the stop boundary is what makes the app feel immediate: the FSM is
 *        back at Idle before the tail has even been submitted.
 *
 *        DELIVERIES ARE SEQUENTIAL, and that is the second reason this is a
 *        module rather than a detached task per recording. If two recordings
 *        are in flight, their text MUST arrive in the order it was spoken —
 *        dictate a sentence, dictate the next, and getting them back reversed
 *        is worse than waiting. A single consumer reading one channel gives
 *        that ordering by construction, with no locking and nothing to reason
 *        about. Spawning a task per delivery would have been simpler to write
 *        and would have raced two pastes into the same text field.
 *
 *        It also means a slow decode delays only the deliveries behind it,
 *        never the microphone.
 * WHERE: Fed by SessionActor when a recording finishes; runs as one task
 *        spawned alongside the actor.
 */

use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

use crate::error::AppError;
use crate::ipc::context::SessionContext;
use crate::pipeline::assembler::Assembler;
use crate::ports::injector::InjectionRequest;
use crate::ports::enhancer::EnhanceContext;
use crate::services;
use crate::session::machine::SessionEvent;
use crate::session::settings_view::SessionSettings;
use crate::telemetry::latency::{now_ms, LatencyRecorder};
use crate::types::{
    DeliveryKind, LanguageCode, LatencyStage, SessionId, SessionOutcome,
};

/**
 * SOURCE OF TRUTH KEYWORDS: PendingDelivery
 * WHAT:  A recording whose capture has ended but whose trailing fragment is
 *        still being decoded.
 * WHY:   Held by the actor, keyed by session id, because decode results arrive
 *        on one shared channel and have to be routed back to the recording they
 *        belong to. Before deliveries could overlap there was only ever one
 *        candidate and the actor's own fields were enough; now a result can
 *        arrive for a recording that ended two recordings ago.
 * WHERE: Created by SessionActor on HandOffToDelivery; becomes a
 *        FinishedRecording when its tail decodes.
 */
pub struct PendingDelivery {
    pub session_id: SessionId,
    pub assembler: Assembler,
    pub settings: SessionSettings,
    pub detected_language: Option<LanguageCode>,
    pub latency: Arc<LatencyRecorder>,
    /// Wall clock from the start of capture, for the History row's duration.
    pub started_at: Option<Instant>,
    /// From the stop keypress, for the "stop to pasted" metric.
    pub finalize_started: Option<Instant>,
    /**
     * True when the microphone produced pure digital silence for the whole
     * recording. Carried rather than recomputed because the chunker that
     * measured it has already been reset for the next recording.
     */
    pub heard_nothing_at_all: bool,
    pub peak_amplitude: f32,
}

/// A recording with everything decoded, queued for its turn to be delivered.
pub struct FinishedRecording {
    pub pending: PendingDelivery,
}

/**
 * SOURCE OF TRUTH KEYWORDS: run_delivery_worker
 * WHAT:  The single sequential consumer. Reads finished recordings in the order
 *        they finished and delivers them one at a time.
 * WHY:   See the module WHY — ordering is the reason this is one task and not
 *        one task each. It awaits each delivery fully before taking the next,
 *        so two pastes can never interleave into the same field.
 * WHERE: Spawned once by bootstrap, alongside the session actor.
 */
pub async fn run_delivery_worker(ctx: SessionContext, mut rx: mpsc::Receiver<FinishedRecording>) {
    while let Some(job) = rx.recv().await {
        deliver(&ctx, job.pending).await;
    }
    tracing::info!("delivery worker stopped");
}

/**
 * SOURCE OF TRUTH KEYWORDS: deliver, background_failure
 * WHAT:  Turns one finished recording into pasted text and a History row.
 * WHY:   Failures here are reported very differently from capture failures,
 *        and the difference is the point of the split. This runs while the user
 *        may already be recording again, so it must never take the screen. It
 *        asks the FSM to show something only when nothing is in progress; the
 *        row and the log carry it otherwise.
 * WHERE: Called only by run_delivery_worker, one at a time.
 */
async fn deliver(ctx: &SessionContext, pending: PendingDelivery) {
    let PendingDelivery {
        session_id,
        assembler,
        settings,
        detected_language,
        latency,
        started_at,
        finalize_started,
        heard_nothing_at_all,
        peak_amplitude,
    } = pending;

    let raw = {
        let _timer = latency.stage_timer(LatencyStage::Assemble);
        assembler.finish()
    };

    if raw.trim().is_empty() {
        // An empty transcript has two very different causes and they must not
        // be reported the same way. Pure digital silence means the capture path
        // is broken and the user has to be told; saying nothing means they said
        // nothing, which is not a complaint worth making.
        if heard_nothing_at_all {
            tracing::warn!(
                peak_amplitude,
                "the microphone delivered no signal for the whole session"
            );
            report_failure(ctx, AppError::microphone_silent()).await;
            persist(
                ctx,
                &session_id,
                &latency,
                Outcome::failed(AppError::microphone_silent()),
                started_at,
                None,
            );
            return;
        }

        persist(
            ctx,
            &session_id,
            &latency,
            Outcome::nothing_said(),
            started_at,
            None,
        );
        return;
    }

    let language = assembler
        .language()
        .map(|l| LanguageCode(l.to_string()))
        .or(detected_language);

    let dictionary = services::dictionary::enabled_entries(&ctx.db).unwrap_or_default();

    let context = EnhanceContext {
        language: language.clone(),
        dictionary,
        strip_fillers: settings.strip_fillers,
        expand_spoken_commands: settings.spoken_commands,
        normalise_punctuation: settings.normalise_punctuation,
        capitalise_sentences: settings.capitalise_sentences,
        apply_corrections: settings.apply_corrections,
    };

    let final_text = {
        let _timer = latency.stage_timer(LatencyStage::Enhance);
        ctx.ports
            .enhancer
            .enhance(&raw, &context)
            .unwrap_or_else(|err| {
                // Enhancement is polish. If it fails the words are still
                // correct, and shipping the raw transcript beats shipping
                // nothing.
                tracing::warn!(error = %err, "enhancement failed; delivering raw text");
                raw.clone()
            })
    };

    let outcome = {
        let _timer = latency.stage_timer(LatencyStage::Inject);
        ctx.ports.injector.deliver(&InjectionRequest {
            text: final_text.clone(),
            auto_paste: settings.auto_paste,
            restore_clipboard: settings.restore_clipboard,
            paste_delay_ms: settings.paste_delay_ms,
            clipboard_restore_delay_ms: settings.clipboard_restore_delay_ms,
        })
    };

    let delivery = match outcome {
        Ok(outcome) => {
            if let Some(reason) = &outcome.reason {
                tracing::info!(reason, "delivery degraded to clipboard only");
            }
            latency.record(LatencyStage::ClipboardWrite, outcome.clipboard_write_ms);
            outcome.delivery
        }
        Err(err) => {
            tracing::warn!(error = %err, "delivery failed");
            report_failure(ctx, err).await;
            DeliveryKind::None
        }
    };

    // "Stop to pasted" — the headline number, and the only stage that spans
    // from a keypress to text on screen. Recorded here because this is the
    // moment it ends.
    if let Some(started) = finalize_started {
        latency.record(
            LatencyStage::TotalFinalize,
            started.elapsed().as_secs_f64() * 1000.0,
        );
    }

    let word_count = final_text.split_whitespace().count() as u32;
    ctx.ports.events.transcript_delivered(word_count, delivery);

    // NO SOUND HERE, deliberately. The stop chime plays the instant capture
    // ends, in the actor's emit_state — see FeedbackSound::Stop. Delivery can
    // land a second or two later, and a confirmation that arrives after the
    // thing it confirms is not a confirmation. A second chime at paste time
    // would be worse than the bug it replaced.

    persist(
        ctx,
        &session_id,
        &latency,
        Outcome::delivered(delivery),
        started_at,
        Some((raw, final_text, language)),
    );
}

/**
 * SOURCE OF TRUTH KEYWORDS: report_failure, never_interrupt_a_recording
 * WHAT:  Surfaces a background failure on the pill, but only when the pill is
 *        free.
 * WHY:   The whole reason delivery moved to the background is that the user
 *        carries on. Taking the screen to report a recording they finished ten
 *        seconds ago — while they are speaking into the next one — would undo
 *        that. The FSM refuses the event unless it is Idle, so this is a hint
 *        rather than a command, and it is deliberately not retried.
 * WHERE: Called from deliver on the two failure paths.
 */
async fn report_failure(ctx: &SessionContext, err: AppError) {
    let _ = ctx.session.send(SessionEvent::DeliveryFailed(err)).await;
}

/**
 * SOURCE OF TRUTH KEYWORDS: persist_capture_failure
 * WHAT:  Writes the terminal row for a session that never reached delivery —
 *        the microphone was refused, the engine was not ready, the recording
 *        was abandoned before any audio was captured.
 * WHY:   These rows still belong in History. A failure that leaves no trace is
 *        a failure the user cannot report and we cannot diagnose, and "nothing
 *        appeared and nothing was written down" is the single worst outcome
 *        this app has.
 * WHERE: Called by the actor on the PersistOutcome effect, which now only
 *        fires for capture-side failures.
 */
pub fn persist_capture_failure(
    ctx: &SessionContext,
    session_id: &SessionId,
    latency: &LatencyRecorder,
    code: crate::error::ErrorCode,
    message: String,
    started_at: Option<Instant>,
) {
    persist(
        ctx,
        session_id,
        latency,
        Outcome {
            outcome: SessionOutcome::Failed,
            delivery: DeliveryKind::None,
            error_code: Some(format!("{code:?}")),
            error_message: Some(message),
        },
        started_at,
        None,
    );
}

/// The three shapes a finished recording can take in History.
struct Outcome {
    outcome: SessionOutcome,
    delivery: DeliveryKind,
    error_code: Option<String>,
    error_message: Option<String>,
}

impl Outcome {
    fn delivered(delivery: DeliveryKind) -> Self {
        Self {
            outcome: SessionOutcome::Delivered,
            delivery,
            error_code: None,
            error_message: None,
        }
    }

    fn nothing_said() -> Self {
        Self {
            outcome: SessionOutcome::Delivered,
            delivery: DeliveryKind::None,
            error_code: None,
            error_message: None,
        }
    }

    fn failed(err: AppError) -> Self {
        Self {
            outcome: SessionOutcome::Failed,
            delivery: DeliveryKind::None,
            error_code: Some(format!("{:?}", err.code)),
            error_message: Some(err.message),
        }
    }
}

/**
 * WHAT:  Writes the terminal row and the session's latency samples.
 * WHY:   Every exit from `deliver` goes through here, including the ones that
 *        deliver nothing. A recording that produced no text is still a
 *        recording that happened, and History that quietly omits it is History
 *        the user cannot trust.
 */
fn persist(
    ctx: &SessionContext,
    session_id: &SessionId,
    latency: &LatencyRecorder,
    outcome: Outcome,
    started_at: Option<Instant>,
    text: Option<(String, String, Option<LanguageCode>)>,
) {
    let duration_ms = started_at
        .map(|start| start.elapsed().as_millis() as i64)
        .unwrap_or_default();

    let (raw_text, final_text, language) = match text {
        Some((raw, final_text, language)) => (
            Some(raw),
            Some(final_text),
            language.map(|l| l.as_str().to_string()),
        ),
        None => (None, None, None),
    };

    let word_count = final_text
        .as_ref()
        .map(|text| text.split_whitespace().count() as i64);

    let result = services::sessions::SessionResult {
        ended_at: now_ms(),
        outcome: outcome.outcome,
        duration_ms,
        language,
        raw_text,
        final_text,
        word_count,
        delivery: outcome.delivery,
        error_code: outcome.error_code,
        error_message: outcome.error_message,
    };

    if let Err(err) = services::sessions::finalize_session(&ctx.db, session_id, &result) {
        tracing::error!(error = %err, "could not persist the session outcome");
    }

    let samples = latency.take_samples();
    if !samples.is_empty() {
        if let Err(err) =
            services::metrics::record_metrics(&ctx.db, session_id, &samples, now_ms())
        {
            tracing::warn!(error = %err, "could not record session metrics");
        }
    }
}
