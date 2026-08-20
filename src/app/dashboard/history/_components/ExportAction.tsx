/**
 * SOURCE OF TRUTH KEYWORDS: ExportAction, exportHistory, ExportFormat,
 *   FORMAT_META, saveTextFile
 * WHAT:  The history export control: pick a format, get a file.
 * WHY:   The three formats are shown as buttons rather than hidden in a select,
 *        because a select that performs an action instead of holding a value
 *        lies about what it is — and there are only three. They stay behind one
 *        "Export" disclosure so the toolbar is not three buttons wide for
 *        something used twice a year. It exports EVERYTHING, not the current
 *        search: an export that silently honoured a filter would hand someone a
 *        partial archive they believed was complete, and this is the feature
 *        that exists so the data is not locked in.
 * WHERE: The toolbar slot of the history DataList.
 */

import { useState } from "react";
import { Download } from "lucide-react";
import { commands, type AppError, type ExportFormat } from "@/lib/bindings";
import { unwrapCommand } from "@/lib/ipc";
import { saveTextFile } from "@/lib/save-file";
import { cn } from "@/lib/utils";

const FORMAT_META: Readonly<Record<ExportFormat, { label: string; extension: string }>> = {
  JSON: { label: "JSON", extension: "json" },
  MARKDOWN: { label: "Markdown", extension: "md" },
  PLAIN_TEXT: { label: "Plain text", extension: "txt" },
};

const FORMATS: readonly ExportFormat[] = ["JSON", "MARKDOWN", "PLAIN_TEXT"];

const BUTTON_CLASS =
  "hairline h-8 shrink-0 rounded-input bg-sunken px-3 text-body text-text-primary transition-colors hover:bg-sunken-strong disabled:opacity-50";

export function ExportAction({ onError }: { onError: (error: AppError | null) => void }) {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<ExportFormat | null>(null);

  const run = (format: ExportFormat): void => {
    setBusy(format);
    void unwrapCommand(() => commands.exportHistory({ format }))
      .then(async (result) => {
        if (result.status === "error") {
          onError(result.error);
          return;
        }
        const meta = FORMAT_META[format];
        const stamp = new Date().toISOString().slice(0, 10);
        const outcome = await saveTextFile(`murmur-history-${stamp}.${meta.extension}`, result.data);

        // Cancelling is a decision, not a failure: nothing is shown, and the
        // format buttons stay open so a change of mind costs one click.
        if (outcome.status === "cancelled") {
          onError(null);
          return;
        }
        onError(outcome.status === "error" ? outcome.error : null);
        if (outcome.status === "saved") setOpen(false);
      })
      .finally(() => setBusy(null));
  };

  if (!open) {
    return (
      <button type="button" onClick={() => setOpen(true)} className={cn(BUTTON_CLASS, "flex items-center gap-2")}>
        <Download className="size-4" />
        Export
      </button>
    );
  }

  return (
    <span className="flex items-center gap-1">
      {FORMATS.map((format) => (
        <button
          key={format}
          type="button"
          disabled={busy !== null}
          onClick={() => run(format)}
          className={BUTTON_CLASS}
        >
          {busy === format ? "Exporting…" : FORMAT_META[format].label}
        </button>
      ))}
      <button type="button" onClick={() => setOpen(false)} className={cn(BUTTON_CLASS, "text-text-secondary")}>
        Cancel
      </button>
    </span>
  );
}
