/*!
 * SOURCE OF TRUTH KEYWORDS: commands, session, history, settings, stats,
 *   dictionary, models, system, updates
 * WHAT:  Barrel for the IPC command handlers, one module per capability.
 * WHY:   Every function here is thin on purpose. If a handler starts checking
 *        permissions, validating input by hand, or mapping errors, that work
 *        belongs in ipc/factory.rs — a handler doing it means the factory or
 *        the registry entry is wrong.
 * WHERE: Collected into the specta builder in lib.rs.
 */

pub mod dictionary;
pub mod engine;
pub mod files;
pub mod history;
pub mod models;
pub mod profiles;
pub mod session;
pub mod settings;
pub mod stats;
pub mod system;
pub mod updates;
