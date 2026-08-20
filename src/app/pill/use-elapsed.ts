/**
 * SOURCE OF TRUTH KEYWORDS: useElapsed, elapsedAnchor, performance-now, tickSeconds
 * WHAT:  Turns the `elapsed_ms` carried on a session state into a timer that
 *        keeps counting between events.
 * WHY:   SessionStateChanged is emitted on TRANSITIONS, and recording is one
 *        state — so a timer rendered straight from the event would freeze at
 *        whatever it read when RECORDING began. This anchors on the value Rust
 *        sent plus the monotonic time since it arrived, so it is a VIEW of the
 *        backend's clock rather than a second copy of it, and it re-anchors on
 *        every event Rust does send. performance.now() rather than Date.now()
 *        because a clock change mid-recording must not make the timer jump.
 *        A second Escape returns CANCEL_PENDING -> RECORDING carrying the SAME
 *        elapsed_ms, deliberately, because nothing was lost — so an unchanged
 *        value must NOT re-anchor or the timer would rewind by the length of
 *        the countdown. The idle reset below is what keeps that from also
 *        meaning a fresh session inherits the previous session's anchor.
 *        State is the whole second, so React bails out of ~4 of every 5 ticks
 *        and the pill re-renders once a second instead of five times.
 * WHERE: Used by the pill's RECORDING and CANCEL_PENDING states.
 */

import { useEffect, useRef, useState } from "react";

/** Sampled often enough that the displayed second is never visibly late. */
const TICK_MS = 200;

export function useElapsed(elapsedMs: number | null): number {
  // base -1 is "no session": a real elapsed_ms of 0 must still re-anchor.
  const anchor = useRef({ base: -1, at: 0 });
  const [seconds, setSeconds] = useState(0);

  if (elapsedMs === null) {
    anchor.current = { base: -1, at: 0 };
  } else if (anchor.current.base !== elapsedMs) {
    anchor.current = { base: elapsedMs, at: performance.now() };
  }

  useEffect(() => {
    if (elapsedMs === null) return;
    const tick = () => {
      const total = anchor.current.base + (performance.now() - anchor.current.at);
      setSeconds(Math.floor(total / 1000));
    };
    tick();
    const handle = window.setInterval(tick, TICK_MS);
    return () => window.clearInterval(handle);
  }, [elapsedMs]);

  return elapsedMs === null ? 0 : seconds * 1000;
}
