/**
 * SOURCE OF TRUTH KEYWORDS: CommandResult, useCommand, CommandState, toAppError,
 *   INTERNAL_ERROR, unwrapCommand
 * WHAT:  The one way the frontend calls a Rust command: a hook returning
 *        { data, error, loading, reload } where error is always an AppError.
 * WHY:   tauri-specta returns a result union for handled failures but THROWS a
 *        real Error for transport failures — a webview open outside Tauri, an
 *        ACL denial, a panicking handler. Two failure channels means every
 *        caller writes the same try/catch, and the one that forgets renders a
 *        white screen. This collapses both into AppError, which is the single
 *        error surface the architecture already committed to (CLAUDE.md §4), so
 *        ErrorSurface can render any failure without knowing where it came from.
 * WHERE: Used by every dashboard view and by onboarding. Wraps commands from
 *        lib/bindings.ts. Re-exported from src/lib/index.ts.
 */

import { useCallback, useEffect, useState, type DependencyList } from "react";
import type { AppError } from "./bindings";

export type CommandResult<T> = { status: "ok"; data: T } | { status: "error"; error: AppError };

export interface CommandState<T> {
  data: T | null;
  error: AppError | null;
  loading: boolean;
  reload: () => void;
}

/** A thrown transport failure, given the shape every error surface understands. */
export function toAppError(cause: unknown): AppError {
  if (cause !== null && typeof cause === "object" && "code" in cause && "message" in cause) {
    return cause as AppError;
  }
  return {
    code: "INTERNAL",
    message: "Murmur could not reach its background service.",
    recoverable: true,
    action: { kind: "RETRY" },
    detail: cause instanceof Error ? cause.message : String(cause),
  };
}

/** Await a command and get its value or its AppError — no throw, no union. */
export async function unwrapCommand<T>(run: () => Promise<CommandResult<T>>): Promise<CommandResult<T>> {
  try {
    return await run();
  } catch (cause) {
    return { status: "error", error: toAppError(cause) };
  }
}

/**
 * WHAT:  Runs `command` on mount and whenever `deps` change.
 * WHY:   A generation counter rather than an AbortController: commands cannot be
 *        cancelled once dispatched, so the only correct thing is to ignore a
 *        stale answer. Without it, a slow first call lands after a fast reload
 *        and the view shows the older data with no way to tell.
 */
export function useCommand<T>(command: () => Promise<CommandResult<T>>, deps: DependencyList): CommandState<T> {
  const [state, setState] = useState<{ data: T | null; error: AppError | null; loading: boolean }>({
    data: null,
    error: null,
    loading: true,
  });
  const [generation, setGeneration] = useState(0);

  const reload = useCallback(() => setGeneration((value) => value + 1), []);

  useEffect(() => {
    let live = true;
    setState((previous) => ({ ...previous, loading: true }));

    void unwrapCommand(command).then((result) => {
      if (!live) return;
      if (result.status === "ok") setState({ data: result.data, error: null, loading: false });
      else setState({ data: null, error: result.error, loading: false });
    });

    return () => {
      live = false;
    };
    // `command` is deliberately not a dependency: callers pass an inline arrow,
    // and depending on its identity would re-run the command every render.
    // The caller's `deps` are the declared inputs, and `generation` is reload().
  }, [...deps, generation]);

  return { ...state, reload };
}
