/**
 * SOURCE OF TRUTH KEYWORDS: StepShell, StepShellProps, StepDots, onboarding-layout
 * WHAT:  The frame every onboarding step renders inside: heading, one line of
 *        copy, the step's body, its primary action, and the progress dots.
 * WHY:   Three steps that each invent their own layout is three chances to move
 *        the button and make the flow feel like three different apps. The dots
 *        are the only progress indicator: a numbered "step 2 of 3" invites the
 *        question of what happens if you stop, and the answer here is nothing
 *        bad — every step is independently recoverable from Settings.
 * WHERE: Wraps PermissionStep, ModelStep and HotkeyStep.
 */

import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

export interface StepShellProps {
  title: string;
  description: string;
  children: ReactNode;
  action?: ReactNode;
  stepIndex: number;
  stepCount: number;
}

export function StepShell({ title, description, children, action, stepIndex, stepCount }: StepShellProps) {
  return (
    <section className="flex h-full flex-col items-center justify-center gap-6 px-8">
      <header className="flex flex-col items-center gap-2 text-center">
        {/* NO MARK HERE. The tour that now opens onboarding introduces the
            product — three slides, each with the mark — so by the time anyone
            reaches a setup step the identity has been shown three times
            already. A fourth turns it into a watermark, which is the thing the
            original "first screen only" rule existed to prevent; the rule did
            not change, the first screen did (docs/04 §12). */}
        <h1 className="text-title text-text-primary">{title}</h1>
        <p className="max-w-96 text-body text-text-secondary">{description}</p>
      </header>

      <div className="w-full max-w-96">{children}</div>

      {action ? <div className="flex items-center gap-2">{action}</div> : null}

      <div className="flex items-center gap-2" aria-hidden="true">
        {Array.from({ length: stepCount }, (_, index) => (
          <span
            key={index}
            className={cn(
              "size-[var(--pill-dot-size)] rounded-pill transition-colors",
              index === stepIndex ? "bg-text-primary" : "bg-sunken",
            )}
          />
        ))}
      </div>
    </section>
  );
}
