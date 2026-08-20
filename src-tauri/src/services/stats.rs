/*!
 * SOURCE OF TRUTH KEYWORDS: totals, activity_by_day, language_breakdown,
 *   streak_days, Totals, since_timestamp
 * WHAT:  Read-only aggregate queries over sessions. The raw numbers the Stats
 *        view is built from.
 * WHY:   Every function returns measured counts and nothing derived — time
 *        saved, words per minute and the comparison against a typing baseline
 *        are business rules, and they belong in the command layer where the
 *        user's configured baseline is known. Keeping the derivation out of SQL
 *        also means the honest number and the flattering number cannot diverge.
 *
 *        Aggregation is restricted to delivered sessions. Counting a failed or
 *        orphaned session toward "words dictated" would inflate the one number
 *        the product uses to prove it works.
 * WHERE: Called by ipc/commands/stats.rs, which assembles StatsSummary.
 */

use rusqlite::Row;

use crate::db::Database;
use crate::error::AppResult;
use crate::types::{ActivityDay, LanguageCount, SessionOutcome};

/**
 * SOURCE OF TRUTH KEYWORDS: Totals
 * WHAT:  Lifetime and windowed counts over delivered sessions.
 * WHERE: Produced by `totals`; folded into StatsSummary by the command layer.
 */
#[derive(Debug, Clone, Default)]
pub struct Totals {
    pub session_count: i64,
    pub word_count: i64,
    pub speaking_ms: i64,
}

/// Lifetime totals, delivered sessions only.
pub fn totals(db: &Database) -> AppResult<Totals> {
    db.with_connection(|conn| {
        conn.query_row(
            "SELECT count(*), coalesce(sum(word_count), 0), coalesce(sum(duration_ms), 0)
               FROM sessions
              WHERE outcome = ?1",
            [SessionOutcome::Delivered.as_str()],
            |row| {
                Ok(Totals {
                    session_count: row.get(0)?,
                    word_count: row.get(1)?,
                    speaking_ms: row.get(2)?,
                })
            },
        )
        .map_err(Into::into)
    })
}

/// Totals for sessions started at or after `since_ms`.
pub fn totals_since(db: &Database, since_ms: i64) -> AppResult<Totals> {
    db.with_connection(|conn| {
        conn.query_row(
            "SELECT count(*), coalesce(sum(word_count), 0), coalesce(sum(duration_ms), 0)
               FROM sessions
              WHERE outcome = ?1 AND started_at >= ?2",
            rusqlite::params![SessionOutcome::Delivered.as_str(), since_ms],
            |row| {
                Ok(Totals {
                    session_count: row.get(0)?,
                    word_count: row.get(1)?,
                    speaking_ms: row.get(2)?,
                })
            },
        )
        .map_err(Into::into)
    })
}

/**
 * SOURCE OF TRUTH KEYWORDS: activity_by_day
 * WHAT:  Sessions and words per LOCAL calendar day, for the heatmap.
 * WHY:   Grouped with SQLite's 'localtime' modifier rather than by UTC day. A
 *        heatmap bucketed in UTC puts an evening session on the wrong square
 *        for anyone west of Greenwich, which is visible and wrong.
 * WHERE: The activity heatmap on the Stats view.
 */
pub fn activity_by_day(db: &Database, since_ms: i64) -> AppResult<Vec<ActivityDay>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT date(started_at / 1000, 'unixepoch', 'localtime') AS day,
                    count(*),
                    coalesce(sum(word_count), 0)
               FROM sessions
              WHERE outcome = ?1 AND started_at >= ?2
              GROUP BY day
              ORDER BY day",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![SessionOutcome::Delivered.as_str(), since_ms],
            |row: &Row<'_>| {
                Ok(ActivityDay {
                    date: row.get(0)?,
                    session_count: row.get(1)?,
                    word_count: row.get(2)?,
                })
            },
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}

pub fn language_breakdown(db: &Database) -> AppResult<Vec<LanguageCount>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT coalesce(language, 'unknown'), count(*)
               FROM sessions
              WHERE outcome = ?1
              GROUP BY language
              ORDER BY count(*) DESC",
        )?;
        let rows = stmt.query_map([SessionOutcome::Delivered.as_str()], |row: &Row<'_>| {
            Ok(LanguageCount {
                language: row.get(0)?,
                session_count: row.get(1)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}

/**
 * SOURCE OF TRUTH KEYWORDS: distinct_active_days
 * WHAT:  The local dates on which anything was delivered, newest first.
 * WHY:   Returns the dates rather than a streak number, because "what counts as
 *        an unbroken streak" is a product decision — whether today's absence
 *        breaks it before the day is over, for instance — and product decisions
 *        do not belong in the service layer.
 * WHERE: The command layer folds this into `current_streak_days`.
 */
pub fn distinct_active_days(db: &Database, limit: i64) -> AppResult<Vec<String>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT date(started_at / 1000, 'unixepoch', 'localtime') AS day
               FROM sessions
              WHERE outcome = ?1
              ORDER BY day DESC
              LIMIT ?2",
        )?;
        let rows = stmt.query_map(
            rusqlite::params![SessionOutcome::Delivered.as_str(), limit],
            |row| row.get::<_, String>(0),
        )?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::sessions::{create_session, finalize_session, NewSession, SessionResult};
    use crate::types::{DeliveryKind, SessionId};

    fn deliver(db: &Database, id: &str, started_at: i64, words: i64) -> AppResult<()> {
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
        finalize_session(
            db,
            &session_id,
            &SessionResult {
                ended_at: started_at + 1000,
                outcome: SessionOutcome::Delivered,
                duration_ms: 1000,
                language: Some("en".into()),
                raw_text: None,
                final_text: Some("some words".into()),
                word_count: Some(words),
                delivery: DeliveryKind::Pasted,
                error_code: None,
                error_message: None,
            },
        )
    }

    #[test]
    fn totals_count_only_delivered_sessions() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        deliver(&db, "delivered", 1_000, 10)?;

        // An orphan — created but never finalized — must not inflate the count.
        create_session(
            &db,
            &NewSession {
                id: SessionId("orphan".into()),
                started_at: 2_000,
                engine_id: "whisper".into(),
                model_id: "turbo".into(),
                app_bundle_id: None,
            },
        )?;

        let totals = totals(&db)?;
        assert_eq!(totals.session_count, 1);
        assert_eq!(totals.word_count, 10);
        Ok(())
    }

    #[test]
    fn language_breakdown_groups_by_language() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        deliver(&db, "a", 1_000, 5)?;
        deliver(&db, "b", 2_000, 5)?;

        let breakdown = language_breakdown(&db)?;
        assert_eq!(breakdown.len(), 1);
        assert_eq!(breakdown[0].session_count, 2);
        Ok(())
    }

    #[test]
    fn activity_buckets_sessions_by_day() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        deliver(&db, "a", 1_700_000_000_000, 5)?;
        deliver(&db, "b", 1_700_000_060_000, 5)?;

        let activity = activity_by_day(&db, 0)?;
        assert_eq!(activity.len(), 1, "two sessions a minute apart are one day");
        assert_eq!(activity[0].session_count, 2);
        assert_eq!(activity[0].word_count, 10);
        Ok(())
    }
}
