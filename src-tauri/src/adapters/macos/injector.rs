/*!
 * SOURCE OF TRUTH KEYWORDS: MacosInjector, deliver, can_inject, frontmost_app,
 *   IsSecureEventInputEnabled, post_paste, KEYCODE_V, PasteTiming
 * WHAT:  Puts finished text on the clipboard and, when allowed, pastes it into
 *        whatever had focus.
 * WHY:   Three separate silent failures live in this one gesture, and each has
 *        cost somebody a day:
 *
 *        1. **Secure Input.** When any app has secure text entry active — a
 *           password field in a browser, Terminal at a sudo prompt — macOS
 *           blocks synthetic keyboard events system-wide. `CGEventPost`
 *           RETURNS SUCCESS and nothing happens. The user experiences "the
 *           paste sometimes doesn't work" and reports it as random. So we check
 *           first and degrade to clipboard-only WITH a reason.
 *        2. **Paste timing.** Two real races, in opposite directions. Paste too
 *           soon after writing the clipboard and the target app pastes the
 *           PREVIOUS contents. Restore the old clipboard too soon after pasting
 *           and you clobber a paste still in flight. Both delays are required
 *           and both are settings, because Electron apps are slower than native
 *           ones and no single number fits every target.
 *        3. **The Command flag must be set on BOTH the keydown and the keyup.**
 *           Setting it only on keydown works in some apps and not others, which
 *           is the worst possible failure signature.
 *
 *        Clipboard-only is a SUCCESS throughout this file, never an error. The
 *        app is fully useful without Accessibility, and treating the fallback
 *        as a failure would nag a user whose setup is working as intended.
 * WHERE: Implements the TextInjector port; called by pipeline/deliver.rs.
 */

use std::time::Duration;

use arboard::Clipboard;
use core_graphics::event::{CGEvent, CGEventFlags, CGEventTapLocation};
use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::ports::injector::{FrontmostApp, InjectionOutcome, InjectionRequest, TextInjector};
use crate::ports::permissions::{OsPermission, PermissionProvider, PermissionState};
use crate::types::DeliveryKind;

/// macOS virtual keycode for `V`.
const KEYCODE_V: u16 = 9;

/**
 * SOURCE OF TRUTH KEYWORDS: PasteTiming
 * WHAT:  The two delays around a synthetic paste.
 * WHY:   Configurable because the right values differ by target application —
 *        see the module WHY. The defaults are the middle of the range that
 *        works across native apps, Terminal and Electron, and they are asserted
 *        against the registry's declared defaults by
 *        `the_registry_defaults_match_the_adapter_defaults` so the two cannot
 *        drift.
 * WHERE: Built per delivery by `PasteTiming::from_request`, from the two values
 *        the session actor puts on the InjectionRequest. This comment used to
 *        say "built from settings by pipeline/deliver.rs" — a file that has
 *        never existed, describing wiring that was never built, while both
 *        settings sat inert. Do not describe a route until it is there.
 */
#[derive(Debug, Clone, Copy)]
pub struct PasteTiming {
    /// After writing the clipboard, before pasting.
    pub before_paste: Duration,
    /// After pasting, before restoring the previous clipboard.
    pub before_restore: Duration,
}

impl Default for PasteTiming {
    fn default() -> Self {
        Self {
            before_paste: Duration::from_millis(40),
            before_restore: Duration::from_millis(150),
        }
    }
}

impl PasteTiming {
    /**
     * WHAT:  The timing the caller asked for, on this request.
     * WHY:   Built per delivery rather than held on the injector, because these
     *        are per-app-profile settings — see the WHY on the request fields.
     *        The adapter reads no settings itself; it is handed two numbers.
     * WHERE: `deliver`, once per delivery.
     */
    fn from_request(request: &InjectionRequest) -> Self {
        Self {
            before_paste: Duration::from_millis(request.paste_delay_ms),
            before_restore: Duration::from_millis(request.clipboard_restore_delay_ms),
        }
    }
}

pub struct MacosInjector<P: PermissionProvider> {
    permissions: P,
}

impl<P: PermissionProvider> MacosInjector<P> {
    pub fn new(permissions: P) -> Self {
        Self { permissions }
    }

