/**
 * SOURCE OF TRUTH KEYWORDS: saveTextFile, SaveOutcome, save_text_file,
 *   native-save-dialog, user-cancelled
 * WHAT:  Hands the user a text file to save through the OS save dialog, and
 *        reports whether it was written, cancelled, or failed.
 * WHY:   Rust owns the save. The browser route — a Blob and a download anchor —
 *        is not merely unreliable in a WKWebView, it fails SILENTLY: there is no
 *        error, no event, nothing to detect, so there is no signal to fall back
 *        FROM and a fallback would just be the broken path with extra steps.
 *        That matters most here of all places, because a user who believes they
 *        exported their history and did not has lost the thing the feature
 *        exists to protect. The Rust side also writes through a temp file and
 *        renames, so an interrupted write cannot leave a truncated archive that
 *        looks complete.
 *
 *        Cancelling is NOT a failure. The command returns null for it, and this
 *        returns "cancelled" as its own outcome so no caller can mistake a user
 *        changing their mind for something going wrong.
 * WHERE: Used by the history export action. Wraps the save_text_file command.
 */

import { commands, type AppError } from "./bindings";
import { unwrapCommand } from "./ipc";

export type SaveOutcome =
  /** Written. `path` is where the user put it. */
  | { status: "saved"; path: string }
  /** The user dismissed the dialog. Render nothing. */
  | { status: "cancelled" }
  | { status: "error"; error: AppError };

export async function saveTextFile(suggestedName: string, contents: string): Promise<SaveOutcome> {
  const result = await unwrapCommand(() =>
    commands.saveTextFile({ contents, suggested_name: suggestedName }),
  );
  if (result.status === "error") return { status: "error", error: result.error };
  return result.data === null ? { status: "cancelled" } : { status: "saved", path: result.data };
}
