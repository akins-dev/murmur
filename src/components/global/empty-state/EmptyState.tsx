/**
 * SOURCE OF TRUTH KEYWORDS: EmptyState, EmptyStateProps, EmptyStateSize
 * WHAT:  The designed empty state: an icon or keycap slot, a headline, one
 *        line of copy, and an optional action slot.
 * WHY:   A blank panel reads as broken, so every list and stat in the app has
 *        one of these (docs/04 §9). Every word and every glyph arrives as a
 *        prop — the component carries no copy, because the same shape has to
 *        serve "no transcriptions yet", "permission missing" and "model
 *        downloading" without a branch per case.
 * WHERE: The empty slot of data-list, the stats view before first use, and
 *        onboarding's permission steps.
 */

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export type EmptyStateSize = "compact" | "default";

export interface EmptyStateProps {
  /** Icon, illustration or <Keycap /> — anything, including nothing. */
  icon?: ReactNode;
  headline: string;
  /** One line. If it needs two, the state is explaining too much. */
  description?: ReactNode;
  action?: ReactNode;
  size?: EmptyStateSize;
  className?: string;
}

const PADDING: Readonly<Record<EmptyStateSize, string>> = {
  compact: "py-6 gap-2",
  default: "py-12 gap-3",
};

export function EmptyState({ icon, headline, description, action, size = "default", className }: EmptyStateProps) {
  return (
    <div
      className={cn("flex flex-col items-center justify-center px-6 text-center", PADDING[size], className)}
    >
      {icon ? <div className="text-text-tertiary">{icon}</div> : null}
      <p className="text-heading text-text-primary">{headline}</p>
      {description ? <p className="text-body text-text-secondary max-w-80">{description}</p> : null}
      {action ? <div className="pt-2">{action}</div> : null}
    </div>
  );
}