    /**
     * WHAT:  Posts ⌘V as a synthetic key press.
     * WHY:   `CGEventTapLocation::HID` is used rather than Session or
     *        AnnotatedSession because it has the widest compatibility across
     *        target applications. The flag is set on both events — see the
     *        module WHY.
     * WHERE: Called by `deliver` only after the secure-input check passes.
     */
    fn post_paste() -> AppResult<()> {
        let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).map_err(|_| {
            AppError::new(
                ErrorCode::InjectionFailed,
                "Murmur could not send the paste. Your text is on the clipboard.",
            )
        })?;

        let key_down = CGEvent::new_keyboard_event(source.clone(), KEYCODE_V, true).map_err(|_| {
            AppError::new(
                ErrorCode::InjectionFailed,
                "Murmur could not send the paste. Your text is on the clipboard.",
            )
        })?;
        let key_up = CGEvent::new_keyboard_event(source, KEYCODE_V, false).map_err(|_| {
            AppError::new(
                ErrorCode::InjectionFailed,
                "Murmur could not send the paste. Your text is on the clipboard.",
            )
        })?;

        // BOTH events. See the module WHY.
        key_down.set_flags(CGEventFlags::CGEventFlagCommand);
        key_up.set_flags(CGEventFlags::CGEventFlagCommand);

        key_down.post(CGEventTapLocation::HID);
        key_up.post(CGEventTapLocation::HID);
        Ok(())
    }

    fn clipboard() -> AppResult<Clipboard> {
        Clipboard::new().map_err(|err| {
            AppError::new(
                ErrorCode::ClipboardUnavailable,
                "Murmur could not reach the clipboard.",
            )
            .with_detail(err)
        })
    }
}

impl<P: PermissionProvider> TextInjector for MacosInjector<P> {
    fn can_inject(&self) -> bool {
        // `check`, not `ensure`. This is a REPORT — the UI calls it to decide
        // what to show — and a report must never have the side effect of
        // putting a system dialog on screen. `deliver` is the one that is about
        // to act, so `deliver` is the one that asks.
        self.permissions
            .check(OsPermission::Accessibility)
            .is_granted()
            && !secure_input_active()
    }

    fn frontmost_app(&self) -> Option<FrontmostApp> {
        frontmost_application()
    }

