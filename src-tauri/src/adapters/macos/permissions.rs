/*!
 * SOURCE OF TRUTH KEYWORDS: MacosPermissions, check, request, open_privacy_pane,
 *   AXIsProcessTrusted, authorization_status, PRIVACY_PANE_URL
 * WHAT:  Reads and requests the two macOS grants Murmur needs, and deep-links
 *        into System Settings when one has been denied.
 * WHY:   The single most important fact about TCC, and the reason this file is
 *        shaped the way it is: **a permission dialog appears exactly once per
 *        bundle identifier and code signature.** Once a user clicks "Don't
 *        Allow", `request` becomes a no-op forever and no amount of asking
 *        brings the dialog back. The only recovery is sending them to the right
 *        settings pane, which is why that is part of the port rather than a
 *        detail of the UI.
 *
 *        The two grants also fail very differently, and conflating them would
 *        be wrong:
 *          - Microphone missing is fatal to recording. Worse, without
 *            NSMicrophoneUsageDescription in Info.plist the process does not
 *            get denied — it CRASHES on first access.
 *          - Accessibility missing is not fatal at all. It only downgrades
 *            delivery to clipboard-only, which is a supported outcome.
 * WHERE: Implements the PermissionProvider port; consumed by the command
 *        factory's preflight and by onboarding.
 */

use std::sync::atomic::{AtomicBool, Ordering};

use block2::RcBlock;
use objc2_av_foundation::{AVAuthorizationStatus, AVCaptureDevice, AVMediaTypeAudio};

use crate::error::{AppError, AppResult, ErrorCode, PrivacyPane};
use crate::ports::permissions::{OsPermission, PermissionProvider, PermissionState};

/// Deep links into the two panes. These URL forms are stable across the macOS
/// versions we support and are the documented way in.
const MICROPHONE_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
const ACCESSIBILITY_PANE_URL: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility";

/**
 * SOURCE OF TRUTH KEYWORDS: ACCESSIBILITY_ASKED, ask_once
 * WHAT:  Whether this process has already put the Accessibility dialog on
 *        screen during this run.
 * WHY:   The two prompts behave differently and only one of them needs
 *        guarding. AVFoundation coalesces microphone requests, so asking twice
 *        is harmless. `AXIsProcessTrustedWithOptions` does NOT: while the app
 *        is untrusted it will re-present the dialog every time it is called
 *        with the prompt option. Delivery runs after every finished sentence,
 *        so an unguarded ask would put a system dialog in front of a user once
 *        per utterance — which is how a permission prompt teaches someone to
 *        click Deny.
 *
 *        Process-global rather than a field because the grant is a property of
 *        the process, not of any instance holding this struct.
 * WHERE: Read and set by `request`.
 */
static ACCESSIBILITY_ASKED: AtomicBool = AtomicBool::new(false);

pub struct MacosPermissions;

impl MacosPermissions {
    pub fn new() -> Self {
        Self
    }

    /**
     * WHAT:  Microphone authorization, without touching the device.
     * WHY:   Reads the status rather than attempting to open a stream. Opening
     *        one would trigger the consent prompt as a side effect of a check,
     *        which means onboarding could burn the single available prompt just
     *        by rendering.
     * WHERE: Called by `check`.
     */
    fn microphone_state() -> PermissionState {
        // The media-type constant is an Option because the framework symbol
        // could in principle be absent. If it ever is, we cannot determine
        // anything — NotDetermined is the honest answer, and it lets onboarding
        // ask rather than falsely reporting a denial.
        let Some(media_type) = (unsafe { AVMediaTypeAudio }) else {
            return PermissionState::NotDetermined;
        };

        // SAFETY: AVCaptureDevice's authorization query is thread-safe and
        // borrows the media-type constant without taking ownership.
        let status = unsafe { AVCaptureDevice::authorizationStatusForMediaType(media_type) };

        match status {
            AVAuthorizationStatus::Authorized => PermissionState::Granted,
            AVAuthorizationStatus::NotDetermined => PermissionState::NotDetermined,
            // Restricted means parental controls or MDM. It is not the user's
            // to grant, so it reads as denied — the deep link is still the
            // right affordance, it just leads somewhere they cannot change.
            _ => PermissionState::Denied,
        }
    }

