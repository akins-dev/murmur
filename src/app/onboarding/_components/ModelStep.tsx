/**
 * SOURCE OF TRUTH KEYWORDS: ModelStep, ModelReport, ModelState, downloadModel,
 *   modelDownloadProgress, onboardingProgress, OPTIMIZING, ProgressBar
 * WHAT:  The model step: downloads the default model with real progress, then
 *        shows the Neural Engine compile as its own named stage.
 * WHY:   Two long waits, and they are different waits. The download is bytes
 *        over a network — it gets a real fraction, a real rate and a real
 *        estimate. The compile takes 15 to 60 seconds and has no byte count, so
 *        it gets the honest thing instead: what is happening and roughly how
 *        long, from the onboarding-progress event when it carries a fraction.
 *        Neither gets a bare spinner, because this is the first impression of an
 *        app whose entire claim is that it is instant.
 * WHERE: Step two of onboarding.
 */

import { useCallback, useState } from "react";
import { Download } from "lucide-react";
import { commands, events, type DownloadProgress, type ModelReport } from "@/lib/bindings";
import { unwrapCommand } from "@/lib/ipc";
import { useTauriEvent } from "@/lib/use-event";
import { formatBytes, formatEta, formatRate } from "@/lib/format";
import { ProgressBar } from "@/components/global";

export interface ModelStepProps {
  model: ModelReport;
  onChanged: () => void;
}

export function ModelStep({ model, onChanged }: ModelStepProps) {
  const [progress, setProgress] = useState<DownloadProgress | null>(null);
  const [stage, setStage] = useState<{ message: string; fraction: number | null } | null>(null);

  useTauriEvent(events.modelDownloadProgress, (payload) => {
    if (payload.progress.model_id === model.descriptor.id) setProgress(payload.progress);
  });

  useTauriEvent(events.onboardingProgress, (payload) => {
    setStage({ message: payload.message, fraction: payload.fraction });
  });

  const start = useCallback(() => {
    void unwrapCommand(() => commands.downloadModel({ model_id: model.descriptor.id })).then(onChanged);
  }, [model.descriptor.id, onChanged]);

  const { descriptor, state } = model;

  if (state.kind === "DOWNLOADING" || progress !== null) {
    const received = progress?.received_bytes ?? (state.kind === "DOWNLOADING" ? state.received_bytes : 0);
    const total = progress?.total_bytes ?? descriptor.size_bytes;
    const eta = progress ? formatEta(Math.max(0, total - received), progress.bytes_per_second) : null;

    return (
      <ProgressBar
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
    );
  }

  if (state.kind === "VERIFYING" || state.kind === "OPTIMIZING") {
    const optimizing = state.kind === "OPTIMIZING";
    return (
      <ProgressBar
        label={optimizing ? "Optimising the model" : "Verifying the model"}
        fraction={stage?.fraction ?? null}
        caption={
          stage?.message ??
          (optimizing
            ? "Preparing the model for the Neural Engine. 15 to 60 seconds, once on this Mac."
            : "Checking the download is intact.")
        }
      />
    );
  }

  if (state.kind === "READY") {
    return (
      <p className="text-body text-text-secondary">
        {descriptor.display_name} is ready — {formatBytes(descriptor.size_bytes)} on disk.
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-body text-text-secondary">
        {descriptor.description} {formatBytes(descriptor.size_bytes)} to download, about{" "}
        {descriptor.approx_ram_mb} MB of memory while running.
      </p>
      {state.kind === "FAILED" ? <p className="text-body text-danger">{state.message}</p> : null}
      <button
        type="button"
        onClick={start}
        className="hairline flex h-8 w-fit items-center gap-2 rounded-input bg-sunken px-3 text-body text-text-primary transition-colors hover:bg-sunken-strong"
      >
        <Download className="size-4" />
        {state.kind === "FAILED" ? "Try again" : "Download"}
      </button>
    </div>
  );
}
