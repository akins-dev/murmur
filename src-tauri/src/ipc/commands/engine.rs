/*!
 * SOURCE OF TRUTH KEYWORDS: list_languages, get_engine_capabilities,
 *   copy_text, CopyTextInput
 * WHAT:  What the selected engine can do, the language picker's contents, and
 *        a clipboard write the frontend can call.
 * WHY:   `get_engine_capabilities` is what makes `SettingDef::requires_engine`
 *        mean something. Without it the frontend has no way to know whether the
 *        selected engine supports auto-detect or Hindi, so Settings would have
 *        to either hide the control or offer one that fails at record time —
 *        the exact "dead button" docs/01 §6 says the engine seam exists to
 *        prevent.
 *
 *        `copy_text` exists because a WKWebView's `navigator.clipboard` is
 *        gated on transient user activation and a permissive CSP, and both can
 *        change under us. Copy is a one-line guarantee we would rather own than
 *        inherit — and the same arboard path the delivery pipeline already
 *        uses is the one that is known to work here.
 * WHERE: Consumed by Settings, onboarding, and History's copy action.
 */

use tauri::State;

use crate::error::{AppError, ErrorCode};
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec, Validate};
use crate::registry::CapabilityKey;
use crate::types::{language_options, EngineCapabilities, LanguageOption};

const CAPABILITIES: CommandSpec =
    CommandSpec::new("get_engine_capabilities", CapabilityKey::Models);

#[tauri::command]
#[specta::specta]
pub async fn get_engine_capabilities(
    state: State<'_, AppState>,
) -> Result<EngineCapabilities, AppError> {
    execute(&state, CAPABILITIES, (), |ctx, ()| async move {
        Ok(ctx.ports().engine.capabilities())
    })
    .await
}

/**
 * WHAT:  The language picker's contents, flagged by what the engine supports.
 * WHY:   Resolves ChoiceSource::Languages. Unsupported languages are RETURNED
 *        and marked rather than omitted, so the UI can explain the absence — a
 *        user whose language silently vanished from a list has no way to work
 *        out that the engine is the reason.
 * WHERE: The Language setting in Settings.
 */
const LANGUAGES: CommandSpec = CommandSpec::new("list_languages", CapabilityKey::Models);

#[tauri::command]
#[specta::specta]
pub async fn list_languages(
    state: State<'_, AppState>,
) -> Result<Vec<LanguageOption>, AppError> {
    execute(&state, LANGUAGES, (), |ctx, ()| async move {
        let capabilities = ctx.ports().engine.capabilities();
        let auto_detect = capabilities.has(crate::types::EngineFeature::LanguageAutoDetect);
        Ok(language_options(&capabilities.languages, auto_detect))
    })
    .await
}

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct CopyTextInput {
    pub text: String,
}

impl Validate for CopyTextInput {
    fn validate(&self) -> Result<(), String> {
        if self.text.is_empty() {
            return Err("There is nothing to copy.".into());
        }
        Ok(())
    }
}

const COPY: CommandSpec = CommandSpec::new("copy_text", CapabilityKey::History);

#[tauri::command]
#[specta::specta]
pub async fn copy_text(state: State<'_, AppState>, input: CopyTextInput) -> Result<(), AppError> {
    execute(&state, COPY, input, |_ctx, input| async move {
        let mut clipboard = arboard::Clipboard::new().map_err(|err| {
            AppError::new(
                ErrorCode::ClipboardUnavailable,
                "Murmur could not reach the clipboard.",
            )
            .with_detail(err)
        })?;
        clipboard.set_text(input.text).map_err(|err| {
            AppError::new(
                ErrorCode::ClipboardUnavailable,
                "Murmur could not copy that.",
            )
            .with_detail(err)
        })
    })
    .await
}
