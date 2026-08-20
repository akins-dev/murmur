/*!
 * SOURCE OF TRUTH KEYWORDS: EventSink, session_state_changed, audio_level,
 *   set_pill_visible, NullEventSink
 * WHAT:  The trait the domain uses to push state outward.
 * WHY:   Two reasons, and the first is a rule this codebase enforces on
 *        everything else:
 *
 *        1. **Layering.** `session/` is domain code. Before this port existed
 *           it imported `tauri` directly and called `emit` on an AppHandle —
 *           an upward dependency from the domain into the boundary, which is
 *           exactly what CLAUDE.md §3 forbids and what makes a codebase
 *           impossible to move. The domain now says WHAT happened; the adapter
 *           decides how it reaches a window.
 *
 *        2. **Testability.** The session actor is where the interesting
 *           correctness lives — cancellation, seams, delivery — and none of it
 *           could be tested without constructing a Tauri app. With this seam an
 *           end-to-end test drives the real actor, the real chunker and the
 *           real engine, and simply records what came out.
 *
 *        Deliberately fire-and-forget: no method returns a Result. A window
 *        that has closed is not a failure the domain can act on, and making the
 *        actor handle an emit error would put UI concerns back inside it.
 * WHERE: Implemented by adapters/events (Tauri) and by the recording fake in
 *        the end-to-end tests. Consumed by session/actor.rs.
 */

use crate::types::{AudioLevel, DownloadProgress, ModelId, ModelState, SessionState};

pub trait EventSink: Send + Sync {
    /// The recording state changed. The pill renders exactly this.
    fn session_state_changed(&self, state: &SessionState);

    /// A meter reading for the waveform. High frequency and droppable.
    fn audio_level(&self, level: AudioLevel);

    /// Show or hide the pill overlay.
    /// A recording's text reached the user. See ipc::events::TranscriptDelivered.
    fn transcript_delivered(&self, word_count: u32, delivery: crate::types::DeliveryKind);

    fn set_pill_visible(&self, visible: bool);

    /// Model download progress, during onboarding and model switches.
    fn download_progress(&self, progress: DownloadProgress);

    /**
     * WHAT:  A model's state settled into something new.
     * WHY:   `list` and `status` deliberately do not hash, so a model that is
     *        on disk but unverified reports `Verifying` — and the hash that
     *        resolves it runs somewhere else entirely, on the background
     *        prepare thread. Without this the model manager would sit on
     *        "Verifying" forever: a spinner that never lands, which is a worse
     *        lie than the stall it replaced.
     * WHERE: Emitted by adapters/http_models/store.rs whenever a hash verdict
     *        is recorded or a model is removed.
     */
    fn model_state_changed(&self, model_id: ModelId, state: ModelState);

    /**
     * WHAT:  Grab or release the Escape key.
     * WHY:   Escape belongs to whatever app the user is actually working in. A
     *        global grab held for the process lifetime would break Escape
     *        everywhere — closing a dialog, leaving a field, exiting vim — for
     *        a key we need for a few seconds at a time. It is therefore a
     *        consequence of session state, which is why it lives on this seam
     *        rather than being reached for directly: the domain says "there is
     *        something to cancel", and the adapter decides what that means to
     *        the OS.
     */
    fn set_cancel_key_active(&self, active: bool);
}

/**
 * SOURCE OF TRUTH KEYWORDS: NullEventSink
 * WHAT:  An EventSink that discards everything.
 * WHY:   For code paths that run before any window exists — crash recovery at
 *        launch, for instance — so they do not need an Option and cannot
 *        accidentally depend on a UI being present.
 * WHERE: Used during bootstrap before the sink is built.
 */
pub struct NullEventSink;

impl EventSink for NullEventSink {
    fn session_state_changed(&self, _state: &SessionState) {}
    fn audio_level(&self, _level: AudioLevel) {}
    fn transcript_delivered(&self, _word_count: u32, _delivery: crate::types::DeliveryKind) {}

    fn set_pill_visible(&self, _visible: bool) {}
    fn download_progress(&self, _progress: DownloadProgress) {}
    fn model_state_changed(&self, _model_id: ModelId, _state: ModelState) {}
    fn set_cancel_key_active(&self, _active: bool) {}
}
