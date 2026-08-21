/*!
 * SOURCE OF TRUTH KEYWORDS: check_permissions, request_permission,
 *   open_privacy_pane, list_input_devices, PermissionReport
 * WHAT:  OS permission state, the one-shot request, the settings deep link, and
 *        the input device list.
 * WHY:   These deliberately do NOT go through a capability that requires the
 *        permission they are reporting on — a command that needs the microphone
 *        in order to tell you the microphone is denied would be useless. They
 *        report state; they never refuse.
 * WHERE: Consumed by onboarding and by the permission empty-states.
 */

use tauri::State;

use crate::error::AppError;
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec, Validate};
use crate::ports::permissions::{OsPermission, PermissionState};
use crate::registry::CapabilityKey;
use crate::types::DeviceInfo;

// PartialEq so the permission watcher can emit only on CHANGE rather than
// pushing an identical report every second. See bootstrap::watch_permissions.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct PermissionReport {
    pub permission: OsPermission,
    pub state: PermissionState,
}

const CHECK: CommandSpec = CommandSpec::new("check_permissions", CapabilityKey::Onboarding);

#[tauri::command]
#[specta::specta]
pub async fn check_permissions(
    state: State<'_, AppState>,
) -> Result<Vec<PermissionReport>, AppError> {
    execute(&state, CHECK, (), |ctx, ()| async move {
        let provider = &ctx.ports().permissions;
        Ok([OsPermission::Microphone, OsPermission::Accessibility]
            .into_iter()
            .map(|permission| PermissionReport {
                permission,
                state: provider.check(permission),
            })
            .collect())
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct PermissionInput {
    pub permission: OsPermission,
}

impl Validate for PermissionInput {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

/**
 * WHAT:  Shows the system consent dialog.
 * WHY:   Only ever effective once — macOS shows its prompt a single time per
 *        bundle and signature, and after a denial this silently returns the
 *        same denied state. That is why the UI must follow a denial with the
 *        deep link rather than another attempt.
 * WHERE: Onboarding's permission step.
 */
const REQUEST: CommandSpec =
    CommandSpec::new("request_permission", CapabilityKey::Onboarding).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn request_permission(
    state: State<'_, AppState>,
    input: PermissionInput,
) -> Result<PermissionState, AppError> {
    execute(&state, REQUEST, input, |ctx, input| async move {
        ctx.ports().permissions.request(input.permission)
    })
    .await
}

const OPEN_PANE: CommandSpec = CommandSpec::new("open_privacy_pane", CapabilityKey::Onboarding);

#[tauri::command]
#[specta::specta]
pub async fn open_privacy_pane(
    state: State<'_, AppState>,
    input: PermissionInput,
) -> Result<(), AppError> {
    execute(&state, OPEN_PANE, input, |ctx, input| async move {
        let pane = match input.permission {
            OsPermission::Microphone => crate::error::PrivacyPane::Microphone,
            OsPermission::Accessibility => crate::error::PrivacyPane::Accessibility,
        };
        ctx.ports().permissions.open_privacy_pane(pane)
    })
    .await
}

/// Resolves the options for the input-device setting. See ChoiceSource.
// Enumerating devices needs no grant — and onboarding lists them BEFORE asking
// for one, so preflighting this would make the setup screen unusable.
const DEVICES: CommandSpec =
    CommandSpec::new("list_input_devices", CapabilityKey::Dictation).reports();

#[tauri::command]
#[specta::specta]
pub async fn list_input_devices(
    state: State<'_, AppState>,
) -> Result<Vec<DeviceInfo>, AppError> {
    execute(&state, DEVICES, (), |ctx, ()| async move {
        ctx.ports().audio.list_devices()
    })
    .await
}
