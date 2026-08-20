/**
 * SOURCE OF TRUTH KEYWORDS: Onboarding, OnboardingStep, usePermissions,
 *   checkPermissions, listModels, modelStateChanged, defaultModel, StepShell
 * WHAT:  The first-run flow: a three-slide tour of what the app does, then
 *        permissions, then the model, then a real hotkey test.
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
import { ErrorSurface, GlassPanel } from "@/components/global";
import { StepShell } from "./_components/StepShell";
import { usePermissions } from "@/lib/use-permissions";
import { PermissionStep } from "./_components/PermissionStep";
import { ModelStep } from "./_components/ModelStep";
import { HotkeyStep } from "./_components/HotkeyStep";
import { TourStep } from "./_components/TourStep";

const STEP_COUNT = 3;

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
  // The tour teaches; it has no backend state to derive from, so it is the
  // one genuinely local step in this flow. See TourStep.
  const [toured, setToured] = useState(false);
  const [tested, setTested] = useState(false);
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
    // "elevated", because the native material behind this window is Popover,
    // and docs/04 §3 maps Popover to glass-elevated. The web tint has to name
    // the same material the vibrancy is, or the two layers disagree and the
    // window reads a shade heavier than every other surface in the app.
    <GlassPanel material="elevated" radius="none" className="h-full">
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
      ) : !toured ? (
        <TourStep hotkey={hotkey} onDone={() => setToured(true)} />
      ) : !micGranted ? (
        <StepShell
          stepIndex={0}
          stepCount={STEP_COUNT}
          title="Two permissions"
          description="Murmur runs entirely on your Mac. It needs the microphone to hear you, and accessibility to paste for you."
        >
          <PermissionStep reports={permissions.data ?? []} onChanged={permissions.reload} />
        </StepShell>
      ) : !modelReady && model ? (
        <StepShell
          stepIndex={1}
          stepCount={STEP_COUNT}
          title="One model to download"
          description="This runs on your machine, so the model lives on your disk. It is downloaded once."
        >
          <ModelStep model={model} onChanged={models.reload} />
        </StepShell>
      ) : (
        <StepShell
          stepIndex={2}
          stepCount={STEP_COUNT}
          title="Try it"
          description="Press the hotkey anywhere, say something, and press it again."
          action={
            tested ? (
              <button
                type="button"
                onClick={finish}
                // The app's one primary action, so it is the one inverted fill:
                // solid --text-primary with the surface colour as its label. A
                // monochrome primary still outweighs every secondary on screen,
                // and it does it with contrast rather than with a hue the palette
                // reserves for a live session (docs/04 §1.3).
                className="h-[var(--control-height)] rounded-input bg-text-primary px-4 text-body font-medium text-opaque-elevated transition-opacity hover:opacity-90"
              >
                Start using Murmur
              </button>
            ) : null
          }
        >
          <>
            <HotkeyStep hotkey={hotkey} onDelivered={() => setTested(true)} />
            {finishError ? <ErrorSurface size="compact" error={finishError} onRetry={finish} /> : null}
          </>
        </StepShell>
      )}
    </GlassPanel>
  );
}
