/**
 * SOURCE OF TRUTH KEYWORDS: SegmentedControl, SegmentedControlProps, SegmentOption,
 *   segment-height, segment-padding, radiogroup
 * WHAT:  A low-contrast segmented picker: two to four mutually exclusive
 *        options, all of them visible, one selected.
 * WHY:   A select hides every option but the chosen one behind a click. When
 *        there are three of them and they are the axis the user is reasoning
 *        along — a date range, a unit — hiding two is hiding the control's
 *        whole meaning. Contrast is deliberately low (docs/04 §11): the track
 *        is --surface-sunken, the selected segment is --surface-sunken-strong,
 *        and nothing here is accented, because choosing a range is not a live
 *        session and ember is spent only on one (§1.3).
 *
 *        Marked up as a real radiogroup with roving focus, so ←/→ move between
 *        segments and only the selected one is in the tab order — which is what
 *        a native NSSegmentedControl does and what a row of <button>s does not.
 * WHERE: The activity chart's range picker. Any future two-to-four-way choice
 *        that is not a boolean.
 */

import { useId, type KeyboardEvent } from "react";
import { cn } from "@/lib/utils";

export interface SegmentOption<T extends string> {
  value: T;
  label: string;
}

export interface SegmentedControlProps<T extends string> {
  /** Names the group for assistive tech: "Range", "Units". */
  label: string;
  options: readonly SegmentOption<T>[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
}

export function SegmentedControl<T extends string>({
  label,
  options,
  value,
  onChange,
  className,
}: SegmentedControlProps<T>) {
  const groupId = useId();

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const delta = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (delta === 0) return;
    event.preventDefault();
    const index = options.findIndex((option) => option.value === value);
    // Wraps, because a segmented control is a ring of equals with no first or last.
    const next = options[(index + delta + options.length) % options.length];
    if (next) onChange(next.value);
  }

  return (
    <div
      role="radiogroup"
      aria-label={label}
      onKeyDown={onKeyDown}
      className={cn(
        "hairline flex h-[var(--segment-height)] shrink-0 items-center gap-0 rounded-[var(--segment-radius)] bg-sunken p-[var(--segment-padding)]",
        className,
      )}
    >
      {options.map((option) => {
        const isSelected = option.value === value;
        return (
          <button
            key={option.value}
            id={`${groupId}-${option.value}`}
            type="button"
            role="radio"
            aria-checked={isSelected}
            tabIndex={isSelected ? 0 : -1}
            onClick={() => onChange(option.value)}
            className={cn(
              "flex h-full min-w-10 items-center justify-center rounded-[calc(var(--segment-radius)-var(--segment-padding))] px-3 text-label tabular-nums transition-colors",
              isSelected
                ? "bg-sunken-strong text-text-primary"
                : "text-text-secondary hover:text-text-primary",
            )}
          >
            {option.label}
          </button>
        );
      })}
    </div>
  );
}
