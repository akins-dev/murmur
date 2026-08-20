/**
 * SOURCE OF TRUTH KEYWORDS: StepShell, StepShellProps, StepDots, onboarding-layout
 * WHAT:  The frame every setup step renders inside: heading, one line of copy,
 *        the step's body, and its primary action.
 * WHY:   Steps that each invent their own layout is a chance to move the button
 *        and make the flow feel like several different apps.
 *
 *        THERE ARE NO PROGRESS DOTS HERE, and their removal fixed a real
 *        problem rather than tidying one. The tour that now opens onboarding
 *        has its own three dots — the slider the operator asked for — so a
 *        second three-dot row immediately afterwards read as progress RESETTING
 *        from three-of-three back to one-of-three.
 *
 *        They were also claiming a certainty this flow does not have. These
 *        steps are DERIVED from backend state, not a fixed-length sequence: a
 *        user who already granted the microphone never sees that screen, so
 *        "1 of 3" was announcing a length that varies per machine. Nothing is
 *        lost by dropping them, because every step here is independently
 *        recoverable from Settings.
 * WHERE: Wraps PermissionStep, ModelStep and HotkeyStep.
 */

import type { ReactNode } from "react";

export interface StepShellProps {
  title: string;
  description: string;
  children: ReactNode;
  action?: ReactNode;
}

export function StepShell({ title, description, children, action }: StepShellProps) {
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

    </section>
  );
}
