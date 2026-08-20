/*!
 * SOURCE OF TRUTH KEYWORDS: execute, CommandSpec, Reentrancy, Validate,
 *   ValidationError, murmur_command, preflight_permissions
 * WHAT:  The command factory. Every IPC command runs through `execute`, which
 *        validates the input, preflights permissions, guards reentrancy, opens
 *        a tracing span, runs the handler, and maps whatever comes back to an
 *        AppError.
 * WHY:   This is the analog of a protected procedure, and it exists so those
 *        six concerns have ONE implementation instead of one per handler. A
 *        permission check written thirty times is a permission check that is
 *        wrong in three places; a validation step a handler can forget is a
 *        validation step that will be forgotten.
 *
 *        The corollary is the rule that matters when writing a handler: do NOT
 *        re-check any of it. If a handler is checking a permission, the
 *        capability's `requires` list in the registry is wrong, and that is
 *        where the fix belongs.
 * WHERE: Called by every function in ipc/commands/. Reads capability metadata
 *        from registry/.
 */

use std::future::Future;

use crate::error::{AppError, AppResult, ErrorCode, ErrorAction, PrivacyPane};
use crate::ports::permissions::{OsPermission, PermissionState};
use crate::registry::{self, CapabilityKey};

use super::context::{AppState, CommandContext};

/**
 * SOURCE OF TRUTH KEYWORDS: Validate, ValidationError
 * WHAT:  The contract every command input implements.
 * WHY:   Rust has no Zod, so validation is a trait the factory calls rather
 *        than a schema it interprets. The effect is the same and the guarantee
 *        is stronger: an input type that does not implement this cannot be used
 *        as a command input at all, so "someone forgot to validate" is a
 *        compile error rather than a code review.
 * WHERE: Implemented by every input struct in ipc/commands/.
 */
pub trait Validate {
    /// Return a user-facing reason. The factory turns it into an AppError.
    fn validate(&self) -> Result<(), String>;
}

/// Inputs with nothing to check still opt in explicitly, so the absence of a
/// check is a decision someone made rather than one nobody made.
impl Validate for () {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: Reentrancy
 * WHAT:  Whether concurrent calls to this command are allowed.
 * WHY:   Reads are safe to overlap and should not be serialised — a dashboard
 *        opening three panels at once must not queue. Anything that mutates
 *        session state is Exclusive, which is what makes a double-fired hotkey
 *        harmless.
 * WHERE: Declared per command in its CommandSpec.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reentrancy {
    /// One at a time per capability.
    Exclusive,
    /// Overlapping calls are fine.
    Concurrent,
}

/**
 * SOURCE OF TRUTH KEYWORDS: ResourceUse, Acquires, Reports
 * WHAT:  Whether a command actually touches the OS resources its capability
 *        declares, or merely reports on them.
 * WHY:   A capability's `requires` list describes what the FEATURE needs — the
 *        microphone, for dictation. It does not follow that every command on
 *        that capability needs it. `get_session_state` only reads what the FSM
 *        is doing, and preflighting it against the microphone meant the pill
 *        could not render its own idle state until permission was granted:
 *        the app looked broken in exactly the moment it was trying to explain
 *        how to fix it.
 *
 *        So the requirement is enforced where the resource is USED. Acquires is
 *        the default, because defaulting to "no permission needed" would let a
 *        new command quietly skip a check nobody notices is missing.
 * WHERE: Declared per command in its CommandSpec; read by preflight_permissions.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceUse {
    /// Uses the capability's OS resources. Preflight applies. The default.
    Acquires,
    /// Only reports on them. No OS resource is touched, so no grant is needed.
    Reports,
}

/**
 * SOURCE OF TRUTH KEYWORDS: CommandSpec
 * WHAT:  The declaration a command hands the factory.
 * WHERE: One per function in ipc/commands/.
 */
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    /// Used in the tracing span and in errors.
    pub name: &'static str,
    /// Which registry entry governs this command's permissions.
    pub capability: CapabilityKey,
    pub reentrancy: Reentrancy,
    pub resource_use: ResourceUse,
}

