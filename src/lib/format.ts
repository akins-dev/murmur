/**
 * SOURCE OF TRUTH KEYWORDS: formatDuration, formatClock, formatRelativeTime,
 *   formatBytes, formatRate, formatCompactDuration, formatLatency, formatLanguage
 * WHAT:  Every number-to-string the UI needs: clocks, durations, relative
 *        times, byte sizes, transfer rates and latencies.
 * WHY:   One place, because the same duration appearing as "1:23" in the pill
 *        and "1m 23s" in history is the kind of drift nobody notices until it
 *        looks sloppy everywhere. Every timestamp crossing IPC is epoch
 *        MILLISECONDS and says so in its field name (started_at_ms), so nothing
 *        here guesses at a unit — an earlier version sniffed it by magnitude,
 *        and that heuristic is exactly the kind of thing that works until the
 *        day it silently does not.
 * WHERE: Used by the history rows, the stats view, the model manager and the
 *        pill timer. Re-exported from src/lib/index.ts.
 */

/** M:SS, or H:MM:SS past an hour. The pill timer and history durations. */
export function formatClock(ms: number): string {
  const total = Math.max(0, Math.floor(ms / 1000));
  const seconds = total % 60;
  const minutes = Math.floor(total / 60) % 60;
  const hours = Math.floor(total / 3600);
  const mm = hours > 0 ? String(minutes).padStart(2, "0") : String(minutes);
  return `${hours > 0 ? `${hours}:` : ""}${mm}:${String(seconds).padStart(2, "0")}`;
}

/** "4h 12m", "12m", "48s" — for totals a person reads rather than tracks. */
export function formatCompactDuration(ms: number): string {
  const totalMinutes = Math.round(ms / 60000);
  if (totalMinutes < 1) return `${Math.max(0, Math.round(ms / 1000))}s`;
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours === 0) return `${minutes}m`;
  return minutes === 0 ? `${hours}h` : `${hours}h ${minutes}m`;
}

const RELATIVE_STEPS: readonly [limit: number, divisor: number, unit: Intl.RelativeTimeFormatUnit][] = [
  [60_000, 1_000, "second"],
  [3_600_000, 60_000, "minute"],
  [86_400_000, 3_600_000, "hour"],
  [604_800_000, 86_400_000, "day"],
  [2_629_800_000, 604_800_000, "week"],
  [31_557_600_000, 2_629_800_000, "month"],
];

const relative = new Intl.RelativeTimeFormat(undefined, { numeric: "auto", style: "narrow" });

/** "2m ago", "3d ago". Intl does the localisation and the pluralisation. */
export function formatRelativeTime(epochMs: number, now: number = Date.now()): string {
  const deltaMs = epochMs - now;
  const magnitude = Math.abs(deltaMs);
  for (const [limit, divisor, unit] of RELATIVE_STEPS) {
    if (magnitude < limit) return relative.format(Math.round(deltaMs / divisor), unit);
  }
  return relative.format(Math.round(deltaMs / 31_557_600_000), "year");
}

const BYTE_UNITS = ["B", "KB", "MB", "GB"] as const;

/** Real numbers on a 574MB download — never a spinner (docs/04 §9). */
export function formatBytes(bytes: number): string {
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value >= 100 || unit === 0 ? 0 : 1)} ${BYTE_UNITS[unit]}`;
}

export function formatRate(bytesPerSecond: number): string {
  return `${formatBytes(bytesPerSecond)}/s`;
}

/** Seconds remaining at the current rate, or null when it cannot be known. */
export function formatEta(remainingBytes: number, bytesPerSecond: number): string | null {
  if (bytesPerSecond <= 0) return null;
  return formatCompactDuration((remainingBytes / bytesPerSecond) * 1000);
}

/** Latency is the number the product promises, so it is never rounded up into
 *  a friendlier unit — 284 ms stays 284 ms. */
export function formatLatency(ms: number | null): string {
  return ms === null ? "—" : `${Math.round(ms)} ms`;
}

const compact = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
const plain = new Intl.NumberFormat();

export function formatCount(value: number): string {
  return value >= 10_000 ? compact.format(value) : plain.format(value);
}

/** Language codes as names, falling back to the code when Intl has no name. */
export function formatLanguage(code: string): string {
  try {
    return new Intl.DisplayNames(undefined, { type: "language" }).of(code) ?? code;
  } catch {
    return code;
  }
}

const chartDate = new Intl.DateTimeFormat(undefined, { month: "short", day: "numeric" });

/**
 * WHAT:  A `YYYY-MM-DD` activity date as the short label under a chart's end.
 * WHY:   Parsed with an explicit `T00:00:00` so it is read as local midnight.
 *        A bare `new Date("2026-08-20")` is parsed as UTC, which in every
 *        timezone west of Greenwich renders the previous day — the two labels
 *        on this chart are the only dates the user can check it against, so
 *        being a day out is the whole label being wrong. No year: the range is
 *        at most a few months and the year is noise the axis does not have room
 *        for (docs/04 §11).
 */
export function formatChartDate(date: string): string {
  return chartDate.format(new Date(`${date}T00:00:00`));
}
