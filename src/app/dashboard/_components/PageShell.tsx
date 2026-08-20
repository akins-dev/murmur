/**
 * SOURCE OF TRUTH KEYWORDS: PageShell, PageHeader, page-header-height,
 *   data-tauri-drag-region, data-scroll-area, traffic-lights
 * WHAT:  The frame every dashboard page is rendered into: a sticky page header
 *        overlaying a scrolling body.
 * WHY:   THE HEADER IS WHY THE TRAFFIC LIGHTS LOOKED WRONG. macOS draws them
 *        over the content at a fixed offset, and they span to roughly 66px —
 *        wider than the 56px sidebar rail — so they spilled out of the rail and
 *        floated over the page with nothing to belong to. The fix is not to
 *        move the lights, it is to give them a band that continues across the
 *        content pane: this header's first row is exactly --page-header-height,
 *        the same height the sidebar reserves at its top, so the page title and
 *        the three lights sit on one centre line.
 *
 *        IT OVERLAYS THE BODY RATHER THAN SITTING ABOVE IT, because the ask was
 *        that content tuck under the header and fade out. That only works if
 *        they occupy the same space. The consequence is handled in exactly one
 *        place — ScrollArea's headerInset pads the content down past this bar —
 *        so no page has to remember not to hide its first line.
 *
 *        THE WHOLE BAR IS THE DRAG REGION and it is deliberately taller than
 *        the title needs, because the previous one was reported as hard to
 *        grab: it lived INSIDE each page's scroller, so it slid away the moment
 *        anyone scrolled and the draggable surface went with it. Out here it
 *        never moves. Interactive children opt out with data-tauri-drag-region
 *        ={false}; without that a button inside the bar drags the window
 *        instead of firing.
 *
 *        The wheel forwarding is not a flourish. The header overlays the
 *        scroller but is not inside it, so a wheel event over the bar has no
 *        scrollable ancestor to bubble to and the page would simply refuse to
 *        scroll under the cursor. Forwarding it to the sibling marked
 *        data-scroll-area restores what the user expects without putting the
 *        header inside the scroller — where the fade mask, which paints on the
 *        scroll container's own box, would erase the title along with the
 *        content.
 * WHERE: Wrapped around every routed view by Dashboard. The title comes from
 *        the capability registry's NavDef.label, so a new page added to the
 *        registry is titled without touching this file.
 */

import type { ReactNode, WheelEvent } from "react";

export interface PageShellProps {
  title: string;
  /** Right-hand side of the header band. Must opt out of the drag region. */
  actions?: ReactNode;
  children: ReactNode;
}

export function PageShell({ title, actions, children }: PageShellProps) {
  const forwardWheel = (event: WheelEvent<HTMLElement>) => {
    const scroller = event.currentTarget.parentElement?.querySelector<HTMLElement>(
      "[data-scroll-area]",
    );
    if (scroller) scroller.scrollTop += event.deltaY;
  };

  return (
    <div className="relative flex min-h-0 flex-1 flex-col">
      <header
        data-tauri-drag-region
        onWheel={forwardWheel}
        className="absolute inset-x-0 top-0 z-10 flex h-[var(--page-header-height)] flex-col px-6"
      >
        {/* The title sits in the TRAFFIC-LIGHT BAND, not in the middle of the
            header. macOS puts the three buttons at a fixed inset from the
            window's top-left and centres them in a standard 28pt title bar;
            centring the title in the taller header instead dropped it a few
            pixels below them, which is exactly the "not in line" that got
            reported. The header stays taller because the drag target and the
            content inset both want the room — only the alignment uses the
            band. */}
        <div
          data-tauri-drag-region
          className="flex h-[var(--traffic-light-band)] shrink-0 items-center justify-between gap-4"
        >
          <h1 data-tauri-drag-region className="text-title truncate text-text-primary">
            {title}
          </h1>
          {actions ? (
            <div data-tauri-drag-region={false} className="flex shrink-0 items-center gap-2">
              {actions}
            </div>
          ) : null}
        </div>
      </header>
      {children}
    </div>
  );
}
