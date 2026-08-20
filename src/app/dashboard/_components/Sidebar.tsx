/**
 * SOURCE OF TRUTH KEYWORDS: Sidebar, SidebarProps, NavDef, navItems, iconFor,
 *   rail-width, titlebar-drag-region, Mark
 * WHAT:  The dashboard's left rail: the identity mark, then one icon per
 *        registry nav entry, sorted by NavDef.order.
 * WHY:   The nav is not written down here. Every item comes from a capability's
 *        NavDef, so adding a feature puts it in the rail with no frontend change
 *        — which is the whole point of the registry being the single source of
 *        truth for what the app has (CLAUDE.md §3).
 *
 *        Icon-only at --rail-width (docs/04 §8). A 180pt labelled sidebar spent
 *        a fifth of a 960pt window on three words that never change; the icon
 *        and its tooltip say the same thing in a third of the space, and what
 *        the space buys is the content having room to be laid out rather than
 *        squeezed. The label still exists — as the accessible name and as the
 *        tooltip — so nothing is hidden from a screen reader or from a user who
 *        does not recognise a glyph.
 *
 *        The rail is NOT a surface: no fill, no card, one hairline down its
 *        right edge. The window's own vibrancy is the panel, and drawing a
 *        second glass panel inside it puts two seams where the design wants one
 *        object. Selection is --surface-sunken-strong, never --accent-soft: a
 *        rail that glows ember because you clicked Settings contradicts §1.3,
 *        where ember means a session is live.
 *
 *        The top strip is a drag region sized to --titlebar-height because the
 *        title bar is hidden and the traffic lights are inset over this glass;
 *        without it the window cannot be moved. The mark sits below it, clear of
 *        the traffic lights.
 * WHERE: Rendered by Dashboard.tsx. Icons resolved through lib/icons.ts.
 */

import { cn } from "@/lib/utils";
import { iconFor } from "@/lib/icons";
import { Mark } from "@/components/global";
import type { NavDef } from "@/lib/bindings";

export interface SidebarProps {
  items: readonly NavDef[];
  activeRoute: string;
  onSelect: (route: string) => void;
}

export function Sidebar({ items, activeRoute, onSelect }: SidebarProps) {
  return (
    <nav
      aria-label="Sections"
      className="flex w-[var(--rail-width)] shrink-0 flex-col items-center gap-1 pb-4"
    >
      <div data-tauri-drag-region className="h-[var(--titlebar-height)] w-full shrink-0" />
      <Mark label="Murmur" className="mb-4 shrink-0" />

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
              "flex size-9 shrink-0 items-center justify-center rounded-input transition-colors",
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
