/**
 * SOURCE OF TRUTH KEYWORDS: ModelManager, ModelReport, ModelState, downloadModel,
 *   deleteModel, modelDownloadProgress, modelStateChanged, DownloadProgress, optimizing
 * WHAT:  The model list: size, memory, state, with download, delete and live
 *        progress.
 * WHY:   Progress is real bytes, a real rate and a real estimate, never an
 *        indeterminate spinner — the file is 574MB and a spinner on a
 *        multi-minute task reads as a hang (docs/04 §9). Progress arrives on an
 *        event and is kept in a map keyed by model id, so a download reports
 *        itself without the list polling anything. State transitions arrive the
 *        same way and are applied to the row IN PLACE rather than by refetching:
 *        a model on disk but not yet hash-verified reports Verifying and flips
 *        to Ready when the background check finishes, so a fetch-once list would
 *        sit on Verifying forever. The override is dropped the moment we issue a
 *        command against that model, because a delete makes every earlier state
 *        we were told about a lie. OPTIMIZING is given its own
 *        wording and its own expectation: it takes 15-60s once per machine, and
 *        a user who is not told that assumes the app has frozen on first run.
 * WHERE: The transcription section of Settings. Also the shape onboarding's
 *        model step mirrors.
 */

import { useCallback, useState } from "react";
import { Download, Trash2 } from "lucide-react";
import {
  commands,
  events,
  type DownloadProgress,
  type ModelId,
  type ModelReport,
  type ModelState,
} from "@/lib/bindings";
import { unwrapCommand, useCommand } from "@/lib/ipc";
import { useTauriEvent } from "@/lib/use-event";
import { formatBytes, formatEta, formatRate } from "@/lib/format";
import { ErrorSurface, ProgressBar, Skeleton } from "@/components/global";

const STATE_LABEL: Readonly<Record<ModelState["kind"], string>> = {
  NOT_DOWNLOADED: "Not downloaded",
  DOWNLOADING: "Downloading",
  VERIFYING: "Verifying",
  OPTIMIZING: "Optimising for the Neural Engine — 15 to 60 seconds, once",
  READY: "Ready",
  FAILED: "Failed",
};

export function ModelManager() {
  const models = useCommand(commands.listModels, []);
  const [progress, setProgress] = useState<Readonly<Record<string, DownloadProgress>>>({});
  const [liveStates, setLiveStates] = useState<Readonly<Record<string, ModelState>>>({});

  useTauriEvent(events.modelDownloadProgress, (payload) => {
    setProgress((current) => ({ ...current, [payload.progress.model_id]: payload.progress }));
  });

  useTauriEvent(events.modelStateChanged, (payload) => {
    setLiveStates((current) => ({ ...current, [payload.model_id]: payload.state }));
  });

  /** Forget what we were told about a model we are about to change. */
  const forgetState = useCallback((modelId: ModelId) => {
    setLiveStates((current) => {
      const { [modelId]: _dropped, ...rest } = current;
      return rest;
    });
  }, []);

  const download = useCallback(
    (modelId: string) => {
      forgetState(modelId);
      void unwrapCommand(() => commands.downloadModel({ model_id: modelId })).then(models.reload);
    },
    [forgetState, models.reload],
  );

  const remove = useCallback(
    (modelId: string) => {
      forgetState(modelId);
      void unwrapCommand(() => commands.deleteModel({ model_id: modelId })).then(models.reload);
    },
    [forgetState, models.reload],
  );

  if (models.error) return <ErrorSurface error={models.error} onRetry={models.reload} size="compact" />;
  if (!models.data) return <Skeleton rows={2} />;

  return (
    <ul className="flex flex-col">
      {models.data.map((report) => {
        // The event is the freshest thing we know about this model.
        const model: ModelReport = { ...report, state: liveStates[report.descriptor.id] ?? report.state };
        return (
          <li key={model.descriptor.id} className="hairline-b flex items-center gap-4 py-3 last:border-b-0">
            <ModelSummary model={model} progress={progress[model.descriptor.id]} />
            <ModelAction model={model} onDownload={download} onDelete={remove} />
          </li>
        );
      })}
    </ul>
  );
}

function ModelSummary({ model, progress }: { model: ModelReport; progress: DownloadProgress | undefined }) {
  const { descriptor, state } = model;
  const downloading = state.kind === "DOWNLOADING";
  const received = progress?.received_bytes ?? (downloading ? state.received_bytes : 0);
  const total = progress?.total_bytes ?? (downloading ? state.total_bytes : descriptor.size_bytes);
  const eta = progress ? formatEta(Math.max(0, total - received), progress.bytes_per_second) : null;

  return (
    <div className="min-w-0 flex-1">
      <p className="text-body text-text-primary">{descriptor.display_name}</p>
      <p className="text-caption text-text-secondary">{descriptor.description}</p>

      {downloading ? (
        <ProgressBar
          className="mt-2"
          label={`Downloading ${descriptor.display_name}`}
          fraction={total > 0 ? received / total : 0}
          caption={
            <>
              {formatBytes(received)} of {formatBytes(total)}
              {progress ? ` · ${formatRate(progress.bytes_per_second)}` : ""}
              {eta ? ` · ${eta} left` : ""}
            </>
          }
        />
      ) : (
        <p className="text-caption tabular-nums text-text-tertiary">
          {formatBytes(descriptor.size_bytes)} · {descriptor.approx_ram_mb} MB memory ·{" "}
          {state.kind === "FAILED" ? state.message : STATE_LABEL[state.kind]}
        </p>
      )}
    </div>
  );
}

function ModelAction({
  model,
  onDownload,
  onDelete,
}: {
  model: ModelReport;
  onDownload: (modelId: string) => void;
  onDelete: (modelId: string) => void;
}) {
  const { state, descriptor } = model;
  const busy = state.kind === "DOWNLOADING" || state.kind === "VERIFYING" || state.kind === "OPTIMIZING";

  if (busy) return <span className="shrink-0 text-caption text-text-tertiary">{STATE_LABEL[state.kind]}</span>;

  if (state.kind === "READY") {
    return (
      <button
        type="button"
        aria-label={`Delete ${descriptor.display_name}`}
        onClick={() => onDelete(descriptor.id)}
        className="shrink-0 rounded-input p-1 text-text-secondary transition-colors hover:bg-sunken hover:text-danger"
      >
        <Trash2 className="size-4" />
      </button>
    );
  }

  return (
    <button
      type="button"
      onClick={() => onDownload(descriptor.id)}
      className="hairline flex shrink-0 items-center gap-2 rounded-input bg-sunken px-3 py-1 text-body text-text-primary transition-colors hover:bg-sunken-strong"
    >
      <Download className="size-4" />
      {state.kind === "FAILED" ? "Try again" : "Download"}
    </button>
  );
}
