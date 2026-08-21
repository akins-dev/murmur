/**
 * SOURCE OF TRUTH KEYWORDS: sidebar-entry, SidebarWindow, nav-selected,
 *   sidebar.html, detached-rail
 * WHAT:  The detached navigation rail's entry point — the floating window that
 *        sits to the left of the dashboard.
 * WHY:   Its own window, and therefore its own Vite entry, because the GAP
 *        between the rail and the main panel has to be genuinely transparent to
 *        show the desktop through it. A single window cannot do that: its
 *        vibrancy is one rectangle covering the whole frame, so the gap would
 *        render as glass rather than as a hole.
 *
 *        The window is a CHILD of the dashboard (NSWindow addChildWindow), so
 *        macOS moves, orders, minimises and hides it with its parent. Nothing
 *        here tracks the parent's position; anything we wrote ourselves would
 *        drift on move and flicker on resize.
 *
 *        Navigation travels one way, rail to dashboard, over `nav-selected`.
 *        The rail keeps its own highlight optimistically rather than waiting
 *        for an echo, because it is the only thing in the app that changes the
 *        route — so the two cannot disagree in normal use.
 * WHERE: Loaded by sidebar.html, which bootstrap opens and attaches.
 */

import { useState } from "react";
import { emitTo } from "@tauri-apps/api/event";
import { SidebarWindow } from "@/app/sidebar";
import { commands } from "@/lib/bindings";
import { useCommand } from "@/lib/ipc";
import { NAV_SELECTED } from "@/lib/window-events";

/** The dashboard window's label, as declared in tauri.conf.json. */
const DASHBOARD_LABEL = "dashboard";
import "@/styles/global.css";
import { createRoot } from "react-dom/client";

function SidebarEntry() {
  const registry = useCommand(commands.getRegistry, []);

  const items = (registry.data?.capabilities ?? [])
    .flatMap((capability) => (capability.nav ? [capability.nav] : []))
    .sort((a, b) => a.order - b.order);

  const [route, setRoute] = useState("");
  // Falls back to the first item so the rail and the dashboard agree on first
  // paint, before anything has been selected.
  const active = items.some((item) => item.route === route) ? route : (items[0]?.route ?? "");

  return (
    <SidebarWindow
      items={items}
      activeRoute={active}
      onSelect={(next) => {
        setRoute(next);
        // emitTo, not emit. A broadcast relies on the dashboard's listener
        // being registered for globally-emitted events; targeting the window
        // by label is unambiguous and cannot silently miss. The rail's whole
        // job is to reach that one window.
        void emitTo(DASHBOARD_LABEL, NAV_SELECTED, { route: next });
      }}
    />
  );
}

const container = document.getElementById("root");
if (!container) throw new Error("sidebar.html is missing its #root element.");

createRoot(container).render(<SidebarEntry />);
