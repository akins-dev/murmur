/*!
 * SOURCE OF TRUTH KEYWORDS: list_history, search_history, get_history_entry,
 *   delete_history_entry, clear_history, purge_history, ListHistoryInput
 * WHAT:  Reading and pruning the local transcript history.
 * WHY:   Search and list are separate commands rather than one with an optional
 *        query, because they hit different indexes and a caller should have to
 *        choose. Retention is applied here rather than in the service, because
 *        "N days ago" is a business rule that needs the user's setting.
 * WHERE: Consumed by the History view.
 */

use tauri::State;

use crate::error::AppError;
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec, Validate};
use crate::registry::CapabilityKey;
use crate::services::sessions;
use crate::telemetry::now_ms;
use crate::types::numeric::TsNumber;
use crate::types::{SessionId, SessionSummary};

/// Bounded so a caller cannot ask for the whole table and stall the UI.
const MAX_PAGE: i64 = 500;

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct ListHistoryInput {
    #[specta(type = TsNumber)]
    pub limit: i64,
    #[specta(type = TsNumber)]
    pub offset: i64,
}

impl Validate for ListHistoryInput {
    fn validate(&self) -> Result<(), String> {
        if self.limit <= 0 || self.limit > MAX_PAGE {
            return Err(format!("Limit must be between 1 and {MAX_PAGE}."));
        }
        if self.offset < 0 {
            return Err("Offset cannot be negative.".into());
        }
        Ok(())
    }
}

const LIST: CommandSpec = CommandSpec::new("list_history", CapabilityKey::History);

