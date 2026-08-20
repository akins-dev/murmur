/*!
 * SOURCE OF TRUTH KEYWORDS: save_text_file, SaveTextFileInput, save_dialog,
 *   suggested_name
 * WHAT:  Writes text to a file the user picks, through a native save dialog.
 * WHY:   A webview `Blob` + download anchor cannot be trusted here. Our CSP is
 *        restrictive, the download is sandboxed, and — critically — when it
 *        fails it fails SILENTLY: the click appears to work and no file
 *        appears. For history export that is the worst possible failure, since
 *        the whole feature exists so someone's transcripts are not locked
 *        inside this app. A person who believes they have a backup and does
 *        not is worse off than one who knows they have none.
 *
 *        So the write goes through Rust, where a failure is an AppError the UI
 *        can actually show, and a success is a real path on disk.
 * WHERE: Called by the History export action.
 */

use std::io::Write;

use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::error::{AppError, ErrorCode};
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec, Validate};
use crate::registry::CapabilityKey;

/// Generous, but bounded — this is a transcript archive, not a disk image.
const MAX_EXPORT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, serde::Deserialize, specta::Type)]
pub struct SaveTextFileInput {
    pub contents: String,
    /// Offered in the dialog. The user may change it.
    pub suggested_name: String,
}

impl Validate for SaveTextFileInput {
    fn validate(&self) -> Result<(), String> {
        if self.contents.is_empty() {
            return Err("There is nothing to save.".into());
        }
        if self.contents.len() > MAX_EXPORT_BYTES {
            return Err("That export is too large to save in one file.".into());
        }
        if self.suggested_name.trim().is_empty() {
            return Err("A file name is required.".into());
        }
        // A name is not a path. Refusing separators here stops a suggested name
        // from steering the dialog somewhere the user did not choose.
        if self.suggested_name.contains('/') || self.suggested_name.contains("..") {
            return Err("That file name is not allowed.".into());
        }
        Ok(())
    }
}

const SAVE: CommandSpec =
    CommandSpec::new("save_text_file", CapabilityKey::History).exclusive();

/**
 * WHAT:  Shows a save dialog and writes the text to whatever the user picks.
 * WHY:   Returns the chosen path, or None when the user cancelled. Cancelling
 *        is NOT an error — treating it as one would make the UI apologise for
 *        something the user deliberately did.
 * WHERE: The History export flow.
 */
#[tauri::command]
#[specta::specta]
pub async fn save_text_file(
    state: State<'_, AppState>,
    input: SaveTextFileInput,
) -> Result<Option<String>, AppError> {
    execute(&state, SAVE, input, |ctx, input| async move {
        let (tx, rx) = tokio::sync::oneshot::channel();

        ctx.state
            .app
            .dialog()
            .file()
            .set_file_name(&input.suggested_name)
            .save_file(move |path| {
                let _ = tx.send(path);
            });

        // The dialog is modal to the user but not to us; awaiting the callback
        // keeps this command from returning before there is an answer.
        let Ok(Some(path)) = rx.await else {
            return Ok(None);
        };

        let path = path.into_path().map_err(|err| {
            AppError::new(ErrorCode::Io, "That location cannot be written to.").with_detail(err)
        })?;

        // Written through a temp file and renamed, so an interrupted write
        // cannot leave a truncated archive that looks complete.
        let temp = path.with_extension("part");
        {
            let mut file = std::fs::File::create(&temp)?;
            file.write_all(input.contents.as_bytes())?;
            file.sync_all()?;
        }
        std::fs::rename(&temp, &path)?;

        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
}
