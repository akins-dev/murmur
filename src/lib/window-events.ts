/**
 * SOURCE OF TRUTH KEYWORDS: NAV_SELECTED, NavSelected, navSelectedChannel,
 *   cross-window-event, detached-rail
 * WHAT:  Events that travel BETWEEN windows rather than up from Rust.
 * WHY:   These are the only events tauri-specta does not generate for us, so
 *        they are the only ones whose names live in TypeScript — and a channel
 *        name is a contract between two windows that never import each other.
 *        Written once here rather than as a string literal in the emitter and
 *        another in the listener, because two copies of a channel name fail
 *        SILENTLY: nothing throws, no type complains, the event is simply never
 *        heard and the feature looks dead.
 *
 *        The channel is shaped as a TauriEventChannel so it goes through
 *        useTauriEvent like every generated event does, which means it inherits
 *        the disposal handling that hook exists for rather than open-coding a
 *        second listen/unlisten dance. It is a module constant, not built per
 *        render, because that hook keys its effect on the channel's identity.
 * WHERE: Emitted by src/entries/sidebar.tsx, consumed by app/dashboard.
 */

import { listen } from "@tauri-apps/api/event";
import type { TauriEventChannel } from "./use-event";

/** The detached rail asking the dashboard to open a route. */
export const NAV_SELECTED = "nav-selected";

export interface NavSelected {
  route: string;
}

export const navSelectedChannel: TauriEventChannel<NavSelected> = {
  listen: (callback) => listen<NavSelected>(NAV_SELECTED, (event) => callback({ payload: event.payload })),
};
