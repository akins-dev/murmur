/**
 * SOURCE OF TRUTH KEYWORDS: Pill, PillBody, SessionState, sessionStateChanged,
 *   VisibleState, PillTone, pill-width, pill-width-failed, window-is-the-pill
 * WHAT:  The overlay. Renders the current SessionState and nothing else — dot,
 *        waveform or countdown ring, the elapsed timer, or a failure sentence.
 * WHY:   THE WINDOW IS THE PILL. The NSPanel is sized to the pill's tokens and
 *        its vibrancy carries --radius-pill, so every pixel of the window is
 *        painted glass in the pill's shape and there is no slack to hide a
 *        mistake in. Three rules follow, and breaking any one of them puts the
 *        grey rectangle back on screen:
 *
 *        1. THE PILL FILLS THE WINDOW. It never animates its own width and
 *           never transforms itself. A CSS morph would race a native window
 *           resize that is not frame-locked to it, and every frame where the
 *           two disagree is bare glass beside the pill. Motion that moves the
 *           pill AS AN OBJECT — arriving, resizing, leaving — belongs to the
 *           window, in Rust. Motion INSIDE the pill belongs here.
 *        2. IDLE NEVER BLANKS THE VIEW. Rust hides the window on IDLE, and the
 *           same transition also reaches this webview over IPC. Those two do
 *           not land on the same frame, so a view that cleared on IDLE would
 *           leave an empty glass capsule on screen until the native hide caught
 *           up — a grey blob flashing at the end of every single dictation.
 *           The view therefore renders the last state that had something to say
 *           and lets the window take it away. `live` is tracked separately so
 *           the elapsed timer still STOPS on IDLE; only the pixels persist.
 *        3. THE SWITCH IS EXHAUSTIVE, with a `never` assertion rather than a
 *           default. A new variant in the Rust enum has to become a compile
 *           error here, not a silently blank pill.
 *
 *        It holds no domain state: get_session_state is called ONCE for first
 *        paint and every frame after that comes from session-state-changed, so
 *        there is no second copy of the truth and nothing to poll (CLAUDE.md §7).
 * WHERE: Mounted by src/entries/pill.tsx into the NSPanel window. Sizes come
 *        from tokens.css, which src-tauri/src/tray.rs reads and asserts against
 *        its own constants. Draws with the global Waveform and CountdownRing.
 */

import { useEffect, useRef, useState } from "react";
import { commands, events, type SessionState } from "@/lib/bindings";
import { isTransientFailure } from "@/lib/errors";
import { useTauriEvent } from "@/lib/use-event";
import { unwrapCommand } from "@/lib/ipc";
import { formatClock } from "@/lib/format";
import { readDurationMs } from "@/lib/motion";
import { cn } from "@/lib/utils";
import { CountdownLine, GlassPanel } from "@/components/global";
import { StateDot, type PillTone } from "./_components/StateDot";
import { PillWaveform } from "./_components/PillWaveform";
import { useElapsed } from "./use-elapsed";

/** Every state that puts something on screen. IDLE is the window's business. */
type VisibleState = Exclude<SessionState, { kind: "IDLE" }>;

/**
 * WHAT:  The surface tint per state.
 * WHY:   Only failure tints now. docs/04 §7 used to say the pill TURNS its
 *        state colour, and with an ember accent that meant a wash for cancel
 *        and for delivery too. There is no accent hue any more, so a neutral
 *        "tint" would be a wash the same colour as the pill — a layer that
 *        costs a paint and communicates nothing. What separates the states is
 *        FORM: a moving waveform, a draining line, a sentence. Hue was
 *        decorating a distinction that form had already made, which is exactly
 *        why removing it costs nothing.
 *
 *        Failure keeps its wash because that is the one state where losing the
 *        distinction is a safety problem rather than a style one. Still the
 *        soft variant: at full strength over vibrancy this is a red box, and
 *        the pill is an indicator, not an alert. Always rendered and
 *        transparent at rest, so the colour morphs on a state change instead
 *        of a tinted layer popping into existence (docs/04 §6).
 */
const TONE_TINT: Readonly<Record<PillTone, string>> = {
  ink: "bg-transparent",
  danger: "bg-danger-soft",
};

function toneFor(state: VisibleState): PillTone {
  // "Not yet" is not a fault: a red pill on a first-ever keypress says the app
  // is broken when it is seven seconds old (docs/04 §2).
  if (state.kind !== "FAILED") return "ink";
  return isTransientFailure(state.code) ? "ink" : "danger";
}

