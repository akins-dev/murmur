/*!
 * SOURCE OF TRUTH KEYWORDS: MIGRATIONS, Migration, apply_migrations,
 *   schema_version, SCHEMA_VERSION
 * WHAT:  The ordered list of schema migrations and the runner that applies the
 *        ones this database has not seen.
 * WHY:   `PRAGMA user_version` is the whole mechanism — no migration framework,
 *        no metadata table, nothing to keep in sync. Each migration runs inside
 *        a transaction WITH the version bump, so a failure halfway through
 *        leaves the database on the previous version rather than in a state
 *        that is neither. Migrations are embedded with include_str! so a
 *        shipped binary can never disagree with the SQL it was built from.
 * WHERE: Run once by db/connection.rs when the connection is opened.
 */

use rusqlite::Connection;

use crate::error::AppResult;

struct Migration {
    version: i64,
    name: &'static str,
    sql: &'static str,
}

/// Append only. Never edit a migration that has shipped — write the next one.
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial_schema",
        sql: include_str!("migrations/001_initial_schema.sql"),
    },
    Migration {
        version: 2,
        name: "session_error_message",
        sql: include_str!("migrations/002_session_error_message.sql"),
    },
];

/// The version a fresh database ends up at.
pub const SCHEMA_VERSION: i64 = 2;

pub fn schema_version(conn: &Connection) -> AppResult<i64> {
    let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    Ok(version)
}

/**
 * SOURCE OF TRUTH KEYWORDS: apply_migrations
 * WHAT:  Applies every migration newer than the database's current version.
 * WHY:   The version bump is inside the same transaction as the DDL, so the two
 *        cannot disagree. A migration that fails rolls back completely and the
 *        error propagates — a half-migrated database is not something we try to
 *        repair at runtime.
 * WHERE: Called by db/connection.rs immediately after opening.
 */
pub fn apply_migrations(conn: &mut Connection) -> AppResult<i64> {
    let current = schema_version(conn)?;

    for migration in MIGRATIONS.iter().filter(|m| m.version > current) {
        let tx = conn.transaction()?;
        tx.execute_batch(migration.sql)?;
        // PRAGMA does not accept a bound parameter, and the value is a compile
        // time constant from the table above, so formatting it is safe here.
        tx.execute_batch(&format!("PRAGMA user_version = {}", migration.version))?;
        tx.commit()?;

        tracing::info!(
            version = migration.version,
            name = migration.name,
            "applied migration"
        );
    }

    schema_version(conn)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_are_ordered_and_contiguous() {
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                migration.version,
                index as i64 + 1,
                "migration versions must start at 1 and increase by one"
            );
        }
        assert_eq!(
            MIGRATIONS.last().map(|m| m.version),
            Some(SCHEMA_VERSION),
            "SCHEMA_VERSION must match the last migration"
        );
    }

    #[test]
    fn applies_cleanly_to_an_empty_database() -> AppResult<()> {
        let mut conn = Connection::open_in_memory()?;
        let version = apply_migrations(&mut conn)?;
        assert_eq!(version, SCHEMA_VERSION);

        // Re-running must be a no-op rather than an error.
        let again = apply_migrations(&mut conn)?;
        assert_eq!(again, SCHEMA_VERSION);
        Ok(())
    }

    #[test]
    fn fts_index_tracks_session_writes() -> AppResult<()> {
        // The external-content FTS table is only correct if its triggers fire.
        // This is the test that catches a drifting search index.
        let mut conn = Connection::open_in_memory()?;
        apply_migrations(&mut conn)?;

        conn.execute(
            "INSERT INTO sessions (id, started_at, outcome, engine_id, model_id, final_text)
             VALUES ('s1', 0, 'delivered', 'whisper', 'turbo', 'hello registry world')",
            [],
        )?;

        let hits: i64 = conn.query_row(
            "SELECT count(*) FROM sessions_fts WHERE sessions_fts MATCH 'registry'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(hits, 1, "insert trigger did not populate the FTS index");

        conn.execute("DELETE FROM sessions WHERE id = 's1'", [])?;
        let after_delete: i64 = conn.query_row(
            "SELECT count(*) FROM sessions_fts WHERE sessions_fts MATCH 'registry'",
            [],
            |row| row.get(0),
        )?;
        assert_eq!(after_delete, 0, "delete trigger left the FTS index stale");
        Ok(())
    }
}