    /**
     * WHAT:  Whether this process may post synthetic keyboard events.
     * WHY:   `AXIsProcessTrusted` reads the grant WITHOUT prompting. The
     *        prompting variant is deliberately confined to `request`, so that a
     *        status check can never consume the one-shot dialog.
     * WHERE: Called by `check`.
     */
    fn accessibility_state() -> PermissionState {
        /*
         * SOURCE OF TRUTH KEYWORDS: accessibility_is_one_toggle, AXIsProcessTrusted
         * WHAT:  Whether the user has granted Murmur Accessibility — the single
         *        switch in Privacy & Security > Accessibility.
         * WHY:   `AXIsProcessTrusted` and NOTHING ELSE, and the "nothing else"
         *        is the fix. An earlier version required
         *        CGPreflightPostEventAccess AND CGPreflightListenEventAccess,
         *        on the reasoning that those are the precise capabilities the
         *        app uses. The reasoning was right and the mapping was wrong:
         *
         *            kTCCServicePostEvent   <- granted by Accessibility
         *            kTCCServiceListenEvent <- granted by INPUT MONITORING,
         *                                     a DIFFERENT switch, on a
         *                                     different pane
         *
         *        So the app demanded a permission the user was never asked for
         *        and could not have given by flipping the switch we told them
         *        to flip. It reported "not granted" no matter how many times
         *        they granted it — including after a correct grant, which is
         *        the cruellest version of the bug because the user has done
         *        everything right.
         *
         *        This function answers exactly one question, about exactly one
         *        switch, so that what the app reports and what the user toggles
         *        are the same thing. Posting is cross-checked below because a
         *        mismatch is diagnostic, but it does NOT gate the answer.
         *
         *        Murmur does not need Input Monitoring. The modifier-tap
         *        watcher is the only thing that listens, it is used only for a
         *        modifier-only hotkey, it fails soft, and it retries — so it
         *        must never hold the whole app's permission state hostage.
         */
        // SAFETY: parameterless boolean queries into system frameworks. Neither
        // prompts, and both are safe from any thread.
        let (trusted, can_post) =
            unsafe { (AXIsProcessTrusted(), CGPreflightPostEventAccess()) };

        if trusted != can_post {
            // Genuinely diagnostic: Accessibility grants both, so these two
            // disagreeing means a stale TCC row from an older signature — the
            // one case the user cannot fix by toggling, only by removing the
            // entry with the minus button and allowing it again.
            tracing::warn!(
                ax_trusted = trusted,
                can_post_events = can_post,
                "Accessibility and the post-event grant disagree — a stale TCC row. \
                 Remove Murmur under Privacy & Security > Accessibility with the minus \
                 button, then allow it again."
            );
        }

        if trusted {
            PermissionState::Granted
        } else {
            // Indistinguishable from "never asked" through this API, and that
            // is fine: `request` prompts, and if the dialog has already been
            // used the OS simply does nothing.
            PermissionState::NotDetermined
        }
    }
}

impl Default for MacosPermissions {
    fn default() -> Self {
        Self::new()
    }
}

impl PermissionProvider for MacosPermissions {
    fn check(&self, permission: OsPermission) -> PermissionState {
        match permission {
            OsPermission::Microphone => Self::microphone_state(),
            OsPermission::Accessibility => Self::accessibility_state(),
        }
    }

    fn request(&self, permission: OsPermission) -> AppResult<PermissionState> {
        match permission {
            OsPermission::Microphone => {
                if Self::microphone_state() != PermissionState::NotDetermined {
                    return Ok(Self::microphone_state());
                }
                let Some(media_type) = (unsafe { AVMediaTypeAudio }) else {
                    return Ok(PermissionState::NotDetermined);
                };

                // The system prompt is asynchronous and its result arrives on a
                // callback. We deliberately do NOT block on it: the UI polls
                // `check` after the user answers, which keeps this call from
                // hanging a command if they walk away from the dialog. The
                // handler is therefore empty by design, not by omission.
                let handler = RcBlock::new(|_granted: objc2::runtime::Bool| {});

                // SAFETY: the block is retained by the framework for the life
                // of the request; RcBlock hands it over correctly.
                unsafe {
                    AVCaptureDevice::requestAccessForMediaType_completionHandler(
                        media_type, &handler,
                    );
                }
                Ok(PermissionState::NotDetermined)
            }
            OsPermission::Accessibility => {
                // Already trusted: answer without going near the dialog.
                if Self::accessibility_state().is_granted() {
                    return Ok(PermissionState::Granted);
                }

                // At most one dialog per run. `swap` claims the right to ask
                // atomically, so two threads finishing sentences at the same
                // moment cannot both present it. See ACCESSIBILITY_ASKED.
                if ACCESSIBILITY_ASKED.swap(true, Ordering::SeqCst) {
                    return Ok(PermissionState::NotDetermined);
                }

                /*
                 * Ask for the two capabilities we actually use FIRST. They are
                 * the rows that gate posting and observing keystrokes, and
                 * asking for them directly is what puts a grant on the right
                 * row rather than on a neighbouring one. Both prompt, and both
                 * are no-ops once answered.
                 */
                // SAFETY: the prompting variant of the post-event query. It
                // presents system UI and returns the resulting state.
                //
                // Deliberately NOT CGRequestListenEventAccess: that asks for
                // Input Monitoring, a separate permission on a separate pane
                // that this app does not need. Asking for it would put a second
                // dialog in front of the user for a capability nothing uses.
                if unsafe { CGRequestPostEventAccess() } {
                    return Ok(PermissionState::Granted);
                }

                // Falls through to the Accessibility dialog, which is the one
                // that actually opens the pane for a user who has to go and
                // find the switch.
                let trusted = unsafe { prompt_for_accessibility() };
                Ok(if trusted {
                    PermissionState::Granted
                } else {
                    PermissionState::Denied
                })
            }
        }
    }

