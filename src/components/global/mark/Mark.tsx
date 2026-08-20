/**
 * SOURCE OF TRUTH KEYWORDS: Mark, MarkProps, MarkSize, mark-bars, mark-size-md,
 *   mark-ratio-peak, identity
 * WHAT:  The app's identity mark (docs/04 §12) — the waveform at rest: five
 *        rounded bars, centre-weighted, in the proportions the pill draws while
 *        it is listening.
 * WHY:   The mark is the waveform because the waveform is already the app's
 *        signature object — the user has seen it in the pill before they ever
 *        see a logo, so the identity costs nothing to introduce and is
 *        recognised the first time it appears. Geometry lives in the
 *        `mark-bars` utility as ratios of --mark-size, so this component sets
 *        one custom property and holds no number of its own.
 *
 *        It inherits `currentColor` and every caller is expected to leave that
 *        as --text-primary. The mark is NEVER ember: ember means a session is
 *        live, and a logo that looks live while nothing is happening is the
 *        same lie as a toggle that reads ON while nothing works.
 * WHERE: The top of the dashboard rail, onboarding's first screen, and the
 *        first-run empty state. The menu-bar and bundle icons are the same
 *        shape, exported as assets/mark.svg for the Rust side to consume.
 */

import { cn } from "@/lib/utils";

export type MarkSize = "md" | "lg";

export interface MarkProps {
  size?: MarkSize;
  /** The accessible name. Null renders the mark as decoration, which is right
   *  wherever the product name is already written next to it. */
  label?: string | null;
  className?: string;
}

const SIZE_CLASS: Readonly<Record<MarkSize, string>> = {
  md: "[--mark-size:var(--mark-size-md)]",
  lg: "[--mark-size:var(--mark-size-lg)]",
};

/** Five bars, and the utility decides how tall each one is. */
const BARS = [0, 1, 2, 3, 4] as const;

export function Mark({ size = "md", label = null, className }: MarkProps) {
  return (
    <span
      role={label ? "img" : undefined}
      aria-label={label ?? undefined}
      aria-hidden={label ? undefined : "true"}
      className={cn("mark-bars text-text-primary", SIZE_CLASS[size], className)}
    >
      {BARS.map((bar) => (
        <span key={bar} />
      ))}
    </span>
  );
}
