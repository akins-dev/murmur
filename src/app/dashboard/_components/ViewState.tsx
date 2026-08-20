/**
 * SOURCE OF TRUTH KEYWORDS: ViewState, ViewStateProps, loading-skeleton,
 *   ErrorSurface, CommandState
 * WHAT:  The loading / error / empty gate every command-backed view sits behind.
 * WHY:   Three views each fetching one command means three chances to render a
 *        blank panel while a promise settles, or to forget the error case
 *        entirely. This makes all three states the caller's decision once, and
 *        renders the same ErrorSurface for any AppError. The loading state is a
 *        quiet pulse rather than a spinner — these commands are local SQLite
 *        reads that finish in single-digit milliseconds, so a spinner would only
 *        ever be seen as a flash.
 * WHERE: Wraps the body of Stats, History and Settings.
 */

import type { ReactNode } from "react";
import type { AppError } from "@/lib/bindings";
import { ErrorSurface, Skeleton } from "@/components/global";

export interface ViewStateProps<T> {
  state: { data: T | null; error: AppError | null; loading: boolean };
  onRetry: () => void;
  onOpenSettings?: (section: string) => void;
  children: (data: T) => ReactNode;
}

export function ViewState<T>({ state, onRetry, onOpenSettings, children }: ViewStateProps<T>) {
  if (state.error) {
    return <ErrorSurface error={state.error} onRetry={onRetry} onOpenSettings={onOpenSettings} />;
  }
  if (state.data === null) {
    return state.loading ? <Skeleton rows={3} /> : null;
  }
  return <>{children(state.data)}</>;
}
