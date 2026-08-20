/**
 * SOURCE OF TRUTH KEYWORDS: LanguageBreakdown, LanguageCount, chart-ink,
 *   no-chart-chrome, tabular-nums
 * WHAT:  The language breakdown: a label, a bar, and its count, one row each.
 * WHY:   No chart library. Once the doctrine in docs/04 §11 is applied — no
 *        gridlines, no axes, no frame, no ticks, no fills, no legend, no load
 *        animation — what is left of a horizontal bar chart is a div with a
 *        percentage width, and reaching for 109KB of charting to draw that is
 *        how a product ends up looking like every other product: the default
 *        styling of a popular library IS the generic look, because everyone
 *        else ships it too.
 *
 *        The value sits at the end of its own row rather than inside a hover
 *        tooltip. A tooltip is chrome, and it hides the number until you go
 *        looking for it — in a panel whose whole job is showing numbers.
 * WHERE: The stats view. Consumes StatsSummary.languages. Ink and geometry are
 *        tokens; nothing here is coloured, because nothing here is state.
 */

import { formatCount, formatLanguage } from "@/lib/format";
import type { LanguageCount } from "@/lib/bindings";

/** Enough rows to be informative; the tail is noise in a language breakdown. */
const MAX_ROWS = 6;

export function LanguageBreakdown({ languages }: { languages: readonly LanguageCount[] }) {
  const rows = [...languages].sort((a, b) => b.session_count - a.session_count).slice(0, MAX_ROWS);
  if (rows.length === 0) return null;

  const busiest = Math.max(...rows.map((row) => row.session_count), 1);

  return (
    <ul className="flex flex-col">
      {rows.map((row) => (
        <li
          key={row.language}
          className="flex h-[var(--chart-row-height)] items-center gap-3"
        >
          <span className="w-[var(--chart-label-gutter)] shrink-0 truncate text-label text-text-secondary">
            {formatLanguage(row.language)}
          </span>
          <span className="flex h-1 min-w-0 flex-1 items-center">
            <span
              className="h-full rounded-pill bg-text-primary opacity-[var(--chart-ink-opacity)]"
              style={{ width: `${(row.session_count / busiest) * 100}%` }}
            />
          </span>
          <span className="w-12 shrink-0 text-right text-label tabular-nums text-text-tertiary">
            {formatCount(row.session_count)}
          </span>
        </li>
      ))}
    </ul>
  );
}
