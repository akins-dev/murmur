/**
 * SOURCE OF TRUTH KEYWORDS: cn, clsx, twMerge, ClassValue, classNames,
 *   extendTailwindMerge, TYPE_SCALE, font-size-collision
 * WHAT:  cn() — merges conditional class names and resolves Tailwind conflicts,
 *        so a caller's className always wins over a component's default.
 * WHY:   Without twMerge, `cn("p-4", "p-2")` emits both and the winner is
 *        decided by stylesheet order rather than by the call site, which makes
 *        every global component un-overridable in exactly the cases it exists
 *        to serve. clsx alone handles the conditionals but not the conflict.
 *
 *        THE TYPE SCALE HAS TO BE DECLARED HERE, and leaving it out was a real
 *        bug with a large blast radius. tailwind-merge knows Tailwind's own
 *        `text-sm`/`text-lg` scale and nothing about ours, so it filed
 *        `text-display` under text-COLOUR — the same group as `text-text-primary`
 *        — decided the two conflicted, and silently dropped the size. Every
 *        value in the app that combined a size and a colour through cn() was
 *        rendering at the inherited 13px body size, including the hero stat,
 *        which docs/04 §12 makes the loudest thing on its screen. A 40px number
 *        was shipping at 13px and the interface read as flat and generic
 *        because its entire typographic hierarchy was being deleted at runtime.
 *
 *        It is invisible in review — every component's class string is correct —
 *        and invisible to `tsc`. Adding a name to §5 of the design system means
 *        adding it to TYPE_SCALE below, or it silently stops working the moment
 *        a colour is set alongside it.
 * WHERE: Used by every component in src/components/global. Re-exported from
 *        src/lib/index.ts. Names mirror the @theme keys in styles/global.css.
 */

import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

/** The §5 roles, exactly as global.css publishes them as --text-* theme keys. */
const TYPE_SCALE = ["display", "title", "heading", "body", "label", "caption", "mono"] as const;

const twMerge = extendTailwindMerge({
  extend: {
    classGroups: {
      "font-size": [{ text: [...TYPE_SCALE] }],
    },
  },
});

export function cn(...inputs: ClassValue[]): string {
  return twMerge(clsx(inputs));
}
