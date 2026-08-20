/**
 * SOURCE OF TRUTH KEYWORDS: ErrorBoundary, ErrorBoundaryProps, ErrorBoundaryState,
 *   ErrorFallbackArgs, componentDidCatch, getDerivedStateFromError, reset
 * WHAT:  Catches a render error in its subtree and hands the error plus a
 *        reset() to the caller's fallback slot.
 * WHY:   The fallback is a required prop and not a default UI. A boundary that
 *        renders its own apology carries copy, and copy in a global component
 *        is how two screens end up disagreeing about how the app talks; a
 *        boundary that renders nothing is worse, because a blank window is
 *        indistinguishable from a crash. Requiring the slot forces the caller
 *        to decide what a failure looks like where it happens.
 * WHERE: Wraps each dashboard view and the pill root, so one broken view cannot
 *        take the window with it.
 */

import { Component, type ErrorInfo, type ReactNode } from "react";

export interface ErrorFallbackArgs {
  error: Error;
  /** Clears the error and re-renders the subtree — for a retry affordance. */
  reset: () => void;
}

export interface ErrorBoundaryProps {
  children: ReactNode;
  fallback: (args: ErrorFallbackArgs) => ReactNode;
  /** Side channel for logging. Never used for UI. */
  onError?: (error: Error, info: ErrorInfo) => void;
}

interface ErrorBoundaryState {
  error: Error | null;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    this.props.onError?.(error, info);
  }

  private reset = (): void => {
    this.setState({ error: null });
  };

  render(): ReactNode {
    const { error } = this.state;
    if (error) return this.props.fallback({ error, reset: this.reset });
    return this.props.children;
  }
}
