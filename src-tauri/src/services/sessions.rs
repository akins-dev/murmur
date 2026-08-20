/*!
 * SOURCE OF TRUTH KEYWORDS: create_session, finalize_session, get_session,
 *   list_sessions, search_sessions, delete_session, delete_all_sessions,
 *   find_orphans, mark_orphaned, purge_older_than, NewSession, row_to_summary
 * WHAT:  Pure SQLite access to the sessions table. One verb per function.
 * WHY:   No business rules live here — no deciding what an outcome means, no
 *        calling another service, no orchestration. That is what keeps these
 *        functions reusable by the FSM, by crash recovery and by the History
 *        view without any of them inheriting the others' assumptions.
 *
 *        `create_session` is called BEFORE any audio is transcribed. That is
 *        deliberate and it is the whole basis of crash recovery: a row with no
 *        `ended_at` on the next launch is a session the process died during,
 *        and it can still be finished.
 * WHERE: Called by session/machine.rs on every transition, by session/recovery.rs
 *        at startup, and by ipc/commands/history.rs.
 */

use rusqlite::{params, Connection, Row};

use crate::db::Database;
use crate::error::AppResult;
use crate::types::{DeliveryKind, SessionId, SessionOutcome, SessionSummary};

/**
 * SOURCE OF TRUTH KEYWORDS: NewSession
 * WHAT:  The columns known at the moment a recording starts.
 * WHY:   Separate from SessionSummary because most of that shape does not exist
 *        yet — modelling it as one optional-heavy struct would let a caller
 *        create a row that claims to be finished.
 * WHERE: Built by session/machine.rs on the transition into Recording.
 */
#[derive(Debug, Clone)]
pub struct NewSession {
    pub id: SessionId,
    pub started_at: i64,
    pub engine_id: String,
    pub model_id: String,
    pub app_bundle_id: Option<String>,
}

/// Writes the in-flight row. Outcome starts as `orphaned` so that a crash
/// before finalize leaves an accurate record rather than a false success.
pub fn create_session(db: &Database, session: &NewSession) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO sessions (id, started_at, outcome, engine_id, model_id, app_bundle_id, delivery)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                session.id.as_str(),
                session.started_at,
                SessionOutcome::Orphaned.as_str(),
                session.engine_id,
                session.model_id,
                session.app_bundle_id,
                DeliveryKind::None.as_str(),
            ],
        )?;
        Ok(())
    })
}

/// The columns that only exist once a session has ended.
#[derive(Debug, Clone)]
pub struct SessionResult {
    pub ended_at: i64,
    pub outcome: SessionOutcome,
    pub duration_ms: i64,
    pub language: Option<String>,
    pub raw_text: Option<String>,
    pub final_text: Option<String>,
    pub word_count: Option<i64>,
    pub delivery: DeliveryKind,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

pub fn finalize_session(
    db: &Database,
    id: &SessionId,
    result: &SessionResult,
) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE sessions
                SET ended_at = ?2, outcome = ?3, duration_ms = ?4, language = ?5,
                    raw_text = ?6, final_text = ?7, word_count = ?8,
                    delivery = ?9, error_code = ?10, error_message = ?11
              WHERE id = ?1",
            params![
                id.as_str(),
                result.ended_at,
                result.outcome.as_str(),
                result.duration_ms,
                result.language,
                result.raw_text,
                result.final_text,
                result.word_count,
                result.delivery.as_str(),
                result.error_code,
                result.error_message,
            ],
        )?;
        Ok(())
    })
}

pub fn get_session(db: &Database, id: &SessionId) -> AppResult<Option<SessionSummary>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(&format!("{SELECT_COLUMNS} WHERE id = ?1"))?;
        let mut rows = stmt.query(params![id.as_str()])?;
        match rows.next()? {
            Some(row) => Ok(Some(row_to_summary(row)?)),
            None => Ok(None),
        }
    })
}

