/*!
 * SOURCE OF TRUTH KEYWORDS: PermissionProvider, OsPermission, PermissionState,
 *   check_permission, request_permission, open_privacy_pane
 * WHAT:  The trait for reading and requesting OS-level permissions.
 * WHY:   A sixth port beyond the five in the technical plan, and it earns its
 *        place: the command factory runs a permission preflight on every call,
 *        and without this seam the IPC layer would have to call into the macOS
 *        adapter directly — an upward import that breaks the layering the whole
 *        architecture rests on. It is also the first thing Windows would need.
 *
 *        `request` can only ever be useful once per permission: macOS shows its
 *        consent dialog a single time per bundle-and-signature, and after a
 *        denial the only recovery is deep-linking into System Settings. That is
 *        why `open_privacy_pane` is part of the contract rather than a detail.
 * WHERE: Implemented by adapters/macos; consumed by ipc/factory.rs preflight
 *        and by the onboarding flow.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

use crate::error::{AppResult, PrivacyPane};

/**
 * SOURCE OF TRUTH KEYWORDS: OsPermission
 * WHAT:  The OS grants a capability can require.
 * WHERE: Declared per capability in registry/; checked by the factory preflight.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum OsPermission {
    Microphone,
    /// Required for synthetic keystrokes. Its absence degrades paste to
    /// clipboard-only; it never blocks recording.
    Accessibility,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PermissionState {
    Granted,
    Denied,
    /// Never asked. The one state in which `request` will show a dialog.
    NotDetermined,
}

impl PermissionState {
    pub fn is_granted(&self) -> bool {
        matches!(self, PermissionState::Granted)
    }
}

pub trait PermissionProvider: Send + Sync {
    fn check(&self, permission: OsPermission) -> PermissionState;

    /**
     * SOURCE OF TRUTH KEYWORDS: ensure, never_asked_means_ask
     * WHAT:  The current state, having asked the OS first if it has never been
     *        asked. This is what callers who are about to USE a permission want;
     *        `check` is for callers who only want to report on one.
     * WHY:   `NotDetermined` and `Denied` are opposite situations that a caller
     *        using `check` alone cannot tell apart, and the correct response to
     *        each is the opposite of the other. Never asked means ASK. Refused
     *        means deep-link into System Settings.
     *
     *        Collapsing them produces a specific, hard-to-diagnose dead end:
     *        the app refuses to record and sends the user to a privacy pane
     *        that does not list Murmur at all, because macOS only lists an app
     *        there once it has actually requested. The user sees an empty list,
     *        concludes the app is broken, and they are not wrong.
     *
     *        Living on the trait rather than at the call sites is deliberate.
     *        There are two callers with opposite rhythms — a preflight that
     *        runs on every hotkey press and a delivery that runs after every
     *        sentence — and a rule enforced in two places is a rule that will
     *        eventually disagree with itself. That is precisely how this bug
     *        got in.
     * WHERE: The command factory preflight, and the text injector.
     */
    fn ensure(&self, permission: OsPermission) -> PermissionState {
        let state = self.check(permission);
        if state != PermissionState::NotDetermined {
            return state;
        }

        // Asking is best-effort: a failure to even present the dialog must not
        // become the caller's problem, so it degrades to the state we already
        // read rather than propagating.
        match self.request(permission) {
            Ok(after) => after,
            Err(err) => {
                tracing::warn!(permission = ?permission, error = %err, "could not ask for a permission");
                state
            }
        }
    }

    /// Shows the system dialog, but only when state is NotDetermined. After a
    /// denial this is a no-op by design — the OS will not ask twice.
    fn request(&self, permission: OsPermission) -> AppResult<PermissionState>;

    /// Deep-link into System Settings. The only recovery from a denial.
    fn open_privacy_pane(&self, pane: PrivacyPane) -> AppResult<()>;
}
