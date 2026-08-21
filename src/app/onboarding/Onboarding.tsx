/**
 * SOURCE OF TRUTH KEYWORDS: Onboarding, OnboardingStep, usePermissions,
 *   checkPermissions, listModels, modelStateChanged, defaultModel, StepShell
 * WHAT:  The first-run flow: a three-slide tour of what the app does, then
 *        permissions, then the model, then a real hotkey test, and finally an
 *        invitation that asks for nothing.
 * WHY:   The tour comes first because the operator asked for the app to teach
 *        before it asks: a permission dialog is a strange first thing to meet,
 *        and someone who has been shown what the app does has a reason to grant
 *        it. Past the tour, the step is DERIVED from what the backend reports,
 *        never advanced by a counter — permission granted, model ready, session delivered. So a user
 *        who already granted microphone access skips that screen, and one who
 *        quits mid-download returns to exactly where they were rather than to
 *        step one. Only the microphone blocks progress: accessibility merely
 *        degrades paste to clipboard-only, so gating on it would be inventing a
 *        requirement the app does not have. The model's live state is applied
 *        here rather than inside the model step, because it is what decides
 *        whether the flow ADVANCES: a model on disk but unverified reports
 *        Verifying and only flips to Ready when the background hash check
 *        finishes, so a step listening on its own would update its own display
 *        and leave the user stuck on it forever. Finishing writes
 *        general.onboarding_complete and only then closes the window: the Rust
 *        side gates this window on that setting at launch, so closing without
 *        it would reopen onboarding on the next start with everything already
 *        done. A failed write therefore keeps the window open and says why,
 *        rather than closing on a promise it did not keep.
 * WHERE: Mounted by src/entries/onboarding.tsx. Steps live in ./_components.
 */

import { useCallback, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  commands,
  events,
  type AppError,
  type HotkeyBinding,
  type ModelReport,
  type ModelState,
} from "@/lib/bindings";
import { unwrapCommand, useCommand } from "@/lib/ipc";
import { useTauriEvent } from "@/lib/use-event";
import { ErrorSurface } from "@/components/global";
import { StepShell } from "./_components/StepShell";
import { usePermissions } from "@/lib/use-permissions";
import { dictationModeFrom } from "@/lib/dictation-mode";
import { PermissionStep } from "./_components/PermissionStep";
import { ModelStep } from "./_components/ModelStep";
import { HotkeyStep } from "./_components/HotkeyStep";
import { TourStep } from "./_components/TourStep";
import { InviteStep } from "./_components/InviteStep";

/** Declared by the Onboarding capability in the registry. Named here because
 *  "which setting means first run is over" is a contract between the two
 *  windows, and it is the only settings key the frontend has to know. */
const ONBOARDING_COMPLETE_KEY = "general.onboarding_complete";

function pickModel(models: readonly ModelReport[]): ModelReport | null {
  return models.find((model) => model.descriptor.is_default) ?? models[0] ?? null;
}

function dictationHotkey(hotkeys: readonly (HotkeyBinding | null)[]): HotkeyBinding | null {
  return hotkeys.find((binding): binding is HotkeyBinding => binding !== null) ?? null;
}

