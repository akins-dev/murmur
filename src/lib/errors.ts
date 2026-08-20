/**
 * SOURCE OF TRUTH KEYWORDS: isTransientFailure, TRANSIENT_FAILURES, ErrorCode,
 *   ENGINE_NOT_READY, not-yet-state
 * WHAT:  Whether a failure code means "not yet" rather than "something is wrong".
 * WHY:   Lives here, not in the pill, because two different WINDOWS need the
 *        same answer — the pill's failed state and onboarding's hotkey test —
 *        and they are separate bundles that would otherwise each carry their own
 *        opinion and drift. Matched on ErrorCode because that is the field the
 *        frontend is meant to branch on; message text is wording and changes.
 *
 *        This is a narrower idea than AppError's `recoverable`. Recoverable
 *        means the user CAN fix it — a missing model is recoverable, and it
 *        needs a download. Transient means it fixes ITSELF if they simply wait,
 *        which is the only case that should not look like a problem at all.
 * WHERE: Used by app/pill/Pill.tsx and app/onboarding/_components/HotkeyStep.tsx.
 *        Colour treatment is defined in docs/04 §2.
 */

import type { ErrorCode } from "./bindings";

/** The engine warming at launch: model load plus a hash of a 574MB file. It is
 *  the most likely failure a new user ever sees, on their first keypress. */
const TRANSIENT_FAILURES: readonly ErrorCode[] = ["ENGINE_NOT_READY"];

export function isTransientFailure(code: ErrorCode): boolean {
  return TRANSIENT_FAILURES.includes(code);
}
