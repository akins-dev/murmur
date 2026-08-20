/**
 * SOURCE OF TRUTH KEYWORDS: toControlSetting, DynamicOptions, SettingValueOf,
 *   valueFromControl, RegistrySettingDef, SettingKind, SettingValue
 * WHAT:  Adapts one registry SettingDef plus its stored SettingValue into the
 *        declarative shape SettingControl renders, and back again on change.
 * WHY:   The two shapes are deliberately different. The registry describes what
 *        a setting IS — kind, bounds, options, defaults — and knows nothing
 *        about React; the control describes what to RENDER and needs a value and
 *        an onChange bound to it. This file is the only place those meet, so
 *        neither side has to know about the other, and adding a SettingKind in
 *        Rust surfaces here as a non-exhaustive switch rather than as a control
 *        that silently renders nothing. DYNAMIC_CHOICE options are resolved by
 *        the caller and passed in, because they come from other commands.
 *        `requires_engine` and `requires_permission` are both enforced here: a
 *        setting the engine cannot honour, or whose OS grant is missing, is
 *        disabled and SAYS what is missing rather than being offered and doing
 *        nothing. "Paste automatically: ON" while Accessibility is denied is a
 *        control that lies — the user said yes, the app agreed, and every
 *        session still delivered clipboard-only.
 *
 *        A blocked control keeps its STORED VALUE rather than reading as off.
 *        The preference is real; it simply cannot take effect yet, and the
 *        disabled state plus the reason already carry that. Showing it as off
 *        would destroy the user's actual setting to say something the reason
 *        line says better — and it would appear to flip itself on the moment
 *        the grant arrived, which looks like the app editing their choices.
 * WHERE: Used by SettingsView.tsx. Bridges lib/bindings.ts to
 *        components/global/setting-control.
 */

import { bindingFromCapture, glyphsForBinding } from "@/lib/hotkey";
import { missingPermissions, permissionLabel } from "@/lib/use-permissions";
import type {
  ChoiceSource,
  EngineCapabilities,
  EngineFeature,
  PermissionReport,
  SettingDef as RegistrySettingDef,
  SettingValue,
} from "@/lib/bindings";
import type { SettingDef as ControlSetting, SettingOption } from "@/components/global";

export type DynamicOptions = Readonly<Record<ChoiceSource, readonly SettingOption[]>>;

/** Plain-language names for the abilities a setting can require of an engine. */
const FEATURE_LABEL: Readonly<Record<EngineFeature, string>> = {
  STREAMING: "streaming transcription",
  LANGUAGE_AUTO_DETECT: "automatic language detection",
  INITIAL_PROMPT: "vocabulary prompting",
  OFFLINE: "offline transcription",
};

/**
 * WHAT:  The abilities `def` needs that the selected engine does not declare.
 * WHY:   Returns empty while capabilities are still loading, so a setting is
 *        never disabled on the strength of an answer that has not arrived —
 *        briefly greying out every control reads as breakage.
 */
function missingFeatures(def: RegistrySettingDef, engine: EngineCapabilities | null): EngineFeature[] {
  if (!engine) return [];
  return def.requires_engine.filter((feature) => !engine.features.includes(feature));
}

function boolOf(value: SettingValue): boolean {
  return value.type === "BOOL" ? value.value : false;
}

function numberOf(value: SettingValue): number | null {
  return value.type === "NUMBER" ? value.value : null;
}

function textOf(value: SettingValue): string {
  if (value.type === "TEXT") return value.value;
  return value.type === "CHOICE" ? value.value : "";
}

/**
 * WHAT:  One registry setting as a renderable control.
 * WHY:   `stored ?? def.default` rather than an empty control: a setting the
 *        user has never touched still has a declared default, and rendering it
 *        blank would invite them to "fix" something that was already correct.
 */
export function toControlSetting(
  def: RegistrySettingDef,
  stored: SettingValue | undefined,
  dynamic: DynamicOptions,
  engine: EngineCapabilities | null,
  permissions: readonly PermissionReport[] | null,
  onChange: (value: SettingValue) => void,
): ControlSetting {
  const value = stored ?? def.default;
  const missing = missingFeatures(def, engine);
  const ungranted = missingPermissions(def.requires_permission, permissions);

  // Both blockers can apply at once, so they append rather than replace.
  const notes: string[] = [];
  if (missing.length > 0 && engine) {
    notes.push(
      `Unavailable on ${engine.display_name} — it has no ${missing
        .map((feature) => FEATURE_LABEL[feature])
        .join(" or ")}.`,
    );
  }
  if (ungranted.length > 0) {
    notes.push(
      `Needs ${ungranted.map(permissionLabel).join(" and ")} access, which Murmur does not have yet.`,
    );
  }

  const base = {
    id: def.key,
    label: def.label,
    description: [def.description, ...notes].join(" "),
    disabled: missing.length > 0 || ungranted.length > 0,
  };

  switch (def.kind.kind) {
    case "TOGGLE":
      return {
        ...base,
        kind: "toggle",
        value: boolOf(value),
        onChange: (next) => onChange({ type: "BOOL", value: next }),
      };

    case "TEXT":
      return {
        ...base,
        kind: "text",
        value: textOf(value),
        placeholder: def.kind.placeholder ?? undefined,
        maxLength: def.kind.max_len ?? undefined,
        onChange: (next) => onChange({ type: "TEXT", value: next }),
      };

    case "NUMBER":
      return {
        ...base,
        kind: "number",
        value: numberOf(value),
        min: def.kind.min ?? undefined,
        max: def.kind.max ?? undefined,
        step: def.kind.step ?? undefined,
        unit: def.kind.unit ?? undefined,
        onChange: (next) => onChange({ type: "NUMBER", value: next }),
      };

    case "CHOICE":
      return {
        ...base,
        kind: "select",
        value: textOf(value),
        options: def.kind.options.map((option) => ({
          value: option.value,
          label: option.label,
          description: option.description ?? undefined,
        })),
        onChange: (next) => onChange({ type: "CHOICE", value: next }),
      };

    case "DYNAMIC_CHOICE": {
      const options = dynamic[def.kind.source];
      return {
        ...base,
        kind: "select",
        value: textOf(value),
        options,
        // Nothing to choose from is a real state — a machine with one input
        // device, or a model list that has not loaded. Disabled says so;
        // an empty dropdown that opens onto nothing does not.
        disabled: base.disabled || options.length === 0,
        onChange: (next) => onChange({ type: "CHOICE", value: next }),
      };
    }

    case "HOTKEY":
      return {
        ...base,
        kind: "hotkey",
        value: value.type === "HOTKEY" ? glyphsForBinding(value.value).join("") : null,
        placeholder: "Press keys",
        onChange: (_display, capture) => onChange({ type: "HOTKEY", value: bindingFromCapture(capture) }),
      };
  }
}
