/*!
 * SOURCE OF TRUTH KEYWORDS: TauriEventSink, EventSink, emit, pill_visibility
 * WHAT:  The EventSink implementation that pushes typed events to the windows
 *        and shows or hides the pill.
 * WHY:   This is the only place in the app that knows both the domain's
 *        vocabulary and Tauri's. Keeping it in adapters/ means `session/` can
 *        be compiled, reasoned about and tested without a UI framework in
 *        scope — and it means a second frontend, or none at all, is a new
 *        adapter rather than a change to the recording logic.
 *
 *        Every emit failure is logged and swallowed. A window that has closed
 *        mid-session is normal, and turning that into an error the actor has to
 *        handle would put UI lifecycle concerns inside the state machine.
 * WHERE: Constructed in bootstrap and handed to the session actor.
 */

use tauri::AppHandle;
use tauri_specta::Event;

use crate::ipc::events::{
    AudioLevelChanged, ModelDownloadProgress, ModelStateChanged, SessionStateChanged,
    TranscriptDelivered,
};
use crate::ports::events::EventSink;
use crate::types::{AudioLevel, DownloadProgress, ModelId, ModelState, SessionState};

pub struct TauriEventSink {
    app: AppHandle,
}

impl TauriEventSink {
    pub fn new(app: AppHandle) -> Self {
        Self { app }
    }
}

impl EventSink for TauriEventSink {
    fn session_state_changed(&self, state: &SessionState) {
        // The pill window IS the pill, so its bounds are a function of session
        // state. Sizing it here rather than adding a presentational flag to the
        // port is deliberate: this adapter is the one place that already speaks
        // both vocabularies, and `session/` must not learn what a pixel is.
        crate::tray::fit_pill_to_state(&self.app, state);

        if let Err(err) = (SessionStateChanged {
            state: state.clone(),
        })
        .emit(&self.app)
        {
            tracing::warn!(error = %err, "could not emit session state");
        }
    }

    fn audio_level(&self, level: AudioLevel) {
        // Droppable by design: a missed meter frame is invisible, and the pill
        // has a hard frame budget while inference is running.
        let _ = AudioLevelChanged { level }.emit(&self.app);
    }

    fn transcript_delivered(&self, word_count: u32, delivery: crate::types::DeliveryKind) {
        if let Err(err) = (TranscriptDelivered {
            word_count,
            delivery,
        })
        .emit(&self.app)
        {
            tracing::warn!(error = %err, "could not emit a delivery");
        }
    }

    fn set_pill_visible(&self, visible: bool) {
        crate::tray::set_pill_visible(&self.app, visible);
    }

    fn set_cancel_key_active(&self, active: bool) {
        crate::bootstrap::set_escape_registered(&self.app, active);
    }

    fn download_progress(&self, progress: DownloadProgress) {
        if let Err(err) = (ModelDownloadProgress { progress }).emit(&self.app) {
            tracing::warn!(error = %err, "could not emit download progress");
        }
    }

    fn model_state_changed(&self, model_id: ModelId, state: ModelState) {
        if let Err(err) = (ModelStateChanged { model_id, state }).emit(&self.app) {
            tracing::warn!(error = %err, "could not emit model state");
        }
    }
}
