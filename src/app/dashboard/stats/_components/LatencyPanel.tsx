/**
 * SOURCE OF TRUTH KEYWORDS: LatencyPanel, LatencySummary, MetricDef, user_facing,
 *   p50, p95
 * WHAT:  The p50/p95 table — the honest engineering metric, per stage.
 * WHY:   Which stages appear and what they are called both come from the
 *        registry's MetricDefs, filtered on `user_facing`. That flag exists so a
 *        diagnostic stage can be recorded without being shown, and re-deciding
 *        that here would put a second opinion next to the declaration. A stage
 *        with no samples renders an em dash rather than a zero: zero latency is
 *        a claim, and "not measured yet" is the truth.
 *        Drawn as a quiet label/value grid, not as a table with rules and a
 *        bold header: the column names are caption-size --text-tertiary and the
 *        only hairline is between rows. A header that competes with its own
 *        numbers is chart chrome wearing a different hat (docs/04 §11), and the
 *        numbers are the reason the panel exists. Row height is
 *        --chart-row-height so these rows and the language bars above them
 *        share one rhythm rather than each picking their own padding.
 * WHERE: The stats view. Joins StatsSummary.latency to registry MetricDefs.
 */

import { formatLatency } from "@/lib/format";
import { EmptyState } from "@/components/global";
import type { LatencySummary, MetricDef } from "@/lib/bindings";

export interface LatencyPanelProps {
  metrics: readonly MetricDef[];
  latency: readonly LatencySummary[];
}

export function LatencyPanel({ metrics, latency }: LatencyPanelProps) {
  const byStage = new Map(latency.map((entry) => [entry.stage, entry]));
  const rows = metrics.filter((metric) => metric.user_facing);
  if (rows.length === 0) return null;

  const measured = rows.some((metric) => (byStage.get(metric.stage)?.sample_count ?? 0) > 0);
  if (!measured) {
    return (
      <EmptyState
        size="compact"
        headline="No timings yet"
        description="Latency is measured from your own transcriptions — it appears here after the first one."
      />
    );
  }

  return (
    <table className="w-full">
      <thead>
        <tr>
          <th scope="col" className="pb-1 text-left text-caption font-normal text-text-tertiary">
            Stage
          </th>
          <th scope="col" className="pb-1 text-right text-caption font-normal text-text-tertiary">
            p50
          </th>
          <th scope="col" className="pb-1 text-right text-caption font-normal text-text-tertiary">
            p95
          </th>
          <th scope="col" className="pb-1 text-right text-caption font-normal text-text-tertiary">
            Samples
          </th>
        </tr>
      </thead>
      <tbody>
        {rows.map((metric) => {
          const entry = byStage.get(metric.stage);
          return (
            <tr key={metric.stage} className="h-[var(--chart-row-height)] hairline-t">
              <th scope="row" className="text-left text-body font-normal text-text-secondary">
                {metric.label}
              </th>
              <td className="text-right text-body tabular-nums text-text-primary">
                {formatLatency(entry?.p50_ms ?? null)}
              </td>
              <td className="text-right text-body tabular-nums text-text-primary">
                {formatLatency(entry?.p95_ms ?? null)}
              </td>
              <td className="text-right text-caption tabular-nums text-text-tertiary">{entry?.sample_count ?? 0}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}
