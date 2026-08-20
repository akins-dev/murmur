/**
 * SOURCE OF TRUTH KEYWORDS: ProfileOverrides, AppProfile, sparse-overrides,
 *   addOverride, removeOverride, toControlSetting
 * WHAT:  The override editor for one app profile: the settings this profile
 *        changes, and a way to add another.
 * WHY:   It renders ONLY the overridden settings, never the full list with the
 *        overridden ones marked. That is the whole design of the feature: a
 *        profile is sparse, so everything it does not name keeps following the
 *        global setting and keeps following it as the global changes. A UI that
 *        showed every setting would invite the user to "set" them all, which
 *        would freeze that app's preferences at the moment the profile was
 *        created and quietly stop tracking anything they change later.
 *        A new override seeds from the CURRENT global value rather than the
 *        registry default, because the user is starting from what the app does
 *        today and changing one thing about it.
 * WHERE: Inside each profile row of AppProfiles.tsx. Reuses SettingControl
 *        through to-setting-def.ts, so an override renders identically to the
 *        global setting it shadows.
 */

import { useState } from "react";
import { X } from "lucide-react";
import type {
  AppProfile,
  EngineCapabilities,
  PermissionReport,
  SettingDef as RegistrySettingDef,
  SettingValue,
} from "@/lib/bindings";
import { SettingControl } from "@/components/global";
import { toControlSetting, type DynamicOptions } from "../to-setting-def";

export interface ProfileOverridesProps {
  profile: AppProfile;
  defs: readonly RegistrySettingDef[];
  /** Global values — what an override starts from and what it shadows. */
  globals: { [key in string]: SettingValue } | null;
  dynamic: DynamicOptions;
  engine: EngineCapabilities | null;
  permissions: readonly PermissionReport[] | null;
  onChange: (profile: AppProfile) => void;
}

export function ProfileOverrides({
  profile,
  defs,
  globals,
  dynamic,
  engine,
  permissions,
  onChange,
}: ProfileOverridesProps) {
  const [adding, setAdding] = useState("");

  const overridden = defs.filter((def) => def.key in profile.overrides);
  const available = defs.filter((def) => !(def.key in profile.overrides));

  const setOverride = (key: string, value: SettingValue): void => {
    onChange({ ...profile, overrides: { ...profile.overrides, [key]: value } });
  };

  const removeOverride = (key: string): void => {
    const { [key]: _removed, ...rest } = profile.overrides;
    onChange({ ...profile, overrides: rest });
  };

  const addOverride = (key: string): void => {
    const def = defs.find((candidate) => candidate.key === key);
    if (!def) return;
    setOverride(key, globals?.[key] ?? def.default);
    setAdding("");
  };

  return (
    <div className="flex flex-col gap-1 pl-6">
      {overridden.map((def) => (
        <div key={def.key} className="flex items-center gap-2">
          <SettingControl
            className="flex-1"
            setting={toControlSetting(def, profile.overrides[def.key], dynamic, engine, permissions, (value) =>
              setOverride(def.key, value),
            )}
          />
          <button
            type="button"
            aria-label={`Stop overriding ${def.label}`}
            onClick={() => removeOverride(def.key)}
            className="shrink-0 rounded-input p-1 text-text-tertiary transition-colors hover:bg-sunken hover:text-danger"
          >
            <X className="size-4" />
          </button>
        </div>
      ))}

      {overridden.length === 0 ? (
        <p className="py-2 text-caption text-text-tertiary">
          Nothing overridden — this app follows every global setting.
        </p>
      ) : null}

      {available.length > 0 ? (
        <select
          value={adding}
          aria-label={`Add an override for ${profile.display_name}`}
          onChange={(event) => addOverride(event.target.value)}
          className="hairline h-8 w-fit rounded-input bg-sunken px-2 text-body text-text-secondary"
        >
          <option value="">Add an override…</option>
          {available.map((def) => (
            <option key={def.key} value={def.key}>
              {def.label}
            </option>
          ))}
        </select>
      ) : null}
    </div>
  );
}
