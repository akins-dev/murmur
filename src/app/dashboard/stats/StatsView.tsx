/**
 * SOURCE OF TRUTH KEYWORDS: StatsView, StatsSummary, getStats, hero-stat,
 *   LanguageBreakdown, ActivityChart, LatencyPanel, elevated-card
 * WHAT:  The stats view: one elevated card holding the hero number and the
 *        activity line, then the secondary grid, the language breakdown and the
 *        latency rows sitting directly on the panel below it.
 * WHY:   One get_stats call fills the whole page — six commands would be six
 *        chances to render half a page, which is why the Rust side returns one
 *        shape. Nothing here computes a statistic: every number is served,
 *        because a frontend that derives "time saved" is a second opinion about
 *        the product's central claim.
 *
 *        The card is the ONE elevated surface this view is allowed (docs/04
 *        §1.2), and it holds the hero number and the chart because those are
 *        the thing the view exists to show. Everything under it is grouped by
 *        spacing and a hairline instead of by a surface — repeat the card and
 *        none of them is elevated any more, they are six boxes on glass, which
 *        is the widget look §1.2 is about.
 * WHERE: Rendered by Dashboard.tsx for the registry's "stats" route.
 */

import { commands, type HotkeyBinding, type MetricDef } from "@/lib/bindings";
import { useCommand } from "@/lib/ipc";
import { formatCompactDuration, formatCount } from "@/lib/format";
import { GlassPanel, ScrollArea, StatCard } from "@/components/global";
import { NoTranscriptionsYet } from "../_components/NoTranscriptionsYet";
import { ViewState } from "../_components/ViewState";
import { ActivityChart } from "./_components/ActivityChart";
import { LatencyPanel } from "./_components/LatencyPanel";
import { LanguageBreakdown } from "./_components/LanguageBreakdown";

export interface StatsViewProps {
  /** Registry metric declarations — they decide what the latency panel shows. */
  metrics: readonly MetricDef[];
  /** For the zero state, which is the same one History shows. */
  hotkey: HotkeyBinding | null;
}

export function StatsView({ metrics, hotkey }: StatsViewProps) {
  const stats = useCommand(commands.getStats, []);

  return (
    <ScrollArea contentClassName="flex flex-col gap-5 px-[var(--page-padding-x)] pb-8">
        <ViewState state={stats} onRetry={stats.reload}>
          {(data) =>
            data.total_sessions === 0 ? (
              <NoTranscriptionsYet hotkey={hotkey} />
            ) : (
              <>
                <GlassPanel material="elevated" radius="card" className="flex flex-col gap-4 p-5">
                  <StatCard
                    size="hero"
                    label="Time saved"
                    value={formatCompactDuration(data.time_saved_ms)}
                    delta={
                      data.speaking_wpm !== null && data.baseline_typing_wpm !== null
                        ? {
                            label: `${Math.round(data.speaking_wpm)} wpm spoken vs ${Math.round(
                              data.baseline_typing_wpm,
                            )} typed`,
                          }
                        : undefined
                    }
                  />
                  {data.activity.length > 1 ? <ActivityChart days={data.activity} /> : null}
                </GlassPanel>

                <div className="grid grid-cols-3 gap-x-6 gap-y-5">
                  <StatCard label="Transcriptions" value={formatCount(data.total_sessions)} />
                  <StatCard label="Words" value={formatCount(data.total_words)} />
                  <StatCard label="Time spoken" value={formatCompactDuration(data.total_speaking_ms)} />
                  <StatCard label="This week" value={formatCount(data.sessions_this_week)} unit="sessions" />
                  <StatCard label="Words this week" value={formatCount(data.words_this_week)} />
                  <StatCard label="Streak" value={formatCount(data.current_streak_days)} unit="days" />
                </div>

                {data.languages.length > 0 ? (
                  <section className="flex flex-col gap-2 hairline-t pt-5">
                    <h2 className="text-label text-text-secondary">Languages</h2>
                    <LanguageBreakdown languages={data.languages} />
                  </section>
                ) : null}

                <section className="flex flex-col gap-2 hairline-t pt-5">
                  <h2 className="text-label text-text-secondary">Latency</h2>
                  <LatencyPanel metrics={metrics} latency={data.latency} />
                </section>
              </>
            )
          }
        </ViewState>
    </ScrollArea>
  );
}
