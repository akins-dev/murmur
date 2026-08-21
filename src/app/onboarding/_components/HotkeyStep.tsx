/**
 * SOURCE OF TRUTH KEYWORDS: HotkeyStep, transcriptDelivered, sessionStateChanged,
 *   hotkey-test, Keycap
 * WHAT:  The hotkey test: shows the shortcut as keys and waits for a real
 *        session to complete.
 * WHY:   The step is not finished by a button that says "I tried it" — it is
 *        finished by the app actually observing a delivery. That is the only
 *        evidence that the hotkey registered, the microphone opened, the model
 *        ran and delivery worked, which is the whole point of testing rather
 *        than asserting.
 *
 *        It listens on transcriptDelivered rather than on the session state,
 *        and the difference is not cosmetic. Delivery moved off the session
 *        state machine so that releasing the hotkey frees the app immediately,
 *        which means the state stream now ends at "the recording stopped" and
 *        can no longer testify that any words arrived. Completing this step on
 *        a stopped recording — or on a timer — would pass while nothing was
 *        delivered, in the one screen whose entire job is proving that
 *        dictation works.
 *        A transient failure is not drawn in red here for the same reason it is
 *        not in the pill, and it matters more here: this step IS a first launch,
 *        so the engine is at its least warm exactly when the user is told to
 *        press the key. "Still starting up" in red would be the first thing they
 *        ever see the app say.
 * WHERE: Step three of onboarding.
 */

import { useState } from "react";
import { Check } from "lucide-react";
import { events, type HotkeyBinding, type SessionState } from "@/lib/bindings";
import { useTauriEvent } from "@/lib/use-event";
import { isTransientFailure } from "@/lib/errors";
import { glyphsForBinding } from "@/lib/hotkey";
import type { DictationMode } from "@/lib/dictation-mode";
import { Keycap } from "@/components/global";

export function HotkeyStep({
  hotkey,
  mode,
  onDelivered,
}: {
  hotkey: HotkeyBinding | null;
  mode: DictationMode;
  onDelivered: () => void;
}) {
  const [seen, setSeen] = useState<SessionState | null>(null);

  const [delivered, setDelivered] = useState<number | null>(null);

  useTauriEvent(events.sessionStateChanged, (payload) => {
    setSeen(payload.state);
  });

  useTauriEvent(events.transcriptDelivered, (payload) => {
    setDelivered(payload.word_count);
    onDelivered();
  });

  if (delivered !== null) {
    return (
      <p className="flex items-center justify-center gap-2 text-body text-success">
        <Check className="size-4" />
        Heard you — {delivered} {delivered === 1 ? "word" : "words"} delivered.
      </p>
    );
  }

  const status =
    seen?.kind === "RECORDING" || seen?.kind === "ARMING"
      ? "Listening…"
      : // The recording has stopped and the words are on their way. This is the
        // one place in the app that says so, because it is the one place asking
        // the user to wait for a result rather than get on with their work.
        seen?.kind === "IDLE"
        ? "Writing it out…"
        : seen?.kind === "FAILED"
          ? seen.message
          : mode === "push_to_talk"
            ? "Press and hold a thought."
            : "Press it, and say what you are thinking.";

  const alarming = seen?.kind === "FAILED" && !isTransientFailure(seen.code);

  return (
    <div className="flex flex-col items-center gap-3">
      {hotkey ? <Keycap keys={glyphsForBinding(hotkey)} /> : null}
      <p className={alarming ? "text-body text-danger" : "text-body text-text-secondary"}>{status}</p>
    </div>
  );
}
