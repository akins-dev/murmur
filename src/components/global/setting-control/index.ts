/**
 * SOURCE OF TRUTH KEYWORDS: setting-control-barrel, SettingControl, SettingDef,
 *   SettingKind, SettingOption, HotkeyCapture, formatHotkey
 * WHAT:  Barrel for the setting-control component and its declarative types.
 * WHY:   Folder boundaries export through a barrel (CLAUDE.md §8). The types
 *        are the contract the registry mirror will produce, so they are part of
 *        the public surface, not an internal detail.
 * WHERE: Re-exported by src/components/global/index.ts.
 */

export { SettingControl, type SettingControlProps } from "./SettingControl";
export { formatHotkey } from "./controls";
export type {
  SettingDef,
  SettingKind,
  SettingOption,
  HotkeyCapture,
  ToggleSetting,
  SelectSetting,
  NumberSetting,
  TextSetting,
  HotkeySetting,
  CustomSetting,
} from "./types";
