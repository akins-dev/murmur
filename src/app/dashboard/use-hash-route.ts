/**
 * SOURCE OF TRUTH KEYWORDS: useHashRoute, hashchange, navigateTo, DashboardRoute
 * WHAT:  The dashboard's routing: reads the current route from location.hash and
 *        re-renders on change.
 * WHY:   A hash rather than a router library. The window has three views and no
 *        need for a 12KB dependency, and — the actual reason — a hash is
 *        addressable from outside the frontend, so ErrorAction::OpenSettings and
 *        the menu bar can deep-link straight to a section by opening
 *        `#settings/privacy` instead of needing an IPC event and a listener.
 * WHERE: Owned by Dashboard.tsx; written by the sidebar and by error actions.
 */

import { useEffect, useState } from "react";

export interface DashboardRoute {
  /** Registry NavDef.route — "stats", "history", "settings". */
  route: string;
  /** Optional deep link within the view, e.g. a settings section. */
  section: string | null;
}

function read(): DashboardRoute {
  const [route = "", section = ""] = window.location.hash.replace(/^#\/?/, "").split("/");
  return { route, section: section || null };
}

export function navigateTo(route: string, section?: string): void {
  window.location.hash = section ? `/${route}/${section}` : `/${route}`;
}

export function useHashRoute(): DashboardRoute {
  const [current, setCurrent] = useState<DashboardRoute>(read);

  useEffect(() => {
    const onChange = () => setCurrent(read());
    window.addEventListener("hashchange", onChange);
    return () => window.removeEventListener("hashchange", onChange);
  }, []);

  return current;
}
