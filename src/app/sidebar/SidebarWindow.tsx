/**
 * SOURCE OF TRUTH KEYWORDS: SidebarWindow, SidebarWindowProps, detached-rail,
 *   rail-width, rail-item-size, nav-selected
 * WHAT:  The contents of the detached sidebar WINDOW: the identity mark, then
 *        one icon per registry nav entry, sorted by NavDef.order.
 * WHY:   THE WINDOW IS THE RAIL, which is the same rule the pill lives under
 *        and it carries the same consequences. The window is transparent with
 *        native vibrancy cut to --radius-window, so this component fills it
 *        exactly and draws NO surface of its own — no rounding, no shadow, no
 *        glass panel. A CSS rounding here would leave the effect view's square
 *        corners outside a rounded page, and a CSS shadow has nowhere to fall
 *        because the window ends where the rail ends.
 *
 *        IT HAS NO TRAFFIC-LIGHT SPACER, and that is the substantive difference
 *        from the rail this replaces. The embedded version reserved
 *        --titlebar-height at its top because the window's hidden title bar put
 *        the three buttons over that glass. A detached window has no buttons to
 *        clear, so reserving the space would leave a dead 38pt gap above the
 *        mark and make a deliberate floating object look mis-padded. Padding is
 *        --rail-padding, equal top and bottom, because a widget is symmetrical.
 *
 *        IT HAS NO DRAG REGION either. It is attached as a child window of the
 *        dashboard so macOS moves it with the parent; dragging a child moves
 *        only the child, which would tear the rail away from the window it is
 *        supposed to be hanging off. The dashboard's header remains the drag
 *        surface for the app.
 *
 *        Selection is --surface-sunken-strong, never a hue: there is no accent
 *        colour in this app (docs/04 §2), and a rail that lights up because you
 *        clicked Settings would be inventing one.
 * WHERE: Rendered by src/entries/sidebar.tsx into the `sidebar` window. Route
 *        choices leave via onSelect, which the entry forwards to the dashboard
 *        as `nav-selected`. Icons resolve through lib/icons.ts.
 */

import { cn } from "@/lib/utils";
import { iconFor } from "@/lib/icons";
import { Mark } from "@/components/global";
import type { NavDef } from "@/lib/bindings";

export interface SidebarWindowProps {
  items: readonly NavDef[];
  activeRoute: string;
  onSelect: (route: string) => void;
}

export function SidebarWindow({ items, activeRoute, onSelect }: SidebarWindowProps) {
  return (
    <nav
      aria-label="Sections"
      className="flex h-full w-full flex-col items-center gap-[var(--rail-item-gap)] p-[var(--rail-padding)]"
    >
      <Mark label="Murmur" className="mb-[var(--rail-mark-gap)] shrink-0" />

      {items.map((item) => {
        const Icon = iconFor(item.icon);
        const isActive = item.route === activeRoute;
        return (
          <button
            key={item.route}
            type="button"
            onClick={() => onSelect(item.route)}
            title={item.label}
            aria-label={item.label}
            aria-current={isActive ? "page" : undefined}
            className={cn(
              "flex size-[var(--rail-item-size)] shrink-0 items-center justify-center rounded-input transition-colors",
              isActive
                ? "bg-sunken-strong text-text-primary"
                : "text-text-secondary hover:bg-sunken hover:text-text-primary",
            )}
          >
            <Icon className="size-4 shrink-0" aria-hidden="true" />
          </button>
        );
      })}
    </nav>
  );
}
