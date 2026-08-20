/*!
 * SOURCE OF TRUTH KEYWORDS: services, sessions, settings, metrics, dictionary,
 *   stats
 * WHAT:  Barrel for the database layer. Every function here is pure SQLite.
 * WHY:   Services are the ONLY layer that touches the database, and they hold
 *        no business rules, no orchestration, and never call each other. That
 *        restriction is what lets the FSM, crash recovery and the History view
 *        share these functions without any of them inheriting decisions the
 *        others made.
 * WHERE: Imported by ipc/commands/, session/ and pipeline/ as
 *        `use crate::services::sessions` so call sites read as `sessions::get`.
 */

pub mod dictionary;
pub mod metrics;
pub mod profiles;
pub mod sessions;
pub mod settings;
pub mod stats;
