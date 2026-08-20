/**
 * SOURCE OF TRUTH KEYWORDS: PermissionNotice, openPrivacyPane, OsPermission,
 *   permissionLabel, section-remedy
 * WHAT:  One line per missing OS grant at the top of a settings section, with
 *        the button that opens the right macOS pane.
 * WHY:   The REMEDY lives here, once, while the REASON lives on each disabled
 *        control. That split is deliberate: two controls blocked by the same
 *        missing grant are one problem with one fix, and repeating the button
 *        beside each of them implies there are two different things to do. The
 *        reason still has to sit on the control itself, because a greyed-out
 *        toggle whose explanation is somewhere else is the tooltip problem
 *        wearing a different hat.
 *
 *        It renders per PERMISSION rather than per section, so a section whose
 *        controls need different grants gets one line each rather than one
 *        vague line covering both.
 * WHERE: Top of any SettingsSection containing a setting whose
 *        `requires_permission` is not satisfied.
 */

import { ExternalLink } from "lucide-react";
import { commands, type OsPermission } from "@/lib/bindings";
import { unwrapCommand } from "@/lib/ipc";
import { permissionLabel } from "@/lib/use-permissions";

export function PermissionNotice({ permissions }: { permissions: readonly OsPermission[] }) {
  if (permissions.length === 0) return null;

  return (
    <div className="flex flex-col gap-2">
      {permissions.map((permission) => (
        <div
          key={permission}
          className="hairline flex items-center gap-3 rounded-card bg-warning-soft px-3 py-2"
        >
          <p className="min-w-0 flex-1 text-caption text-text-secondary">
            {permissionLabel(permission)} access is off, so the settings below cannot take effect.
          </p>
          <button
            type="button"
            onClick={() => void unwrapCommand(() => commands.openPrivacyPane({ permission }))}
            className="hairline flex h-8 shrink-0 items-center gap-1 rounded-input bg-glass px-3 text-body text-text-primary transition-colors hover:bg-sunken-strong"
          >
            <ExternalLink className="size-4" />
            Open System Settings
          </button>
        </div>
      ))}
    </div>
  );
}
