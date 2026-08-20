/**
 * SOURCE OF TRUTH KEYWORDS: global-components-barrel, GlassPanel, StatCard,
 *   DataList, SettingControl, EmptyState, ErrorSurface, ErrorBoundary, ProgressBar, Skeleton,
 *   CountdownRing, Waveform, Keycap
 * WHAT:  The global component library — every primitive reusable across routes.
 * WHY:   One import path for the whole set, so a route never reaches into a
 *        component folder and no two routes disagree about where a primitive
 *        lives (CLAUDE.md §7, §8). Anything used by a single route belongs in
 *        that route's _components/ instead, and does not appear here.
 * WHERE: Imported by app/dashboard, app/pill and app/onboarding.
 */

export * from "./glass-panel";
export * from "./stat-card";
export * from "./data-list";
export * from "./setting-control";
export * from "./empty-state";
export * from "./progress-bar";
export * from "./skeleton";
export * from "./error-surface";
export * from "./error-boundary";
export * from "./countdown-line";
export * from "./scroll-area";
export * from "./waveform";
export * from "./keycap";
export * from "./mark";
export * from "./segmented-control";
