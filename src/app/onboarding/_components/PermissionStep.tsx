/**
 * SOURCE OF TRUTH KEYWORDS: PermissionStep, PermissionReport, PermissionState,
 *   requestPermission, openPrivacyPane, NOT_DETERMINED, DENIED, focus-recheck
 * WHAT:  The permission step: microphone and accessibility, each with the one
 *        action that can actually change its state.
 * WHY:   NOT_DETERMINED is the only state where a prompt is possible — macOS
 *        shows the system dialog exactly once per bundle and signature. After a
 *        denial another "Allow" button would do nothing at all, so the UI offers
 *        the privacy pane instead; that deep link is the ONLY recovery, and
 *        pretending otherwise wastes the user's one attempt. State is re-checked
 *        on window focus because the user grants it in System Settings, in
 *        another app, and returns expecting this screen to have noticed.
 *        Accessibility is explicitly optional: without it delivery degrades to
 *        clipboard-only, which is a success, not a failure.
 * WHERE: Step one of onboarding.
 */

import { useCallback } from "react";
import { Check, ExternalLink } from "lucide-react";
import { commands, type OsPermission, type PermissionReport } from "@/lib/bindings";
import { unwrapCommand } from "@/lib/ipc";

const PERMISSION_COPY: Readonly<Record<OsPermission, { label: string; why: string; required: boolean }>> = {
  MICROPHONE: {
    label: "Microphone",
    why: "Murmur cannot hear you without it.",
    required: true,
  },
  ACCESSIBILITY: {
    label: "Accessibility",
    why: "Lets Murmur paste for you. Without it, your words still go to the clipboard.",
    required: false,
  },
};

export function PermissionStep({
  reports,
  onChanged,
}: {
  reports: readonly PermissionReport[];
  onChanged: () => void;
}) {
  const request = useCallback(
    (permission: OsPermission) => {
      void unwrapCommand(() => commands.requestPermission({ permission })).then(onChanged);
    },
    [onChanged],
  );

  const openSettings = useCallback((permission: OsPermission) => {
    void unwrapCommand(() => commands.openPrivacyPane({ permission }));
  }, []);

  return (
    <ul className="flex flex-col gap-2">
      {reports.map((report) => {
        const copy = PERMISSION_COPY[report.permission];
        return (
          <li key={report.permission} className="hairline flex items-center gap-3 rounded-card bg-sunken p-3">
            <div className="min-w-0 flex-1">
              <p className="text-body text-text-primary">
                {copy.label}
                {!copy.required ? <span className="text-text-tertiary"> · optional</span> : null}
              </p>
              <p className="text-caption text-text-secondary">
                {report.state === "DENIED"
                  ? `${copy.why} This has to be enabled in Windows Settings.`
                  : copy.why}
              </p>
            </div>

            {report.state === "GRANTED" ? (
              <Check className="size-4 shrink-0 text-success" aria-label="Granted" />
            ) : report.state === "NOT_DETERMINED" ? (
              <button
                type="button"
                onClick={() => request(report.permission)}
                className="hairline h-8 shrink-0 rounded-input bg-glass px-3 text-body text-text-primary transition-colors hover:bg-sunken-strong"
              >
                Allow
              </button>
            ) : (
              <button
                type="button"
                onClick={() => openSettings(report.permission)}
                className="hairline flex h-8 shrink-0 items-center gap-1 rounded-input bg-glass px-3 text-body text-text-primary transition-colors hover:bg-sunken-strong"
              >
                <ExternalLink className="size-4" />
                System Settings
              </button>
            )}
          </li>
        );
      })}
    </ul>
  );
}
