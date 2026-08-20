/**
 * SOURCE OF TRUTH KEYWORDS: DataList, DataListProps, DataListSearch, DataListRowArgs,
 *   useVirtualWindow, activeIndex, renderRow, renderRowActions, listbox
 * WHAT:  A virtualized, searchable, keyboard-navigable list. It owns the
 *        window, the query, the active row and the roving focus; the row, the
 *        row actions, the toolbar and both empty states arrive as slots.
 * WHY:   The generic half of a list is the half everyone rewrites badly —
 *        virtualization, ⌘F, arrow keys, page keys, scroll-into-view, aria
 *        wiring — and the specific half is only ever "what does a row look
 *        like". Search and pagination are both optional halves: omit `matches`
 *        and the SERVER owns filtering while the list still owns the field and
 *        ⌘F; supply `onReachEnd` and a paginated source loads as it scrolls.
 *        So this component knows nothing about what it is listing:
 *        no field names, no copy, no data shape, one type parameter. History
 *        is its first consumer, not its definition. Keyboard focus stays on
 *        the container with aria-activedescendant rather than moving DOM focus
 *        per row, because focusing a row that virtualization is about to
 *        unmount throws focus back to <body> mid-scroll.
 * WHERE: The dashboard history view, and anything else that lists rows. Uses
 *        use-virtual-window.ts; row height and overscan come from tokens.css.
 */

