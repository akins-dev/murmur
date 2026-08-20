/**
 * SOURCE OF TRUTH KEYWORDS: ActivityChart, ActivityRange, buildSeries, chart-ink,
 *   chart-height, chart-point-size, no-chart-chrome, useChartWidth
 * WHAT:  Sessions per day, drawn as one unadorned line with a single dot on the
 *        most recent value and a quiet date label at each end.
 * WHY:   This replaced a GitHub-style tinted heatmap, and both halves of that
 *        matter. The lattice was a borrowed form — it said GitHub before it said
 *        anything about this product — and tinting it needed a hue, so an idle
 *        stats screen was glowing ember while §1.3 says the app has no colour at
 *        rest. A line says the one thing the hero number cannot: the shape of
 *        the habit.
 *
 *        Everything docs/04 §11 forbids is absent: no gridlines, no axes, no
 *        ticks, no frame, no fill, no legend, no tooltip, no load animation, and
 *        no chart library — once those are gone a line chart is one <path>, and
 *        109KB of recharts would buy only the default styling every other
 *        product also ships.
 *
 *        Geometry is measured in real pixels rather than drawn into a stretched
 *        viewBox. `preserveAspectRatio="none"` is the cheap way to make an SVG
 *        fill a box and it scales x and y by different factors, which turns a
 *        1.5px stroke into a wedge and the end dot into an ellipse — the two
 *        marks this chart consists of.
 *
 *        Counts are plotted raw. A trailing mean would draw a prettier curve and
 *        would be a number this app did not measure, and the spiky line is on
 *        brand anyway: it is the same waveform the mark and the pill draw.
 * WHERE: The stats view, inside the one elevated card (§1.2). Consumes
 *        StatsSummary.activity. Range picker is the shared SegmentedControl.
 */

import { useEffect, useMemo, useRef, useState, type RefObject } from "react";
import { SegmentedControl, type SegmentOption } from "@/components/global";
import { formatChartDate } from "@/lib/format";
import type { ActivityDay } from "@/lib/bindings";

export type ActivityRange = "30" | "90" | "all";

/** Data windows, not design values — days of history, so they live here. */
const RANGE_DAYS: Readonly<Record<ActivityRange, number | null>> = {
  "30": 30,
  "90": 90,
  all: null,
};

const RANGE_OPTIONS: readonly SegmentOption<ActivityRange>[] = [
  { value: "30", label: "30d" },
  { value: "90", label: "90d" },
  { value: "all", label: "All" },
];

/**
 * WHAT:  The drawing box in CSS pixels, or null until it has been measured.
 * WHY:   Null rather than a guessed default. A chart drawn at a made-up width
 *        and corrected one frame later is a visible jump on every mount, and a
 *        chart is the one thing on this screen that must not animate into place.
 */
function useChartBox(): [RefObject<HTMLDivElement | null>, { width: number; height: number } | null] {
  const ref = useRef<HTMLDivElement | null>(null);
  const [box, setBox] = useState<{ width: number; height: number } | null>(null);

  useEffect(() => {
    const element = ref.current;
    if (!element) return;
    const observer = new ResizeObserver((entries) => {
      const rect = entries[0]?.contentRect;
      setBox(rect && rect.width > 0 && rect.height > 0 ? { width: rect.width, height: rect.height } : null);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return [ref, box];
}

interface Series {
  points: readonly { x: number; y: number }[];
  firstLabel: string;
  lastLabel: string;
}

/**
 * WHAT:  The visible days mapped into pixel coordinates.
 * WHY:   The vertical scale is 0-to-busiest and is never stated. A "peak: 14"
 *        caption on a chart with no axis is the chrome coming back in through
 *        the label (§11) — the number the user came for is the hero above it.
 *        A flat series (every day equal, including all-zero) is pinned to the
 *        middle rather than divided by a zero range, so it reads as a steady
 *        habit rather than as a line stuck to the floor.
 */
function buildSeries(days: readonly ActivityDay[], width: number, height: number): Series | null {
  if (days.length < 2) return null;

  const counts = days.map((day) => day.session_count);
  const busiest = Math.max(...counts);
  const step = width / (days.length - 1);

  const points = days.map((day, index) => ({
    x: index * step,
    y: busiest === 0 ? height / 2 : height * (1 - day.session_count / busiest),
  }));

  return {
    points,
    firstLabel: formatChartDate(days[0].date),
    lastLabel: formatChartDate(days[days.length - 1].date),
  };
}

function toPath(points: readonly { x: number; y: number }[]): string {
  return points.map((point, index) => `${index === 0 ? "M" : "L"}${point.x} ${point.y}`).join(" ");
}

export function ActivityChart({ days }: { days: readonly ActivityDay[] }) {
  const [range, setRange] = useState<ActivityRange>("30");
  const [ref, box] = useChartBox();

  const visible = useMemo(() => {
    const window = RANGE_DAYS[range];
    return window === null ? days : days.slice(-window);
  }, [days, range]);

  const series = useMemo(
    () => (box === null ? null : buildSeries(visible, box.width, box.height)),
    [visible, box],
  );

  const last = series?.points[series.points.length - 1];

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-4">
        <h2 className="text-label text-text-secondary">Activity</h2>
        <SegmentedControl label="Range" options={RANGE_OPTIONS} value={range} onChange={setRange} />
      </div>

      {/* The inset is padding on the outer box, so the measured inner box IS the
          drawing area: a point at 0 or at the top has room for its own radius
          and its stroke, and buildSeries never has to know about either. */}
      <div className="h-[var(--chart-height)] w-full py-[var(--chart-inset)]">
        <div ref={ref} className="h-full w-full">
          {series ? (
            <svg
              role="img"
              aria-label={`Sessions per day, ${series.firstLabel} to ${series.lastLabel}`}
              className="block h-full w-full overflow-visible text-text-primary opacity-[var(--chart-ink-opacity)]"
            >
              {/* Stroke width and radius are set in CSS, not as attributes: an SVG
                  presentation attribute cannot read a custom property, and `r` as
                  an SVG2 geometry property can — the same trick the countdown ring
                  uses. No viewBox, so one user unit is one CSS pixel. */}
              <path
                d={toPath(series.points)}
                fill="none"
                stroke="currentColor"
                strokeLinecap="round"
                strokeLinejoin="round"
                className="[stroke-width:var(--chart-line-width)]"
              />
              {last ? (
                <circle cx={last.x} cy={last.y} fill="currentColor" className="[r:var(--chart-point-size)]" />
              ) : null}
            </svg>
          ) : null}
        </div>
      </div>

      <div className="flex items-center justify-between text-caption tabular-nums text-text-tertiary">
        <span>{series?.firstLabel ?? ""}</span>
        <span>{series?.lastLabel ?? ""}</span>
      </div>
    </div>
  );
}