/// Newest first. Cancelled sessions do not exist, so no filter is needed for
/// them; orphans and failures DO appear, which is the point of history.
pub fn list_sessions(db: &Database, limit: i64, offset: i64) -> AppResult<Vec<SessionSummary>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(&format!(
            "{SELECT_COLUMNS} ORDER BY started_at DESC LIMIT ?1 OFFSET ?2"
        ))?;
        let rows = stmt.query_map(params![limit, offset], row_to_summary)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}

/**
 * SOURCE OF TRUTH KEYWORDS: search_sessions
 * WHAT:  Full-text search over final transcripts, newest first.
 * WHY:   Goes through the FTS index rather than a LIKE scan, which stops being
 *        usable somewhere in the low thousands of rows — and this table is
 *        expected to reach tens of thousands.
 * WHERE: The History view's search box.
 */
pub fn search_sessions(db: &Database, query: &str, limit: i64) -> AppResult<Vec<SessionSummary>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(&format!(
            "{SELECT_COLUMNS}
             WHERE rowid IN (SELECT rowid FROM sessions_fts WHERE sessions_fts MATCH ?1)
             ORDER BY started_at DESC
             LIMIT ?2"
        ))?;
        let rows = stmt.query_map(params![query, limit], row_to_summary)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}

pub fn delete_session(db: &Database, id: &SessionId) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute("DELETE FROM sessions WHERE id = ?1", params![id.as_str()])?;
        Ok(())
    })
}

pub fn delete_all_sessions(db: &Database) -> AppResult<u64> {
    db.with_connection(|conn| {
        let removed = conn.execute("DELETE FROM sessions", [])?;
        Ok(removed as u64)
    })
}

/// Sessions that were never finalized — the process died mid-recording.
pub fn find_orphans(db: &Database) -> AppResult<Vec<SessionSummary>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(&format!(
            "{SELECT_COLUMNS} WHERE ended_at IS NULL ORDER BY started_at DESC"
        ))?;
        let rows = stmt.query_map([], row_to_summary)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}

/// Closes out an orphan that cannot be recovered, so it stops being reported.
pub fn mark_orphaned(db: &Database, id: &SessionId, ended_at: i64) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE sessions SET ended_at = ?2, outcome = ?3 WHERE id = ?1 AND ended_at IS NULL",
            params![id.as_str(), ended_at, SessionOutcome::Orphaned.as_str()],
        )?;
        Ok(())
    })
}

/// Retention policy. Takes an absolute cutoff — computing it from a retention
/// setting is a business rule and belongs to the caller.
pub fn purge_older_than(db: &Database, cutoff_ms: i64) -> AppResult<u64> {
    db.with_connection(|conn| {
        let removed = conn.execute(
            "DELETE FROM sessions WHERE started_at < ?1",
            params![cutoff_ms],
        )?;
        Ok(removed as u64)
    })
}

pub fn count_sessions(db: &Database) -> AppResult<i64> {
    db.with_connection(|conn| {
        let count: i64 = conn.query_row("SELECT count(*) FROM sessions", [], |row| row.get(0))?;
        Ok(count)
    })
}

// ── Row mapping ──────────────────────────────────────────────────────────

/// Column order here is the contract `row_to_summary` reads by index.
const SELECT_COLUMNS: &str = "SELECT id, started_at, ended_at, outcome, duration_ms, language, \
     engine_id, model_id, raw_text, final_text, word_count, app_bundle_id, delivery, error_code, \
     error_message FROM sessions";

fn row_to_summary(row: &Row<'_>) -> rusqlite::Result<SessionSummary> {
    let outcome: String = row.get(3)?;
    let delivery: String = row.get(12)?;

    Ok(SessionSummary {
        id: SessionId(row.get(0)?),
        started_at_ms: row.get(1)?,
        ended_at_ms: row.get(2)?,
        // An unrecognised value means the file was written by a newer build.
        // Treating it as Orphaned is the honest reading: we cannot vouch for it.
        outcome: SessionOutcome::from_stored(&outcome).unwrap_or(SessionOutcome::Orphaned),
        duration_ms: row.get(4)?,
        language: row.get(5)?,
        engine_id: row.get(6)?,
        model_id: row.get(7)?,
        raw_text: row.get(8)?,
        final_text: row.get(9)?,
        word_count: row.get(10)?,
        app_bundle_id: row.get(11)?,
        delivery: DeliveryKind::from_stored(&delivery).unwrap_or(DeliveryKind::None),
        error_code: row.get(13)?,
        error_message: row.get(14)?,
    })
}

