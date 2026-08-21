/**
 * SOURCE OF TRUTH KEYWORDS: SettingsView, getRegistry, getSettings, setSetting,
 *   SettingSection, SECTION_ORDER, advanced-disclosure, toControlSetting, AppProfiles
 * WHAT:  The settings page: every registry SettingDef grouped into its declared
 *        section, plus the model manager, the dictionary, per-app profiles and
 *        the privacy controls.
 * WHY:   Nothing here lists the settings. The rows come from the registry, the
 *        grouping comes from each def's own `section`, and the ordering of the
 *        sections is the only thing this file decides — so adding a setting in
 *        Rust makes it appear here with no frontend change (CLAUDE.md §7).
 *        Advanced settings sit behind a disclosure because they exist to unstick
 *        a specific machine, not to be browsed. Writes go through set_setting
 *        and nothing is patched locally — the settings-changed event brings the
 *        new map back, so the display is correct whoever changed it and whether
 *        or not the write was accepted. A rejected write therefore shows the
 *        old value AND the reason, instead of leaving the control sitting on a
 *        value the backend refused.
 * WHERE: Rendered by Dashboard.tsx for the registry's "settings" route.
 */

import { useCallback, useMemo, useState } from "react";
import { ChevronDown } from "lucide-react";
import {
  commands,
  type AppError,
  type EngineCapabilities,
  type OsPermission,
  type PermissionReport,
  type RegistrySnapshot,
  type SettingDef as RegistrySettingDef,
  type SettingSection,
  type SettingValue,
} from "@/lib/bindings";
import { unwrapCommand, useCommand } from "@/lib/ipc";
import { missingPermissions, usePermissions } from "@/lib/use-permissions";
import { useSettings } from "../use-settings";
import { formatBytes } from "@/lib/format";
import { cn } from "@/lib/utils";
import { ErrorSurface, ScrollArea, SettingControl, Skeleton } from "@/components/global";
import type { SettingOption } from "@/components/global";
import { PermissionNotice } from "./_components/PermissionNotice";
import { ModelManager } from "./_components/ModelManager";
import { DictionaryManager } from "./_components/DictionaryManager";
import { AppProfiles } from "./_components/AppProfiles";
import { toControlSetting, type DynamicOptions } from "./to-setting-def";

/** Presentation order and wording — the one thing the registry does not decide,
 *  because "which group comes first" is a layout judgement, not a fact about
 *  the app. */
const SECTION_ORDER: readonly SettingSection[] = [
  "RECORDING",
  "TRANSCRIPTION",
  "OUTPUT",
  "VOCABULARY",
  "PRIVACY",
  "GENERAL",
];

const SECTION_LABEL: Readonly<Record<SettingSection, string>> = {
  RECORDING: "Recording",
  TRANSCRIPTION: "Transcription",
  OUTPUT: "Output",
  VOCABULARY: "Vocabulary",
  PRIVACY: "Privacy",
  GENERAL: "General",
};

export interface SettingsViewProps {
  registry: RegistrySnapshot;
  /** Deep-link target, e.g. from an error action. */
  section: string | null;
}

