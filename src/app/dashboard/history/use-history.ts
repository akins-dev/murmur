/**
 * SOURCE OF TRUTH KEYWORDS: useHistory, HistoryFeed, listHistory, searchHistory,
 *   PAGE_SIZE, loadMore, exhausted
 * WHAT:  The history feed: a paginated list_history, swapped for search_history
 *        while a query is present, with loadMore/refresh and the rows it has so
 *        far.
 * WHY:   History reaches tens of thousands of rows, so it is never fetched
 *        whole — pages accumulate as the virtualized list reaches its end.
 *        Search is a different command rather than a filter over the loaded
 *        pages, because filtering client-side would only ever search the
 *        handful of pages that happen to be in memory and would quietly report
 *        "no results" for a session that exists. A generation counter guards
 *        the swap: a page requested before the query changed must not be
 *        appended after it.
 * WHERE: Owned by HistoryView.tsx. Wraps list_history / search_history.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { commands, type AppError, type SessionSummary } from "@/lib/bindings";
import { unwrapCommand } from "@/lib/ipc";

/** Big enough that a page lasts several screens, small enough to stay instant. */
const PAGE_SIZE = 200;

export interface HistoryFeed {
  items: readonly SessionSummary[];
  error: AppError | null;
  loading: boolean;
  /** True once a page came back short — there is nothing further to ask for. */
  exhausted: boolean;
  /** True once the first response has landed, success or failure. Until then
   *  "no rows" means "not asked yet", which must not be drawn as "none". */
  loaded: boolean;
  loadMore: () => void;
  refresh: () => void;
  /** Drops one row locally after a confirmed delete, without refetching. */
  forget: (id: string) => void;
}

export function useHistory(query: string): HistoryFeed {
  const [items, setItems] = useState<readonly SessionSummary[]>([]);
  const [error, setError] = useState<AppError | null>(null);
  const [loading, setLoading] = useState(true);
  const [exhausted, setExhausted] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const [generation, setGeneration] = useState(0);
  const offset = useRef(0);
  const inFlight = useRef(false);

  const searching = query.trim().length > 0;

  const fetchPage = useCallback(
    async (from: number, forGeneration: number) => {
      if (inFlight.current) return;
      inFlight.current = true;
      setLoading(true);

      const result = searching
        ? await unwrapCommand(() => commands.searchHistory({ query, limit: PAGE_SIZE }))
        : await unwrapCommand(() => commands.listHistory({ limit: PAGE_SIZE, offset: from }));

      inFlight.current = false;
      // The query changed while this was in flight — its rows are stale.
      if (forGeneration !== generation) return;

      if (result.status === "error") {
        setError(result.error);
        setLoading(false);
        setLoaded(true);
        return;
      }

      setError(null);
      setLoading(false);
      setLoaded(true);
      setExhausted(searching || result.data.length < PAGE_SIZE);
      offset.current = from + result.data.length;
      setItems((previous) => (from === 0 ? result.data : [...previous, ...result.data]));
    },
    [generation, query, searching],
  );

  useEffect(() => {
    offset.current = 0;
    setExhausted(false);
    setLoaded(false);
    void fetchPage(0, generation);
    // fetchPage is recreated whenever its inputs change, which is exactly when
    // the feed must restart.
  }, [fetchPage, generation]);

  const loadMore = useCallback(() => {
    if (exhausted || loading || searching) return;
    void fetchPage(offset.current, generation);
  }, [exhausted, fetchPage, generation, loading, searching]);

  const refresh = useCallback(() => setGeneration((value) => value + 1), []);

  const forget = useCallback((id: string) => {
    setItems((previous) => previous.filter((item) => item.id !== id));
  }, []);

  return { items, error, loading, exhausted, loaded, loadMore, refresh, forget };
}
