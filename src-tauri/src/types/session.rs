/*!
 * SOURCE OF TRUTH KEYWORDS: SessionId, SessionState, SessionOutcome,
 *   DeliveryKind, RecordingMode, SessionSummary, CancelPending, Finalizing,
 *   Delivered, is_capturing, is_terminal
 * WHAT:  The vocabulary of one recording session — its identity, its live
 *        state, how it ended, and the row shape History reads back.
 * WHY:   Live state and persisted outcome are deliberately two different types.
 *        The FSM has states that must never reach the database (Arming,
 *        CancelPending) and the database has an outcome the FSM never produces
 *        (Orphaned, written by crash recovery, not by a transition). Collapsing
 *        them into one enum is what leads to a row saved mid-transition that no
 *        longer means anything on the next launch.
 * WHERE: SessionState is owned by session/machine.rs and pushed to the pill as
 *        a typed event; SessionSummary is produced by services/sessions.rs and
 *        consumed by the History view.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

use super::numeric::TsNumber;

/// Correlation id shared by the database row, the tracing span, and the logs,
/// so one string ties a user report to everything that happened.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct SessionId(pub String);

impl SessionId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: SessionState
 * WHAT:  The live state of the one session that may exist at a time.
 * WHY:   Modelled as data-carrying variants so illegal states are
 *        unrepresentable — there is no way to be in CancelPending without a
 *        remaining time, and no way to be Failed without a reason.
 * WHERE: session/machine.rs owns every transition; the pill renders this and
 *        holds no state of its own.
 */
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
/*
 * SOURCE OF TRUTH KEYWORDS: SessionState, capture_only, no_finalizing_state
 * NOTE ON WHAT IS DELIBERATELY ABSENT: there is no Finalizing and no Delivered.
 * This enum describes CAPTURE — the part of a session a human is waiting
 * through — and nothing else. Transcribing and pasting happen after it, on
 * their own schedule, and the user is not held there while they do.
 *
 * They used to be here, and binding them to capture is what made the app feel
 * slow: releasing the hotkey left the pill on screen for as long as the model
 * took, then announced a word count nobody asked for, and refused to start the
 * next recording until it was all finished. The machine's timescale was
 * imposed on the person's.
 */
pub enum SessionState {
    /// Nothing is happening. The pill is not on screen.
    Idle,
    /// Permission preflight passed, the device is opening. Sub-100ms normally.
    Arming,
    Recording {
        #[specta(type = TsNumber)]
        elapsed_ms: u64,
    },
    /// Escape armed. Audio is STILL being captured — nothing is torn down —
    /// which is what makes a second Escape resume with no gap.
    CancelPending {
        #[specta(type = TsNumber)]
        elapsed_ms: u64,
        #[specta(type = TsNumber)]
        remaining_ms: u64,
    },
    Failed {
        code: crate::error::ErrorCode,
        message: String,
    },
}

impl SessionState {
    /// True while audio is being captured. Both Recording and CancelPending
    /// qualify — that is the whole point of the cancel countdown.
    pub fn is_capturing(&self) -> bool {
        matches!(
            self,
            SessionState::Recording { .. } | SessionState::CancelPending { .. }
        )
    }

    /// True once the session can no longer change without a new hotkey press.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            SessionState::Idle | SessionState::Failed { .. }
        )
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: SessionOutcome
 * WHAT:  How a session ended, as persisted. Cancelled sessions are absent by
 *        design — they are deleted, not marked.
 * WHY:   Orphaned exists only because a process can die mid-recording; it is
 *        written by recovery on the next launch, never by a transition.
 * WHERE: The `outcome` column on sessions; filtered by the History view.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum SessionOutcome {
    Delivered,
    Failed,
    Orphaned,
}

impl SessionOutcome {
    pub fn as_str(&self) -> &'static str {
        match self {
            SessionOutcome::Delivered => "delivered",
            SessionOutcome::Failed => "failed",
            SessionOutcome::Orphaned => "orphaned",
        }
    }

    /// Parses the value as stored in the database. Named to avoid shadowing
    /// `std::str::FromStr`, which carries different expectations.
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "delivered" => Some(SessionOutcome::Delivered),
            "failed" => Some(SessionOutcome::Failed),
            "orphaned" => Some(SessionOutcome::Orphaned),
            _ => None,
        }
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: DeliveryKind
 * WHAT:  Where the finished text actually landed.
 * WHY:   ClipboardOnly is a SUCCESS, not a failure — the app is fully useful
 *        without Accessibility permission, and treating the fallback as an
 *        error would nag a user whose setup is working as intended.
 * WHERE: Set by pipeline/deliver.rs; shown as a badge in History.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum DeliveryKind {
    /// Clipboard written and synthetic paste accepted.
    Pasted,
    /// Clipboard written; paste skipped or refused. Still a success.
    ClipboardOnly,
    /// Nothing was delivered.
    None,
}

impl DeliveryKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            DeliveryKind::Pasted => "pasted",
            DeliveryKind::ClipboardOnly => "clipboard_only",
            DeliveryKind::None => "none",
        }
    }

    /// Parses the value as stored in the database. Named to avoid shadowing
    /// `std::str::FromStr`, which carries different expectations.
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "pasted" => Some(DeliveryKind::Pasted),
            "clipboard_only" => Some(DeliveryKind::ClipboardOnly),
            "none" => Some(DeliveryKind::None),
            _ => None,
        }
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: RecordingMode
 * WHAT:  Whether the hotkey toggles recording or holds it.
 * WHY:   Push-to-talk changes the meaning of the key-up event, so it has to be
 *        known at session start rather than inferred later.
 * WHERE: Chosen in Settings, passed on the start command, read by the hotkey
 *        handler.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RecordingMode {
    Toggle,
    PushToTalk,
}

/**
 * SOURCE OF TRUTH KEYWORDS: SessionSummary
 * WHAT:  One persisted session as History and Stats read it.
 * WHY:   `raw_text` sits beside `final_text` so a bad result can be attributed
 *        to the model or to our own enhancement rules — without it that is
 *        guesswork.
 * WHERE: Produced by services/sessions.rs; consumed by the History view.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct SessionSummary {
    pub id: SessionId,
    #[specta(type = TsNumber)]
    pub started_at_ms: i64,
    #[specta(type = Option<TsNumber>)]
    pub ended_at_ms: Option<i64>,
    pub outcome: SessionOutcome,
    #[specta(type = Option<TsNumber>)]
    pub duration_ms: Option<i64>,
    pub language: Option<String>,
    pub engine_id: String,
    pub model_id: String,
    pub raw_text: Option<String>,
    pub final_text: Option<String>,
    #[specta(type = Option<TsNumber>)]
    pub word_count: Option<i64>,
    pub app_bundle_id: Option<String>,
    pub delivery: DeliveryKind,
    pub error_code: Option<String>,
    /// The plain-language reason, written once where the failure happened.
    /// History renders this verbatim rather than mapping the code to its own
    /// wording — see migration 002.
    pub error_message: Option<String>,
}
