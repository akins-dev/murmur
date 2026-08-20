/*!
 * SOURCE OF TRUTH KEYWORDS: list_entries, enabled_entries, create_entry,
 *   update_entry, delete_entry, touch_entry, recent_terms
 * WHAT:  Pure SQLite access to the dictionary table.
 * WHY:   `recent_terms` exists because the engine's vocabulary prompt has a
 *        hard token budget. A prompt that overflows is silently truncated by
 *        the engine, which quietly drops whichever terms happened to be last —
 *        so we choose deliberately, most-recently-used first, rather than
 *        letting the cut fall where it may.
 * WHERE: Called by ipc/commands/dictionary.rs and by pipeline/worker.rs when
 *        building a TranscribeRequest.
 */

use rusqlite::{params, Row};

use crate::db::Database;
use crate::error::AppResult;
use crate::types::{DictionaryEntry, DictionaryId, MatchKind};

pub fn list_entries(db: &Database) -> AppResult<Vec<DictionaryEntry>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(&format!("{SELECT_COLUMNS} ORDER BY pattern COLLATE NOCASE"))?;
        let rows = stmt.query_map([], row_to_entry)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}

pub fn enabled_entries(db: &Database) -> AppResult<Vec<DictionaryEntry>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(&format!("{SELECT_COLUMNS} WHERE enabled = 1"))?;
        let rows = stmt.query_map([], row_to_entry)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}

/// Replacement terms, most recently used first, for the engine prompt.
pub fn recent_terms(db: &Database, limit: i64) -> AppResult<Vec<String>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT replacement FROM dictionary
              WHERE enabled = 1
              ORDER BY used_at DESC NULLS LAST, id DESC
              LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    })
}

pub fn create_entry(
    db: &Database,
    pattern: &str,
    replacement: &str,
    match_kind: MatchKind,
    created_at: i64,
) -> AppResult<DictionaryId> {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO dictionary (pattern, replacement, match_kind, enabled, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)
             ON CONFLICT (pattern, match_kind)
             DO UPDATE SET replacement = excluded.replacement, enabled = 1",
            params![pattern, replacement, match_kind.as_str(), created_at],
        )?;
        let id: i64 = conn.query_row(
            "SELECT id FROM dictionary WHERE pattern = ?1 AND match_kind = ?2",
            params![pattern, match_kind.as_str()],
            |row| row.get(0),
        )?;
        Ok(DictionaryId(id))
    })
}

pub fn update_entry(
    db: &Database,
    id: &DictionaryId,
    replacement: &str,
    match_kind: MatchKind,
    enabled: bool,
) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute(
            "UPDATE dictionary SET replacement = ?2, match_kind = ?3, enabled = ?4 WHERE id = ?1",
            params![id.0, replacement, match_kind.as_str(), enabled],
        )?;
        Ok(())
    })
}

pub fn delete_entry(db: &Database, id: &DictionaryId) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute("DELETE FROM dictionary WHERE id = ?1", params![id.0])?;
        Ok(())
    })
}

/// Marks a term as used, so it survives prompt truncation next time.
pub fn touch_entries(db: &Database, patterns: &[String], used_at: i64) -> AppResult<()> {
    if patterns.is_empty() {
        return Ok(());
    }
    db.with_connection_mut(|conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt =
                tx.prepare("UPDATE dictionary SET used_at = ?2 WHERE pattern = ?1")?;
            for pattern in patterns {
                stmt.execute(params![pattern, used_at])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

const SELECT_COLUMNS: &str =
    "SELECT id, pattern, replacement, match_kind, enabled, used_at FROM dictionary";

fn row_to_entry(row: &Row<'_>) -> rusqlite::Result<DictionaryEntry> {
    let match_kind: String = row.get(3)?;
    Ok(DictionaryEntry {
        id: DictionaryId(row.get(0)?),
        pattern: row.get(1)?,
        replacement: row.get(2)?,
        // An unknown kind falls back to the safe one rather than the powerful
        // one: Substring can corrupt unrelated words, Word cannot.
        match_kind: MatchKind::from_stored(&match_kind).unwrap_or(MatchKind::Word),
        enabled: row.get(4)?,
        used_at: row.get(5)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creating_the_same_pattern_twice_updates_it() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        create_entry(&db, "clod code", "Claude Code", MatchKind::Word, 1)?;
        create_entry(&db, "clod code", "Claude Code!", MatchKind::Word, 2)?;

        let entries = list_entries(&db)?;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].replacement, "Claude Code!");
        Ok(())
    }

    #[test]
    fn recent_terms_prefer_recently_used_entries() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        create_entry(&db, "alpha", "Alpha", MatchKind::Word, 1)?;
        create_entry(&db, "beta", "Beta", MatchKind::Word, 2)?;
        touch_entries(&db, &["beta".to_string()], 500)?;

        let terms = recent_terms(&db, 10)?;
        assert_eq!(terms.first().map(String::as_str), Some("Beta"));
        Ok(())
    }

    #[test]
    fn disabled_entries_are_excluded_from_the_active_set() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        let id = create_entry(&db, "alpha", "Alpha", MatchKind::Word, 1)?;
        update_entry(&db, &id, "Alpha", MatchKind::Word, false)?;

        assert_eq!(list_entries(&db)?.len(), 1);
        assert_eq!(enabled_entries(&db)?.len(), 0);
        assert_eq!(recent_terms(&db, 10)?.len(), 0);
        Ok(())
    }
}
