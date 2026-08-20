/**
 * SOURCE OF TRUTH KEYWORDS: springs, SpringName, SpringTransition, MotionTransition,
 *   transitionFor, readPxToken, readNumberToken, readDurationMs, REDUCED_FADE
 * WHAT:  Reads the --spring-* / --motion-* / metric tokens out of the live
 *        stylesheet and hands them to JS as typed objects and numbers.
 * WHY:   Framer needs numbers and a component is not allowed to contain one
 *        (CLAUDE.md §7). This is the seam that makes that rule enforceable
 *        rather than aspirational: springs are authored once in tokens.css and
 *        every animation in the app asks for them by name. Values are memoised
 *        because getComputedStyle forces style resolution — cheap once, a
 *        frame-killer inside the pill's 60fps loop. Reads are lazy so import
 *        order can never beat the stylesheet.
 * WHERE: Consumed by src/components/global/* (countdown-ring, waveform,
 *        data-list). NOT by the pill's appearance or exit: those move the
 *        WINDOW, not the document, and their timing lives in --pill-exit-* for
 *        src-tauri to read. A spring here can only drive motion INSIDE a
 *        surface. Reads src/styles/tokens.css; re-exported from src/lib/index.ts.
 */

/** Framer's own Transition type is not re-exported by framer-motion v13, and
 *  these two shapes are all the app animates with, so they are declared here
 *  rather than reached for through a transitive package. */
export interface SpringTransition {
  readonly type: "spring";
  readonly stiffness: number;
  readonly damping: number;
}

export interface FadeTransition {
  readonly type: "tween";
  readonly duration: number;
  readonly ease: "linear";
}

export type MotionTransition = SpringTransition | FadeTransition;

export type SpringName = "pillAppear" | "state" | "panel";

const SPRING_TOKEN: Readonly<Record<SpringName, string>> = {
  pillAppear: "pill-appear",
  state: "state",
  panel: "panel",
};

const cache = new Map<string, number>();

function rootStyle(): CSSStyleDeclaration {
  return getComputedStyle(document.documentElement);
}

function readNumber(token: string): number {
  const cached = cache.get(token);
  if (cached !== undefined) return cached;

  const parsed = Number.parseFloat(rootStyle().getPropertyValue(token));
  if (!Number.isFinite(parsed)) {
    // A missing token means tokens.css did not load. Failing here names the
    // cause; falling back to a literal would hide it behind a design that is
    // subtly wrong everywhere.
    throw new Error(`Design token ${token} is missing or not numeric.`);
  }
  cache.set(token, parsed);
  return parsed;
}

/** A `<length>` token in px — `--row-height` → 56. */
export function readPxToken(token: string): number {
  return readNumber(token);
}

/** A unitless token — `--waveform-bars` → 24. */
export function readNumberToken(token: string): number {
  return readNumber(token);
}

/** A `<time>` token in ms, whether authored as `120ms` or `2s`. */
export function readDurationMs(token: string): number {
  const raw = rootStyle().getPropertyValue(token).trim();
  const value = Number.parseFloat(raw);
  if (!Number.isFinite(value)) {
    throw new Error(`Design token ${token} is missing or not a duration.`);
  }
  return raw.endsWith("ms") ? value : value * 1000;
}

export function springFor(name: SpringName): SpringTransition {
  const slug = SPRING_TOKEN[name];
  return {
    type: "spring",
    stiffness: readNumber(`--spring-${slug}-stiffness`),
    damping: readNumber(`--spring-${slug}-damping`),
  };
}

/** The one reduced-motion answer: a fade at the app's smallest duration, never
 *  an instant appearance — see docs/04 §6. */
export function reducedFade(): FadeTransition {
  return { type: "tween", duration: readDurationMs("--motion-duration-fast") / 1000, ease: "linear" };
}

/**
 * WHAT:  The spring for `name`, or the fade when the user has asked for less
 *        motion. Pass framer-motion's useReducedMotion() result as `reduced`.
 * WHERE: Every mount/dismiss/morph animation in the app goes through here.
 */
export function transitionFor(name: SpringName, reduced: boolean): MotionTransition {
  return reduced ? reducedFade() : springFor(name);
}
