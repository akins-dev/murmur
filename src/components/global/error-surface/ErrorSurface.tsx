/**
 * SOURCE OF TRUTH KEYWORDS: ErrorSurface, ErrorSurfaceProps, ACTION_LABEL,
 *   ErrorAction, AppError, runErrorAction
 * WHAT:  Renders an AppError as something a user can act on: the plain-language
 *        message, and the one button its ErrorAction says would fix it.
 * WHY:   AppError is the only error crossing IPC precisely so the UI has one
 *        error surface rather than forty (CLAUDE.md §4) — this is that surface.
 *        The two recoveries that are pure IPC, opening a privacy pane and
 *        re-downloading a model, are performed here rather than re-implemented
 *        by each caller; retry and navigation are callbacks, because only the
 *        host knows what to re-run or where to go. Action wording lives in one
 *        map so the same remedy never reads two ways in two views. `detail` is
 *        never rendered: it is a sqlite or CoreAudio string, which is noise to
 *        the user and a support burden to us.
 * WHERE: Every dashboard view, the model manager and onboarding. Built on
 *        EmptyState so a failure and an absence share one visual language.
 */

import { TriangleAlert } from "lucide-react";
import { cn } from "@/lib/utils";
import { commands, type AppError, type ErrorAction } from "@/lib/bindings";
import { EmptyState, type EmptyStateSize } from "../empty-state";

const ACTION_LABEL: Readonly<Record<ErrorAction["kind"], string>> = {
  OPEN_PRIVACY_PANE: "Open System Settings",
  OPEN_SETTINGS: "Open Settings",
  DOWNLOAD_MODEL: "Download again",
  RETRY: "Try again",
};

export interface ErrorSurfaceProps {
  error: AppError;
  /** Re-runs whatever failed. Required for a RETRY action to appear. */
  onRetry?: () => void;
  /** Navigates to one of our own settings sections. */
  onOpenSettings?: (section: string) => void;
  size?: EmptyStateSize;
  className?: string;
}

export function ErrorSurface({ error, onRetry, onOpenSettings, size, className }: ErrorSurfaceProps) {
  const action = error.action;

  const run = (): void => {
    if (!action) return;
    switch (action.kind) {
      case "OPEN_PRIVACY_PANE":
        void commands.openPrivacyPane({ permission: action.pane });
        break;
      case "DOWNLOAD_MODEL":
        void commands.downloadModel({ model_id: action.model_id });
        break;
      case "OPEN_SETTINGS":
        onOpenSettings?.(action.section);
        break;
      case "RETRY":
        onRetry?.();
        break;
    }
  };

  const canRun =
    action !== null &&
    (action.kind === "OPEN_PRIVACY_PANE" ||
      action.kind === "DOWNLOAD_MODEL" ||
      (action.kind === "OPEN_SETTINGS" && onOpenSettings !== undefined) ||
      (action.kind === "RETRY" && onRetry !== undefined));

  return (
    <EmptyState
      size={size}
      className={className}
      icon={<TriangleAlert className={cn("size-6", error.recoverable ? "text-warning" : "text-danger")} />}
      headline={error.message}
      action={
        canRun && action ? (
          <button
            type="button"
            onClick={run}
            className="hairline h-8 rounded-input bg-sunken px-3 text-body text-text-primary transition-colors hover:bg-sunken-strong"
          >
            {ACTION_LABEL[action.kind]}
          </button>
        ) : null
      }
    />
  );
}
