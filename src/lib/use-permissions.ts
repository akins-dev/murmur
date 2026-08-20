/**
 * SOURCE OF TRUTH KEYWORDS: usePermissions, permissionLabel, missingPermissions,
 *   checkPermissions, PermissionReport, PermissionState, focus-recheck
 * WHAT:  The OS grants and their current state, re-checked whenever the window
 *        regains focus, plus the helpers for naming and testing them.
 * WHY:   Shared because two windows need the same answer — onboarding's
 *        permission step and Settings, which has to disable any control whose
 *        grant is missing. The focus re-check is the load-bearing part: a
 *        permission is granted in System Settings, in ANOTHER app, and the user
 *        comes back expecting the screen to have noticed. Without it the UI
 *        keeps saying "not granted" after it has been, which is the same class
 *        of lie as the toggle this exists to fix.
 *
 *        It matters more than it looks: Accessibility grants are keyed to the
 *        code signature, so every unsigned rebuild produces a new one and
 *        silently voids the grant. This is not a rare edge during development.
 * WHERE: Used by app/onboarding/_components/PermissionStep.tsx and by
 *        app/dashboard/settings. Wraps check_permissions.
 */

import { useEffect } from "react";
import { commands, type OsPermission, type PermissionReport } from "./bindings";
import { useCommand, type CommandState } from "./ipc";

/** The macOS pane names, so ours match what the user is about to look at. */
const PERMISSION_LABEL: Readonly<Record<OsPermission, string>> = {
  MICROPHONE: "Microphone",
  ACCESSIBILITY: "Accessibility",
};

export function permissionLabel(permission: OsPermission): string {
  return PERMISSION_LABEL[permission];
}

/**
 * WHAT:  Which of `required` are not granted, given the current reports.
 * WHY:   Returns EMPTY while the reports are still loading, so a control is
 *        never disabled on the strength of an answer that has not arrived —
 *        the same rule as the engine gate. Briefly greying out a working
 *        control reads as breakage.
 */
export function missingPermissions(
  required: readonly OsPermission[],
  reports: readonly PermissionReport[] | null,
): OsPermission[] {
  if (!reports) return [];
  return required.filter(
    (permission) => reports.find((report) => report.permission === permission)?.state !== "GRANTED",
  );
}

export function usePermissions(): CommandState<PermissionReport[]> {
  const permissions = useCommand(commands.checkPermissions, []);

  useEffect(() => {
    const onFocus = () => permissions.reload();
    window.addEventListener("focus", onFocus);
    return () => window.removeEventListener("focus", onFocus);
  }, [permissions]);

  return permissions;
}
