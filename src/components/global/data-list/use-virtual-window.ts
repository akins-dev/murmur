/**
 * SOURCE OF TRUTH KEYWORDS: useVirtualWindow, VirtualWindow, VirtualWindowOptions,
 *   overscan, offsetY, totalHeight, scrollTop
 * WHAT:  Given a row count, a uniform row height and a scroll container, returns
 *        the index range that is actually on screen plus the spacer geometry.
 * WHY:   Uniform row height is the whole trick: the visible range is arithmetic
 *        on scrollTop, so a list of tens of thousands of rows costs the same as
 *        a list of ten and nothing is ever measured. Scroll events are coalesced
 *        into one rAF because a trackpad fires them faster than frames, and a
 *        setState per event would drop the frame the scroll is trying to paint.
 *        Written here rather than pulled in: it is forty lines against a new
 *        dependency, and CLAUDE.md §1 says confirm before adding a library.
 * WHERE: Used only by DataList.tsx. Row height and overscan default to the
 *        --row-height / --row-overscan tokens, read by DataList.
 */

import { useEffect, useState, type RefObject } from "react";

export interface VirtualWindowOptions {
  count: number;
  rowHeight: number;
  overscan: number;
  containerRef: RefObject<HTMLElement | null>;
}

export interface VirtualWindow {
  /** First rendered index, inclusive. */
  start: number;
  /** Last rendered index, exclusive. */
  end: number;
  /** Pixel offset of `start` — what the rendered window is translated by. */
  offsetY: number;
  /** Height of the full list, so the scrollbar is honest. */
  totalHeight: number;
  viewportHeight: number;
}

export function useVirtualWindow({ count, rowHeight, overscan, containerRef }: VirtualWindowOptions): VirtualWindow {
  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(0);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    let frame = 0;
    const onScroll = () => {
      if (frame) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        setScrollTop(container.scrollTop);
      });
    };

    const observer = new ResizeObserver(() => setViewportHeight(container.clientHeight));
    observer.observe(container);
    container.addEventListener("scroll", onScroll, { passive: true });
    setViewportHeight(container.clientHeight);
    setScrollTop(container.scrollTop);

    return () => {
      if (frame) cancelAnimationFrame(frame);
      container.removeEventListener("scroll", onScroll);
      observer.disconnect();
    };
  }, [containerRef]);

  const visibleRows = Math.ceil(viewportHeight / rowHeight) + 1;
  const start = Math.max(0, Math.floor(scrollTop / rowHeight) - overscan);
  const end = Math.min(count, start + visibleRows + overscan * 2);

  return { start, end, offsetY: start * rowHeight, totalHeight: count * rowHeight, viewportHeight };
}
