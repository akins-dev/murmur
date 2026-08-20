/**
 * SOURCE OF TRUTH KEYWORDS: PillWaveform, audioLevelChanged, WaveformHandle,
 *   displayLevel, waveform-input-ceiling, waveform-input-gamma, AudioLevel
 * WHAT:  Subscribes to audio-level-changed, maps the audio's RMS onto the
 *        waveform's visual range, and pushes it straight into the Waveform's
 *        imperative handle.
 * WHY:   The event fires far faster than React should re-render, and the payload
 *        is explicitly droppable — a missed frame is invisible, queueing them
 *        would add latency to the very thing being drawn. Pushing through the
 *        ref means a level arriving costs one DOM transform write and zero
 *        React work, so the pill stays inside its frame budget while whisper is
 *        decoding on another thread. rms is preferred over peak because peak
 *        pins to the top on any transient and the bars stop meaning anything.
 *        The scaling lives here rather than in the Waveform because it is
 *        knowledge about SPEECH, not about drawing: the component's contract is
 *        a 0..1 display level, and anything feeding it something else has to
 *        say so at its own boundary.
 * WHERE: The pill's RECORDING state. Wraps components/global/waveform; curve
 *        values are tokens (docs/04 §7).
 */

import { useRef, useState } from "react";
import { events } from "@/lib/bindings";
import { useTauriEvent } from "@/lib/use-event";
import { readNumberToken } from "@/lib/motion";
import { Waveform, type WaveformHandle } from "@/components/global";

export function PillWaveform({ className }: { className?: string }) {
  const waveform = useRef<WaveformHandle>(null);
  // Read once: this runs on every audio frame, and getComputedStyle there
  // would cost a style resolution per frame.
  const [curve] = useState(() => ({
    ceiling: readNumberToken("--waveform-input-ceiling"),
    gamma: readNumberToken("--waveform-input-gamma"),
  }));

  useTauriEvent(events.audioLevelChanged, (payload) => {
    const rms = payload.level.rms ?? payload.level.peak ?? 0;
    const ratio = Math.min(1, Math.max(0, rms / curve.ceiling));
    waveform.current?.push(Math.pow(ratio, curve.gamma));
  });

  return <Waveform ref={waveform} className={className} />;
}
