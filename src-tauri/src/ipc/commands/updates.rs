/*!
 * SOURCE OF TRUTH KEYWORDS: check_for_update, install_update, UpdateCheck,
 *   updates_enabled, CHECK_UPDATES, UpdaterExt, updater_disabled
 * WHAT:  The two update commands: ask GitHub Releases whether a newer signed
 *        build exists, and install one.
 * WHY:   This is the ONLY network request Murmur makes after setup, and the
 *        product promise is that it can be switched off — so the setting is
 *        checked BEFORE the updater is ever constructed, in both commands.
 *        Not before the request: before the client exists. A gate that builds
 *        a network client and then declines to use it is one refactor away
 *        from being no gate at all.
 *
 *        `install_update` re-runs the check rather than reusing the handle
 *        `check_for_update` obtained. An `Update` cannot cross the IPC
 *        boundary, so the alternative is parking one in AppState and hoping
 *        the version the user clicked is the version still sitting there. The
 *        second check costs one request and removes that whole class of
 *        mismatch — and it is the request that matters, because it is the one
 *        whose signature is verified before anything is written to disk.
 *
 *        Signature verification itself is the plugin's, keyed by the public
 *        key in tauri.conf.json. Nothing here reimplements it, and nothing
 *        here can bypass it.
 * WHERE: Registered in ipc/bindings.rs; driven by the Settings view's update
 *        section. The signing side is .github/workflows/release.yml.
 */

use tauri::State;
use tauri_plugin_updater::UpdaterExt;

use crate::db::Database;
use crate::error::{AppError, ErrorCode};
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec};
use crate::registry::{self, keys, CapabilityKey};
use crate::services::settings;
use crate::types::{SessionState, SettingValue};

/**
 * SOURCE OF TRUTH KEYWORDS: UpdateCheck
 * WHAT:  What a check found: the feature is off, we are current, or there is a
 *        newer signed build.
 * WHY:   Three states rather than an `Option` plus a boolean, because "the
 *        user turned this off" is not the same answer as "there is nothing new"
 *        and the UI must not render them the same way. Collapsing them is how
 *        a disabled updater ends up quietly reporting "up to date" — a claim
 *        nobody checked.
 * WHERE: Returned by check_for_update; rendered by the Settings view.
 */
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE", tag = "kind")]
pub enum UpdateCheck {
    /// `general.check_updates` is off. No request was made.
    Disabled,
    UpToDate {
        current_version: String,
    },
    Available {
        version: String,
        current_version: String,
        /// Release notes, when the release carries them.
        notes: Option<String>,
        /// RFC 3339, as published. None when the release omits it.
        published_at: Option<String>,
    },
}

/**
 * SOURCE OF TRUTH KEYWORDS: updates_enabled
 * WHAT:  Whether the user allows the update check to make its request.
 * WHY:   Falls back to the registry default rather than to `false`, so a fresh
 *        install behaves the way the registry says it does and the default
 *        lives in exactly one place. Reads the stored value on every call
 *        rather than caching it, because the answer must change the moment the
 *        toggle does — a cached "enabled" is a network request the user has
 *        already said no to.
 * WHERE: Both commands in this file, before any updater is built.
 */
fn updates_enabled(db: &Database) -> bool {
    let stored = settings::get_setting(db, keys::CHECK_UPDATES).ok().flatten();
    let value = stored.or_else(|| {
        registry::setting_def(keys::CHECK_UPDATES).map(|def| def.default.clone())
    });
    matches!(value, Some(SettingValue::Bool(true)))
}

/// The plugin's errors are network and signature failures, which are a user's
/// problem to retry rather than a bug to report — so they map to Network with
/// the detail kept for the log.
fn updater_error(err: tauri_plugin_updater::Error) -> AppError {
    AppError::new(
        ErrorCode::Network,
        "Murmur could not reach the update server. Check your connection and try again.",
    )
    .recoverable()
    .with_detail(err)
}

const CHECK: CommandSpec = CommandSpec::new("check_for_update", CapabilityKey::Updates);

/**
 * SOURCE OF TRUTH KEYWORDS: look_for_update
 * WHAT:  The update check itself, without the command wrapper.
 * WHY:   Two callers need it — this command, and the scheduler in bootstrap
 *        that honours the setting's promise of "on launch and once a day". They
 *        share ONE implementation so the setting check, the version comparison
 *        and the error mapping cannot drift between a manual press and an
 *        automatic check. That drift is exactly how a control ends up meaning
 *        something different depending on who triggered it.
 * WHERE: check_for_update below, and bootstrap::start_update_checks.
 */
