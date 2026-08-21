/**
 * SOURCE OF TRUTH KEYWORDS: DictationMode, DICTATION_MODE_KEY, dictationModeFrom,
 *   toggle, push_to_talk, gesture-copy
 * WHAT:  Which gesture the dictation hotkey uses, read from stored settings.
 * WHY:   EVERY SCREEN THAT TEACHES THE GESTURE HAS TO ASK. The first onboarding
 *        slide said "Hold your shortcut and say it, let go when you are done"
 *        while `dictation.mode` was — and defaults to — "toggle". A new user
 *        held the key, let go, and the recording carried on with nothing on
 *        screen saying they had done anything unexpected. That is a control
 *        that lies, in copy rather than in code, on the one screen whose entire
 *        job is teaching the gesture.
 *
 *        The fix is derivation, not rewording. Rewording to match today's
 *        default leaves the sentence wrong for anyone who switches to
 *        push-to-talk, and silently wrong again the day the default changes.
 *        Reading the setting is the only version that cannot rot.
 *
 *        Unknown or missing resolves to "toggle" because that is the registry's
 *        declared default, so the copy matches a fresh install during the
 *        moment before settings have loaded.
 * WHERE: Onboarding's tour and hotkey step, and anywhere else that tells
 *        someone how to start dictating. The key mirrors registry::keys.
 */

import type { SettingValue } from "./bindings";

/** Mirrors registry::keys::DICTATION_MODE. */
export const DICTATION_MODE_KEY = "dictation.mode";

export type DictationMode = "toggle" | "push_to_talk";

export function dictationModeFrom(
  settings: Readonly<Record<string, SettingValue>> | null | undefined,
): DictationMode {
  const stored = settings?.[DICTATION_MODE_KEY];
  return stored?.type === "CHOICE" && stored.value === "push_to_talk" ? "push_to_talk" : "toggle";
}