export function SettingsView({ registry, section }: SettingsViewProps) {
  const settings = useSettings();
  const devices = useCommand(commands.listInputDevices, []);
  const models = useCommand(commands.listModels, []);
  const languages = useCommand(commands.listLanguages, []);
  const engine = useCommand(commands.getEngineCapabilities, []);
  // Re-checked on window focus: the user grants this in System Settings, in
  // another app, and comes back expecting these controls to have noticed.
  const permissions = usePermissions();
  const [writeError, setWriteError] = useState<AppError | null>(null);

  const dynamic = useMemo<DynamicOptions>(() => {
    const deviceOptions: SettingOption[] = (devices.data ?? []).map((device) => ({
      value: device.id,
      label: device.is_default ? `${device.name} (system default)` : device.name,
    }));
    const modelOptions: SettingOption[] = (models.data ?? []).map((model) => ({
      value: model.descriptor.id,
      label: model.descriptor.display_name,
      description: `${formatBytes(model.descriptor.size_bytes)} · ${model.descriptor.approx_ram_mb} MB memory`,
      disabled: model.state.kind !== "READY",
    }));
    // Unsupported languages are shown and disabled, never dropped: a language
    // that silently vanished from the list is one the user cannot work out how
    // to get back. The reason goes in the label because a native <option>
    // cannot render anything else.
    // Named only once known. "the selected engine" is true whether or not the
    // capabilities call has landed, so the reason is never briefly wrong.
    const engineName = engine.data?.display_name ?? "the selected engine";
    const languageOptions: SettingOption[] = (languages.data ?? []).map((language) => ({
      value: language.code,
      label: language.supported ? language.label : `${language.label} — not supported by ${engineName}`,
      description: language.supported ? undefined : `${engineName} cannot transcribe ${language.label}.`,
      disabled: !language.supported,
    }));
    return { INPUT_DEVICES: deviceOptions, MODELS: modelOptions, LANGUAGES: languageOptions };
  }, [devices.data, engine.data, languages.data, models.data]);

  const write = useCallback((key: string, value: SettingValue) => {
    // No reload here: settings-changed brings the new map back on its own.
    void unwrapCommand(() => commands.setSetting({ key, value })).then((result) => {
      setWriteError(result.status === "error" ? result.error : null);
    });
  }, []);

  const allDefs = useMemo(
    () => registry.capabilities.flatMap((capability) => capability.settings),
    [registry],
  );

  const grouped = useMemo(() => {
    const map = new Map<SettingSection, RegistrySettingDef[]>();
    for (const capability of registry.capabilities) {
      for (const def of capability.settings) {
        const bucket = map.get(def.section);
        if (bucket) bucket.push(def);
        else map.set(def.section, [def]);
      }
    }
    return map;
  }, [registry]);

  if (settings.error) return <ErrorSurface error={settings.error} onRetry={settings.reload} />;

  // Until the stored values land, every control would render from its registry
  // default — a toggle showing off and then flipping on reads as the app having
  // forgotten the user's preference, which is worse than showing nothing.
  if (!settings.data) {
    return (
      <ScrollArea contentClassName="px-[var(--page-padding-x)]">
          <Skeleton rows={8} />
        </ScrollArea>
    );
  }

  return (
    <ScrollArea contentClassName="flex flex-col gap-8 px-[var(--page-padding-x)] pb-8">
        {writeError ? <ErrorSurface size="compact" error={writeError} /> : null}
        {SECTION_ORDER.map((key) => {
          const defs = grouped.get(key) ?? [];
          const extra = EXTRAS[key];
          if (defs.length === 0 && !extra) return null;

          return (
            <SettingsSection
              key={key}
              title={SECTION_LABEL[key]}
              highlighted={section === key.toLowerCase()}
              defs={defs}
              values={settings.data}
              dynamic={dynamic}
              engine={engine.data}
              permissions={permissions.data}
              onWrite={write}
              extra={extra}
            />
          );
        })}

        {/* Its own section rather than folded into a registry one: a profile is
            not a setting, and claiming it belongs to Recording or Output would
            be a layout lie about what the registry declares. */}
        <section className="flex flex-col gap-1">
          <h2 className="text-heading text-text-primary">Per-app profiles</h2>
          <p className="text-caption text-text-secondary">
            Settings that apply only while a particular app is in front. Anything a profile does not override keeps
            following the global setting.
          </p>
          <AppProfiles
            defs={allDefs}
            globals={settings.data}
            dynamic={dynamic}
            engine={engine.data}
            permissions={permissions.data}
          />
        </section>
    </ScrollArea>
  );
}

/** Panels that are not settings but belong inside a section. */
type SectionPanel = "MODELS_PANEL" | "DICTIONARY_PANEL" | "PRIVACY_PANEL";

const EXTRAS: Partial<Record<SettingSection, SectionPanel>> = {
  TRANSCRIPTION: "MODELS_PANEL",
  VOCABULARY: "DICTIONARY_PANEL",
  PRIVACY: "PRIVACY_PANEL",
};

