/**
 * SOURCE OF TRUTH KEYWORDS: SettingControl, SettingControlProps, SettingDef,
 *   renderControl, settings-row
 * WHAT:  Renders one settings row from a SettingDef — label and description on
 *        the left, the control for its `kind` right-aligned.
 * WHY:   It branches on `kind` and nothing else. A branch on a feature name
 *        here would put a second source of truth next to the Rust registry, and
 *        the registry is the only place that is allowed to know what the app
 *        has (CLAUDE.md §3). That is also why the definition arrives whole as
 *        one prop rather than as spread fields: the generated registry mirror
 *        hands over an object, and destructuring it here would mean editing
 *        this file every time the registry grows a field.
 * WHERE: The settings view maps the registry's SettingDefs straight onto this.
 *        Controls live in ./controls.tsx, types in ./types.ts.
 */

import { cn } from "@/lib/utils";
import { HotkeyControl, NumberControl, SelectControl, TextControl, ToggleControl } from "./controls";
import type { SettingDef } from "./types";

export interface SettingControlProps {
  setting: SettingDef;
  className?: string;
}

function renderControl(setting: SettingDef) {
  switch (setting.kind) {
    case "toggle":
      return <ToggleControl setting={setting} />;
    case "select":
      return <SelectControl setting={setting} />;
    case "number":
      return <NumberControl setting={setting} />;
    case "text":
      return <TextControl setting={setting} />;
    case "hotkey":
      return <HotkeyControl setting={setting} />;
    case "custom":
      return setting.control;
  }
}

export function SettingControl({ setting, className }: SettingControlProps) {
  return (
    <div className={cn("flex items-center gap-4 py-3", setting.disabled && "opacity-50", className)}>
      <div className="min-w-0 flex-1">
        <label htmlFor={setting.id} className="text-body text-text-primary">
          {setting.label}
        </label>
        {setting.description ? <p className="text-caption text-text-secondary">{setting.description}</p> : null}
      </div>
      <div className="flex shrink-0 items-center justify-end">{renderControl(setting)}</div>
    </div>
  );
}
