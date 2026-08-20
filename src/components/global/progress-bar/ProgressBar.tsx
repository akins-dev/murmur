/**
 * SOURCE OF TRUTH KEYWORDS: ProgressBar, ProgressBarProps, fraction, indeterminate,
 *   progressbar-role
 * WHAT:  A determinate progress track with an optional caption slot beneath it.
 * WHY:   Determinate by default and indeterminate only when explicitly told
 *        `fraction={null}` — the app downloads a 574MB file and compiles a model
 *        for up to a minute, and an indeterminate bar on either reads as a hang
 *        (docs/04 §9). Making "I don't know" the awkward option is the point.
 *        The caption is a slot rather than a formatted string so the same bar
 *        can carry bytes and a rate here and a step name there.
 * WHERE: The model manager in Settings and the model step in onboarding.
 */

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface ProgressBarProps {
  /** 0..1. Null means genuinely unknowable — use it sparingly. */
  fraction: number | null;
  /** Accessible name for the operation in progress. */
  label: string;
  caption?: ReactNode;
  className?: string;
}

export function ProgressBar({ fraction, label, caption, className }: ProgressBarProps) {
  const percent = fraction === null ? null : Math.max(0, Math.min(1, fraction)) * 100;

  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <div
        role="progressbar"
        aria-label={label}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={percent ?? undefined}
        className="h-1 w-full overflow-hidden rounded-pill bg-sunken"
      >
        <div
          className={cn("h-full rounded-pill bg-accent", percent === null && "w-1/3 animate-pulse")}
          style={percent === null ? undefined : { width: `${percent}%` }}
        />
      </div>
      {caption ? <p className="text-caption tabular-nums text-text-tertiary">{caption}</p> : null}
    </div>
  );
}