function SettingsSection({
  title,
  highlighted,
  defs,
  values,
  dynamic,
  engine,
  permissions,
  onWrite,
  extra,
}: {
  title: string;
  highlighted: boolean;
  defs: readonly RegistrySettingDef[];
  values: { [key in string]: SettingValue } | null;
  dynamic: DynamicOptions;
  engine: EngineCapabilities | null;
  permissions: readonly PermissionReport[] | null;
  onWrite: (key: string, value: SettingValue) => void;
  extra: SectionPanel | undefined;
}) {
  const [showAdvanced, setShowAdvanced] = useState(false);
  const plain = defs.filter((def) => !def.advanced);
  const advanced = defs.filter((def) => def.advanced);

  // One line per distinct missing grant, not one per blocked control: two
  // controls behind the same permission are one problem with one fix.
  const blocking = [
    ...new Set(defs.flatMap((def) => missingPermissions(def.requires_permission, permissions))),
  ] as OsPermission[];

  return (
    <section className={cn("flex flex-col gap-1", highlighted && "rounded-card bg-sunken-strong p-4")}>
      <h2 className="text-heading text-text-primary">{title}</h2>

      <PermissionNotice permissions={blocking} />

      {plain.map((def) => (
        <SettingControl
          key={def.key}
          className="hairline-b last:border-b-0"
          setting={toControlSetting(def, values?.[def.key], dynamic, engine, permissions, (value) =>
            onWrite(def.key, value),
          )}
        />
      ))}

      {extra === "MODELS_PANEL" ? <ModelManager /> : null}
      {extra === "DICTIONARY_PANEL" ? <DictionaryManager /> : null}
      {extra === "PRIVACY_PANEL" ? <ClearHistoryControl /> : null}

      {advanced.length > 0 ? (
        <>
          <button
            type="button"
            onClick={() => setShowAdvanced((open) => !open)}
            aria-expanded={showAdvanced}
            className="flex w-fit items-center gap-1 py-2 text-label text-text-secondary transition-colors hover:text-text-primary"
          >
            <ChevronDown className={cn("size-4 transition-transform", showAdvanced && "rotate-180")} />
            Advanced
          </button>
          {showAdvanced
            ? advanced.map((def) => (
                <SettingControl
                  key={def.key}
                  className="hairline-b last:border-b-0"
                  setting={toControlSetting(def, values?.[def.key], dynamic, engine, permissions, (value) =>
                    onWrite(def.key, value),
                  )}
                />
              ))
            : null}
        </>
      ) : null}
    </section>
  );
}

/**
 * WHAT:  The delete-everything control.
 * WHY:   Two-step, and the second step is the destructive one. There is no
 *        tombstone and no undo behind this — clear_history destroys the rows —
 *        so the confirmation is the only safety net, exactly as the cancel
 *        countdown is for a recording.
 */
function ClearHistoryControl() {
  const [confirming, setConfirming] = useState(false);
  const [deleted, setDeleted] = useState<number | null>(null);

  return (
    <div className="flex items-center justify-between gap-4 py-3">
      <div className="min-w-0 flex-1">
        <p className="text-body text-text-primary">Delete all history</p>
        <p className="text-caption text-text-secondary">
          {deleted === null
            ? "Every transcript, permanently. This cannot be undone."
            : `Deleted ${deleted} transcript${deleted === 1 ? "" : "s"}.`}
        </p>
      </div>
      <button
        type="button"
        onClick={() => {
          if (!confirming) {
            setConfirming(true);
            return;
          }
          void unwrapCommand(commands.clearHistory).then((result) => {
            if (result.status === "ok") setDeleted(result.data);
            setConfirming(false);
          });
        }}
        onBlur={() => setConfirming(false)}
        className={cn(
          "hairline h-8 shrink-0 rounded-input px-3 text-body transition-colors",
          confirming ? "bg-danger text-opaque-elevated" : "bg-sunken text-text-primary hover:text-danger",
        )}
      >
        {confirming ? "Delete everything" : "Delete…"}
      </button>
    </div>
  );
}