#[tauri::command]
#[specta::specta]
pub async fn list_history(
    state: State<'_, AppState>,
    input: ListHistoryInput,
) -> Result<Vec<SessionSummary>, AppError> {
    execute(&state, LIST, input, |ctx, input| async move {
        sessions::list_sessions(ctx.db(), input.limit, input.offset)
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct SearchHistoryInput {
    pub query: String,
    #[specta(type = TsNumber)]
    pub limit: i64,
}

impl Validate for SearchHistoryInput {
    fn validate(&self) -> Result<(), String> {
        if self.query.trim().is_empty() {
            return Err("Enter something to search for.".into());
        }
        if self.limit <= 0 || self.limit > MAX_PAGE {
            return Err(format!("Limit must be between 1 and {MAX_PAGE}."));
        }
        Ok(())
    }
}

const SEARCH: CommandSpec = CommandSpec::new("search_history", CapabilityKey::History);

#[tauri::command]
#[specta::specta]
pub async fn search_history(
    state: State<'_, AppState>,
    input: SearchHistoryInput,
) -> Result<Vec<SessionSummary>, AppError> {
    execute(&state, SEARCH, input, |ctx, input| async move {
        // FTS5 treats bare punctuation as syntax. Quoting the whole phrase
        // makes a user's apostrophe a character to find rather than a parse
        // error they cannot interpret.
        let escaped = format!("\"{}\"", input.query.trim().replace('"', "\"\""));
        sessions::search_sessions(ctx.db(), &escaped, input.limit)
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct SessionIdInput {
    pub id: SessionId,
}

impl Validate for SessionIdInput {
    fn validate(&self) -> Result<(), String> {
        if self.id.as_str().trim().is_empty() {
            return Err("A session id is required.".into());
        }
        Ok(())
    }
}

/**
 * WHAT:  One session by id.
 * WHY:   Returns the summary or a NotFound error, rather than an Option.
 *        Two reasons, and the second is the load-bearing one:
 *          - Asking for an id that does not exist is a real error the UI can
 *            already render, because AppError carries its own presentation.
 *          - specta INLINES `Option<SessionSummary>` in return position while
 *            it references `Vec<SessionSummary>`, so an Option here generated a
 *            second anonymous copy of the shape in TypeScript that had to be
 *            kept in step with the real one by hand. Naming the success type
 *            keeps one definition.
 * WHERE: Opening a single transcript from History.
 */
const GET: CommandSpec = CommandSpec::new("get_history_entry", CapabilityKey::History);

#[tauri::command]
#[specta::specta]
pub async fn get_history_entry(
    state: State<'_, AppState>,
    input: SessionIdInput,
) -> Result<SessionSummary, AppError> {
    execute(&state, GET, input, |ctx, input| async move {
        sessions::get_session(ctx.db(), &input.id)?
            .ok_or_else(|| AppError::not_found("That transcript"))
    })
    .await
}

const DELETE: CommandSpec =
    CommandSpec::new("delete_history_entry", CapabilityKey::History).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn delete_history_entry(
    state: State<'_, AppState>,
    input: SessionIdInput,
) -> Result<(), AppError> {
    execute(&state, DELETE, input, |ctx, input| async move {
        sessions::delete_session(ctx.db(), &input.id)
    })
    .await
}

const CLEAR: CommandSpec = CommandSpec::new("clear_history", CapabilityKey::History).exclusive();

/// The privacy escape hatch. Deletes everything, immediately, with no tombstone.
#[tauri::command]
#[specta::specta]
pub async fn clear_history(state: State<'_, AppState>) -> Result<u32, AppError> {
    execute(&state, CLEAR, (), |ctx, ()| async move {
        Ok(sessions::delete_all_sessions(ctx.db())? as u32)
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct PurgeHistoryInput {
    /// 0 means keep everything forever.
    #[specta(type = TsNumber)]
    pub retention_days: i64,
}

impl Validate for PurgeHistoryInput {
    fn validate(&self) -> Result<(), String> {
        if self.retention_days < 0 {
            return Err("Retention cannot be negative.".into());
        }
        Ok(())
    }
}

const PURGE: CommandSpec = CommandSpec::new("purge_history", CapabilityKey::History).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn purge_history(
    state: State<'_, AppState>,
    input: PurgeHistoryInput,
) -> Result<u32, AppError> {
    execute(&state, PURGE, input, |ctx, input| async move {
        if input.retention_days == 0 {
            return Ok(0);
        }
        let cutoff = now_ms() - input.retention_days * 24 * 60 * 60 * 1000;
        Ok(sessions::purge_older_than(ctx.db(), cutoff)? as u32)
    })
    .await
}

/**
 * SOURCE OF TRUTH KEYWORDS: export_history, ExportHistoryInput, ExportFormat,
 *   to_markdown, to_plaintext
 * WHAT:  Serialises history to JSON, Markdown or plain text and returns the
 *        content for the caller to save.
 * WHY:   It is the user's data and it stays that way — no lock-in was a stated
 *        non-negotiable, and an app that can only show you your transcripts has
 *        quietly become the only place they exist.
 *
 *        Returns the CONTENT rather than writing a file, so the save dialog
 *        stays where the user is and Rust never picks a path on their behalf.
 * WHERE: The export action in History.
 */
#[derive(Debug, Clone, Copy, serde::Deserialize, specta::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ExportFormat {
    Json,
    Markdown,
    PlainText,
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct ExportHistoryInput {
    pub format: ExportFormat,
}

impl Validate for ExportHistoryInput {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

const EXPORT: CommandSpec = CommandSpec::new("export_history", CapabilityKey::History);

#[tauri::command]
#[specta::specta]
pub async fn export_history(
    state: State<'_, AppState>,
    input: ExportHistoryInput,
) -> Result<String, AppError> {
    execute(&state, EXPORT, input, |ctx, input| async move {
        // Exports everything, not a page. A partial export that looks complete
        // is worse than no export at all.
        let total = sessions::count_sessions(ctx.db())?;
        let all = sessions::list_sessions(ctx.db(), total.max(1), 0)?;

        Ok(match input.format {
            ExportFormat::Json => serde_json::to_string_pretty(&all)?,
            ExportFormat::Markdown => to_markdown(&all),
            ExportFormat::PlainText => to_plaintext(&all),
        })
    })
    .await
}

fn to_markdown(sessions: &[SessionSummary]) -> String {
    let mut out = String::from("# Murmur history\n\n");
    for session in sessions {
        out.push_str(&format!(
            "## {}\n\n_{} · {} words_\n\n{}\n\n",
            format_timestamp(session.started_at_ms),
            session.language.as_deref().unwrap_or("unknown"),
            session.word_count.unwrap_or(0),
            session.final_text.as_deref().unwrap_or("(no transcript)")
        ));
    }
    out
}

fn to_plaintext(sessions: &[SessionSummary]) -> String {
    sessions
        .iter()
        .filter_map(|session| session.final_text.as_deref())
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// ISO-8601 in UTC. Deliberately not localised: an export is a data file, and a
/// locale-dependent timestamp in one is a problem for whoever reads it later.
fn format_timestamp(epoch_ms: i64) -> String {
    let secs = epoch_ms / 1000;
    let days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400);
    format!(
        "{} {:02}:{:02}:{:02} UTC",
        crate::ipc::commands::stats::civil_from_days_public(days),
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

#[cfg(test)]
mod export_tests {
    use super::*;
    use crate::types::{DeliveryKind, SessionOutcome};

    fn summary(text: &str) -> SessionSummary {
        SessionSummary {
            id: SessionId("s1".into()),
            started_at_ms: 1_700_000_000_000,
            ended_at_ms: Some(1_700_000_005_000),
            outcome: SessionOutcome::Delivered,
            duration_ms: Some(5000),
            language: Some("en".into()),
            engine_id: "whisper".into(),
            model_id: "turbo".into(),
            raw_text: Some(text.into()),
            final_text: Some(text.into()),
            word_count: Some(text.split_whitespace().count() as i64),
            app_bundle_id: None,
            delivery: DeliveryKind::Pasted,
            error_code: None,
            error_message: None,
        }
    }

    #[test]
    fn plaintext_contains_only_the_transcripts() {
        let out = to_plaintext(&[summary("hello there"), summary("second one")]);
        assert_eq!(out, "hello there\n\nsecond one");
    }

    #[test]
    fn markdown_carries_the_metadata_a_transcript_needs_to_be_useful() {
        let out = to_markdown(&[summary("hello there")]);
        assert!(out.contains("hello there"));
        assert!(out.contains("2 words"));
        assert!(out.contains("en"));
        assert!(out.contains("UTC"));
    }

    #[test]
    fn a_session_with_no_transcript_does_not_break_an_export() {
        let mut empty = summary("x");
        empty.final_text = None;
        assert!(to_markdown(&[empty.clone()]).contains("(no transcript)"));
        assert_eq!(to_plaintext(&[empty]), "");
    }
}
