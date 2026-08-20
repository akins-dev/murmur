/*!
 * SOURCE OF TRUTH KEYWORDS: list_models, download_model, delete_model,
 *   get_model_status, ModelIdInput
 * WHAT:  The model manager: what is available, what is on disk, and downloading
 *        or removing it.
 * WHY:   Download is Exclusive so two concurrent calls cannot write the same
 *        partial file — resumable downloads share a `.part` path, and two
 *        writers to it would produce a file that hashes to nothing and looks
 *        like a corrupt server.
 * WHERE: Consumed by onboarding and the model manager in Settings.
 */

use tauri::State;

use crate::error::AppError;
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec, Validate};
use crate::ports::models::ModelStatus;
use crate::registry::CapabilityKey;
use crate::types::{ModelId, ModelState};

#[derive(Debug, Clone, serde::Serialize, specta::Type)]
pub struct ModelReport {
    pub descriptor: crate::types::ModelDescriptor,
    pub state: ModelState,
    /// Absolute path, present only once the file is hash-verified.
    pub path: Option<String>,
}

impl From<ModelStatus> for ModelReport {
    fn from(status: ModelStatus) -> Self {
        Self {
            descriptor: status.descriptor,
            state: status.state,
            path: status.path.map(|p| p.to_string_lossy().into_owned()),
        }
    }
}

const LIST: CommandSpec = CommandSpec::new("list_models", CapabilityKey::Models);

#[tauri::command]
#[specta::specta]
pub async fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelReport>, AppError> {
    execute(&state, LIST, (), |ctx, ()| async move {
        Ok(ctx
            .ports()
            .models
            .list()
            .await?
            .into_iter()
            .map(ModelReport::from)
            .collect())
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct ModelIdInput {
    pub model_id: ModelId,
}

impl Validate for ModelIdInput {
    fn validate(&self) -> Result<(), String> {
        if self.model_id.as_str().trim().is_empty() {
            return Err("A model is required.".into());
        }
        Ok(())
    }
}

const STATUS: CommandSpec = CommandSpec::new("get_model_status", CapabilityKey::Models);

#[tauri::command]
#[specta::specta]
pub async fn get_model_status(
    state: State<'_, AppState>,
    input: ModelIdInput,
) -> Result<ModelReport, AppError> {
    execute(&state, STATUS, input, |ctx, input| async move {
        Ok(ctx.ports().models.status(&input.model_id).await?.into())
    })
    .await
}

/// Downloads if absent, verifies by hash, and returns the verified path.
/// Progress arrives on the ModelDownloadProgress event, not from this call.
const DOWNLOAD: CommandSpec =
    CommandSpec::new("download_model", CapabilityKey::Models).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn download_model(
    state: State<'_, AppState>,
    input: ModelIdInput,
) -> Result<String, AppError> {
    execute(&state, DOWNLOAD, input, |ctx, input| async move {
        let path = ctx.ports().models.ensure(&input.model_id).await?;
        Ok(path.to_string_lossy().into_owned())
    })
    .await
}

const DELETE: CommandSpec = CommandSpec::new("delete_model", CapabilityKey::Models).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn delete_model(
    state: State<'_, AppState>,
    input: ModelIdInput,
) -> Result<(), AppError> {
    execute(&state, DELETE, input, |ctx, input| async move {
        ctx.ports().models.delete(&input.model_id).await
    })
    .await
}
