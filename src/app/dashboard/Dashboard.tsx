/**
 * SOURCE OF TRUTH KEYWORDS: Dashboard, getRegistry, navItems, useHashRoute,
 *   dictationHotkey, StatsView, HistoryView, SettingsView
 * WHAT:  The dashboard shell: fetches the registry once, builds the sidebar from
 *        its nav entries, and renders the view for the current route.
 * WHY:   One get_registry for the whole window — the nav, the latency panel's
 *        metric labels and every settings row all come out of that one snapshot,
 *        so the frontend reads exactly the declarations the backend enforces
 *        rather than a mirror someone has to remember to update. The route table
 *        below is the only place a route string meets a React component; the
 *        nav itself (what exists, what it is called, what order it is in) is
 *        never written down here. Each view is wrapped in its own ErrorBoundary
 *        so a failure in one cannot take the window with it.
 * WHERE: Mounted by src/entries/dashboard.tsx. Views live in ./stats, ./history
 *        and ./settings.
 */

import { useMemo } from "react";
import { commands, type HotkeyBinding, type NavDef, type RegistrySnapshot, type SettingValue } from "@/lib/bindings";
import { useCommand } from "@/lib/ipc";
import { useSettings } from "./use-settings";
import { EmptyState, ErrorBoundary, ErrorSurface, Skeleton, ScrollArea } from "@/components/global";
import { Sidebar } from "./_components/Sidebar";
import { PageShell } from "./_components/PageShell";
import { UpdateNotice } from "./_components/UpdateNotice";
import { BillingView } from "./billing";
import { navigateTo, useHashRoute } from "./use-hash-route";
import { StatsView } from "./stats/StatsView";
import { HistoryView } from "./history/HistoryView";
import { SettingsView } from "./settings/SettingsView";

/**
 * WHAT:  The user's dictation hotkey — their override if they have one, the
 *        registry's default otherwise.
 * WHY:   Both halves are needed. The default lives in the registry; the override
 *        lives in settings under the key the registry names, so neither side has
 *        to know the other's storage.
 */
function dictationHotkey(
  registry: RegistrySnapshot,
  values: { [key in string]: SettingValue } | null,
): HotkeyBinding | null {
  for (const capability of registry.capabilities) {
    const hotkey = capability.hotkey;
    if (!hotkey) continue;
    const override = hotkey.setting_key ? values?.[hotkey.setting_key] : undefined;
    return override?.type === "HOTKEY" ? override.value : hotkey.default;
  }
  return null;
}

export function Dashboard() {
  const registry = useCommand(commands.getRegistry, []);
  // Live, so a hotkey rebind in Settings updates the keycap in History's
  // empty state without a reload.
  const settings = useSettings();
  const { route, section } = useHashRoute();

  const navItems = useMemo<NavDef[]>(() => {
    const items = (registry.data?.capabilities ?? [])
      .map((capability) => capability.nav)
      .filter((nav): nav is NavDef => nav !== null);
    return items.sort((a, b) => a.order - b.order);
  }, [registry.data]);

  const metrics = useMemo(
    () => (registry.data?.capabilities ?? []).flatMap((capability) => capability.metrics),
    [registry.data],
  );

  if (registry.error) {
    return (
      <main className="flex h-full items-center justify-center">
        <ErrorSurface error={registry.error} onRetry={registry.reload} />
      </main>
    );
  }

  const activeRoute = navItems.some((item) => item.route === route)
    ? route
    : (navItems[0]?.route ?? "");

  // The registry already names every page. Reading the title from NavDef.label
  // rather than from a literal inside each view means a page added to the
  // registry — Billing, say — arrives titled without an edit here.
  const activeTitle = navItems.find((item) => item.route === activeRoute)?.label ?? "";

  return (
    <main className="flex h-full">
      {/* The rail is a hairline, not a panel. The window's native vibrancy is
          already the floating glass object (docs/04 §8) — wrapping the rail in a
          second GlassPanel drew a surface on a surface and gave the eye two
          seams to resolve where the design has one object. */}
      {/* THE RAIL'S DIVIDER STARTS BELOW THE HEADER BAND, and that is the fix
          for the window controls looking "squeezed into the sidebar". macOS
          draws the traffic lights at a fixed inset spanning roughly 20-72pt,
          which is WIDER than the 56pt rail, so a full-height divider at 56pt
          ran straight through the middle of the button cluster and made the
          three of them read as something trying and failing to fit inside the
          sidebar. Above the band there is now no seam at all: the top of the
          window is one continuous surface across rail and content, which is
          what a title bar looks like and what lets the lights simply sit in
          it. */}
      <div className="relative flex h-full shrink-0">
        <Sidebar items={navItems} activeRoute={activeRoute} onSelect={(next) => navigateTo(next)} />
        <span
          aria-hidden="true"
          className="hairline-r absolute top-[var(--page-header-height)] right-0 bottom-0 w-0"
        />
      </div>

      <div className="flex min-w-0 flex-1 flex-col">
        <PageShell title={activeTitle}>
        <ErrorBoundary
          key={activeRoute}
          fallback={({ error, reset }) => (
            <ErrorSurface
              error={{
                code: "INTERNAL",
                message: "This view stopped working.",
                recoverable: true,
                action: { kind: "RETRY" },
                detail: error.message,
              }}
              onRetry={reset}
            />
          )}
        >
          {registry.data ? (
            <View
              route={activeRoute}
              section={section}
              registry={registry.data}
              metrics={metrics}
              hotkey={dictationHotkey(registry.data, settings.data)}
            />
          ) : (
            <ScrollArea contentClassName="px-6 pb-8">
              <Skeleton rows={4} />
            </ScrollArea>
          )}
        </ErrorBoundary>
        </PageShell>
        <UpdateNotice />
      </div>
    </main>
  );
}

/** The one route-to-component table. Everything else about the nav is declared. */
function View({
  route,
  section,
  registry,
  metrics,
  hotkey,
}: {
  route: string;
  section: string | null;
  registry: RegistrySnapshot;
  metrics: RegistrySnapshot["capabilities"][number]["metrics"];
  hotkey: HotkeyBinding | null;
}) {
  switch (route) {
    case "stats":
      return <StatsView metrics={metrics} hotkey={hotkey} />;
    case "history":
      return <HistoryView hotkey={hotkey} />;
    case "settings":
      return <SettingsView registry={registry} section={section} />;
    case "billing":
      return <BillingView />;
    default:
      return (
        <EmptyState
          headline="Nothing here yet"
          description="This section of Murmur has not been built."
        />
      );
  }
}
