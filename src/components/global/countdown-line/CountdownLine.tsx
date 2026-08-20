/**
 * SOURCE OF TRUTH KEYWORDS: CountdownLine, CountdownLineProps, CountdownLineState,
 *   remainingMs, DRIFT_MS, scaleX, WebAnimations, countdown-line-height
 * WHAT:  A horizontal line that starts full and drains to empty over the time
 *        the backend says is left. Can be handed back (refilled) mid-drain.
 * WHY:   REPLACES THE COUNTDOWN RING, WHICH WAS BROKEN, AND THE BUG IS WORTH
 *        KNOWING BECAUSE IT IS EASY TO REINTRODUCE. The ring re-ran its effect
 *        on [state, durationMs], and durationMs WAS the state's remaining_ms —
 *        a value Rust re-emits several times a second. So every tick cancelled
 *        the running animation and started a fresh full-to-empty drain over the
 *        new remainder. It never progressed and it flickered, because it was
 *        being restarted five times a second. Two clocks disagreeing, exactly.
 *
 *        The fix is that the animation is created ONCE, when the drain begins,
 *        and later ticks may only SEEK it — never restart it. Rust stays the
 *        authority (there is no local countdown here, no setInterval, no
 *        second copy of the deadline), but its ticks correct a running
 *        animation instead of replacing one. A seek is skipped entirely unless
 *        the two disagree by more than DRIFT_MS, so ordinary tick jitter costs
 *        nothing and a genuine correction still lands. This is the same
 *        anchor-and-interpolate shape as app/pill/use-elapsed.ts, which is the
 *        established answer in this codebase for "backend owns the clock, the
 *        view owns the frames".
 *
 *        Drawn with transform: scaleX rather than width so the drain is a
 *        compositor job and costs no layout, which matters because it runs
 *        inside the pill while an audio thread is live. Linear, and it stays
 *        linear under prefers-reduced-motion: it is a clock, and an eased
 *        clock lies about how much time is left (docs/04 §6).
 * WHERE: The pill's cancel-armed state (docs/04 §7). Height is
 *        --countdown-line-height; its WIDTH is whatever slot it is given, so it
 *        occupies exactly the space the waveform it replaces did.
 */

import { useEffect, useRef } from "react";
import { cn } from "@/lib/utils";
import { readDurationMs } from "@/lib/motion";

export type CountdownLineState = "draining" | "refilling";

export interface CountdownLineProps {
  /** Milliseconds left, straight off the session state. The backend owns this. */
  remainingMs: number;
  state: CountdownLineState;
  /** Announced by VoiceOver; the pill is a live region (docs/04 §10). */
  label?: string;
  className?: string;
}

/** How far the animation may disagree with the backend before we correct it.
 *  Below this, a seek would be visible jitter correcting invisible error. */
const DRIFT_MS = 120;

export function CountdownLine({ remainingMs, state, label, className }: CountdownLineProps) {
  const fillRef = useRef<HTMLSpanElement>(null);
  const animationRef = useRef<Animation | null>(null);
  /** The full duration, captured when the drain starts. Deliberately a ref:
   *  reading it must never re-run the effect that owns the animation. */
  const totalRef = useRef(0);
  const remainingRef = useRef(remainingMs);
  remainingRef.current = remainingMs;

  // Owns the animation. Keyed ONLY on `state` — putting remainingMs in here is
  // precisely the bug this component exists to fix.
  useEffect(() => {
    const fill = fillRef.current;
    if (!fill) return;

    // Where the line actually is right now, before we drop the current
    // animation — a refill has to be handed back from here, not reset to full.
    const currentScale = new DOMMatrixReadOnly(getComputedStyle(fill).transform).a;
    animationRef.current?.cancel();
    animationRef.current = null;

    if (state === "draining") {
      const total = Math.max(0, remainingRef.current);
      totalRef.current = total;
      const animation = fill.animate(
        [{ transform: "scaleX(1)" }, { transform: "scaleX(0)" }],
        { duration: total, easing: "linear", fill: "forwards" },
      );
      animationRef.current = animation;
    } else {
      // A second Escape: nothing was lost, so the line springs back to full
      // from wherever the drain reached rather than snapping.
      animationRef.current = fill.animate(
        [{ transform: `scaleX(${currentScale})` }, { transform: "scaleX(1)" }],
        {
          duration: readDurationMs("--motion-duration-medium"),
          easing: getComputedStyle(document.documentElement)
            .getPropertyValue("--motion-ease-standard")
            .trim(),
          fill: "forwards",
        },
      );
    }

    return () => {
      animationRef.current?.cancel();
      animationRef.current = null;
    };
  }, [state]);

  // Rust's ticks CORRECT the running animation; they never replace it.
  useEffect(() => {
    const animation = animationRef.current;
    if (!animation || state !== "draining") return;
    const total = totalRef.current;
    if (total <= 0) return;
    const elapsed = Number(animation.currentTime ?? 0);
    if (Math.abs(total - elapsed - remainingMs) <= DRIFT_MS) return;
    animation.currentTime = Math.min(total, Math.max(0, total - remainingMs));
  }, [remainingMs, state]);

  return (
    <span
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={undefined}
      className={cn(
        "block h-[var(--countdown-line-height)] w-full overflow-hidden rounded-pill bg-sunken-strong",
        className,
      )}
    >
      <span
        ref={fillRef}
        aria-hidden="true"
        className="block h-full w-full origin-left rounded-pill bg-text-primary"
      />
    </span>
  );
}
