/**
 * SOURCE OF TRUTH KEYWORDS: lib-barrel, cn, springFor, transitionFor, useCommand,
 *   useTauriEvent, formatClock, formatRelativeTime, bindingFromCapture, iconFor
 * WHAT:  Barrel for src/lib — the frontend's non-visual helpers.
 * WHY:   One import path per folder boundary (CLAUDE.md §8), so a consumer
 *        never has to know which file inside lib/ owns a helper. bindings.ts is
 *        deliberately NOT re-exported: it is generated, and importing it by its
 *        real path keeps it obvious in every file which types came from Rust.
 * WHERE: Imported by every entry, view and global component.
 */

export { cn } from "./utils";
export {
  springFor,
  reducedFade,
  transitionFor,
  readPxToken,
  readNumberToken,
  readDurationMs,
  type SpringName,
  type SpringTransition,
  type FadeTransition,
  type MotionTransition,
} from "./motion";
export { useCommand, unwrapCommand, toAppError, type CommandResult, type CommandState } from "./ipc";
export { useTauriEvent, type TauriEventChannel } from "./use-event";
export {
  formatClock,
  formatCompactDuration,
  formatRelativeTime,
  formatBytes,
  formatRate,
  formatEta,
  formatLatency,
  formatCount,
  formatLanguage,
} from "./format";
export { bindingFromCapture, glyphForCode, glyphsForBinding, displayForBinding } from "./hotkey";
export { iconFor, type LucideIcon } from "./icons";
export { isTransientFailure } from "./errors";
export { usePermissions, permissionLabel, missingPermissions } from "./use-permissions";