import {
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { cn } from "@/lib/utils";
import { useScrollEdges } from "../scroll-area/use-scroll-edges";
import { readNumberToken, readPxToken } from "@/lib/motion";
import { useVirtualWindow } from "./use-virtual-window";

export interface DataListRowArgs<T> {
  item: T;
  index: number;
  isActive: boolean;
}

export interface DataListSearch<T> {
  /** Client-side filter. Keep it stable (useCallback) — it is a filter
   *  dependency over the whole collection. Omit it when the SERVER owns the
   *  search: the list then still owns the field, the query and ⌘F, and the
   *  caller re-fetches `items` on query change. */
  matches?: (item: T, query: string) => boolean;
  placeholder?: string;
  /** Controlled query. Omit both and the list owns its own. */
  query?: string;
  onQueryChange?: (query: string) => void;
  /** ⌘F focuses the field from anywhere in the window. Default true. */
  hotkey?: boolean;
}

export interface DataListProps<T> {
  items: readonly T[];
  getKey: (item: T, index: number) => string;
  renderRow: (args: DataListRowArgs<T>) => ReactNode;
  /** Right-aligned per-row slot, revealed on hover or when the row is active. */
  renderRowActions?: (args: DataListRowArgs<T>) => ReactNode;
  /** Shown when there is nothing at all. */
  empty: ReactNode;
  /** Shown when a query matched nothing. Falls back to `empty`. */
  noResults?: ReactNode;
  search?: DataListSearch<T>;
  /** Sits beside the search field — filters, sort, bulk actions. */
  toolbar?: ReactNode;
  /** ⏎ on the active row. */
  onActivate?: (item: T, index: number) => void;
  onActiveChange?: (item: T | null, index: number) => void;
  /** Fired when the rendered window comes within an overscan of the last row.
   *  Paginated sources load their next page here; it fires once per arrival at
   *  the end, not once per scroll event. */
  onReachEnd?: () => void;
  /** Rendered below the rows — a loading row, a "that's everything" line. */
  footer?: ReactNode;
  rowHeight?: number;
  overscan?: number;
  dividers?: boolean;
  /** Accessible name for the listbox — the caller's words, not ours. */
  label: string;
  className?: string;
}

export function DataList<T>({
  items,
  getKey,
  renderRow,
  renderRowActions,
  empty,
  noResults,
  search,
  toolbar,
  onActivate,
  onActiveChange,
  onReachEnd,
  footer,
  rowHeight,
  overscan,
  dividers = true,
  label,
  className,
}: DataListProps<T>) {
  const listId = useId();
  const scrollRef = useRef<HTMLDivElement>(null);
  // Same scroller the virtualiser measures, so the fade can never disagree
  // with what is actually rendered.
  const edges = useScrollEdges(scrollRef);
  const inputRef = useRef<HTMLInputElement>(null);

  const [tokenRowHeight] = useState(() => readPxToken("--row-height"));
  const [tokenOverscan] = useState(() => readNumberToken("--row-overscan"));
  const height = rowHeight ?? tokenRowHeight;
  const overscanRows = overscan ?? tokenOverscan;

  const [internalQuery, setInternalQuery] = useState("");
  const query = search?.query ?? internalQuery;
  const matches = search?.matches;

  const filtered = useMemo(() => {
    if (!matches || query.length === 0) return items;
    return items.filter((item) => matches(item, query));
  }, [items, query, matches]);

  const [activeIndex, setActiveIndex] = useState(0);
  const clampedActive = filtered.length === 0 ? -1 : Math.min(activeIndex, filtered.length - 1);

  const window_ = useVirtualWindow({
    count: filtered.length,
    rowHeight: height,
    overscan: overscanRows,
    containerRef: scrollRef,
  });

  const onReachEndRef = useRef(onReachEnd);
  onReachEndRef.current = onReachEnd;
  const reachedFor = useRef(-1);
  useEffect(() => {
    if (!onReachEndRef.current || filtered.length === 0) return;
    const atEnd = window_.end >= filtered.length - overscanRows;
    // Latched on the length that triggered it, so one arrival at the end asks
    // for one page rather than one per scroll frame.
    if (atEnd && reachedFor.current !== filtered.length) {
      reachedFor.current = filtered.length;
      onReachEndRef.current();
    }
  }, [window_.end, filtered.length, overscanRows]);

  const onActiveChangeRef = useRef(onActiveChange);
  onActiveChangeRef.current = onActiveChange;
  useEffect(() => {
    onActiveChangeRef.current?.(clampedActive < 0 ? null : filtered[clampedActive], clampedActive);
  }, [clampedActive, filtered]);

  /** Keeps the active row on screen by arithmetic — nothing is measured. */
  const revealRow = useCallback(
    (index: number) => {
      const container = scrollRef.current;
      if (!container) return;
      const top = index * height;
      if (top < container.scrollTop) container.scrollTop = top;
      else if (top + height > container.scrollTop + container.clientHeight) {
        container.scrollTop = top + height - container.clientHeight;
      }
    },
    [height],
  );

  const moveActive = useCallback(
    (next: number) => {
      if (filtered.length === 0) return;
      const bounded = Math.max(0, Math.min(next, filtered.length - 1));
      setActiveIndex(bounded);
      revealRow(bounded);
    },
    [filtered.length, revealRow],
  );

  const setQuery = useCallback(
    (next: string) => {
      setActiveIndex(0);
      if (scrollRef.current) scrollRef.current.scrollTop = 0;
      if (search?.onQueryChange) search.onQueryChange(next);
      else setInternalQuery(next);
    },
    [search],
  );

  useEffect(() => {
    if (!search || search.hotkey === false) return;
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      if (event.metaKey && event.key.toLowerCase() === "f") {
        event.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [search]);

  const onKeyDown = useCallback(
    (event: ReactKeyboardEvent<HTMLElement>) => {
      const page = Math.max(1, Math.floor(window_.viewportHeight / height) - 1);
      switch (event.key) {
        case "ArrowDown":
          event.preventDefault();
          moveActive(clampedActive + 1);
          break;
        case "ArrowUp":
          event.preventDefault();
          moveActive(clampedActive - 1);
          break;
        case "PageDown":
          event.preventDefault();
          moveActive(clampedActive + page);
          break;
        case "PageUp":
          event.preventDefault();
          moveActive(clampedActive - page);
          break;
        case "Home":
          event.preventDefault();
          moveActive(0);
          break;
        case "End":
          event.preventDefault();
          moveActive(filtered.length - 1);
          break;
        case "Enter":
          if (clampedActive >= 0) {
            event.preventDefault();
            onActivate?.(filtered[clampedActive], clampedActive);
          }
          break;
        case "Escape":
          if (query.length > 0) {
            event.preventDefault();
            setQuery("");
          }
          break;
        default:
          break;
      }
    },
    [clampedActive, filtered, height, moveActive, onActivate, query, setQuery, window_.viewportHeight],
  );

  const rows: ReactNode[] = [];
  for (let index = window_.start; index < window_.end; index += 1) {
    const item = filtered[index];
    const isActive = index === clampedActive;
    const args: DataListRowArgs<T> = { item, index, isActive };
    rows.push(
      <div
        key={getKey(item, index)}
        id={`${listId}-row-${index}`}
        role="option"
        aria-selected={isActive}
        data-active={isActive}
        style={{ height }}
        onMouseDown={() => setActiveIndex(index)}
        onDoubleClick={() => onActivate?.(item, index)}
        className={cn(
          "group flex items-center gap-3 px-4 cursor-default transition-colors",
          dividers && "hairline-b",
          isActive ? "bg-sunken-strong" : "hover:bg-sunken",
        )}
      >
        <div className="min-w-0 flex-1">{renderRow(args)}</div>
        {renderRowActions ? (
          <div
            className={cn(
              "shrink-0 opacity-0 transition-opacity",
              "group-hover:opacity-100 group-data-[active=true]:opacity-100 focus-within:opacity-100",
            )}
          >
            {renderRowActions(args)}
          </div>
        ) : null}
      </div>,
    );
  }

  // Which empty state to show is decided by whether a QUERY is active, never by
  // comparing counts. With server-owned search `filtered` IS `items`, so a
  // search that matched nothing looks identical to having no data at all — and
  // telling someone with 4,000 transcripts that they have none, because they
  // searched for a typo, is the worst version of an empty state.
  const searching = query.trim().length > 0;
  const showNoResults = filtered.length === 0 && searching;
  const showEmpty = filtered.length === 0 && !searching;

  return (
    <div className={cn("flex h-full min-h-0 flex-col", className)}>
      {search || toolbar ? (
        <div className="flex items-center gap-2 px-4 py-3">
          {search ? (
            <input
              ref={inputRef}
              type="search"
              value={query}
              placeholder={search.placeholder}
              onChange={(event) => setQuery(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "ArrowDown" || event.key === "Enter") {
                  event.preventDefault();
                  scrollRef.current?.focus();
                  if (event.key === "ArrowDown") moveActive(clampedActive + 1);
                  else if (clampedActive >= 0) onActivate?.(filtered[clampedActive], clampedActive);
                } else if (event.key === "Escape" && query.length > 0) {
                  event.preventDefault();
                  setQuery("");
                }
              }}
              className={cn(
                "hairline h-8 min-w-0 flex-1 rounded-input bg-sunken px-3",
                "text-body text-text-primary placeholder:text-text-tertiary",
              )}
            />
          ) : null}
          {toolbar}
        </div>
      ) : null}

      <div
        ref={scrollRef}
        role="listbox"
        aria-label={label}
        aria-activedescendant={clampedActive >= 0 ? `${listId}-row-${clampedActive}` : undefined}
        tabIndex={0}
        onKeyDown={onKeyDown}
        data-scroll-area=""
        data-fade-top={edges.hasAbove}
        data-fade-bottom={edges.hasBelow}
        className="scroll-fade min-h-0 flex-1 overflow-auto outline-offset-[calc(var(--focus-ring-offset)*-1)]"
      >
        {showEmpty ? empty : null}
        {showNoResults ? (noResults ?? empty) : null}
        {!showEmpty && !showNoResults ? (
          <div style={{ height: window_.totalHeight }} className="relative">
            <div style={{ transform: `translateY(${window_.offsetY}px)` }}>{rows}</div>
          </div>
        ) : null}
        {footer}
      </div>
    </div>
  );
}
