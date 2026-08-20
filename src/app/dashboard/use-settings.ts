/**
 * SOURCE OF TRUTH KEYWORDS: useSettings, settingsChanged, getSettings,
 *   SettingValue, settings-live
 * WHAT:  The stored settings map, kept current by the settings-changed event.
 * WHY:   Replaces refetching after every write. A write is not the only way
 *        settings change — reset_setting, a rebind from another window, a
 *        migration on launch — so refreshing only where this window happens to
 *        write leaves it confidently displaying a stale value. Listening means
 *        the map is right regardless of who changed it. The event's `key` is
 *        ignored on purpose: get_settings returns the whole map in one cheap
 *        local read, so a targeted patch would be more code and one more thing
 *        to get out of step.
 * WHERE: Used by Dashboard.tsx (for the hotkey) and by SettingsView.tsx.
 */

import { commands, events, type SettingValue } from "@/lib/bindings";
import { useCommand, type CommandState } from "@/lib/ipc";
import { useTauriEvent } from "@/lib/use-event";

export function useSettings(): CommandState<{ [key in string]: SettingValue }> {
  const settings = useCommand(commands.getSettings, []);
  useTauriEvent(events.settingsChanged, () => settings.reload());
  return settings;
}
