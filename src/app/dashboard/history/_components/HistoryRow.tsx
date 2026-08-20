/**
 * SOURCE OF TRUTH KEYWORDS: HistoryRow, SessionSummary, OutcomeBadge,
 *   firstLine, DeliveryKind, SessionOutcome
 * WHAT:  One history row: the first line of the transcript, then duration ·
 *        language · word count · relative time, with a badge when the session
 *        did not deliver cleanly.
 * WHY:   Failed and orphaned sessions are shown, not hidden — recovering the
 *        one that went wrong is the entire reason history exists (docs/01 §7).
 *        Cancelled sessions cannot appear because the backend destroys them, so
 *        there is no filter for them here. The row falls back to `raw_text` when
 *        `final_text` is absent: a failed session still has words worth reading,
 *        and showing nothing would imply the audio was lost. A failed row leads
 *        its metadata with the REASON and still carries the duration, so the
 *        user learns both what went wrong and how much audio it happened to —
 *        docs/04 §9. The reason goes on the metadata line rather than a third
 *        line because the row height is fixed by the virtualized list, and a
 *        third line would overflow it. The reason is `error_message`, rendered
 *        VERBATIM — it is written once where the failure happened, which knows
 *        more about itself than any later reader does. This file deliberately
 *        holds no code-to-sentence map: that would be a second set of wording
 *        for failures the app already has words for, and it would drift.
 * WHERE: The renderRow slot of the history DataList.
 */

import { cn } from "@/lib/utils";
import { formatClock, formatCount, formatLanguage, formatRelativeTime } from "@/lib/format";
import type { SessionOutcome, SessionSummary } from "@/lib/bindings";

const OUTCOME_TONE: Readonly<Record<SessionOutcome, string>> = {
  DELIVERED: "",
  FAILED: "text-danger",
  ORPHANED: "text-warning",
};

const OUTCOME_LABEL: Readonly<Record<SessionOutcome, string>> = {
  DELIVERED: "",
  FAILED: "Failed",
  ORPHANED: "Recovered",
};

function firstLine(session: SessionSummary): string | null {
  const text = session.final_text ?? session.raw_text;
  if (text === null) return null;
  const line = text.trim().split("\n", 1)[0];
  return line.length > 0 ? line : null;
}

export function HistoryRow({ session }: { session: SessionSummary }) {
  const line = firstLine(session);

  // The persisted message when there is one. The fallback is only for FAILED:
  // an ORPHANED session was RECOVERED — recovery finished transcribing it and
  // it may well have text — so telling the user it did not finish would
  // contradict the transcript sitting on the line above.
  const reason = session.error_message ?? (session.outcome === "FAILED" ? "The recording did not finish" : null);

  const meta = [
    session.duration_ms !== null ? formatClock(session.duration_ms) : null,
    session.language !== null ? formatLanguage(session.language) : null,
    session.word_count !== null ? `${formatCount(session.word_count)} words` : null,
    formatRelativeTime(session.started_at_ms),
    session.delivery === "CLIPBOARD_ONLY" ? "Clipboard only" : null,
  ].filter((entry): entry is string => entry !== null);

  return (
    <div className="flex min-w-0 flex-col gap-0.5">
      <div className="flex min-w-0 items-center gap-2">
        <span className={cn("truncate text-body", line ? "text-text-primary" : "text-text-tertiary")}>
          {line ?? "No transcript"}
        </span>
        {session.outcome !== "DELIVERED" ? (
          <span className={cn("shrink-0 text-caption", OUTCOME_TONE[session.outcome])}>
            {OUTCOME_LABEL[session.outcome]}
          </span>
        ) : null}
      </div>

      {/* The reason truncates; the metadata does not. Duration leads the
          metadata and is what docs/04 §9 calls "what was lost" — a long failure
          message must never be the thing that pushes it off the row. */}
      <span className="flex min-w-0 items-baseline gap-1 text-caption text-text-tertiary">
        {reason ? <span className={cn("truncate", OUTCOME_TONE[session.outcome])}>{reason}</span> : null}
        {meta.length > 0 ? (
          <span className="shrink-0">
            {reason ? "· " : null}
            {meta.join(" · ")}
          </span>
        ) : null}
      </span>
    </div>
  );
}
