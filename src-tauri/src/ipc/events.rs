/*!
 * SOURCE OF TRUTH KEYWORDS: SessionStateChanged, AudioLevelChanged,
 *   ModelDownloadProgress, OnboardingProgress, emit_state, TypedEvent
 * WHAT:  Every event Rust pushes to the frontend, as typed structs.
 * WHY:   Rust owns domain state and PUSHES it. The frontend never polls and
 *        never keeps a second copy — so the pill cannot show a state the
 *        machine has already left, which is the failure a polling design
 *        produces under load. Declaring events as types means specta generates
 *        the listener signatures too, so a renamed field breaks the TypeScript
 *        build instead of silently delivering undefined.
 * WHERE: Emitted by session/actor.rs and the model store; consumed by
 *        src/lib/events.ts.
 */

use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use crate::types::{AudioLevel, DeliveryKind, DownloadProgress, ModelId, ModelState, SessionState};

/// The pill's entire input. Emitted on every transition.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct SessionStateChanged {
    pub state: SessionState,
}

/**
 * SOURCE OF TRUTH KEYWORDS: TranscriptDelivered
 * WHAT:  A recording finished its journey — the text has been pasted or copied.
 * WHY:   Delivery no longer ends when the session state machine does, so the
 *        state stream can no longer tell anyone that words arrived. Anything
 *        that needs to know a delivery actually SUCCEEDED — as opposed to a
 *        recording merely having stopped — has to hear it here.
 *
 *        Onboarding is the case that proves it is needed rather than
 *        convenient: its hotkey step is deliberately completed by observing a
 *        real delivery rather than by a button the user clicks, because that is
 *        the only evidence that the shortcut registered, the microphone opened,
 *        the model ran AND the paste worked. Without this event that step would
 *        have had to settle for "a recording stopped", which is exactly the
 *        weaker assertion it was written to avoid.
 * WHERE: Emitted by session/delivery.rs at the end of every delivery.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct TranscriptDelivered {
    pub word_count: u32,
    pub delivery: DeliveryKind,
}

/// Waveform data. High frequency, and droppable by design — a missed frame is
/// invisible, whereas queueing them would add latency to the thing being drawn.
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct AudioLevelChanged {
    pub level: AudioLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct ModelDownloadProgress {
    pub progress: DownloadProgress,
}

/**
 * SOURCE OF TRUTH KEYWORDS: ModelStateChanged
 * WHAT:  A model's state changed — typically the flip from Verifying to Ready.
 * WHY:   The state a model is in is decided in three places that are nowhere
 *        near the UI: a hash completing on the background prepare thread, a
 *        download installing, and a removal. `list_models` is a fetch-once, so
 *        without this the model manager renders whatever was true when it
 *        mounted and never learns otherwise.
 * WHERE: Emitted by adapters/http_models/store.rs through the EventSink port;
 *        consumed by the model manager and onboarding.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct ModelStateChanged {
    pub model_id: ModelId,
    pub state: ModelState,
}

/**
 * SOURCE OF TRUTH KEYWORDS: OnboardingProgress
 * WHAT:  Long-running first-run work, reported honestly.
 * WHY:   The Neural Engine compile takes 15-60 seconds once per machine, and a
 *        574MB download takes minutes. Both need real progress rather than an
 *        indeterminate spinner — a spinner on a multi-minute task reads as a
 *        hang, and this is the user's first impression of a "blazing fast" app.
 * WHERE: Emitted during onboarding and after any model switch.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct OnboardingProgress {
    pub step: String,
    pub message: String,
    /// 0.0 to 1.0, or None when genuinely unknowable.
    pub fraction: Option<f32>,
}

/**
 * SOURCE OF TRUTH KEYWORDS: SettingsChanged
 * WHAT:  Announces that a setting was written or reset.
 * WHY:   More than one window reads settings — the dashboard, the pill and
 *        onboarding are separate documents with separate memory. Without this
 *        event each one keeps whatever it last fetched, so rebinding the hotkey
 *        in Settings leaves the pill rendering the old keycap indefinitely.
 *        Carrying the key lets a listener ignore changes it does not care
 *        about rather than refetching everything on every write.
 * WHERE: Emitted by ipc/commands/settings.rs after a successful write.
 */
/**
 * SOURCE OF TRUTH KEYWORDS: UpdateAvailable
 * WHAT:  A newer signed build exists.
 * WHY:   The `general.check_updates` setting promises a check "on launch and
 *        once a day", and until this event existed nothing delivered it —
 *        `check_for_update` and `install_update` were registered, exported, and
 *        called by nobody. A control that describes a schedule the app does not
 *        run is a control that lies.
 *
 *        Pushed rather than polled, like every other state in this app: the
 *        scheduler in bootstrap decides when to look, and the UI learns the
 *        answer whenever it arrives. A frontend that polled would have to own
 *        the schedule, and then the setting would describe one place while
 *        another did the work.
 * WHERE: Emitted by bootstrap's update scheduler; consumed by the dashboard,
 *        which offers the Update action that calls `install_update`.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct UpdateAvailable {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type, Event)]
pub struct SettingsChanged {
    /// The setting that changed, or None when many changed at once.
    pub key: Option<String>,
}
