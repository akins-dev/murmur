/*!
 * SOURCE OF TRUTH KEYWORDS: ipc, execute, CommandSpec, AppState, CommandContext
 * WHAT:  Barrel for the IPC layer: the command factory, its context, and the
 *        command handlers.
 * WHY:   Nothing writes `#[tauri::command]` outside commands/, and nothing in
 *        commands/ does its own validation, permission checking, guarding or
 *        error mapping — the factory did all of it. That split is what keeps a
 *        handler down to the logic that is actually specific to it.
 * WHERE: Registered with Tauri in lib.rs.
 */

pub mod bindings;
pub mod commands;
pub mod context;
pub mod events;
pub mod factory;

pub use context::{AppState, CommandContext, Ports, SessionContext, SessionHandle};
pub use factory::{execute, CommandSpec, Reentrancy, ResourceUse, Validate};
