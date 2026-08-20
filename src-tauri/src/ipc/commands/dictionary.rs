/*!
 * SOURCE OF TRUTH KEYWORDS: list_dictionary, create_dictionary_entry,
 *   update_dictionary_entry, delete_dictionary_entry, DictionaryEntryInput
 * WHAT:  CRUD over the custom vocabulary.
 * WHY:   The highest value-per-line feature in the app: every user has twenty
 *        terms the model gets wrong, and fixing them fixes most of what people
 *        experience as accuracy. Validation is strict about empty patterns
 *        because an empty pattern would match everywhere and corrupt every
 *        transcript.
 * WHERE: Consumed by the Dictionary section of Settings.
 */

use tauri::State;

use crate::error::AppError;
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec, Validate};
use crate::registry::CapabilityKey;
use crate::services::dictionary;
use crate::telemetry::now_ms;
use crate::types::{DictionaryEntry, DictionaryId, MatchKind};

/// Long enough for a product name, short enough that it cannot be a paragraph.
const MAX_TERM_LEN: usize = 120;

const LIST: CommandSpec = CommandSpec::new("list_dictionary", CapabilityKey::Dictionary);

#[tauri::command]
#[specta::specta]
pub async fn list_dictionary(
    state: State<'_, AppState>,
) -> Result<Vec<DictionaryEntry>, AppError> {
    execute(&state, LIST, (), |ctx, ()| async move {
        dictionary::list_entries(ctx.db())
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct CreateDictionaryEntryInput {
    pub pattern: String,
    pub replacement: String,
    pub match_kind: MatchKind,
}

impl Validate for CreateDictionaryEntryInput {
    fn validate(&self) -> Result<(), String> {
        if self.pattern.trim().is_empty() {
            return Err("Enter the word Murmur keeps getting wrong.".into());
        }
        if self.replacement.trim().is_empty() {
            return Err("Enter what it should say instead.".into());
        }
        if self.pattern.len() > MAX_TERM_LEN || self.replacement.len() > MAX_TERM_LEN {
            return Err(format!("Keep entries under {MAX_TERM_LEN} characters."));
        }
        Ok(())
    }
}

const CREATE: CommandSpec =
    CommandSpec::new("create_dictionary_entry", CapabilityKey::Dictionary).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn create_dictionary_entry(
    state: State<'_, AppState>,
    input: CreateDictionaryEntryInput,
) -> Result<DictionaryId, AppError> {
    execute(&state, CREATE, input, |ctx, input| async move {
        dictionary::create_entry(
            ctx.db(),
            input.pattern.trim(),
            input.replacement.trim(),
            input.match_kind,
            now_ms(),
        )
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct UpdateDictionaryEntryInput {
    pub id: DictionaryId,
    pub replacement: String,
    pub match_kind: MatchKind,
    pub enabled: bool,
}

impl Validate for UpdateDictionaryEntryInput {
    fn validate(&self) -> Result<(), String> {
        if self.replacement.trim().is_empty() {
            return Err("Enter what it should say instead.".into());
        }
        if self.replacement.len() > MAX_TERM_LEN {
            return Err(format!("Keep entries under {MAX_TERM_LEN} characters."));
        }
        Ok(())
    }
}

const UPDATE: CommandSpec =
    CommandSpec::new("update_dictionary_entry", CapabilityKey::Dictionary).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn update_dictionary_entry(
    state: State<'_, AppState>,
    input: UpdateDictionaryEntryInput,
) -> Result<(), AppError> {
    execute(&state, UPDATE, input, |ctx, input| async move {
        dictionary::update_entry(
            ctx.db(),
            &input.id,
            input.replacement.trim(),
            input.match_kind,
            input.enabled,
        )
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct DeleteDictionaryEntryInput {
    pub id: DictionaryId,
}

impl Validate for DeleteDictionaryEntryInput {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

const DELETE: CommandSpec =
    CommandSpec::new("delete_dictionary_entry", CapabilityKey::Dictionary).exclusive();

#[tauri::command]
#[specta::specta]
pub async fn delete_dictionary_entry(
    state: State<'_, AppState>,
    input: DeleteDictionaryEntryInput,
) -> Result<(), AppError> {
    execute(&state, DELETE, input, |ctx, input| async move {
        dictionary::delete_entry(ctx.db(), &input.id)
    })
    .await
}
