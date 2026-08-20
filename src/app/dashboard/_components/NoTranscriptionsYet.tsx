/**
 * SOURCE OF TRUTH KEYWORDS: NoTranscriptionsYet, first-run-empty-state, Keycap,
 *   HotkeyBinding, glyphsForBinding
 * WHAT:  The "you have not dictated anything yet" state, exactly as docs/04 §9
 *        specifies it: the hotkey as a physical key, and one line telling you
 *        what to press.
 * WHY:   Written once because it appears in two places — History and Stats —
 *        and the same absence explained two different ways in one window is how
 *        a product starts feeling assembled rather than designed. It is also the
 *        single most-seen screen in the app's life: it is what a new user is
 *        looking at before anything else works, so it has one job, which is to
 *        say what to press.
 * WHERE: The empty slot of the history list and the zero state of the stats
 *        view. Takes the binding from the dashboard shell.
 */

import type { HotkeyBinding } from "@/lib/bindings";
import { glyphsForBinding, displayForBinding } from "@/lib/hotkey";
import { EmptyState, Keycap } from "@/components/global";

export function NoTranscriptionsYet({ hotkey }: { hotkey: HotkeyBinding | null }) {
  return (
    <EmptyState
      icon={hotkey ? <Keycap keys={glyphsForBinding(hotkey)} /> : null}
      headline="No transcriptions yet"
      description={
        hotkey
          ? `Press ${displayForBinding(hotkey)} anywhere to start.`
          : "Press your dictation hotkey anywhere to start."
      }
    />
  );
}