    fn open_privacy_pane(&self, pane: PrivacyPane) -> AppResult<()> {
        let url = match pane {
            PrivacyPane::Microphone => MICROPHONE_PANE_URL,
            PrivacyPane::Accessibility => ACCESSIBILITY_PANE_URL,
        };

        // `open` is used rather than NSWorkspace so this stays a small, easily
        // audited surface — it is the documented way to reach a settings pane
        // and it works whether or not Settings is already running.
        std::process::Command::new("open")
            .arg(url)
            .status()
            .map_err(|err| {
                AppError::new(
                    ErrorCode::Internal,
                    "Murmur could not open System Settings. Open Privacy & Security yourself and grant access there.",
                )
                .with_detail(err)
            })?;
        Ok(())
    }
}

// ── Raw bindings ─────────────────────────────────────────────────────────
//
// ApplicationServices is linked directly rather than through a wrapper crate:
// these are two C functions, and a dependency to reach them would be more
// surface than the thing it wraps.

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// Non-prompting. Safe to call as often as we like.
    fn AXIsProcessTrusted() -> bool;
}

// SOURCE OF TRUTH KEYWORDS: CGPreflightPostEventAccess, CGRequestPostEventAccess
// WHAT:  The precise questions: may this process SEND keyboard events, and may
//        it WATCH them.
// WHY:   This reads kTCCServicePostEvent, the row that gates CGEventPost, and
//        it is here as a CROSS-CHECK rather than as the answer. Accessibility
//        grants both AX trust and post-event, so the two disagreeing is a
//        reliable signature of a stale TCC row left by an older code signature
//        — the one failure a user cannot fix by toggling the switch.
//
//        Its sibling, CGPreflightListenEventAccess, is deliberately absent.
//        That reads kTCCServiceListenEvent, which is INPUT MONITORING — a
//        different switch on a different pane, which this app does not need and
//        must never demand. Requiring it made the app report "not granted"
//        even after the user had granted Accessibility correctly.
//        Preflight never prompts; Request does.
// WHERE: accessibility_state and request.
//
// A plain comment rather than a doc comment: rustdoc has nothing to attach one
// to on an extern block, and clippy denies it.
#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightPostEventAccess() -> bool;
    fn CGRequestPostEventAccess() -> bool;
}

/**
 * WHAT:  The prompting form of the Accessibility trust check.
 * WHY:   Isolated in its own function so that the prompting and non-prompting
 *        paths cannot be confused at a call site. Building the options
 *        dictionary by hand keeps us from pulling in a Core Foundation wrapper
 *        for one key.
 * WHERE: Called only from `request`.
 *
 * # Safety
 * Calls into ApplicationServices with a dictionary it does not retain.
 */
unsafe fn prompt_for_accessibility() -> bool {
    use core_foundation::base::TCFType;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    // kAXTrustedCheckOptionPrompt — hardcoded rather than imported, because
    // the symbol export varies across SDK versions and the string does not.
    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let value = core_foundation::boolean::CFBoolean::true_value();
    let options = CFDictionary::from_CFType_pairs(&[(key, value)]);

    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef() as *const _) }
}

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrustedWithOptions(options: *const std::ffi::c_void) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checking_a_permission_never_blocks_or_panics() {
        // The important property: a status check is safe to call from the
        // command factory on every single IPC call.
        let provider = MacosPermissions::new();
        let _ = provider.check(OsPermission::Microphone);
        let _ = provider.check(OsPermission::Accessibility);
    }

    #[test]
    fn accessibility_absence_is_never_reported_as_fatal() {
        // Clipboard-only is a supported outcome, so a missing Accessibility
        // grant must produce a recoverable error with a way out.
        let err = AppError::accessibility_denied();
        assert!(err.recoverable);
        assert!(err.action.is_some());
    }

    #[test]
    fn both_panes_have_a_deep_link() {
        assert!(MICROPHONE_PANE_URL.starts_with("x-apple.systempreferences:"));
        assert!(ACCESSIBILITY_PANE_URL.starts_with("x-apple.systempreferences:"));
    }
}
