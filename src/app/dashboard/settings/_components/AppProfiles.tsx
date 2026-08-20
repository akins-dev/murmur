/**
 * SOURCE OF TRUTH KEYWORDS: AppProfiles, listAppProfiles, saveAppProfile,
 *   deleteAppProfile, AppProfile, bundleCandidates, displayNameFor
 * WHAT:  Per-app profiles: which apps have one, whether it is on, and what it
 *        overrides.
 * WHY:   The app is chosen from bundle ids seen in HISTORY, offered as
 *        suggestions on a free-text field. No command lists installed apps, and
 *        asking someone to type "com.tinyspeck.slackmacgap" from memory is not a
 *        UI — but the sessions table already knows every app they have actually
 *        dictated into, which is exactly the set worth having a profile for. The
 *        field stays free text so an app they have not used yet is still
 *        reachable. Saves go straight to the backend on every change and the
 *        list reloads: save_app_profile rejects an override naming a setting the
 *        registry does not declare, so its AppError is surfaced rather than the
 *        frontend pre-filtering and hiding a real disagreement.
 * WHERE: Its own section in SettingsView. Override editing is ProfileOverrides.
 */

import { useCallback, useId, useMemo, useState, type FormEvent } from "react";
import { Plus, Trash2 } from "lucide-react";
import {
  commands,
  type AppError,
  type AppProfile,
  type EngineCapabilities,
  type PermissionReport,
  type SettingDef as RegistrySettingDef,
  type SettingValue,
} from "@/lib/bindings";
import { unwrapCommand, useCommand } from "@/lib/ipc";
import { EmptyState, ErrorSurface } from "@/components/global";
import { ProfileOverrides } from "./ProfileOverrides";
import type { DynamicOptions } from "../to-setting-def";

/** Enough history to see the apps someone actually dictates into. */
const CANDIDATE_SCAN = 200;
const FIELD_CLASS = "hairline h-8 min-w-0 rounded-input bg-sunken px-2 text-body text-text-primary";

/** "com.apple.Terminal" → "Terminal". A starting point the user can edit, not
 *  a claim about the app's real name — nothing here knows that. */
function displayNameFor(bundleId: string): string {
  const last = bundleId.split(".").filter(Boolean).pop() ?? bundleId;
  return last.charAt(0).toUpperCase() + last.slice(1);
}

export interface AppProfilesProps {
  defs: readonly RegistrySettingDef[];
  globals: { [key in string]: SettingValue } | null;
  dynamic: DynamicOptions;
  engine: EngineCapabilities | null;
  permissions: readonly PermissionReport[] | null;
}

export function AppProfiles({ defs, globals, dynamic, engine, permissions }: AppProfilesProps) {
  const profiles = useCommand(commands.listAppProfiles, []);
  const recent = useCommand(() => commands.listHistory({ limit: CANDIDATE_SCAN, offset: 0 }), []);
  const [bundleId, setBundleId] = useState("");
  const [saveError, setSaveError] = useState<AppError | null>(null);
  const listId = useId();

  const candidates = useMemo(() => {
    const known = new Set((profiles.data ?? []).map((profile) => profile.bundle_id));
    const seen = new Set<string>();
    for (const session of recent.data ?? []) {
      if (session.app_bundle_id && !known.has(session.app_bundle_id)) seen.add(session.app_bundle_id);
    }
    return [...seen].sort();
  }, [profiles.data, recent.data]);

  const save = useCallback(
    (profile: AppProfile) => {
      void unwrapCommand(() => commands.saveAppProfile({ profile })).then((result) => {
        setSaveError(result.status === "error" ? result.error : null);
        profiles.reload();
      });
    },
    [profiles],
  );

  const remove = useCallback(
    (profile: AppProfile) => {
      void unwrapCommand(() => commands.deleteAppProfile({ bundle_id: profile.bundle_id })).then(profiles.reload);
    },
    [profiles],
  );

  const add = useCallback(
    (event: FormEvent) => {
      event.preventDefault();
      const trimmed = bundleId.trim();
      if (trimmed.length === 0) return;
      save({ bundle_id: trimmed, display_name: displayNameFor(trimmed), overrides: {}, enabled: true });
      setBundleId("");
    },
    [bundleId, save],
  );

  if (profiles.error) return <ErrorSurface error={profiles.error} onRetry={profiles.reload} size="compact" />;

  return (
    <div className="flex flex-col gap-3">
      {saveError ? <ErrorSurface error={saveError} size="compact" /> : null}

      <form onSubmit={add} className="flex items-center gap-2">
        <input
          value={bundleId}
          list={listId}
          onChange={(event) => setBundleId(event.target.value)}
          placeholder="Application bundle id, e.g. com.apple.Terminal"
          className={`${FIELD_CLASS} flex-1`}
        />
        <datalist id={listId}>
          {candidates.map((candidate) => (
            <option key={candidate} value={candidate} />
          ))}
        </datalist>
        <button
          type="submit"
          className="hairline flex h-8 shrink-0 items-center gap-1 rounded-input bg-sunken px-3 text-body text-text-primary transition-colors hover:bg-sunken-strong"
        >
          <Plus className="size-4" />
          Add
        </button>
      </form>

      {profiles.data && profiles.data.length === 0 ? (
        <EmptyState
          size="compact"
          headline="No per-app profiles"
          description="Give an app its own settings — filler removal always on in Slack, formatting commands on in the terminal."
        />
      ) : null}

      <ul className="flex flex-col">
        {(profiles.data ?? []).map((profile) => (
          <li key={profile.bundle_id} className="hairline-b flex flex-col gap-2 py-3 last:border-b-0">
            <div className="flex items-center gap-2">
              <input
                type="checkbox"
                checked={profile.enabled}
                aria-label={`Enable profile for ${profile.display_name}`}
                onChange={(event) => save({ ...profile, enabled: event.target.checked })}
                className="accent-[var(--accent)]"
              />
              <input
                defaultValue={profile.display_name}
                aria-label={`Name for ${profile.bundle_id}`}
                onBlur={(event) => {
                  if (event.target.value !== profile.display_name) {
                    save({ ...profile, display_name: event.target.value });
                  }
                }}
                className={`${FIELD_CLASS} w-40`}
              />
              <span className="min-w-0 flex-1 truncate text-caption text-text-tertiary">{profile.bundle_id}</span>
              <button
                type="button"
                aria-label={`Delete profile for ${profile.display_name}`}
                onClick={() => remove(profile)}
                className="shrink-0 rounded-input p-1 text-text-secondary transition-colors hover:bg-sunken hover:text-danger"
              >
                <Trash2 className="size-4" />
              </button>
            </div>

            <ProfileOverrides
              profile={profile}
              defs={defs}
              globals={globals}
              dynamic={dynamic}
              engine={engine}
              permissions={permissions}
              onChange={save}
            />
          </li>
        ))}
      </ul>
    </div>
  );
}
