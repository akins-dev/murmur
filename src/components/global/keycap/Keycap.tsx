/**
 * SOURCE OF TRUTH KEYWORDS: Keycap, KeycapProps, KeycapSize, KEYCAP_SIZE_CLASS
 * WHAT:  Renders a hotkey as physical keys — one cap per element of `keys`,
 *        e.g. ["⌥", "Space"].
 * WHY:   A hotkey written as plain text reads as prose and users skim past it;
 *        as a key it reads as something to press. Caps are square at minimum so
 *        a single glyph still looks like a key, and widen with their content so
 *        a word stays one key rather than becoming a box of text (docs/04 §11).
 *        The caller supplies the glyphs — this component knows nothing about
 *        which hotkey the app uses.
 * WHERE: The empty-state slot ("Press ⌥Space anywhere to start"), the settings
 *        hotkey control, and onboarding's hotkey step.
 */

import type { Ref } from "react";
import { cn } from "@/lib/utils";

export type KeycapSize = "sm" | "md";

export interface KeycapProps {
  /** One entry per cap, already in display form: ["⌘", "F"]. */
  keys: readonly string[];
  size?: KeycapSize;
  className?: string;
  ref?: Ref<HTMLSpanElement>;
}

const KEYCAP_SIZE_CLASS: Readonly<Record<KeycapSize, string>> = {
  sm: "h-[var(--keycap-height-sm)] min-w-[var(--keycap-min-width-sm)] text-caption px-1",
  md: "h-[var(--keycap-height-md)] min-w-[var(--keycap-min-width-md)] text-label px-2",
};

export function Keycap({ keys, size = "md", className, ref }: KeycapProps) {
  return (
    <span ref={ref} className={cn("inline-flex items-center gap-1", className)}>
      {keys.map((key, index) => (
        <kbd
          key={`${key}-${index}`}
          className={cn(
            "material-elevated inline-flex items-center justify-center rounded-[var(--keycap-radius)]",
            "font-mono text-text-primary",
            KEYCAP_SIZE_CLASS[size],
          )}
        >
          {key}
        </kbd>
      ))}
    </span>
  );
}