/// Used by tests and by callers that already hold a connection.
#[allow(dead_code)]
fn row_count(conn: &Connection, table: &str) -> rusqlite::Result<i64> {
    conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed(db: &Database, id: &str, started_at: i64) -> AppResult<SessionId> {
        let session_id = SessionId(id.to_string());
        create_session(
            db,
            &NewSession {
                id: session_id.clone(),
                started_at,
                engine_id: "whisper".into(),
                model_id: "turbo".into(),
                app_bundle_id: None,
            },
        )?;
        Ok(session_id)
    }

    #[test]
    fn a_new_session_is_an_orphan_until_finalized() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        let id = seed(&db, "s1", 1000)?;

        // This is the crash-recovery guarantee: the row exists and is findable
        // as an orphan from the moment audio starts.
        let orphans = find_orphans(&db)?;
        assert_eq!(orphans.len(), 1);
        assert_eq!(orphans[0].id, id);

        finalize_session(
            &db,
            &id,
            &SessionResult {
                ended_at: 2000,
                outcome: SessionOutcome::Delivered,
                duration_ms: 1000,
                language: Some("en".into()),
                raw_text: Some("hello there".into()),
                final_text: Some("Hello there.".into()),
                word_count: Some(2),
                delivery: DeliveryKind::Pasted,
                error_code: None,
                error_message: None,
            },
        )?;

        assert!(find_orphans(&db)?.is_empty());
        let stored = get_session(&db, &id)?.expect("session should exist");
        assert_eq!(stored.outcome, SessionOutcome::Delivered);
        assert_eq!(stored.delivery, DeliveryKind::Pasted);
        Ok(())
    }

    #[test]
    fn search_finds_a_finalized_transcript() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        let id = seed(&db, "s1", 1000)?;
        finalize_session(
            &db,
            &id,
            &SessionResult {
                ended_at: 2000,
                outcome: SessionOutcome::Delivered,
                duration_ms: 1000,
                language: Some("en".into()),
                raw_text: None,
                final_text: Some("the quick brown fox".into()),
                word_count: Some(4),
                delivery: DeliveryKind::Pasted,
                error_code: None,
                error_message: None,
            },
        )?;

        assert_eq!(search_sessions(&db, "brown", 10)?.len(), 1);
        assert_eq!(search_sessions(&db, "elephant", 10)?.len(), 0);
        Ok(())
    }

    #[test]
    fn deleting_a_session_takes_its_metrics_with_it() -> AppResult<()> {
        // Cancellation depends on this: one DELETE has to leave nothing behind.
        let db = Database::open_in_memory()?;
        let id = seed(&db, "s1", 1000)?;

        db.with_connection(|conn| {
            conn.execute(
                "INSERT INTO session_metrics (session_id, stage, duration_ms, recorded_at)
                 VALUES (?1, 'tail_decode', 120.0, 1500)",
                params![id.as_str()],
            )?;
            Ok(())
        })?;

        delete_session(&db, &id)?;

        db.with_connection(|conn| {
            assert_eq!(row_count(conn, "session_metrics")?, 0);
            assert_eq!(row_count(conn, "sessions")?, 0);
            Ok(())
        })
    }

    #[test]
    fn purge_removes_only_rows_older_than_the_cutoff() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        seed(&db, "old", 1_000)?;
        seed(&db, "new", 9_000)?;

        assert_eq!(purge_older_than(&db, 5_000)?, 1);
        assert_eq!(count_sessions(&db)?, 1);
        Ok(())
    }
}
