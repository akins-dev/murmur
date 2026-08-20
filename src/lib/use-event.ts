/**
 * SOURCE OF TRUTH KEYWORDS: useTauriEvent, TauriEventChannel, EventCallback,
 *   unlisten, subscribe
 * WHAT:  Subscribes to one typed Rust event for the lifetime of a component.
 * WHY:   listen() resolves to its own unsubscribe function asynchronously, so a
 *        component that unmounts before it resolves leaks a listener that then
 *        calls setState on a dead tree. The disposed flag is the fix, and it is
 *        the kind of thing that gets written correctly once and wrongly four
 *        times. The handler is held in a ref so a caller can pass an inline
 *        arrow without resubscribing on every render — resubscribing to
 *        audio-level-changed would drop frames on every parent render.
 * WHERE: The pill (session-state-changed, audio-level-changed), the model
 *        manager (model-download-progress) and onboarding (onboarding-progress).
 */

import { useEffect, useRef } from "react";

/** The shape tauri-specta's makeEvent() produces. Structural, so this hook does
 *  not depend on the generated file's internal types. */
export interface TauriEventChannel<T> {
  listen: (callback: (event: { payload: T }) => void) => Promise<() => void>;
}

export function useTauriEvent<T>(channel: TauriEventChannel<T>, onEvent: (payload: T) => void): void {
  const handlerRef = useRef(onEvent);
  handlerRef.current = onEvent;

  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;

    void channel
      .listen((event) => handlerRef.current(event.payload))
      .then((dispose) => {
        if (disposed) dispose();
        else unlisten = dispose;
      })
      .catch(() => {
        // Outside a Tauri webview there is nothing to listen to. The view still
        // renders; it simply never receives an event.
      });

    return () => {
      disposed = true;
      unlisten?.();
    };
  }, [channel]);
}
