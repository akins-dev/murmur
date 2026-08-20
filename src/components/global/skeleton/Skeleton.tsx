/**
 * SOURCE OF TRUTH KEYWORDS: Skeleton, SkeletonProps, loading-placeholder,
 *   aria-busy, animate-pulse
 * WHAT:  A pulsing placeholder block, optionally repeated as rows.
 * WHY:   Exists so "still loading" is never drawn as "empty" or, worse, as a
 *        wrong value. Every command here is a local SQLite read that finishes in
 *        single-digit milliseconds, so this is rarely seen — but the case it
 *        covers is the one that matters: a settings toggle rendered from its
 *        default before the user's real value arrives reads as the app having
 *        forgotten their preference. A spinner would be wrong for the same
 *        reason it is wrong elsewhere in this app: it implies waiting, and this
 *        implies shape.
 * WHERE: Used by every command-backed view while its first response is in
 *        flight — Stats, History, Settings, the model manager, the dashboard
 *        shell.
 */

import { cn } from "@/lib/utils";

export interface SkeletonProps {
  /** How many stacked bars to draw. One by default. */
  rows?: number;
  className?: string;
}

export function Skeleton({ rows = 1, className }: SkeletonProps) {
  return (
    <div aria-busy="true" aria-live="polite" className="flex flex-col gap-2">
      {Array.from({ length: rows }, (_, index) => (
        <div key={index} className={cn("h-8 animate-pulse rounded-card bg-sunken", className)} />
      ))}
    </div>
  );
}