impl CommandSpec {
    pub const fn new(name: &'static str, capability: CapabilityKey) -> Self {
        Self {
            name,
            capability,
            reentrancy: Reentrancy::Concurrent,
            resource_use: ResourceUse::Acquires,
        }
    }

    pub const fn exclusive(mut self) -> Self {
        self.reentrancy = Reentrancy::Exclusive;
        self
    }

    /// This command only reads state; it needs no OS grant. See ResourceUse.
    pub const fn reports(mut self) -> Self {
        self.resource_use = ResourceUse::Reports;
        self
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: execute
 * WHAT:  Runs one command through every cross-cutting concern.
 * WHY:   The order is deliberate and each step depends on the one before it:
 *          1. Validate  — an invalid input must never reach a handler, and must
 *                         never cost a permission prompt or a lock.
 *          2. Preflight — a missing OS grant becomes a typed, actionable error
 *                         rather than a panic deep inside an adapter.
 *          3. Guard     — claimed only after we know the call is legal, so an
 *                         invalid call cannot lock out a valid one.
 *          4. Handler   — business logic, and nothing else.
 *          5. Map       — one error surface for the UI.
 * WHERE: Called by every command in ipc/commands/.
 */
pub async fn execute<I, O, F, Fut>(
    state: &AppState,
    spec: CommandSpec,
    input: I,
    handler: F,
) -> Result<O, AppError>
where
    I: Validate,
    F: FnOnce(CommandContext, I) -> Fut,
    Fut: Future<Output = AppResult<O>>,
{
    let correlation_id = uuid::Uuid::new_v4().to_string();
    let span = tracing::info_span!(
        "command",
        name = spec.name,
        capability = spec.capability.as_str(),
        correlation_id = %correlation_id,
    );
    let _entered = span.enter();

    // 1. Validation. The input schema is the source of truth.
    if let Err(reason) = input.validate() {
        tracing::warn!(reason, "input rejected");
        return Err(AppError::invalid_input(reason));
    }

    // 2. Permission preflight, straight from the registry declaration — but
    // only for commands that actually use the resource. See ResourceUse.
    if spec.resource_use == ResourceUse::Acquires {
        preflight_permissions(state, spec.capability)?;
    }

    // 3. Reentrancy. Held for the life of the call, released on any exit.
    let _guard = match spec.reentrancy {
        Reentrancy::Exclusive => match state.begin_exclusive(spec.capability) {
            Some(guard) => Some(guard),
            None => {
                tracing::debug!("rejected a reentrant call");
                return Err(AppError::new(
                    ErrorCode::SessionAlreadyActive,
                    "That is already in progress.",
                )
                .recoverable());
            }
        },
        Reentrancy::Concurrent => None,
    };

    // 4. The handler. Only what is specific to this task.
    let context = CommandContext {
        state: state.clone(),
        correlation_id: correlation_id.clone(),
    };

    let started = std::time::Instant::now();
    let outcome = handler(context, input).await;
    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

    // 5. One error surface. Per-stage latency is recorded by the pipeline
    // itself against the registry's declared metrics; this is the coarse
    // command timing, which belongs in the log rather than the dashboard.
    match &outcome {
        Ok(_) => tracing::info!(elapsed_ms, "ok"),
        Err(err) => tracing::warn!(elapsed_ms, code = ?err.code, detail = ?err.detail, "failed"),
    }

    outcome
}

/**
 * SOURCE OF TRUTH KEYWORDS: preflight_permissions
 * WHAT:  Checks every OS grant the capability declares it requires.
 * WHY:   Reads `capability.requires` rather than a hardcoded list, so adding a
 *        permission to a feature is a registry edit and nothing else. The error
 *        it produces carries the deep link into the right settings pane —
 *        which is the ONLY recovery once a user has denied a TCC prompt, since
 *        macOS never asks twice.
 * WHERE: Step 2 of execute.
 */
fn preflight_permissions(state: &AppState, key: CapabilityKey) -> AppResult<()> {
    let Some(capability) = registry::capability(key) else {
        // A command naming a capability that is not declared is a wiring bug,
        // not a user problem — fail loudly rather than running unprotected.
        return Err(AppError::internal(format!(
            "command declared capability `{}`, which is not in the registry",
            key.as_str()
        )));
    };

    for permission in &capability.requires {
        // `ensure`, not `check`. A capability declaring a permission is about
        // to USE it, and the honest response to "never asked" is to ask — see
        // the WHY on PermissionProvider::ensure for what using `check` here
        // cost us.
        let state = state.ports.permissions.ensure(*permission);
        if state.is_granted() {
            continue;
        }

        tracing::warn!(permission = ?permission, state = ?state, "preflight failed");
        return Err(match (permission, state) {
            // Asked, and the dialog is still on screen. The remedy is to answer
            // it and try again — NOT a trip to System Settings, which would not
            // list Murmur yet.
            (OsPermission::Microphone, PermissionState::NotDetermined) => {
                AppError::microphone_pending()
            }
            (OsPermission::Microphone, _) => AppError::microphone_denied(),
            (OsPermission::Accessibility, _) => AppError::accessibility_denied(),
        });
    }

    Ok(())
}

/// Builds the deep-link error for a permission without failing a command —
/// used by onboarding, which reports state rather than refusing to run.
pub fn permission_action(permission: OsPermission) -> ErrorAction {
    ErrorAction::OpenPrivacyPane {
        pane: match permission {
            OsPermission::Microphone => PrivacyPane::Microphone,
            OsPermission::Accessibility => PrivacyPane::Accessibility,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Input {
        value: i64,
    }

    impl Validate for Input {
        fn validate(&self) -> Result<(), String> {
            if self.value < 0 {
                return Err("Value must not be negative.".into());
            }
            Ok(())
        }
    }

    #[test]
    fn validation_rejects_before_anything_else_happens() {
        let input = Input { value: -1 };
        assert!(input.validate().is_err());

        let input = Input { value: 3 };
        assert!(input.validate().is_ok());
    }

    #[test]
    fn a_spec_acquires_resources_unless_it_says_otherwise() {
        // Defaulting to Acquires matters: a new command that forgets to declare
        // its resource use gets the CHECK, not a silent bypass.
        let default = CommandSpec::new("start_recording", CapabilityKey::Dictation);
        assert_eq!(default.resource_use, ResourceUse::Acquires);

        let query = CommandSpec::new("get_session_state", CapabilityKey::Dictation).reports();
        assert_eq!(query.resource_use, ResourceUse::Reports);
    }

    #[test]
    fn a_spec_is_concurrent_unless_it_says_otherwise() {
        let read = CommandSpec::new("list_history", CapabilityKey::History);
        assert_eq!(read.reentrancy, Reentrancy::Concurrent);

        let write = CommandSpec::new("start_recording", CapabilityKey::Dictation).exclusive();
        assert_eq!(write.reentrancy, Reentrancy::Exclusive);
    }

    #[test]
    fn every_capability_a_command_can_name_exists_in_the_registry() {
        // preflight fails closed on an unknown capability, so this guards the
        // wiring rather than the runtime.
        for key in [
            CapabilityKey::Dictation,
            CapabilityKey::History,
            CapabilityKey::Stats,
            CapabilityKey::Dictionary,
            CapabilityKey::Models,
            CapabilityKey::Settings,
            CapabilityKey::Onboarding,
            CapabilityKey::Updates,
        ] {
            assert!(
                registry::capability(key).is_some(),
                "capability `{}` is named but not declared",
                key.as_str()
            );
        }
    }

    #[test]
    fn permission_errors_carry_the_recovery_deep_link() {
        // A denied TCC grant can never be re-prompted, so the error is useless
        // without the link.
        let err = AppError::microphone_denied();
        assert!(matches!(
            err.action,
            Some(ErrorAction::OpenPrivacyPane {
                pane: PrivacyPane::Microphone
            })
        ));

        let err = AppError::accessibility_denied();
        assert!(err.recoverable, "the app still works without Accessibility");
    }
}
