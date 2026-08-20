/**
 * SOURCE OF TRUTH KEYWORDS: SettingDef, SettingKind, ToggleSetting, SelectSetting,
 *   NumberSetting, TextSetting, HotkeySetting, CustomSetting, SettingOption, HotkeyCapture
 * WHAT:  The declarative shape a settings row is rendered from — a discriminated
 *        union on `kind`, plus the option and hotkey-capture types.
 * WHY:   The Rust registry is the single source of truth for what the app has,
 *        and settings UI is generated from it (CLAUDE.md §7). So the union
 *        branches on `kind` and never on a feature name: adding a setting is a
 *        registry entry, and a new *kind* of control is the only thing that
 *        should ever require touching this file. A hotkey reports both its
 *        display string and the structured capture, because the glyphs are a
 *        display concern and the registry needs the real key and modifiers.
 * WHERE: Consumed by SettingControl.tsx and controls.tsx. Will be produced by
 *        the generated registry mirror in lib/registry.ts.
 */

import type { ReactNode } from "react";

export type SettingKind = "toggle" | "select" | "number" | "text" | "hotkey" | "custom";

export interface SettingOption {
  value: string;
  label: string;
  /** Shown as the option's tooltip — a native <option> cannot render more. */
  description?: string;
  disabled?: boolean;
}

export interface HotkeyCapture {
  key: string;
  code: string;
  alt: boolean;
  ctrl: boolean;
  meta: boolean;
  shift: boolean;
}

interface SettingBase {
  id: string;
  label: string;
  description?: string;
  disabled?: boolean;
}

export interface ToggleSetting extends SettingBase {
  kind: "toggle";
  value: boolean;
  onChange: (value: boolean) => void;
}

export interface SelectSetting extends SettingBase {
  kind: "select";
  value: string;
  options: readonly SettingOption[];
  onChange: (value: string) => void;
}

export interface NumberSetting extends SettingBase {
  kind: "number";
  /** Null is "unset", not zero — the registry distinguishes them and a control
   *  that renders null as 0 silently invents a value the user never chose. */
  value: number | null;
  min?: number;
  max?: number;
  step?: number;
  /** Shown after the field: "wpm", "days". */
  unit?: string;
  onChange: (value: number) => void;
}

export interface TextSetting extends SettingBase {
  kind: "text";
  value: string;
  placeholder?: string;
  /** Enforced in the field, so an over-long value never reaches the backend. */
  maxLength?: number;
  onChange: (value: string) => void;
}

export interface HotkeySetting extends SettingBase {
  kind: "hotkey";
  /** Display form, e.g. "⌥Space". Null until one is recorded. */
  value: string | null;
  placeholder?: string;
  onChange: (display: string, capture: HotkeyCapture) => void;
}

/** The escape hatch: a control this file has no business knowing about. */
export interface CustomSetting extends SettingBase {
  kind: "custom";
  control: ReactNode;
}

export type SettingDef =
  | ToggleSetting
  | SelectSetting
  | NumberSetting
  | TextSetting
  | HotkeySetting
  | CustomSetting;
