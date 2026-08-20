/**
 * SOURCE OF TRUTH KEYWORDS: bindingFromCapture, glyphsForBinding, displayForBinding,
 *   MODIFIER_GLYPH, CODE_GLYPH, HotkeyBinding, KeyModifier
 * WHAT:  Converts between a captured keystroke, the registry's HotkeyBinding,
 *        and the glyphs a Keycap renders.
 * WHY:   HotkeyBinding.key is KeyboardEvent.code — the PHYSICAL key — while the
 *        glyph a user reads comes from the layout. Those are different things
 *        and conflating them is how a rebind works on a US keyboard and binds
 *        the wrong key on an AZERTY one. So the binding stores `code` and only
 *        this file turns it into something readable; nothing else formats a
 *        hotkey.
 * WHERE: Fed by the setting-control hotkey capture, consumed by Settings, the
 *        onboarding hotkey step and any empty state showing the shortcut.
 */

import type { HotkeyBinding, KeyModifier } from "./bindings";
import type { HotkeyCapture } from "@/components/global";

/** Apple's canonical order: Control, Option, Shift, Command. */
const MODIFIER_ORDER: readonly KeyModifier[] = ["CONTROL", "OPTION", "SHIFT", "COMMAND"];

const MODIFIER_GLYPH: Readonly<Record<KeyModifier, string>> = {
  CONTROL: "⌃",
  OPTION: "⌥",
  SHIFT: "⇧",
  COMMAND: "⌘",
};

const CODE_GLYPH: Readonly<Record<string, string>> = {
  Space: "Space",
  Escape: "Esc",
  Enter: "⏎",
  Tab: "⇥",
  Backspace: "⌫",
  Delete: "⌦",
  ArrowUp: "↑",
  ArrowDown: "↓",
  ArrowLeft: "←",
  ArrowRight: "→",
};

/**
 * SOURCE OF TRUTH KEYWORDS: MODIFIER_CODES, isModifierCode
 * WHAT:  The `KeyboardEvent.code` values for the modifier keys themselves.
 * WHY:   Matched on CODE, not on `event.key`. When Option is held, macOS
 *        rewrites `event.key` into the character the chord produces — ⌥S
 *        arrives as "ß" — so any rule written against `key` behaves differently
 *        depending on which modifiers are down. `code` is the physical key and
 *        never moves, which is also why HotkeyBinding stores it.
 *
 *        Lives here rather than in the capture control because two things need
 *        the same answer now: the control, deciding whether a press is a whole
 *        binding, and glyphsForBinding, deciding whether a binding has a key
 *        cap to draw at all.
 */
export const MODIFIER_CODES: ReadonlySet<string> = new Set([
  "AltLeft",
  "AltRight",
  "ControlLeft",
  "ControlRight",
  "MetaLeft",
  "MetaRight",
  "ShiftLeft",
  "ShiftRight",
  "CapsLock",
]);

export function isModifierCode(code: string): boolean {
  return MODIFIER_CODES.has(code);
}

export function bindingFromCapture(capture: HotkeyCapture): HotkeyBinding {
  const modifiers: KeyModifier[] = [];
  if (capture.ctrl) modifiers.push("CONTROL");
  if (capture.alt) modifiers.push("OPTION");
  if (capture.shift) modifiers.push("SHIFT");
  if (capture.meta) modifiers.push("COMMAND");
  return { modifiers, key: capture.code };
}

/** "KeyD" → "D", "Digit1" → "1", "F13" → "F13", "Space" → "Space". */
export function glyphForCode(code: string): string {
  const named = CODE_GLYPH[code];
  if (named) return named;
  if (code.startsWith("Key")) return code.slice(3);
  if (code.startsWith("Digit")) return code.slice(5);
  if (code.startsWith("Numpad")) return code.slice(6);
  return code;
}

/**
 * One glyph per cap, ready for <Keycap keys={...} />.
 *
 * A MODIFIER-ONLY BINDING HAS NO KEY CAP. Option on its own is now bindable, and
 * such a binding carries either an empty key or the modifier's own code
 * ("AltLeft") depending on where it was built. Both must render as a single ⌥,
 * not as "⌥" followed by a second cap reading "AltLeft" — this function feeds
 * every keycap in the app, so getting it wrong shows up in onboarding, in the
 * history empty state and in the tour at the same time.
 */
export function glyphsForBinding(binding: HotkeyBinding): string[] {
  const ordered = MODIFIER_ORDER.filter((modifier) => binding.modifiers.includes(modifier));
  const glyphs = ordered.map((modifier) => MODIFIER_GLYPH[modifier]);
  if (binding.key && !isModifierCode(binding.key)) glyphs.push(glyphForCode(binding.key));
  return glyphs;
}

/** The same thing as one string, for a sentence rather than a key row. */
export function displayForBinding(binding: HotkeyBinding): string {
  return glyphsForBinding(binding).join("");
}
