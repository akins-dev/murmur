/**
 * SOURCE OF TRUTH KEYWORDS: data-list-barrel, DataList, DataListProps,
 *   DataListSearch, DataListRowArgs, useVirtualWindow, VirtualWindow
 * WHAT:  Barrel for the data-list component and its windowing hook.
 * WHY:   Folder boundaries export through a barrel (CLAUDE.md §8). The hook is
 *        exported too — anything else that needs a window over uniform rows
 *        should use it rather than write a second one.
 * WHERE: Re-exported by src/components/global/index.ts.
 */

export { DataList, type DataListProps, type DataListSearch, type DataListRowArgs } from "./DataList";
export { useVirtualWindow, type VirtualWindow, type VirtualWindowOptions } from "./use-virtual-window";