    /**
     * WHAT:  Clipboard write, then paste if permitted, then optional restore.
     * WHY:   The clipboard write happens FIRST and unconditionally, so that
     *        every path through this function leaves the user's words
     *        somewhere they can reach. Everything after it is an enhancement
     *        that is allowed to fail.
     * WHERE: The final step of pipeline/deliver.rs.
     */
    fn deliver(&self, request: &InjectionRequest) -> AppResult<InjectionOutcome> {
        // Per delivery, from the request. The injector holds no timing of its
        // own — see PasteTiming::from_request.
        let timing = PasteTiming::from_request(request);
        let mut clipboard = Self::clipboard()?;

        // Read the previous contents before overwriting, so a restore is
        // possible. An empty or non-text clipboard is normal, not an error.
        let previous = if request.restore_clipboard {
            clipboard.get_text().ok()
        } else {
            None
        };

        // Measured, not recorded — see InjectionOutcome::clipboard_write_ms.
        let clipboard_started = std::time::Instant::now();
        clipboard.set_text(request.text.clone()).map_err(|err| {
            AppError::new(
                ErrorCode::ClipboardUnavailable,
                "Murmur could not copy your text.",
            )
            .with_detail(err)
        })?;
        let clipboard_write_ms = clipboard_started.elapsed().as_secs_f64() * 1000.0;

        // From here on, the words are safe. Every remaining failure downgrades
        // the outcome rather than losing anything.

        if secure_input_active() {
            return Ok(InjectionOutcome {
                delivery: DeliveryKind::ClipboardOnly,
                reason: Some(
                    "Another app is blocking keystrokes — a password field is focused somewhere."
                        .into(),
                ),
                clipboard_write_ms,
            });
        }

        // The user asked for clipboard-only. Return before the permission
        // check, deliberately: asking for Accessibility here would put a system
        // dialog in front of someone who has explicitly said they do not want
        // the feature that needs it.
        //
        // No `reason` either. A reason renders as an explanation for something
        // having gone wrong, and nothing has — this is exactly what they chose.
        if !request.auto_paste {
            return Ok(InjectionOutcome {
                delivery: DeliveryKind::ClipboardOnly,
                reason: None,
                clipboard_write_ms,
            });
        }

        // `ensure`, not `check`. We are about to paste, so if this grant has
        // never been asked for, ask now. Using `check` here is what made auto-
        // paste fail silently forever: the setting was on, the permission had
        // never been requested, nothing ever requested it, and every delivery
        // quietly fell back to clipboard-only. The setting said yes and the app
        // never once asked the question.
        //
        // The ask is guarded to once per run inside the adapter, so this cannot
        // become a dialog after every sentence.
        let access = self.permissions.ensure(OsPermission::Accessibility);
        if !access.is_granted() {
            return Ok(InjectionOutcome {
                delivery: DeliveryKind::ClipboardOnly,
                reason: Some(match access {
                    // Asked just now, or asked earlier this run and still
                    // unanswered. Tell them what to do with the dialog rather
                    // than describing the state.
                    PermissionState::NotDetermined => {
                        "Text copied. Allow Murmur under Accessibility to have it pasted for you."
                            .into()
                    }
                    _ => "Murmur does not have Accessibility access yet.".to_string(),
                }),
                clipboard_write_ms,
            });
        }

        std::thread::sleep(timing.before_paste);

        if let Err(err) = Self::post_paste() {
            tracing::warn!(error = %err, "synthetic paste failed");
            return Ok(InjectionOutcome {
                delivery: DeliveryKind::ClipboardOnly,
                reason: Some("Murmur could not send the paste.".into()),
                clipboard_write_ms,
            });
        }

        if let Some(previous) = previous {
            std::thread::sleep(timing.before_restore);
            // A failed restore is not worth surfacing: the paste succeeded and
            // the only cost is that the old clipboard is gone.
            if let Err(err) = clipboard.set_text(previous) {
                tracing::debug!(error = %err, "could not restore the previous clipboard");
            }
        }

        Ok(InjectionOutcome {
            delivery: DeliveryKind::Pasted,
            reason: None,
            clipboard_write_ms,
        })
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: secure_input_active, IsSecureEventInputEnabled
 * WHAT:  Whether any application currently has secure text entry enabled.
 * WHY:   One line of code that eliminates an entire class of bug report. See
 *        the module WHY.
 * WHERE: Checked by both `can_inject` and `deliver`.
 */
fn secure_input_active() -> bool {
    // SAFETY: a parameterless boolean query into Carbon, safe on any thread.
    unsafe { IsSecureEventInputEnabled() }
}

/**
 * SOURCE OF TRUTH KEYWORDS: frontmost_application
 * WHAT:  The app that currently has focus, for per-app profiles and history.
 * WHERE: Sampled at session start.
 */
fn frontmost_application() -> Option<FrontmostApp> {
    use objc2_app_kit::NSWorkspace;

    // These bindings are safe in objc2 0.6 — no `unsafe` block is needed, and
    // keeping a redundant one would train readers to skim past the keyword in
    // the places where it does carry weight.
    let workspace = NSWorkspace::sharedWorkspace();
    let app = workspace.frontmostApplication()?;
    let bundle_id = app.bundleIdentifier()?.to_string();
    let name = app
        .localizedName()
        .map(|n| n.to_string())
        .unwrap_or_else(|| bundle_id.clone());
    Some(FrontmostApp { bundle_id, name })
}

#[link(name = "Carbon", kind = "framework")]
unsafe extern "C" {
    fn IsSecureEventInputEnabled() -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::permissions::PermissionState;

    /**
     * SOURCE OF TRUTH KEYWORDS: PASTEBOARD_LOCK
     * WHAT:  Serialises the tests that write the real system pasteboard.
     * WHY:   The pasteboard is a process-global OS resource shared with the
     *        whole machine. Two tests writing it and reading it back will
     *        interleave, and each will read the other's string — a flake that
     *        passes when run alone and fails in a full suite, which is the
     *        worst kind because it looks like a real intermittent bug.
     *        Same class as the shared Whisper context: a global resource needs
     *        one owner at a time, in tests as much as in production.
     */
    static PASTEBOARD_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    /**
     * SOURCE OF TRUTH KEYWORDS: PreservedClipboard, do_not_clobber_the_user
     * WHAT:  Saves whatever is on the system clipboard and puts it back when
     *        the test finishes.
     * WHY:   These tests write to the REAL pasteboard, because that is the
     *        thing being tested — a fake one would prove nothing about the
     *        adapter. But the pasteboard belongs to whoever is using the
     *        machine, and `cargo test` left "clipboard only, please" sitting in
     *        it. The operator pasted our test fixture into a message to us,
     *        which is how we found out.
     *
     *        A test may use a shared machine resource. It may not keep it.
     * WHERE: Held by every test that writes the pasteboard.
     */
    struct PreservedClipboard(Option<String>);

    impl PreservedClipboard {
        fn save() -> Self {
            Self(Clipboard::new().ok().and_then(|mut c| c.get_text().ok()))
        }
    }

    impl Drop for PreservedClipboard {
        fn drop(&mut self) {
            // Restored on unwind too, so a FAILING test does not also cost the
            // user their clipboard.
            if let (Some(text), Ok(mut clipboard)) = (self.0.take(), Clipboard::new()) {
                let _ = clipboard.set_text(text);
            }
        }
    }

    /// Records whether `request` was reached, which is the whole point of the
    /// regression test below — the old code never called it.
    struct FakePermissions {
        state: PermissionState,
        asked: std::sync::atomic::AtomicUsize,
    }

    impl FakePermissions {
        fn new(state: PermissionState) -> Self {
            Self {
                state,
                asked: std::sync::atomic::AtomicUsize::new(0),
            }
        }
        fn times_asked(&self) -> usize {
            self.asked.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    impl PermissionProvider for FakePermissions {
        fn check(&self, _permission: OsPermission) -> PermissionState {
            self.state
        }
        fn request(&self, _permission: OsPermission) -> AppResult<PermissionState> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(self.state)
        }
        fn open_privacy_pane(&self, _pane: crate::error::PrivacyPane) -> AppResult<()> {
            Ok(())
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: skip_under_secure_input
     * WHAT:  Skips a delivery test when the machine has Secure Input on.
     * WHY:   Secure Input is GLOBAL and belongs to whatever app currently has a
     *        password field focused — a login prompt, a keychain dialog, a sudo
     *        prompt in a terminal. `deliver` checks it first and by design, so
     *        while it is on, every delivery is clipboard-only and the tests
     *        below assert things that cannot be true.
     *
     *        They are skipped rather than reworked because the behaviour they
     *        guard is real and worth guarding; it simply cannot be observed in
     *        this state. Failing instead would teach whoever hits it that these
     *        tests are unreliable, and the next real regression would be waved
     *        through as "that flaky pasteboard thing". Same pattern as the
     *        fixtures that skip when `say` is unavailable.
     *
     *        Found the honest way: a keychain dialog left on screen by a hung
     *        `codesign` turned Secure Input on, and two green tests went red
     *        with nothing in the codebase having changed.
     * WHERE: The two delivery tests that assert on permissions or on reason.
     */
    fn secure_input_would_mask_this_test() -> bool {
        if secure_input_active() {
            eprintln!(
                "skipped: Secure Input is active on this machine, so every delivery is \
                 clipboard-only. Dismiss any password or keychain prompt and re-run."
            );
            return true;
        }
        false
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: auto_paste_regression, never_asked_means_ask
     * WHAT:  Delivering with Accessibility never requested must ASK for it.
     * WHY:   This is the auto-paste bug, written down so it cannot come back.
     *        The setting was on, the permission had never been requested,
     *        `deliver` called `check` and saw NotDetermined, and every single
     *        delivery fell back to clipboard-only in silence — forever, because
     *        nothing in the app ever asked the question. The user's toggle said
     *        yes to something that was never put to the OS.
     *
     *        Asserting on the ASK rather than on the outcome is deliberate: the
     *        outcome is clipboard-only either way on this machine, so an
     *        outcome assertion would pass against the broken code.
     * WHERE: Guards MacosInjector::deliver's use of `ensure`.
     */
    #[test]
    fn delivering_without_a_decision_asks_for_accessibility() -> AppResult<()> {
        let _guard = PASTEBOARD_LOCK.lock();
        let _clipboard = PreservedClipboard::save();
        if secure_input_would_mask_this_test() {
            return Ok(());
        }
        let permissions = FakePermissions::new(PermissionState::NotDetermined);
        let injector = MacosInjector::new(permissions);

        let outcome = injector.deliver(&InjectionRequest {
            text: "asking is the point".into(),
            auto_paste: true,
            restore_clipboard: false,
            // Zero: these tests assert branching, not timing, and the real
            // defaults would add 190ms of sleep to each of them.
            paste_delay_ms: 0,
            clipboard_restore_delay_ms: 0,
        })?;

        assert_eq!(
            injector.permissions.times_asked(),
            1,
            "deliver must ask for Accessibility when it has never been requested"
        );
        assert_eq!(outcome.delivery, DeliveryKind::ClipboardOnly);
        assert!(outcome.reason.is_some(), "the user must be told why");
        Ok(())
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: auto_paste_off_regression, a_setting_that_lies
     * WHAT:  Turning auto-paste OFF must actually stop the paste, and must not
     *        ask for a permission the user has just declined to need.
     * WHY:   The other half of the auto-paste bug, and the half that is easy to
     *        miss. `output.auto_paste` was loaded into SessionSettings, rendered
     *        as a control, and written to the database correctly — and never
     *        passed to the injector, which pasted whenever it was able to. The
     *        toggle did nothing in either position.
     *
     *        Asserting on times_asked as well as on the outcome is the point:
     *        an app that respects the setting but still raises an Accessibility
     *        dialog has only half-respected it, and the dialog is the part the
     *        user actually sees.
     * WHERE: Guards the early return at the top of MacosInjector::deliver.
     */
    #[test]
    fn turning_auto_paste_off_stops_the_paste_and_asks_for_nothing() -> AppResult<()> {
        let _guard = PASTEBOARD_LOCK.lock();
        let _clipboard = PreservedClipboard::save();
        if secure_input_would_mask_this_test() {
            return Ok(());
        }
        let permissions = FakePermissions::new(PermissionState::NotDetermined);
        let injector = MacosInjector::new(permissions);

        let outcome = injector.deliver(&InjectionRequest {
            text: "clipboard only, please".into(),
            auto_paste: false,
            restore_clipboard: false,
            paste_delay_ms: 0,
            clipboard_restore_delay_ms: 0,
        })?;

        assert_eq!(outcome.delivery, DeliveryKind::ClipboardOnly);
        assert_eq!(
            injector.permissions.times_asked(),
            0,
            "never ask for Accessibility when the user has turned off the thing that needs it"
        );
        assert!(
            outcome.reason.is_none(),
            "this is the user's choice, not a degraded outcome to explain away"
        );
        Ok(())
    }

    /**
     * WHAT:  `can_inject` must stay side-effect free.
     * WHY:   It is a report, called to decide what the UI shows. If it asked,
     *        merely rendering a settings pane would put a system dialog on
     *        screen — the same mistake as checking a permission by opening the
     *        device, in the opposite direction.
     */
    #[test]
    fn asking_what_is_possible_never_prompts() {
        let permissions = FakePermissions::new(PermissionState::NotDetermined);
        let injector = MacosInjector::new(permissions);

        let _ = injector.can_inject();

        assert_eq!(
            injector.permissions.times_asked(),
            0,
            "can_inject is a report and must never raise a dialog"
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: the_registry_defaults_match_the_adapter_defaults
     * WHAT:  The registry's declared defaults for the two paste delays are the
     *        same numbers `PasteTiming::default()` uses.
     * WHY:   There are now two ways to get a timing — the settings the user
     *        sees, and this struct's Default — and two sources for one value
     *        drift. The drift would be invisible: pastes would land stale in
     *        some apps for users who never opened the advanced settings, and
     *        the fix that "works" for them would be a number the registry
     *        disagrees with.
     *
     *        Named in the PasteTiming doc comment. If this test is deleted, that
     *        comment becomes a claim about a guarantee that no longer exists —
     *        which is precisely how both of this file's worst bugs got in.
     */
    #[test]
    fn the_registry_defaults_match_the_adapter_defaults() {
        use crate::registry::{self, keys};
        use crate::types::SettingValue;

        let declared = |key: &str| match registry::setting_def(key).map(|d| d.default.clone()) {
            Some(SettingValue::Number(ms)) => ms as u64,
            other => panic!("`{key}` must be declared as a Number, got {other:?}"),
        };

        let defaults = PasteTiming::default();
        assert_eq!(
            declared(keys::PASTE_DELAY_MS),
            defaults.before_paste.as_millis() as u64,
            "the paste delay the user is shown is not the one the adapter falls back to"
        );
        assert_eq!(
            declared(keys::CLIPBOARD_RESTORE_DELAY_MS),
            defaults.before_restore.as_millis() as u64,
            "the restore delay the user is shown is not the one the adapter falls back to"
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: paste_timing_comes_from_the_request
     * WHAT:  The delays actually used are the ones on the request.
     * WHY:   The regression this replaces: the injector held its own
     *        PasteTiming and the only way to change it was a builder nothing
     *        ever called, so both settings rendered, saved, and did nothing.
     *        Asserting the plumbing rather than the sleep, because a test that
     *        measures elapsed wall-clock asserts the machine, not the product.
     */
    #[test]
    fn the_paste_timing_comes_from_the_request() {
        let request = InjectionRequest {
            text: "timing".into(),
            auto_paste: true,
            restore_clipboard: true,
            paste_delay_ms: 75,
            clipboard_restore_delay_ms: 320,
        };

        let timing = PasteTiming::from_request(&request);
        assert_eq!(timing.before_paste, Duration::from_millis(75));
        assert_eq!(timing.before_restore, Duration::from_millis(320));
    }

    #[test]
    fn default_timing_covers_both_races() {
        // Values below these have been observed to paste stale content or to
        // clobber a paste in flight. Guards against someone "optimising" them
        // to zero to shave latency.
        let timing = PasteTiming::default();
        assert!(timing.before_paste >= Duration::from_millis(30));
        assert!(timing.before_restore >= Duration::from_millis(100));
    }

    #[test]
    fn without_accessibility_delivery_degrades_instead_of_failing() -> AppResult<()> {
        let _guard = PASTEBOARD_LOCK.lock();
        let _clipboard = PreservedClipboard::save();
        let injector = MacosInjector::new(FakePermissions::new(PermissionState::Denied));
        assert!(!injector.can_inject());

        let outcome = injector.deliver(&InjectionRequest {
            text: "hello from a test".into(),
            auto_paste: true,
            restore_clipboard: false,
            // Zero: these tests assert branching, not timing, and the real
            // defaults would add 190ms of sleep to each of them.
            paste_delay_ms: 0,
            clipboard_restore_delay_ms: 0,
        })?;

        // The crucial assertion: this is a SUCCESS with a different delivery,
        // not an Err. The app is fully usable in this state.
        assert_eq!(outcome.delivery, DeliveryKind::ClipboardOnly);
        assert!(outcome.reason.is_some(), "the user must be told why");
        Ok(())
    }

    #[test]
    fn the_text_reaches_the_clipboard_even_when_pasting_is_impossible() -> AppResult<()> {
        let _guard = PASTEBOARD_LOCK.lock();
        let _clipboard = PreservedClipboard::save();
        let injector = MacosInjector::new(FakePermissions::new(PermissionState::Denied));
        let text = "murmur clipboard fallback check";

        injector.deliver(&InjectionRequest {
            text: text.into(),
            auto_paste: true,
            restore_clipboard: false,
            // Zero: these tests assert branching, not timing, and the real
            // defaults would add 190ms of sleep to each of them.
            paste_delay_ms: 0,
            clipboard_restore_delay_ms: 0,
        })?;

        let mut clipboard = MacosInjector::<FakePermissions>::clipboard()?;
        assert_eq!(clipboard.get_text().ok().as_deref(), Some(text));
        Ok(())
    }

    #[test]
    fn secure_input_is_queryable_without_panicking() {
        // Whatever it returns on this machine, the call itself must be safe —
        // it runs on every delivery.
        let _ = secure_input_active();
    }
}
