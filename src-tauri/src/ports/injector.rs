/*!
 * SOURCE OF TRUTH KEYWORDS: TextInjector, InjectionOutcome, InjectionRequest,
 *   FrontmostApp, deliver
 * WHAT:  The trait that puts finished text where the user was typing.
 * WHY:   Returns an outcome rather than a bare Result because clipboard-only is
 *        a SUCCESS. The app is fully useful without Accessibility permission,
 *        and modelling the fallback as an error would turn a working setup into
 *        a recurring complaint. Secure Input is the same shape of problem: when
 *        a password field is focused anywhere on the system, synthetic
 *        keystrokes are silently dropped, so the injector has to detect it and
 *        report clipboard-only rather than claim a paste that never landed.
 * WHERE: Implemented by adapters/macos; called by pipeline/deliver.rs.
 */

use crate::error::AppResult;
use crate::types::DeliveryKind;

/**
 * SOURCE OF TRUTH KEYWORDS: FrontmostApp
 * WHAT:  Which application had focus when a session started.
 * WHY:   Drives per-app profiles, and is recorded on the session row so history
 *        can show where a transcript went.
 * WHERE: Sampled at session start by adapters/macos.
 */
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrontmostApp {
    pub bundle_id: String,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct InjectionRequest {
    pub text: String,
    /**
     * Whether to paste into the focused app, or only put the text on the
     * clipboard. This is the user's `output.auto_paste` setting, and it has to
     * travel WITH the request rather than being read by the adapter, because
     * the adapter has no business reading settings.
     *
     * It was missing here for a while, which made the toggle inert: the
     * injector pasted whenever it was able to, so turning auto-paste OFF
     * changed nothing. A setting the app stores, renders, and ignores is worse
     * than no setting at all — the user has told it something and been agreed
     * with, and the behaviour is unchanged.
     */
    pub auto_paste: bool,
    /// Put the previous clipboard contents back after pasting. Defaults on, but
    /// some apps read the clipboard lazily and a restore beats them to it —
    /// which is why it is a setting rather than always-on behaviour.
    pub restore_clipboard: bool,
    /**
     * SOURCE OF TRUTH KEYWORDS: paste_delay_ms, clipboard_restore_delay_ms
     * WHAT:  The two waits around the synthetic paste, in milliseconds. The
     *        user's `output.paste_delay_ms` and
     *        `output.clipboard_restore_delay_ms`.
     * WHY:   Here for the same reason as `auto_paste`, and they were missing
     *        for the same reason: the adapter held its own `PasteTiming` and
     *        the only way to change it was a builder nothing in the app ever
     *        called. Both settings rendered, saved and did nothing — and the
     *        adapter's own doc comment claimed they were "built from settings
     *        by pipeline/deliver.rs", a file that does not exist.
     *
     *        They have to travel WITH the request rather than being read by
     *        the adapter, because these are per-app-profile settings: the
     *        correct paste delay for Terminal is not the correct one for
     *        Electron, which is the entire reason they are adjustable. A value
     *        the adapter reads once at construction cannot vary by the app that
     *        had focus when the hotkey fired.
     */
    pub paste_delay_ms: u64,
    pub clipboard_restore_delay_ms: u64,
}

#[derive(Debug, Clone)]
pub struct InjectionOutcome {
    pub delivery: DeliveryKind,
    /// Set when delivery degraded to clipboard-only, so the UI can say why.
    pub reason: Option<String>,
    /**
     * SOURCE OF TRUTH KEYWORDS: clipboard_write_ms, ClipboardWrite
     * WHAT:  How long the clipboard write itself took, in milliseconds.
     * WHY:   `LatencyStage::ClipboardWrite` is a declared metric with a row on
     *        the dashboard, and nothing recorded it — because the only code that
     *        can see this duration is inside the adapter, and the adapter has no
     *        LatencyRecorder and should not have one. So the adapter MEASURES
     *        and the actor RECORDS.
     *
     *        It is worth measuring separately from Inject: arboard's set_text
     *        can block for tens of milliseconds when another process holds the
     *        pasteboard, and rolled into the paste it looks like a slow paste
     *        rather than a contended clipboard.
     */
    pub clipboard_write_ms: f64,
}

pub trait TextInjector: Send + Sync {
    /// True when the OS will accept synthetic keystrokes from us right now.
    /// False if Accessibility is denied or Secure Input is active anywhere.
    fn can_inject(&self) -> bool;

    fn frontmost_app(&self) -> Option<FrontmostApp>;

    /// Write to the clipboard and, if permitted, paste. Never returns Err for a
    /// missing permission — that is a clipboard-only outcome.
    fn deliver(&self, request: &InjectionRequest) -> AppResult<InjectionOutcome>;
}
