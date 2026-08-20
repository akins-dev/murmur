/**
 * SOURCE OF TRUTH KEYWORDS: HistoryView, useHistory, DataList, copySession,
 *   deleteHistoryEntry, copyText, search-debounce, HistoryRow
 * WHAT:  The history view: a virtualized, searchable list of past sessions,
 *        ⏎ to copy, delete per row.
 * WHY:   The list is the global DataList doing the generic half — window,
 *        ⌘F, arrow keys, ⏎ — while this file supplies only the row, the actions
 *        and the empty state. Search is handed to the SERVER (no `matches`
 *        prop): searching the pages that happen to be loaded would report "no
 *        results" for a session that exists, which is worse than no search.
 *        The query is debounced by the token rather than fired per keystroke,
 *        because each keystroke is a SQL LIKE across every row. Copying goes
 *        through the copy_text command rather than navigator.clipboard: the web
 *        API needs transient user activation and a permissive CSP, so a copy
 *        triggered by ⏎ on a keyboard-navigated row is exactly the case it
 *        refuses. Rust owns the guarantee, on the same path the paste pipeline
 *        already uses.
 * WHERE: Rendered by Dashboard.tsx for the registry's "history" route.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { Check, Copy, Trash2 } from "lucide-react";
import { commands, type AppError, type SessionSummary } from "@/lib/bindings";
import { unwrapCommand } from "@/lib/ipc";
import { readDurationMs } from "@/lib/motion";
import { DataList, EmptyState, ErrorSurface, Skeleton } from "@/components/global";
import type { HotkeyBinding } from "@/lib/bindings";
import { NoTranscriptionsYet } from "../_components/NoTranscriptionsYet";
import { ExportAction } from "./_components/ExportAction";
import { HistoryRow } from "./_components/HistoryRow";
import { useHistory } from "./use-history";

export interface HistoryViewProps {
  /** The dictation hotkey, for the empty state. Null while it is unknown. */
  hotkey: HotkeyBinding | null;
}

function textOf(session: SessionSummary): string {
  return session.final_text ?? session.raw_text ?? "";
}

export function HistoryView({ hotkey }: HistoryViewProps) {
  const [input, setInput] = useState("");
  const [query, setQuery] = useState("");
  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [exportError, setExportError] = useState<AppError | null>(null);
  const feed = useHistory(query);
  const copyTimer = useRef(0);

  useEffect(() => {
    const handle = window.setTimeout(() => setQuery(input), readDurationMs("--search-debounce"));
    return () => window.clearTimeout(handle);
  }, [input]);

  useEffect(() => () => window.clearTimeout(copyTimer.current), []);

  const copy = useCallback((session: SessionSummary) => {
    const text = textOf(session);
    if (text.length === 0) return;
    void unwrapCommand(() => commands.copyText({ text })).then((result) => {
      if (result.status !== "ok") return;
      setCopiedId(session.id);
      window.clearTimeout(copyTimer.current);
      copyTimer.current = window.setTimeout(() => setCopiedId(null), readDurationMs("--feedback-hold"));
    });
  }, []);

  const remove = useCallback(
    (session: SessionSummary) => {
      void unwrapCommand(() => commands.deleteHistoryEntry({ id: session.id })).then((result) => {
        if (result.status === "ok") feed.forget(session.id);
      });
    },
    [feed],
  );

  if (feed.error && feed.items.length === 0) {
    return (
      <section className="flex h-full flex-col pt-[var(--page-header-height)]">
        <ErrorSurface error={feed.error} onRetry={feed.refresh} />
      </section>
    );
  }

  return (
    /* Padded down past the overlaying page header rather than tucking under
       it: this page leads with DataList's toolbar, and a search field sliding
       beneath the title would be unusable rather than merely faded. The list
       below still fades at its own top edge. */
    <section className="flex h-full min-h-0 flex-col pt-[var(--page-header-height)]">
      {exportError ? <ErrorSurface size="compact" error={exportError} /> : null}
      <DataList
        label="Transcription history"
        items={feed.items}
        getKey={(session) => session.id}
        onActivate={copy}
        onReachEnd={feed.loadMore}
        renderRow={({ item }) => <HistoryRow session={item} />}
        renderRowActions={({ item }) => (
          <span className="flex items-center gap-1">
            <button
              type="button"
              aria-label="Copy transcript"
              onClick={() => copy(item)}
              className="rounded-input p-1 text-text-secondary transition-colors hover:bg-sunken hover:text-text-primary"
            >
              {copiedId === item.id ? (
                <Check className="size-4 text-success" />
              ) : (
                <Copy className="size-4" />
              )}
            </button>
            <button
              type="button"
              aria-label="Delete transcript"
              onClick={() => remove(item)}
              className="rounded-input p-1 text-text-secondary transition-colors hover:bg-sunken hover:text-danger"
            >
              <Trash2 className="size-4" />
            </button>
          </span>
        )}
        search={{ query: input, onQueryChange: setInput, placeholder: "Search transcripts" }}
        toolbar={<ExportAction onError={setExportError} />}
        empty={
          !feed.loaded ? (
            <Skeleton rows={6} className="h-[var(--row-height)] rounded-none" />
          ) : (
            <NoTranscriptionsYet hotkey={hotkey} />
          )
        }
        noResults={<EmptyState headline="Nothing matches that" description="Try a shorter search." />}
        footer={
          feed.loading && feed.items.length > 0 ? (
            <Skeleton className="h-[var(--row-height)] rounded-none" />
          ) : null
        }
      />
    </section>
  );
}
