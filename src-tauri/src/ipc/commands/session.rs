/*!
 * SOURCE OF TRUTH KEYWORDS: start_recording, stop_recording, cancel_recording,
 *   resume_recording, get_session_state, StartRecordingInput
 * WHAT:  The commands that drive one dictation session.
 * WHY:   Each is a few lines because the factory already validated the input,
 *        preflighted the microphone permission, and guarded against a
 *        double-fired hotkey. What is left is the one thing specific to the
 *        command: which event the session actor should receive.
 *
 *        They send rather than await a result, because the FSM's transitions
 *        are what the UI reacts to — a command that returned the new state
 *        would be a second source of truth racing the event stream.
 * WHERE: Registered in lib.rs; called by the frontend and by the hotkey handler.
 */

use tauri::State;

use crate::error::AppError;
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec, Validate};
use crate::registry::CapabilityKey;
use crate::session::SessionEvent;
use crate::types::{RecordingMode, SessionState};

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct StartRecordingInput {
    pub mode: RecordingMode,
}

impl Validate for StartRecordingInput {
    fn validate(&self) -> Result<(), String> {
        // The mode is a closed enum, so deserialization already rejected
        // anything else. Nothing further to check.
        Ok(())
    }
}

const START: CommandSpec =
    CommandSpec::new("start_recording", CapabilityKey::Dictation).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn start_recording(
    state: State<'_, AppState>,
    input: StartRecordingInput,
) -> Result<(), AppError> {
    execute(&state, START, input, |ctx, _input| async move {
        ctx.session().stamp_request();
        ctx.session().send(SessionEvent::StartRequested).await
    })
    .await
}

const STOP: CommandSpec = CommandSpec::new("stop_recording", CapabilityKey::Dictation);

#[tauri::command]
#[specta::specta]
pub async fn stop_recording(state: State<'_, AppState>) -> Result<(), AppError> {
    execute(&state, STOP, (), |ctx, ()| async move {
        ctx.session().stamp_request();
        ctx.session().send(SessionEvent::StopRequested).await
    })
    .await
}

/// First Escape. Arms the countdown; audio keeps recording throughout.
const CANCEL: CommandSpec = CommandSpec::new("cancel_recording", CapabilityKey::Dictation);

#[tauri::command]
#[specta::specta]
pub async fn cancel_recording(state: State<'_, AppState>) -> Result<(), AppError> {
    execute(&state, CANCEL, (), |ctx, ()| async move {
        ctx.session().send(SessionEvent::CancelArmed).await
    })
    .await
}

/// Second Escape. Aborts the countdown and resumes with no gap.
const RESUME: CommandSpec = CommandSpec::new("resume_recording", CapabilityKey::Dictation);

#[tauri::command]
#[specta::specta]
pub async fn resume_recording(state: State<'_, AppState>) -> Result<(), AppError> {
    execute(&state, RESUME, (), |ctx, ()| async move {
        ctx.session().send(SessionEvent::CancelAborted).await
    })
    .await
}

/**
 * WHAT:  The current session state, for a window that just opened.
 * WHY:   A one-shot read for initial paint only. Everything after it arrives on
 *        the SessionStateChanged event — this is deliberately not something to
 *        call on a timer.
 * WHERE: Called once by the pill and the dashboard on mount.
 */
// `.reports()` — reading the FSM's state touches no microphone, and requiring
// one meant the pill could not paint before permission was granted.
const GET_STATE: CommandSpec =
    CommandSpec::new("get_session_state", CapabilityKey::Dictation).reports();

#[tauri::command]
#[specta::specta]
pub async fn get_session_state(state: State<'_, AppState>) -> Result<SessionState, AppError> {
    execute(&state, GET_STATE, (), |ctx, ()| async move {
        Ok(ctx.state.current_state())
    })
    .await
}
