/*!
 * WHAT:  Windows implementation of PermissionProvider.
 * WHY:   Windows desktop apps have unrestricted access to microphone and synthetic keystrokes.
 */

use crate::error::{AppResult, PrivacyPane};
use crate::ports::permissions::{OsPermission, PermissionProvider, PermissionState};

#[derive(Debug, Clone, Default)]
pub struct WindowsPermissions;

impl WindowsPermissions {
    pub fn new() -> Self {
        Self
    }
}

impl PermissionProvider for WindowsPermissions {
    fn check(&self, _permission: OsPermission) -> PermissionState {
        PermissionState::Granted
    }

    fn request(&self, _permission: OsPermission) -> AppResult<PermissionState> {
        Ok(PermissionState::Granted)
    }

    fn open_privacy_pane(&self, pane: PrivacyPane) -> AppResult<()> {
        let uri = match pane {
            PrivacyPane::Microphone => "ms-settings:privacy-microphone",
            PrivacyPane::Accessibility => "ms-settings:easeofaccess-keyboard",
        };
        let _ = std::process::Command::new("cmd")
            .args(["/C", "start", "", uri])
            .spawn();
        Ok(())
    }
}