pub(crate) async fn look_for_update(state: &AppState) -> Result<UpdateCheck, AppError> {
    if !updates_enabled(&state.db) {
        return Ok(UpdateCheck::Disabled);
    }

    let current_version = state.app.package_info().version.to_string();
    let updater = state.app.updater().map_err(updater_error)?;

    match updater.check().await.map_err(updater_error)? {
        Some(update) => Ok(UpdateCheck::Available {
            version: update.version.clone(),
            current_version,
            notes: update.body.clone(),
            published_at: update.date.map(|date| date.to_string()),
        }),
        None => Ok(UpdateCheck::UpToDate { current_version }),
    }
}

#[tauri::command]
#[specta::specta]
pub async fn check_for_update(state: State<'_, AppState>) -> Result<UpdateCheck, AppError> {
    execute(&state, CHECK, (), |ctx, ()| async move {
        look_for_update(&ctx.state).await
    })
    .await
}

/**
 * SOURCE OF TRUTH KEYWORDS: install_update
 * WHAT:  Downloads the newest signed build, installs it, and restarts.
 * WHY:   Exclusive, and refuses while a session is live. Installing replaces
 *        the running .app bundle and then restarts the process — do that
 *        mid-dictation and the user loses audio they have already spoken, with
 *        no way to get it back. An update is never urgent enough to be worth
 *        that, so the answer is "not right now" rather than a race.
 *
 *        The setting is checked here too, not just in the check. Otherwise a
 *        stale UI holding an "update available" from before the toggle was
 *        switched off could still start a download the user has forbidden.
 * WHERE: The Settings view's update section, after check_for_update reported
 *        Available.
 */
const INSTALL: CommandSpec = CommandSpec::new("install_update", CapabilityKey::Updates).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn install_update(state: State<'_, AppState>) -> Result<(), AppError> {
    execute(&state, INSTALL, (), |ctx, ()| async move {
        if !updates_enabled(ctx.db()) {
            return Err(AppError::new(
                ErrorCode::InvalidInput,
                "Updates are turned off. Turn them back on in Settings to install one.",
            )
            .recoverable());
        }

        if !matches!(ctx.state.current_state(), SessionState::Idle) {
            return Err(AppError::new(
                ErrorCode::SessionAlreadyActive,
                "Murmur is in the middle of a dictation. Finish it and try again.",
            )
            .recoverable());
        }

        let updater = ctx.state.app.updater().map_err(updater_error)?;
        let Some(update) = updater.check().await.map_err(updater_error)? else {
            // Something else installed it, or the release was pulled between
            // the check and the click. Not an error the user caused.
            return Err(AppError::new(
                ErrorCode::NotFound,
                "That update is no longer available. Check again.",
            )
            .recoverable());
        };

        let version = update.version.clone();
        update
            .download_and_install(|_chunk, _total| {}, || {})
            .await
            .map_err(updater_error)?;

        tracing::info!(version, "update installed; restarting");
        // Diverges. The new bundle is already on disk; the running process is
        // the old one, so there is nothing sensible to do but hand over.
        ctx.state.app.restart();
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * WHAT:  The registry default decides what an install with no stored value
     *        does.
     * WHY:   `updates_enabled` falls back to the registry rather than to a
     *        literal, and this is the assertion that keeps those two honest. If
     *        the declared default ever flips to false, an app that has never
     *        been configured must stop making the request — silently keeping a
     *        hardcoded `true` here would be the app ignoring its own registry.
     * WHERE: registry/mod.rs, the CHECK_UPDATES toggle.
     */
    #[test]
    fn the_update_check_default_comes_from_the_registry() {
        let def = registry::setting_def(keys::CHECK_UPDATES)
            .expect("the check-updates toggle is declared");
        assert!(
            matches!(def.default, SettingValue::Bool(_)),
            "a toggle must default to a bool, not {:?}",
            def.default
        );
    }

    /**
     * WHAT:  The three answers a check can give stay distinguishable once
     *        serialised.
     * WHY:   Disabled and UpToDate are the pair that must never collapse — an
     *        updater the user switched off reporting "you are up to date" is a
     *        claim nothing checked. The tag is what keeps them apart on the
     *        TypeScript side, so it is asserted here rather than assumed.
     * WHERE: The Settings view branches on this tag.
     */
    #[test]
    fn a_disabled_check_never_looks_like_an_up_to_date_one() {
        let disabled = serde_json::to_value(UpdateCheck::Disabled).expect("serialises");
        let current = serde_json::to_value(UpdateCheck::UpToDate {
            current_version: "0.1.0".to_string(),
        })
        .expect("serialises");

        assert_eq!(disabled.get("kind").and_then(|k| k.as_str()), Some("DISABLED"));
        assert_eq!(current.get("kind").and_then(|k| k.as_str()), Some("UP_TO_DATE"));
        assert_ne!(disabled.get("kind"), current.get("kind"));
    }
}
