/*!
 * SOURCE OF TRUTH KEYWORDS: session, SessionMachine, SessionEvent, Effect,
 *   recovery
 * WHAT:  Barrel for the recording state machine and its owner.
 * WHY:   All recording state lives under here. There is no boolean anywhere
 *        else in the app that means "are we recording", and no second copy of
 *        it in the frontend — the pill renders what this module pushes.
 * WHERE: Driven by ipc/commands/session.rs and the hotkey handler.
 */

pub mod actor;
pub mod delivery;
pub mod machine;

#[cfg(test)]
mod e2e_tests;
pub mod settings_view;

pub use actor::SessionActor;
pub use settings_view::SessionSettings;
pub use machine::{Effect, SessionEvent, SessionMachine, Transition, TransitionError};
