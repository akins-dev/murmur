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

import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { TauriEventChannel } from "./use-event";

/** The detached rail asking the dashboard to open a route. */
export const NAV_SELECTED = "nav-selected";

export interface NavSelected {
  route: string;
}

/**
 * Registered on THIS WEBVIEW BY LABEL, not with the global `listen`.
 *
 * The rail sends with `emitTo(DASHBOARD_LABEL, ...)`, which carries the target
 * `{ kind: "AnyLabel", label: "dashboard" }`. A global `listen()` registers with
 * `{ kind: "Any" }` and by the documented semantics should still match — but
 * "should still match" is not a thing to be unsure about in the one wire the
 * navigation runs over, and the failure is silent on both sides: the rail emits
 * successfully, the dashboard is listening successfully, and nothing happens.
 * Listening on the current webview matches BOTH a label-targeted emit and a
 * global broadcast, so it is correct whichever way the sender is written.
 *
 * getCurrentWebviewWindow() is called inside listen rather than at module load:
 * outside a Tauri webview it throws, and this module is imported by a bundle
 * that has to survive being opened in a plain browser.
 */
export const navSelectedChannel: TauriEventChannel<NavSelected> = {
  listen: (callback) =>
    getCurrentWebviewWindow().listen<NavSelected>(NAV_SELECTED, (event) =>
      callback({ payload: event.payload }),
    ),
};
