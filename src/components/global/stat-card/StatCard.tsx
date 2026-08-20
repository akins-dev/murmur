/**
 * SOURCE OF TRUTH KEYWORDS: StatCard, StatCardProps, StatDelta, StatTone,
 *   StatCardSize, TONE_CLASS, tabular-nums
 * WHAT:  One measurement: a label, a value, an optional delta, and a slot for
 *        a chart or sparkline beneath it.
 * WHY:   No surface of its own by default. Content sits on the material, and a
 *        card-on-glass is the fastest way to make this look like a Vista widget
 *        (docs/04 §2) — pass surface only where a stat genuinely needs to
 *        detach from what is behind it. The value is tabular-nums because a
 *        number that reflows as its digits change reads as cheap, and stats
 *        update live. Tone is opt-in rather than derived from the sign: the app
 *        has one accent by design, so a green number has to be a deliberate
 *        statement about state, not an automatic consequence of arithmetic.
 *        At hero size the delta drops to its own line. Hung off the baseline of
 *        a 40px number it has to be read as part of that number, and it is not —
 *        it is the sentence explaining where the number came from. At default
 *        size it stays inline, because next to a 22px value it reads as an
 *        annotation rather than as a caption fighting for the same line.
 * WHERE: The stats view — one hero card (time saved plus the activity chart)
 *        and the 2×3 grid — and the latency panel. The chart slot takes any
 *        element; there is no chart library in this app (docs/04 §11).
 */

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type StatTone = "neutral" | "positive" | "negative";
export type StatCardSize = "default" | "hero";

export interface StatDelta {
  /** Already formatted by the caller: "+12%", "−4 min". */
  label: string;
  tone?: StatTone;
}

export interface StatCardProps {
  label: string;
  /** Formatted by the caller — formatting rules live in lib/format.ts. */
  value: ReactNode;
  /** Rendered next to the value at label size: "min", "wpm". */
  unit?: ReactNode;
  delta?: StatDelta;
  /** Sparkline, bar chart, heatmap — anything, or nothing. */
  chart?: ReactNode;
  size?: StatCardSize;
  /** Give the card its own elevated material. Off by default, on purpose. */
  surface?: boolean;
  className?: string;
}

const VALUE_CLASS: Readonly<Record<StatCardSize, string>> = {
  default: "text-title",
  hero: "text-display",
};

const TONE_CLASS: Readonly<Record<StatTone, string>> = {
  neutral: "text-text-secondary",
  positive: "text-success",
  negative: "text-danger",
};

export function StatCard({
  label,
  value,
  unit,
  delta,
  chart,
  size = "default",
  surface = false,
  className,
}: StatCardProps) {
  return (
    <div
      className={cn(
        "flex flex-col gap-1",
        surface && "material-elevated rounded-card p-4",
        className,
      )}
    >
      <p className="text-label text-text-secondary">{label}</p>

      <div className="flex items-baseline gap-2">
        <span className={cn(VALUE_CLASS[size], "text-text-primary tabular-nums")}>{value}</span>
        {unit ? <span className="text-label text-text-tertiary">{unit}</span> : null}
        {delta && size !== "hero" ? (
          <span className={cn("text-label tabular-nums", TONE_CLASS[delta.tone ?? "neutral"])}>{delta.label}</span>
        ) : null}
      </div>

      {delta && size === "hero" ? (
        <span className={cn("text-label tabular-nums", TONE_CLASS[delta.tone ?? "neutral"])}>{delta.label}</span>
      ) : null}

      {chart ? <div className="pt-2">{chart}</div> : null}
    </div>
  );
}
