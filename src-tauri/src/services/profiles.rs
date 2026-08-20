/*!
 * SOURCE OF TRUTH KEYWORDS: list_profiles, get_profile, upsert_profile,
 *   delete_profile, AppProfile, settings_json
 * WHAT:  Pure SQLite access to per-application setting overrides.
 * WHY:   Stored as a sparse JSON map rather than a full settings row, so a
 *        profile says only what it CHANGES. A profile that carried every
 *        setting would silently freeze the rest at whatever they were the day
 *        it was created — change a global default later and the app with a
 *        profile would quietly not get it.
 * WHERE: Read by session/settings_view.rs when a session starts; written by
 *        ipc/commands/profiles.rs.
 */

use std::collections::HashMap;

use rusqlite::{params, Row};

use crate::db::Database;
use crate::error::AppResult;
use crate::types::SettingValue;

/**
 * SOURCE OF TRUTH KEYWORDS: AppProfile
 * WHAT:  One application's overrides.
 * WHERE: Rendered in Settings; applied at session start.
 */
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct AppProfile {
    /// e.g. "com.apple.Terminal". Matched against the frontmost app.
    pub bundle_id: String,
    pub display_name: String,
    /// Sparse: only the settings this profile changes.
    pub overrides: HashMap<String, SettingValue>,
    pub enabled: bool,
}

pub fn list_profiles(db: &Database) -> AppResult<Vec<AppProfile>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT bundle_id, display_name, settings_json, enabled
               FROM app_profiles ORDER BY display_name COLLATE NOCASE",
        )?;
        let rows = stmt.query_map([], row_to_profile)?;

        let mut out = Vec::new();
        for row in rows {
            match row {
                Ok(profile) => out.push(profile),
                // A profile whose JSON will not parse was written by a newer
                // build. Skipping it means that app falls back to the global
                // settings, which is strictly better than failing the list.
                Err(err) => tracing::warn!(error = %err, "skipping an unreadable app profile"),
            }
        }
        Ok(out)
    })
}

/// None means this app has no profile — the common case, and not an error.
pub fn get_profile(db: &Database, bundle_id: &str) -> AppResult<Option<AppProfile>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT bundle_id, display_name, settings_json, enabled
               FROM app_profiles WHERE bundle_id = ?1 AND enabled = 1",
        )?;
        let mut rows = stmt.query(params![bundle_id])?;
        match rows.next()? {
            Some(row) => Ok(row_to_profile(row).ok()),
            None => Ok(None),
        }
    })
}

pub fn upsert_profile(db: &Database, profile: &AppProfile) -> AppResult<()> {
    let encoded = serde_json::to_string(&profile.overrides)?;
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO app_profiles (bundle_id, display_name, settings_json, enabled)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (bundle_id) DO UPDATE SET
               display_name = excluded.display_name,
               settings_json = excluded.settings_json,
               enabled = excluded.enabled",
            params![
                profile.bundle_id,
                profile.display_name,
                encoded,
                profile.enabled
            ],
        )?;
        Ok(())
    })
}

pub fn delete_profile(db: &Database, bundle_id: &str) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute(
            "DELETE FROM app_profiles WHERE bundle_id = ?1",
            params![bundle_id],
        )?;
        Ok(())
    })
}

fn row_to_profile(row: &Row<'_>) -> rusqlite::Result<AppProfile> {
    let json: String = row.get(2)?;
    let overrides: HashMap<String, SettingValue> = serde_json::from_str(&json).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(err))
    })?;

    Ok(AppProfile {
        bundle_id: row.get(0)?,
        display_name: row.get(1)?,
        overrides,
        enabled: row.get(3)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(bundle_id: &str, overrides: &[(&str, SettingValue)]) -> AppProfile {
        AppProfile {
            bundle_id: bundle_id.into(),
            display_name: "Test App".into(),
            overrides: overrides
                .iter()
                .map(|(k, v)| ((*k).to_string(), v.clone()))
                .collect(),
            enabled: true,
        }
    }

    #[test]
    fn an_app_without_a_profile_reads_as_none() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        assert!(get_profile(&db, "com.apple.Terminal")?.is_none());
        Ok(())
    }

    #[test]
    fn overrides_round_trip_and_stay_sparse() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        upsert_profile(
            &db,
            &profile(
                "com.tinyspeck.slackmacgap",
                &[("enhance.strip_fillers", SettingValue::Bool(true))],
            ),
        )?;

        let stored = get_profile(&db, "com.tinyspeck.slackmacgap")?.expect("profile exists");
        assert_eq!(
            stored.overrides.len(),
            1,
            "a profile must carry only what it changes"
        );
        assert_eq!(
            stored.overrides.get("enhance.strip_fillers"),
            Some(&SettingValue::Bool(true))
        );
        Ok(())
    }

    #[test]
    fn upserting_twice_updates_rather_than_duplicating() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        upsert_profile(&db, &profile("com.apple.Terminal", &[]))?;
        upsert_profile(
            &db,
            &profile(
                "com.apple.Terminal",
                &[("enhance.spoken_commands", SettingValue::Bool(true))],
            ),
        )?;

        assert_eq!(list_profiles(&db)?.len(), 1);
        Ok(())
    }

    #[test]
    fn a_disabled_profile_is_not_applied() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        let mut disabled = profile("com.apple.Terminal", &[]);
        disabled.enabled = false;
        upsert_profile(&db, &disabled)?;

        assert!(get_profile(&db, "com.apple.Terminal")?.is_none());
        // But it is still listed, so it can be re-enabled.
        assert_eq!(list_profiles(&db)?.len(), 1);
        Ok(())
    }
}
