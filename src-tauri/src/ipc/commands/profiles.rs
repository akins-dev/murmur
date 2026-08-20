/*!
 * SOURCE OF TRUTH KEYWORDS: list_app_profiles, save_app_profile,
 *   delete_app_profile, SaveProfileInput
 * WHAT:  CRUD for per-application setting overrides.
 * WHY:   Validation refuses an override naming a setting the registry does not
 *        declare. Without that check a profile could carry a key nothing ever
 *        reads — which looks like a saved preference that silently does
 *        nothing, and is very hard to tell apart from a bug in the feature it
 *        was supposed to change.
 * WHERE: The per-app section of Settings.
 */

use tauri::State;

use crate::error::AppError;
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec, Validate};
use crate::registry::{self, CapabilityKey};
use crate::services::profiles::{self, AppProfile};

const LIST: CommandSpec = CommandSpec::new("list_app_profiles", CapabilityKey::Settings);

#[tauri::command]
#[specta::specta]
pub async fn list_app_profiles(state: State<'_, AppState>) -> Result<Vec<AppProfile>, AppError> {
    execute(&state, LIST, (), |ctx, ()| async move {
        profiles::list_profiles(ctx.db())
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct SaveProfileInput {
    pub profile: AppProfile,
}

impl Validate for SaveProfileInput {
    fn validate(&self) -> Result<(), String> {
        if self.profile.bundle_id.trim().is_empty() {
            return Err("A profile needs an application.".into());
        }

        for (key, value) in &self.profile.overrides {
            let Some(def) = registry::setting_def(key) else {
                return Err(format!("`{key}` is not a setting."));
            };
            if !value.matches_kind(&def.kind) {
                return Err(format!("That is not a valid value for {}.", def.label));
            }
        }
        Ok(())
    }
}

const SAVE: CommandSpec =
    CommandSpec::new("save_app_profile", CapabilityKey::Settings).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn save_app_profile(
    state: State<'_, AppState>,
    input: SaveProfileInput,
) -> Result<(), AppError> {
    execute(&state, SAVE, input, |ctx, input| async move {
        profiles::upsert_profile(ctx.db(), &input.profile)
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct DeleteProfileInput {
    pub bundle_id: String,
}

impl Validate for DeleteProfileInput {
    fn validate(&self) -> Result<(), String> {
        if self.bundle_id.trim().is_empty() {
            return Err("A profile needs an application.".into());
        }
        Ok(())
    }
}

const DELETE: CommandSpec =
    CommandSpec::new("delete_app_profile", CapabilityKey::Settings).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn delete_app_profile(
    state: State<'_, AppState>,
    input: DeleteProfileInput,
) -> Result<(), AppError> {
    execute(&state, DELETE, input, |ctx, input| async move {
        profiles::delete_profile(ctx.db(), &input.bundle_id)
    })
    .await
}
