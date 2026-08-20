/**
 * SOURCE OF TRUTH KEYWORDS: Waveform, WaveformProps, WaveformHandle, push,
 *   scaleY, levelsRef, applyLevels, waveform-bars
 * WHAT:  N bars driven by an RMS array, newest sample at the right edge, with
 *        an imperative handle (`push`) for the realtime path.
 * WHY:   The bars are written with transform: scaleY straight onto the DOM and
 *        never through React state. Two reasons, both hard: scaleY is a
 *        compositor-only property, so a level change costs no layout and no
 *        paint, and pushing through a ref means the pill does not re-render 60
 *        times a second while whisper is decoding on another thread. Bars are
 *        clamped to a minimum scale so the waveform never looks dead during a
 *        pause — a flat line reads as "it stopped listening" (docs/04 §7).
 * WHERE: The pill's recording state, fed by RMS from the audio thread. The
 *        controlled `values` prop exists for anywhere replaying a stored
 *        envelope. Bar count and sizes come from tokens.css.
 */

import { useCallback, useImperativeHandle, useLayoutEffect, useMemo, useRef, useState, type Ref } from "react";
import { cn } from "@/lib/utils";
import { readNumberToken, readPxToken } from "@/lib/motion";

export interface WaveformHandle {
  /** Append one RMS level (0..1). Shifts the window left by one bar. */
  push(level: number): void;
  /** Return every bar to the floor. */
  reset(): void;
}

export interface WaveformProps {
  /** Controlled levels, oldest first. The last `bars` entries are shown. */
  values?: readonly number[];
  /** Override the token bar count — for a wider or narrower host. */
  bars?: number;
  className?: string;
  ref?: Ref<WaveformHandle>;
}

const clamp01 = (value: number): number => (value < 0 ? 0 : value > 1 ? 1 : value);

export function Waveform({ values, bars, className, ref }: WaveformProps) {
  const [tokenBars] = useState(() => readNumberToken("--waveform-bars"));
  const [minScale] = useState(() => readPxToken("--waveform-bar-min") / readPxToken("--waveform-bar-max"));

  const barCount = bars ?? tokenBars;
  const indices = useMemo(() => Array.from({ length: barCount }, (_, index) => index), [barCount]);

  const barRefs = useRef<(HTMLSpanElement | null)[]>([]);
  const levelsRef = useRef<Float32Array>(new Float32Array(barCount));

  const applyLevels = useCallback(() => {
    const levels = levelsRef.current;
    for (let index = 0; index < levels.length; index += 1) {
      const bar = barRefs.current[index];
      if (!bar) continue;
      bar.style.transform = `scaleY(${minScale + (1 - minScale) * levels[index]})`;
    }
  }, [minScale]);

  useLayoutEffect(() => {
    if (levelsRef.current.length !== barCount) levelsRef.current = new Float32Array(barCount);
    if (!values) return;
    const levels = levelsRef.current;
    const offset = values.length - barCount;
    for (let index = 0; index < barCount; index += 1) {
      const source = offset + index;
      levels[index] = source < 0 ? 0 : clamp01(values[source]);
    }
    applyLevels();
  }, [values, barCount, applyLevels]);

  useImperativeHandle(
    ref,
    () => ({
      push(level: number) {
        const levels = levelsRef.current;
        levels.copyWithin(0, 1);
        levels[levels.length - 1] = clamp01(level);
        applyLevels();
      },
      reset() {
        levelsRef.current.fill(0);
        applyLevels();
      },
    }),
    [applyLevels],
  );

  return (
    <div
      aria-hidden="true"
      className={cn("flex items-end gap-[var(--waveform-bar-gap)] h-[var(--waveform-bar-max)]", className)}
    >
      {indices.map((index) => (
        <span
          key={index}
          ref={(element) => {
            barRefs.current[index] = element;
          }}
          className="w-[var(--waveform-bar-width)] h-full origin-bottom rounded-full bg-current"
          style={{ transform: `scaleY(${minScale})` }}
        />
      ))}
    </div>
  );
}
