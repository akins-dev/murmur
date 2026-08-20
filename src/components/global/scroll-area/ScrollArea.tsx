/**
 * SOURCE OF TRUTH KEYWORDS: ScrollArea, ScrollAreaProps, scroll-fade,
 *   useScrollEdges, data-fade-top, data-fade-bottom, page-header-height
 * WHAT:  The dashboard's scrolling surface: one scroll container with a smart
 *        fade at each edge and no scrollbar.
 * WHY:   Every page scrolled its own way before this — Stats and Settings each
 *        put overflow on their own <section>, History delegated scrolling to a
 *        virtualised list, and the page title lived INSIDE the scroller on two
 *        of the three, so it scrolled away and took the window's drag region
 *        with it. That is why the window was hard to grab. Scrolling is now one
 *        component so there is one place that decides how a page scrolls, what
 *        its edges look like, and where its content starts.
 *
 *        `headerInset` is not decoration: the page header OVERLAYS this
 *        container rather than sitting above it, because the operator asked for
 *        content to tuck under the header and fade. Overlaying means the first
 *        line of content would otherwise be born underneath the title, so the
 *        content is padded down by exactly the header's height. Content must
 *        not end up hidden — that padding IS the guarantee, and it is why the
 *        inset is a prop rather than something each page remembers to add.
 * WHERE: Wraps the body of every dashboard page. History does not use it — its
 *        scroller is DataList's own virtualised one, which applies the same
 *        utility and the same hook directly.
 */

import { useRef, type CSSProperties, type ReactNode } from "react";
import { cn } from "@/lib/utils";
import { useScrollEdges } from "./use-scroll-edges";

export interface ScrollAreaProps {
  children: ReactNode;
  /** Pad the content down past the overlaying page header. On by default —
   *  every dashboard page has one. */
  headerInset?: boolean;
  className?: string;
  contentClassName?: string;
}

export function ScrollArea({
  children,
  headerInset = true,
  className,
  contentClassName,
}: ScrollAreaProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const { hasAbove, hasBelow } = useScrollEdges(scrollRef);

  return (
    <div
      ref={scrollRef}
      data-scroll-area=""
      data-fade-top={hasAbove}
      data-fade-bottom={hasBelow}
      style={
        headerInset
          ? {
              // Content must dissolve completely before it reaches the title,
              // not slide behind it and show through.
              "--fade-size-top":
                "calc(var(--page-header-height) + var(--scroll-fade-size))",
            } as CSSProperties
          : undefined
      }
      className={cn("scroll-fade min-h-0 flex-1 overflow-y-auto", className)}
    >
      <div className={cn(headerInset && "pt-[var(--page-header-height)]", contentClassName)}>
        {children}
      </div>
    </div>
  );
}