export function Onboarding() {
  const permissions = usePermissions();
  const models = useCommand(commands.listModels, []);
  const registry = useCommand(commands.getRegistry, []);
  // Read rather than assumed: every screen here that teaches the gesture has to
  // match dictation.mode, or it teaches the wrong one. No live subscription —
  // nothing changes settings during first run.
  const settings = useCommand(commands.getSettings, []);
  // The tour teaches; it has no backend state to derive from, so it is the
  // one genuinely local step in this flow. See TourStep.
  const [toured, setToured] = useState(false);
  const [tested, setTested] = useState(false);
  /** The invitation is the last screen, after every step that asks for
   *  something. Local, like the tour: there is no backend state behind it. */
  const [invited, setInvited] = useState(false);
  const [finishError, setFinishError] = useState<AppError | null>(null);
  const [liveStates, setLiveStates] = useState<Readonly<Record<string, ModelState>>>({});

  useTauriEvent(events.modelStateChanged, (payload) => {
    setLiveStates((current) => ({ ...current, [payload.model_id]: payload.state }));
  });

  const micGranted =
    permissions.data?.find((report) => report.permission === "MICROPHONE")?.state === "GRANTED";
  const model = useMemo(() => {
    const picked = pickModel(models.data ?? []);
    if (!picked) return null;
    return { ...picked, state: liveStates[picked.descriptor.id] ?? picked.state };
  }, [liveStates, models.data]);
  const modelReady = model?.state.kind === "READY";

  const mode = dictationModeFrom(settings.data);

  const hotkey = dictationHotkey(
    (registry.data?.capabilities ?? []).map((capability) => capability.hotkey?.default ?? null),
  );

  const finish = useCallback(() => {
    void unwrapCommand(() =>
      commands.setSetting({ key: ONBOARDING_COMPLETE_KEY, value: { type: "BOOL", value: true } }),
    ).then((result) => {
      if (result.status === "error") {
        setFinishError(result.error);
        return;
      }
      void getCurrentWindow().close();
    });
  }, []);

  const error = permissions.error ?? models.error ?? registry.error;

  return (
    // NO GLASS PANEL HERE. This window already HAS native Popover vibrancy
    // behind it (bootstrap.rs applies it), and painting material-elevated on
    // top was covering that vibrancy with a 72%-opaque surface — which is
    // exactly why onboarding read as a solid sheet while the rest of the app
    // read as glass. The dashboard has no wrapper for the same reason: when a
    // window IS the surface, the web layer's job is to stay out of the way and
    // let the native material through. The noise layer goes with it; at 3% over
    // a surface that is now genuinely translucent it was texture on nothing.
    <div className="h-full">
      <div data-tauri-drag-region className="h-[var(--titlebar-height)]" />
      {error ? (
        <ErrorSurface
          error={error}
          onRetry={() => {
            permissions.reload();
            models.reload();
            registry.reload();
          }}
        />
      ) : invited ? (
        <InviteStep onFinish={finish} finishError={finishError} />
      ) : !toured ? (
        <TourStep hotkey={hotkey} mode={mode} onDone={() => setToured(true)} />
      ) : !micGranted ? (
        <StepShell
          title="Two permissions"
          description="Murmur runs entirely on your Mac. It needs the microphone to hear you, and accessibility to paste for you."
        >
          <PermissionStep reports={permissions.data ?? []} onChanged={permissions.reload} />
        </StepShell>
      ) : !modelReady && model ? (
        <StepShell
          title="One model to download"
          description="This runs on your machine, so the model lives on your disk. It is downloaded once."
        >
          <ModelStep model={model} onChanged={models.reload} />
        </StepShell>
      ) : (
        <StepShell
          title="Try it"
          description={
            mode === "push_to_talk"
              ? "Hold the hotkey anywhere, say something, and let go."
              : "Press the hotkey anywhere, say something, and press it again."
          }
          action={
            tested ? (
              <button
                type="button"
                onClick={() => setInvited(true)}
                // Advances to the invitation rather than closing: setup is
                // finished here, but there is one screen left that asks for
                // nothing. The inverted fill is the app's one primary style
                // (docs/04 §1.3) — monochrome, outweighing every secondary by
                // contrast rather than by a hue the palette does not have.
                className="h-[var(--control-height)] rounded-input bg-text-primary px-4 text-body font-medium text-opaque-elevated transition-opacity hover:opacity-90"
              >
                Start using Murmur
              </button>
            ) : null
          }
        >
          <HotkeyStep hotkey={hotkey} mode={mode} onDelivered={() => setTested(true)} />
        </StepShell>
      )}
    </div>
  );
}
