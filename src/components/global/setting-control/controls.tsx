/**
 * SOURCE OF TRUTH KEYWORDS: ToggleControl, SelectControl, NumberControl,
 *   TextControl, HotkeyControl, CONTROL_CLASS, formatHotkey, MODIFIER_GLYPH
 * WHAT:  One control per SettingKind, each taking its own slice of the union.
 * WHY:   Split out of SettingControl.tsx so that file stays a row layout plus a
 *        switch, and a new kind is one function here rather than another arm of
 *        a growing component. The hotkey control captures on keydown with
 *        preventDefault while armed — otherwise recording ⌘Q quits the app
 *        instead of being recorded — and reports the structured event
 *        alongside the glyphs, so the registry never has to parse "⌥Space".
 * WHERE: Used only by SettingControl.tsx. Types in ./types.ts.
 */

import { useEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";
import { isModifierCode } from "@/lib/hotkey";
import { Keycap } from "@/components/global/keycap";
import type {
  HotkeyCapture,
  HotkeySetting,
  NumberSetting,
  SelectSetting,
  TextSetting,
  ToggleSetting,
} from "./types";

const CONTROL_CLASS = "hairline h-8 rounded-input bg-sunken px-2 text-body text-text-primary disabled:opacity-50";

export function ToggleControl({ setting }: { setting: ToggleSetting }) {
  return (
    <button
      type="button"
      role="switch"
      id={setting.id}
      aria-checked={setting.value}
      disabled={setting.disabled}
      onClick={() => setting.onChange(!setting.value)}
      className={cn(
        // ON is --text-primary, not --accent. A settings screen is a wall of
        // switches and half of them are on by default; ember on each one is a
        // wall of ember on a screen where nothing is happening (docs/04 §1.3).
        // The inverted fill reads as ON at least as clearly and costs no hue.
        "relative h-6 w-10 shrink-0 rounded-pill transition-colors disabled:opacity-50",
        setting.value ? "bg-text-primary" : "bg-sunken hairline",
      )}
    >
      <span
        className={cn(
          "absolute top-0.5 left-0.5 size-5 rounded-pill bg-opaque-elevated shadow-pill transition-transform",
          setting.value && "translate-x-4",
        )}
      />
    </button>
  );
}

export function SelectControl({ setting }: { setting: SelectSetting }) {
  return (
    <select
      id={setting.id}
      value={setting.value}
      disabled={setting.disabled}
      onChange={(event) => setting.onChange(event.target.value)}
      className={cn(CONTROL_CLASS, "max-w-56")}
    >
      {setting.options.map((option) => (
        <option key={option.value} value={option.value} title={option.description} disabled={option.disabled}>
          {option.label}
        </option>
      ))}
    </select>
  );
}

export function NumberControl({ setting }: { setting: NumberSetting }) {
  return (
    <span className="flex items-center gap-2">
      <input
        id={setting.id}
        type="number"
        value={setting.value ?? ""}
        min={setting.min}
        max={setting.max}
        step={setting.step}
        disabled={setting.disabled}
        onChange={(event) => {
          const next = event.target.valueAsNumber;
          if (Number.isFinite(next)) setting.onChange(next);
        }}
        className={cn(CONTROL_CLASS, "w-20 text-right tabular-nums")}
      />
      {setting.unit ? <span className="text-label text-text-tertiary">{setting.unit}</span> : null}
    </span>
  );
}

export function TextControl({ setting }: { setting: TextSetting }) {
  return (
    <input
      id={setting.id}
      type="text"
      value={setting.value}
      placeholder={setting.placeholder}
      maxLength={setting.maxLength}
      disabled={setting.disabled}
      onChange={(event) => setting.onChange(event.target.value)}
      className={cn(CONTROL_CLASS, "w-56 placeholder:text-text-tertiary")}
    />
  );
}

const MODIFIER_GLYPH = { ctrl: "⌃", alt: "⌥", shift: "⇧", meta: "⌘" } as const;

const NAMED_KEY: Readonly<Record<string, string>> = {
  " ": "Space",
  Escape: "Esc",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
  Enter: "⏎",
  Backspace: "⌫",
  Tab: "⇥",
};

/** Event → the glyph list a Keycap renders, e.g. ["⌥", "Space"]. */
export function formatHotkey(capture: HotkeyCapture): string[] {
  const glyphs: string[] = [];
  if (capture.ctrl) glyphs.push(MODIFIER_GLYPH.ctrl);
  if (capture.alt) glyphs.push(MODIFIER_GLYPH.alt);
  if (capture.shift) glyphs.push(MODIFIER_GLYPH.shift);
  if (capture.meta) glyphs.push(MODIFIER_GLYPH.meta);
  // A modifier bound on its own is one cap, not a cap plus "AltLeft".
  if (!isModifierCode(capture.code)) {
    glyphs.push(NAMED_KEY[capture.key] ?? capture.key.toUpperCase());
  }
  return glyphs;
}

const MODIFIER_GLYPHS: ReadonlySet<string> = new Set(Object.values(MODIFIER_GLYPH));

/** Whether a stored display value is a bare modifier, e.g. "⌥". */
function isModifierOnlyValue(value: string | null): boolean {
  if (!value) return false;
  return Array.from(value).every((glyph) => MODIFIER_GLYPHS.has(glyph));
}

/**
 * SOURCE OF TRUTH KEYWORDS: HotkeyControl, armed, capture_on_window
 * WHAT:  Click to arm, then press a chord to bind it.
 * WHY:   The listener is on WINDOW, not on the button, and that is the whole
 *        fix. It used to be an `onKeyDown` prop, which only fires when the
 *        button has keyboard focus — and on macOS WebKit, which is what Tauri
 *        renders in, clicking a `<button>` does NOT focus it. So the control
 *        armed, said "Press keys", and then never received a single keystroke.
 *        The operator's report was exactly that: "it just said press key and
 *        I'm pressing the key, but nothing's working."
 *
 *        Written to not depend on focus AT ALL rather than to add a
 *        `.focus()` call, because a focus() would have fixed this instance of
 *        the bug while leaving the control one stray re-render or one
 *        focus-stealing sibling away from silence again. While armed the
 *        listener is on the window in the CAPTURE phase, so it also sees keys
 *        no focused element would forward, and preventDefault stops ⌘Q quitting
 *        the app mid-capture.
 *
 *        A BARE MODIFIER IS A REAL BINDING NOW, and it commits on KEY-UP rather
 *        than key-down. That is the whole trick: pressing ⌥ cannot mean "bind
 *        ⌥" at the moment it goes down, because every chord starts that way and
 *        ⌥⌘D would be impossible to enter. Releasing it without having pressed
 *        anything else is unambiguous, so the control waits to find out which
 *        one the user meant instead of guessing.
 *
 *        THE HINT TEACHES THE GESTURE, because the gesture is not what anyone
 *        assumes. A bare modifier fires on a DOUBLE-TAP, not a hold and not a
 *        single press — a single tap would fire every time you reach for Option
 *        to peek at a menu. Someone who binds ⌥, presses it once and sees
 *        nothing happen will report the hotkey as broken, so the moment they
 *        set it is the moment they have to be told. The hint is derived from
 *        the CURRENT VALUE rather than shown once after capture, so it is still
 *        there when they come back to this screen a week later.
 * WHERE: Rendered by SettingControl for SettingKind::Hotkey.
 */
export function HotkeyControl({ setting }: { setting: HotkeySetting }) {
  const [armed, setArmed] = useState(false);
  const [hint, setHint] = useState<string | null>(null);
  /** Whether a non-modifier arrived this capture — i.e. it is a chord. */
  const sawKey = useRef(false);
  /** Guards the release path against a key-up arriving after a chord landed
   *  but before the listeners are torn down. */
  const committed = useRef(false);

  useEffect(() => {
    if (!armed) return;
    sawKey.current = false;
    committed.current = false;

    const onKeyDown = (event: KeyboardEvent) => {
      // Everything is swallowed while armed, including the chords that would
      // otherwise act on the app. Capture phase so nothing downstream sees them.
      event.preventDefault();
      event.stopPropagation();

      if (event.code === "Escape") {
        setArmed(false);
        setHint(null);
        return;
      }

      // Down on a modifier decides nothing: this is either the start of a
      // chord or a bare modifier, and only the release tells us which.
      if (isModifierCode(event.code)) {
        setHint("Let go to use it on its own, or press another key for a combination.");
        return;
      }

      sawKey.current = true;
      setHint(null);
      const capture: HotkeyCapture = {
        key: event.key,
        code: event.code,
        alt: event.altKey,
        ctrl: event.ctrlKey,
        meta: event.metaKey,
        shift: event.shiftKey,
      };
      commit(capture);
    };

    // The release is where a bare modifier becomes a binding.
    const onKeyUp = (event: KeyboardEvent) => {
      if (!isModifierCode(event.code) || sawKey.current || committed.current) return;
      event.preventDefault();
      event.stopPropagation();
      commit({
        key: "",
        code: event.code,
        alt: event.altKey || event.code.startsWith("Alt"),
        ctrl: event.ctrlKey || event.code.startsWith("Control"),
        meta: event.metaKey || event.code.startsWith("Meta"),
        shift: event.shiftKey || event.code.startsWith("Shift"),
      });
    };

    function commit(capture: HotkeyCapture) {
      committed.current = true;
      setting.onChange(formatHotkey(capture).join(""), capture);
      setHint(null);
      setArmed(false);
    }

    // Clicking anywhere else cancels, replacing the old onBlur — which could
    // not be relied on once the button was no longer the thing being focused.
    const onPointerDown = () => {
      setArmed(false);
      setHint(null);
    };

    window.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("keyup", onKeyUp, true);
    window.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      window.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("keyup", onKeyUp, true);
      window.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [armed, setting]);

  // Says what to DO. A bare modifier does nothing on a single press, so naming
  // the binding back at the user would be the one thing that does not help.
  const gestureHint = isModifierOnlyValue(setting.value)
    ? `Double-tap ${setting.value} to start dictating.`
    : null;

  return (
    <div className="flex flex-col items-end gap-1">
      <button
        type="button"
        id={setting.id}
        disabled={setting.disabled}
        // pointerdown, not click: the window-level canceller runs on
        // pointerdown too, and a click handler would arm and then be disarmed
        // by the very press that armed it.
        onPointerDown={(event) => {
          event.stopPropagation();
          setArmed(true);
          setHint(null);
        }}
        className={cn(
          "hairline h-8 min-w-24 rounded-input px-2 transition-colors disabled:opacity-50",
          armed ? "bg-accent-soft" : "bg-sunken",
        )}
      >
        {armed || !setting.value ? (
          <span className="text-body text-text-tertiary">{setting.placeholder}</span>
        ) : (
          <Keycap size="sm" keys={[setting.value]} />
        )}
      </button>
      {hint ?? gestureHint ? (
        <span className="max-w-56 text-right text-caption text-text-tertiary">
          {hint ?? gestureHint}
        </span>
      ) : null}
    </div>
  );
}
