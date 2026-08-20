/*!
 * SOURCE OF TRUTH KEYWORDS: get_setting, set_setting, get_all_settings,
 *   reset_setting, delete_all_settings, SettingRow
 * WHAT:  Pure SQLite access to the settings table. Values are JSON-encoded
 *        SettingValue.
 * WHY:   This layer stores and retrieves; it does NOT decide what a valid value
 *        is. Validation against the registry's declared control kind is a
 *        business rule and lives in the command layer, so that a migration or a
 *        recovery path can still write a value this service would otherwise
 *        have opinions about.
 *
 *        A missing row is not an error. Every setting has a registry default,
 *        and returning None here lets the caller fall back to it — which is
 *        also what makes adding a new setting a no-op for existing installs.
 * WHERE: Called by ipc/commands/settings.rs and by anything reading config at
 *        runtime.
 */

use rusqlite::{params, Row};
use std::collections::HashMap;

use crate::db::Database;
use crate::error::AppResult;
use crate::types::SettingValue;

/// None means "never written" — the caller uses the registry default.
pub fn get_setting(db: &Database, key: &str) -> AppResult<Option<SettingValue>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        match rows.next()? {
            Some(row) => {
                let raw: String = row.get(0)?;
                Ok(Some(serde_json::from_str(&raw)?))
            }
            None => Ok(None),
        }
    })
}

pub fn set_setting(
    db: &Database,
    key: &str,
    value: &SettingValue,
    updated_at: i64,
) -> AppResult<()> {
    let encoded = serde_json::to_string(value)?;
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, encoded, updated_at],
        )?;
        Ok(())
    })
}

/// Every value that has actually been written. Sparse by design.
pub fn get_all_settings(db: &Database) -> AppResult<HashMap<String, SettingValue>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare("SELECT key, value FROM settings")?;
        let rows = stmt.query_map([], |row: &Row<'_>| {
            let key: String = row.get(0)?;
            let raw: String = row.get(1)?;
            Ok((key, raw))
        })?;

        let mut out = HashMap::new();
        for row in rows {
            let (key, raw) = row?;
            // A value that will not parse was written by a newer build or has
            // been corrupted. Skipping it falls back to the registry default,
            // which is strictly better than refusing to load settings at all.
            match serde_json::from_str(&raw) {
                Ok(value) => {
                    out.insert(key, value);
                }
                Err(err) => tracing::warn!(key, error = %err, "ignoring unreadable setting"),
            }
        }
        Ok(out)
    })
}

/// Removes the override so the registry default applies again.
pub fn reset_setting(db: &Database, key: &str) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    })
}

pub fn delete_all_settings(db: &Database) -> AppResult<u64> {
    db.with_connection(|conn| {
        let removed = conn.execute("DELETE FROM settings", [])?;
        Ok(removed as u64)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unwritten_setting_reads_as_none() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        assert!(get_setting(&db, "general.baseline_wpm")?.is_none());
        Ok(())
    }

    #[test]
    fn writing_twice_updates_rather_than_duplicating() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        set_setting(&db, "output.auto_paste", &SettingValue::Bool(true), 1)?;
        set_setting(&db, "output.auto_paste", &SettingValue::Bool(false), 2)?;

        assert_eq!(
            get_setting(&db, "output.auto_paste")?,
            Some(SettingValue::Bool(false))
        );
        assert_eq!(get_all_settings(&db)?.len(), 1);
        Ok(())
    }

    #[test]
    fn reset_restores_the_absent_state() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        set_setting(&db, "output.auto_paste", &SettingValue::Bool(false), 1)?;
        reset_setting(&db, "output.auto_paste")?;
        assert!(get_setting(&db, "output.auto_paste")?.is_none());
        Ok(())
    }
}
