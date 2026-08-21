/**
 * SOURCE OF TRUTH KEYWORDS: UpdateNotice, updateAvailable, checkForUpdate,
 *   installUpdate, UpdateCheck, update-bar
 * WHAT:  A quiet bar along the bottom of the dashboard offering to install a
 *        newer version. Absent entirely until there is one.
 * WHY:   IT MUST NEVER TAKE THE SCREEN. The person using this app is usually
 *        dictating into something else and the dashboard may not even be open —
 *        an update prompt that interrupts that is worse than one he finds late,
 *        because the whole product is "talk and the words appear" and anything
 *        that steals focus mid-sentence breaks the only promise it makes. So
 *        this is not a modal, not a toast and not an overlay: it is docked
 *        BELOW the scroll area, so it covers nothing, moves nothing, and is
 *        simply there when he next looks.
 *
 *        IT ASKS AS WELL AS LISTENS, and that is what makes the feature visible
 *        at all. The backend checks at launch and every 24h and emits an event,
 *        but the dashboard opens from the menu bar and may not exist when that
 *        event fires — a listener alone would miss nearly every one and the
 *        control would appear to be broken while working perfectly. So it calls
 *        check_for_update ONCE on mount for the answer that already exists, and
 *        listens for the event to catch a check that lands while it is open.
 *        Same shape as the pill: a command for first paint, events after that.
 *
 *        THE RESTART IS IN THE BUTTON, not in a sentence beside it. Installing
 *        swaps the running app, and the label is the last thing read before
 *        committing — putting it anywhere else means it can be missed by
 *        exactly the person who most needed to see it.
 *
 *        The button is a bordered secondary, deliberately not the inverted
 *        primary fill: docs/04 reserves that weight for the app's one primary
 *        action, and a bar whose whole point is to stay quiet must not carry
 *        the loudest control in the product.
 * WHERE: Rendered once by Dashboard, below PageShell, so it survives route
 *        changes and shows on every page.
 */

import { useEffect, useState } from "react";
import { X } from "lucide-react";
import { commands, events, type AppError } from "@/lib/bindings";
import { unwrapCommand } from "@/lib/ipc";
import { useTauriEvent } from "@/lib/use-event";

export function UpdateNotice() {
  const [version, setVersion] = useState<string | null>(null);
  const [installing, setInstalling] = useState(false);
  const [error, setError] = useState<AppError | null>(null);
  const [dismissed, setDismissed] = useState(false);

  // The answer that may already exist from the launch check.
  useEffect(() => {
    void unwrapCommand(commands.checkForUpdate).then((result) => {
      if (result.status === "ok" && result.data.kind === "AVAILABLE") {
        setVersion(result.data.version);
      }
    });
  }, []);

  // And a check that lands while the dashboard happens to be open.
  useTauriEvent(events.updateAvailable, (payload) => {
    setVersion(payload.version);
    setDismissed(false);
  });

  if (!version || dismissed) return null;

  const install = () => {
    setInstalling(true);
    setError(null);
    void unwrapCommand(commands.installUpdate).then((result) => {
      // On success the app is replaced and restarts, so there is no success
      // state to render — only the failure needs somewhere to go.
      if (result.status === "error") {
        setError(result.error);
        setInstalling(false);
      }
    });
  };

  return (
    <div
      role="status"
      className="hairline-t flex shrink-0 items-center justify-between gap-4 px-[var(--page-padding-x)] py-3"
    >
      <p className="min-w-0 truncate text-label text-text-secondary">
        {error ? (
          <span className="text-danger">{error.message}</span>
        ) : (
          <>Murmur {version} is ready to install.</>
        )}
      </p>

      <div className="flex shrink-0 items-center gap-1">
        <button
          type="button"
          onClick={install}
          disabled={installing}
          className="hairline h-[var(--control-height-sm)] rounded-input bg-sunken px-3 text-label text-text-primary transition-colors hover:bg-sunken-strong disabled:opacity-50"
        >
          {installing ? "Installing…" : error ? "Try again" : "Install and restart"}
        </button>
        <button
          type="button"
          onClick={() => setDismissed(true)}
          aria-label="Dismiss until the next check"
          className="flex size-[var(--control-height-sm)] items-center justify-center rounded-input text-text-tertiary transition-colors hover:bg-sunken hover:text-text-secondary"
        >
          <X className="size-3.5" />
        </button>
      </div>
    </div>
  );
}