export function Pill() {
  /** The last state worth drawing. Never cleared — see rule 2 above. */
  const [shown, setShown] = useState<VisibleState | null>(null);
  /** Whether a session is actually running, which is a different question. */
  const [live, setLive] = useState(false);
  const [refilling, setRefilling] = useState(false);
  const previousKind = useRef<VisibleState["kind"] | null>(null);

  // First paint only. Every subsequent state arrives on the event.
  useEffect(() => {
    void unwrapCommand(commands.getSessionState).then((result) => {
      if (result.status !== "ok") return;
      const state = result.data;
      setLive(state.kind !== "IDLE");
      if (state.kind !== "IDLE") setShown(state);
    });
  }, []);

  useTauriEvent(events.sessionStateChanged, ({ state }) => {
    setLive(state.kind !== "IDLE");
    if (state.kind !== "IDLE") setShown(state);
  });

  // A second Escape: hold the ring on screen while it springs back to full,
  // or the gesture that saved the recording has no feedback at all.
  const kind = shown?.kind ?? null;
  useEffect(() => {
    const previous = previousKind.current;
    previousKind.current = kind;
    if (previous !== "CANCEL_PENDING" || kind !== "RECORDING") return;
    setRefilling(true);
    const handle = window.setTimeout(() => setRefilling(false), readDurationMs("--motion-duration-medium"));
    return () => window.clearTimeout(handle);
  }, [kind]);

  // Ticks only while a session is live; keeps its pixels after it is not.
  const elapsedMs = useElapsed(
    live && (shown?.kind === "RECORDING" || shown?.kind === "CANCEL_PENDING") ? shown.elapsed_ms : null,
  );

  if (!shown) return null;

  const failed = shown.kind === "FAILED";
  const showLine = shown.kind === "CANCEL_PENDING" || refilling;

  return (
    <GlassPanel
      material="pill"
      radius="pill"
      role="status"
      aria-live="polite"
      className="flex h-full w-full items-center gap-2 px-3"
    >
      <span
        aria-hidden="true"
        className={cn("absolute inset-0 -z-10 transition-colors", TONE_TINT[toneFor(shown)])}
      />
      <StateDot tone={toneFor(shown)} pulsing={shown.kind === "RECORDING"} />

      <PillBody state={shown} showLine={showLine} refilling={refilling} />

      {failed ? null : (
        <span className="w-[var(--pill-timer-width)] shrink-0 text-right text-label tabular-nums text-text-secondary">
          {formatClock(elapsedMs)}
        </span>
      )}
    </GlassPanel>
  );
}

/**
 * WHAT:  The middle slot — waveform, countdown line, or the failure sentence.
 * WHY:   Split out so the switch stays exhaustive over VisibleState["kind"]:
 *        a new variant in the Rust enum becomes a TypeScript error here rather
 *        than a silently blank pill.
 *
 *        ARMING deliberately draws EXACTLY what RECORDING draws. It is normally
 *        under 100ms, and a distinct "Listening…" label for a tenth of a second
 *        is not information, it is a flicker — the pill would appear, flash a
 *        word, and reshape itself before the eye resolved any of it. Drawing
 *        the recording layout with a resting waveform and an unpulsed dot means
 *        the pill arrives already in its working shape and simply comes alive
 *        as sound reaches it, which is the whole feel the app is after. If
 *        arming genuinely stalls, the device error surfaces as FAILED.
 */
function PillBody({
  state,
  showLine,
  refilling,
}: {
  state: VisibleState;
  showLine: boolean;
  refilling: boolean;
}) {
  if (showLine) {
    // No label beside it. The operator asked for a line that fills down to
    // zero and nothing else, and a countdown he armed himself with a keypress
    // does not need a caption telling him he armed it. Dropping the label is
    // also what lets the line occupy the full middle slot, so it reads as the
    // waveform changing mode rather than as a new widget appearing beside one.
    return (
      <CountdownLine
        className="min-w-0 flex-1"
        remainingMs={state.kind === "CANCEL_PENDING" ? state.remaining_ms : 0}
        state={refilling ? "refilling" : "draining"}
        label="Cancelling — press Escape again to keep the recording"
      />
    );
  }

  switch (state.kind) {
    case "ARMING":
    case "RECORDING":
      return <PillWaveform className="min-w-0 flex-1 justify-center text-text-primary" />;
    case "CANCEL_PENDING":
      // Unreachable while showLine covers it; kept so the switch is total.
      return <span className="flex-1" />;
    case "FAILED":
      // Tone follows the code, so "still starting up" does not read as a fault.
      // Two lines, not one and an ellipsis: the whole point of this state is
      // that he finds out why nothing arrived, and a truncated sentence is a
      // failure that failed twice.
      return (
        <span
          className={cn(
            "min-w-0 flex-1 text-label text-pretty",
            isTransientFailure(state.code) ? "text-text-secondary" : "text-danger",
          )}
        >
          {state.message}
        </span>
      );
    default: {
      // A new SessionState variant lands here as a compile error naming it.
      const unreachable: never = state;
      return unreachable;
    }
  }
}
