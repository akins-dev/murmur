/*!
 * SOURCE OF TRUTH KEYWORDS: Database, open_database, with_connection,
 *   DbHandle, configure_pragmas
 * WHAT:  The single SQLite connection, its pragmas, and the guarded accessor
 *        every service goes through.
 * WHY:   One connection behind a mutex, deliberately. This is a single-user
 *        local app; a pool would add contention semantics and a whole class of
 *        borrow problems to solve a problem we do not have. WAL mode is what
 *        keeps a read during a write from blocking, and busy_timeout turns the
 *        rare collision into a short wait instead of an immediate error the
 *        caller has to handle. foreign_keys is ON so session_metrics rows die
 *        with their session — which is what makes cancellation a single DELETE.
 * WHERE: Constructed once during setup and held in app state; used by every
 *        function in services/.
 */

use std::path::Path;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;

use crate::error::{AppError, AppResult, ErrorCode};

use super::migrations::apply_migrations;

/// Shared handle. Cheap to clone; the lock is only held for one statement.
#[derive(Clone)]
pub struct Database {
    inner: Arc<Mutex<Connection>>,
}

impl Database {
    /**
     * WHAT:  Opens the database file, configures it, and migrates it.
     * WHY:   Pragmas are set on every open because they are per-connection, not
     *        stored in the file — a connection that forgets journal_mode gets
     *        different concurrency behaviour than the rest of the app.
     * WHERE: Called once from lib.rs setup with AppPaths::db_path.
     */
    pub fn open(path: &Path) -> AppResult<Self> {
        let mut conn = Connection::open(path).map_err(|err| {
            AppError::from(err).with_detail(format!("opening {}", path.display()))
        })?;

        configure_pragmas(&conn)?;
        apply_migrations(&mut conn)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
        })
    }

    /// In-memory database, for tests only.
    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        let mut conn = Connection::open_in_memory()?;
        configure_pragmas(&conn)?;
        apply_migrations(&mut conn)?;
        Ok(Self {
            inner: Arc::new(Mutex::new(conn)),
        })
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: with_connection
     * WHAT:  Runs a closure with the connection held.
     * WHY:   The lock is acquired and released around one unit of work, so no
     *        caller can hold it across an await point — which is the way a
     *        single-connection design turns into a deadlock. A poisoned mutex
     *        becomes an AppError rather than a panic, because a panic here
     *        would take down the app over a database read.
     * WHERE: The only way services touch SQLite.
     */
    pub fn with_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> AppResult<T>,
    ) -> AppResult<T> {
        let guard = self.inner.lock().map_err(|_| {
            AppError::new(
                ErrorCode::Database,
                "Murmur's database is in an inconsistent state. Restarting the app will fix it.",
            )
        })?;
        operation(&guard)
    }

    /// Same, for work that needs `&mut Connection` — transactions.
    pub fn with_connection_mut<T>(
        &self,
        operation: impl FnOnce(&mut Connection) -> AppResult<T>,
    ) -> AppResult<T> {
        let mut guard = self.inner.lock().map_err(|_| {
            AppError::new(
                ErrorCode::Database,
                "Murmur's database is in an inconsistent state. Restarting the app will fix it.",
            )
        })?;
        operation(&mut guard)
    }
}

/**
 * WHAT:  The pragmas every connection needs.
 * WHY:   Each of these is load-bearing; see the module WHY.
 * WHERE: Called from both open paths so tests and production behave alike.
 */
fn configure_pragmas(conn: &Connection) -> AppResult<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "busy_timeout", 5000)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    // NORMAL is the right durability trade for a local app in WAL mode: it
    // survives process crashes, and only a power loss can lose the most recent
    // commit. FULL would fsync on every transition and we write one per state.
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}
