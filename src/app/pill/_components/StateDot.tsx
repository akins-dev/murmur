/**
 * SOURCE OF TRUTH KEYWORDS: StateDot, StateDotProps, PillTone, TONE_CLASS
 * WHAT:  The pill's status dot, in the tone of the current state, breathing
 *        while recording.
 * WHY:   THERE ARE ONLY TWO TONES NOW, and that is the point. The app has no
 *        accent hue any more (docs/04 §2), so a dot cannot carry state by
 *        colour — and it never really did. What tells the states apart is
 *        FORM: a moving waveform, a draining line, a sentence. The dot's job
 *        shrank to "something is happening" plus the one distinction worth a
 *        colour, which is that something went wrong.
 *
 *        The breathe is a CSS keyframe animation, not a spring or a JS loop: it
 *        runs for the entire length of a recording, and anything driven from JS
 *        here would be competing with the audio thread for frames every frame
 *        for minutes. Under prefers-reduced-motion the global rule collapses it
 *        rather than this component branching.
 * WHERE: The left slot of the pill, every state.
 */

import { cn } from "@/lib/utils";

export type PillTone = "ink" | "danger";

export interface StateDotProps {
  tone: PillTone;
  /** The 2s opacity pulse of docs/04 §7 — recording only. */
  pulsing?: boolean;
}

const TONE_CLASS: Readonly<Record<PillTone, string>> = {
  ink: "bg-text-primary",
  danger: "bg-danger",
};

export function StateDot({ tone, pulsing = false }: StateDotProps) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "size-[var(--pill-dot-size)] shrink-0 rounded-pill",
        TONE_CLASS[tone],
        pulsing && "animate-breathe",
      )}
    />
  );
}
