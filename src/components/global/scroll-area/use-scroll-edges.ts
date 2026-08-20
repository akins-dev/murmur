/**
 * SOURCE OF TRUTH KEYWORDS: useScrollEdges, ScrollEdges, hasAbove, hasBelow,
 *   EDGE_EPSILON, ResizeObserver
 * WHAT:  Whether a scroll container currently has content hidden above it and
 *        below it. The two answers that make the scroll fade "smart".
 * WHY:   Both edges have to be measured, not assumed, and re-measured on far
 *        more than scrolling. A fade that is correct only until the window is
 *        resized is the same bug as no fade at all, just harder to notice —
 *        so this watches THREE things: the scroll position, the size of the
 *        viewport, and the size of the content inside it. Content changing
 *        height is the one people forget: a list that finishes loading, an
 *        error banner appearing, a section collapsing. Each of those changes
 *        whether there is anything left to hide without any scroll event ever
 *        firing.
 *
 *        Measurement is coalesced into one animation frame. Scroll fires far
 *        faster than paint, and setting state per event would put React work
 *        on every wheel tick of a virtualised list. The comparison before
 *        setState matters just as much: these are two booleans that change a
 *        handful of times per scroll, so returning the previous object unless
 *        one actually flipped keeps a continuous scroll at zero re-renders.
 *
 *        EDGE_EPSILON exists because scrollHeight, clientHeight and scrollTop
 *        are fractional under a non-integer device pixel ratio, and an exact
 *        comparison leaves the bottom fade stuck on by a third of a pixel
 *        forever on exactly the pages that have nothing left to show.
 * WHERE: ScrollArea uses it for the page fades; DataList uses it directly on
 *        its own virtualised scroller, which is a different element from the
 *        one ScrollArea would create.
 */

import { useEffect, useState, type RefObject } from "react";

export interface ScrollEdges {
  hasAbove: boolean;
  hasBelow: boolean;
}

/** Sub-pixel slack — see the note on fractional metrics above. */
const EDGE_EPSILON = 1;

const NONE: ScrollEdges = { hasAbove: false, hasBelow: false };

export function useScrollEdges(ref: RefObject<HTMLElement | null>): ScrollEdges {
  const [edges, setEdges] = useState<ScrollEdges>(NONE);

  useEffect(() => {
    const container = ref.current;
    if (!container) return;

    let frame = 0;

    const measure = () => {
      frame = 0;
      const hasAbove = container.scrollTop > EDGE_EPSILON;
      const hasBelow =
        container.scrollTop + container.clientHeight < container.scrollHeight - EDGE_EPSILON;
      setEdges((previous) =>
        previous.hasAbove === hasAbove && previous.hasBelow === hasBelow
          ? previous
          : { hasAbove, hasBelow },
      );
    };

    const schedule = () => {
      if (frame) return;
      frame = requestAnimationFrame(measure);
    };

    container.addEventListener("scroll", schedule, { passive: true });

    // The viewport AND the content: either resizing changes the answer.
    const observer = new ResizeObserver(schedule);
    observer.observe(container);
    for (const child of Array.from(container.children)) observer.observe(child);

    measure();

    return () => {
      if (frame) cancelAnimationFrame(frame);
      container.removeEventListener("scroll", schedule);
      observer.disconnect();
    };
  }, [ref]);

  return edges;
}
