/*!
 * SOURCE OF TRUTH KEYWORDS: AppError, AppResult, ErrorCode, ErrorAction,
 *   PrivacyPane, recoverable, into_app_error, ErrorSurface
 * WHAT:  The single error type that crosses the IPC boundary, its stable code
 *        enum, and the actionable next step the UI can offer for each one.
 * WHY:   One error surface in the UI instead of forty. Every failure the user
 *        can hit is named by a stable `ErrorCode` the frontend matches on, and
 *        carries `recoverable` plus an optional `action` so the UI can render a
 *        button that actually fixes the problem rather than an apology. macOS
 *        denials in particular are unrecoverable-by-dialog — once a user clicks
 *        "Don't Allow" the system never asks again — so the only useful
 *        response is a deep link into the right settings pane, and that link is
 *        part of the error rather than something each screen reinvents.
 * WHERE: Produced by every layer and mapped by ipc/factory.rs, which is the one
 *        place a handler's error becomes an AppError. Consumed by the frontend
 *        through the generated bindings.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

/// Every fallible path in the app returns this.
pub type AppResult<T> = std::result::Result<T, AppError>;

/**
 * SOURCE OF TRUTH KEYWORDS: ErrorCode
 * WHAT:  The stable, exhaustive set of failure kinds the UI can distinguish.
 * WHY:   The frontend branches on this, never on message text. Adding a variant
 *        is a deliberate act that shows up as a non-exhaustive match in the
 *        TypeScript union, which is exactly the review we want.
 * WHERE: Carried on every AppError; mirrored to TypeScript by specta.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    // ── Permissions ──────────────────────────────────────────────────────
    MicrophoneDenied,
    AccessibilityDenied,
    /// The system dialog has just been put on screen and no answer has come
    /// back yet. Distinct from Denied, which means the user already said no.
    /// See AppError::microphone_pending.
    MicrophonePending,

    // ── Audio ────────────────────────────────────────────────────────────
    AudioDeviceUnavailable,
    AudioDeviceLost,
    /// The device opened and delivered pure silence — the capture path is
    /// broken rather than the room being quiet. See AppError::microphone_silent.
    MicrophoneSilent,
    AudioFormatUnsupported,

    // ── Model / engine ───────────────────────────────────────────────────
    ModelMissing,
    ModelDownloadFailed,
    ModelChecksumMismatch,
    EngineNotReady,
    EngineUnsupportedLanguage,
    TranscriptionFailed,

    // ── Session ──────────────────────────────────────────────────────────
    SessionAlreadyActive,
    SessionNotFound,
    IllegalTransition,
    FinalizeTimeout,

    // ── Delivery ─────────────────────────────────────────────────────────
    SecureInputActive,
    InjectionFailed,
    ClipboardUnavailable,

    // ── Hotkeys ──────────────────────────────────────────────────────────
    HotkeyConflict,
    HotkeyRegistrationFailed,

    // ── Infrastructure ───────────────────────────────────────────────────
    InvalidInput,
    NotFound,
    Database,
    Io,
    Network,
    Internal,
}

/**
 * SOURCE OF TRUTH KEYWORDS: PrivacyPane
 * WHAT:  The macOS System Settings privacy panes we deep-link into.
 * WHY:   A denied TCC permission cannot be re-prompted — the system dialog
 *        appears exactly once per bundle+signature. Deep-linking is the only
 *        recovery, so the pane is modelled rather than stringly-typed.
 * WHERE: Carried by ErrorAction::OpenPrivacyPane; resolved to an
 *        x-apple.systempreferences URL by the macOS adapter.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum PrivacyPane {
    Microphone,
    Accessibility,
}

/**
 * SOURCE OF TRUTH KEYWORDS: ErrorAction
 * WHAT:  The one thing the UI can offer that would actually resolve the error.
 * WHY:   An error the user cannot act on is a dead end. Attaching the remedy to
 *        the error means the recovery affordance is decided once, next to the
 *        failure, instead of being re-derived by every screen that displays it.
 * WHERE: Optional field on AppError; rendered as the button in the error surface.
 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorAction {
    /// Open a macOS privacy pane — the only recovery for a denied TCC grant.
    OpenPrivacyPane { pane: PrivacyPane },
    /// Jump to a section of our own settings, e.g. to rebind a clashing hotkey.
    OpenSettings { section: String },
    /// Offer to (re)download a model by id.
    DownloadModel { model_id: String },
    /// The operation is worth trying again as-is.
    Retry,
}

/**
 * SOURCE OF TRUTH KEYWORDS: AppError, AppResult
 * WHAT:  The only error type permitted to cross IPC.
 * WHY:   `recoverable` separates "this session is lost" from "this attempt
 *        failed" so the pill can decide between a red terminal state and a
 *        retry, without the frontend re-deriving that from the code. `detail`
 *        carries the underlying cause for the log file only — it is never shown
 *        to the user, because a raw sqlite or CoreAudio string is noise to them
 *        and a support burden to us.
 * WHERE: Returned by every command; constructed mostly through the helper
 *        constructors below and by the From impls at the bottom of this file.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct AppError {
    pub code: ErrorCode,
    /// Plain-language, user-facing, no jargon, no error numbers.
    pub message: String,
    /// False means the session or operation is over; true means try again.
    pub recoverable: bool,
    pub action: Option<ErrorAction>,
    /// Underlying cause. Logged, never rendered.
    pub detail: Option<String>,
}

impl AppError {
    /// Base constructor. Prefer the named helpers below where one exists.
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            recoverable: false,
            action: None,
            detail: None,
        }
    }

    pub fn recoverable(mut self) -> Self {
        self.recoverable = true;
        self
    }

    pub fn with_action(mut self, action: ErrorAction) -> Self {
        self.action = Some(action);
        self
    }

    pub fn with_detail(mut self, detail: impl std::fmt::Display) -> Self {
        self.detail = Some(detail.to_string());
        self
    }

    // ── Named constructors for the errors with a fixed remedy ────────────

    pub fn microphone_denied() -> Self {
        Self::new(
            ErrorCode::MicrophoneDenied,
            "Murmur needs microphone access to hear you.",
        )
        .with_action(ErrorAction::OpenPrivacyPane {
            pane: PrivacyPane::Microphone,
        })
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: microphone_pending
     * WHAT:  We have just asked for microphone access; the dialog is on screen.
     * WHY:   Reported SEPARATELY from `microphone_denied`, because the remedy
     *        is the opposite one. A denied grant can only be fixed in System
     *        Settings, so its error deep-links there. A grant that has merely
     *        never been asked for is fixed by answering the dialog that is
     *        already in front of the user — and sending them to System Settings
     *        instead would send them to a pane that does not list Murmur yet.
     * WHERE: Raised by the factory preflight when a required permission was
     *        NotDetermined and has just been requested.
     */
    pub fn microphone_pending() -> Self {
        Self::new(
            ErrorCode::MicrophonePending,
            "Murmur just asked for microphone access. Allow it, then press your shortcut again.",
        )
        .recoverable()
        .with_action(ErrorAction::Retry)
    }

    pub fn accessibility_denied() -> Self {
        Self::new(
            ErrorCode::AccessibilityDenied,
            "Murmur needs Accessibility access to paste for you. Without it your text is still copied to the clipboard.",
        )
        .recoverable()
        .with_action(ErrorAction::OpenPrivacyPane {
            pane: PrivacyPane::Accessibility,
        })
    }

    /// Not an error the user caused, and not one they can fix — but the text
    /// was still delivered to the clipboard, so it is recoverable by design.
    pub fn secure_input_active() -> Self {
        Self::new(
            ErrorCode::SecureInputActive,
            "Text copied — another app is blocking keystrokes, probably a focused password field.",
        )
        .recoverable()
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: microphone_silent
     * WHAT:  The microphone delivered no signal at all for a whole recording.
     * WHY:   Reported SEPARATELY from "no speech detected", because the two
     *        look identical from the outside and need opposite responses. A
     *        quiet user should get nothing and no fuss; a dead capture path
     *        should say so plainly, because the user cannot tell the difference
     *        and will otherwise assume the app simply does not work.
     *
     *        The remedy points at the input device rather than at permissions,
     *        because a denied permission is caught earlier by the preflight —
     *        if we are here, the OS said yes and the audio is still empty.
     * WHERE: Raised by the session actor when a session ends with no text and
     *        the chunker saw pure digital silence.
     */
    pub fn microphone_silent() -> Self {
        Self::new(
            ErrorCode::MicrophoneSilent,
            "Murmur heard nothing at all. Check that the right input device is selected and that it is not muted.",
        )
        .recoverable()
        .with_action(ErrorAction::OpenSettings {
            section: "recording".to_string(),
        })
    }

    pub fn invalid_input(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidInput, message).recoverable()
    }

    pub fn internal(detail: impl std::fmt::Display) -> Self {
        Self::new(
            ErrorCode::Internal,
            "Something went wrong inside Murmur. Your transcript is in History.",
        )
        .with_detail(detail)
    }

    pub fn not_found(what: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::NotFound, format!("{what} was not found.")).recoverable()
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{:?}] {}", self.code, self.message)?;
        if let Some(detail) = &self.detail {
            write!(f, " ({detail})")?;
        }
        Ok(())
    }
}

impl std::error::Error for AppError {}

// ── Conversions from the crates we depend on ─────────────────────────────
//
// These exist so `?` works in service and adapter code without every call site
// hand-writing a mapping. Each one deliberately hides the underlying text
// behind `detail` rather than surfacing it.

impl From<rusqlite::Error> for AppError {
    fn from(err: rusqlite::Error) -> Self {
        Self::new(
            ErrorCode::Database,
            "Murmur could not read its local database.",
        )
        .with_detail(err)
    }
}

impl From<std::io::Error> for AppError {
    fn from(err: std::io::Error) -> Self {
        Self::new(ErrorCode::Io, "A file operation failed.").with_detail(err)
    }
}

impl From<serde_json::Error> for AppError {
    fn from(err: serde_json::Error) -> Self {
        Self::new(ErrorCode::Internal, "Stored data could not be read.").with_detail(err)
    }
}

impl From<reqwest::Error> for AppError {
    fn from(err: reqwest::Error) -> Self {
        Self::new(
            ErrorCode::Network,
            "The download could not be reached. Check your connection and try again.",
        )
        .recoverable()
        .with_action(ErrorAction::Retry)
        .with_detail(err)
    }
}
