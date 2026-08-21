/**
 * SOURCE OF TRUTH KEYWORDS: InviteStep, ai-era, AI_ERA_PLANS, openUrl,
 *   onboarding-invite, handwritten-note
 * WHAT:  The last screen of onboarding: a personal note from the operator
 *        inviting the user to AI Era, and the button into the app.
 * WHY:   IT COMES AFTER SETUP, NOT BEFORE. Everything ahead of it is work the
 *        user has to do; this is the only screen that asks for nothing, and
 *        putting it in front of the microphone prompt would have made it the
 *        toll gate it must never be.
 *
 *        IT IS A NOTE, NOT A LANDING PAGE. The ask was that it read like a
 *        handwritten message with a heart in it, so the copy is first person,
 *        left-aligned like something written rather than centred like something
 *        published, and it ends by blessing the exit — "if that's not your
 *        goal, I hope you still enjoy this product". The wording is the
 *        operator's, kept deliberately: "web developer", "software assets", "to
 *        production", "only using AI", "monetise" are doing the targeting and
 *        are not mine to improve.
 *
 *        TYPE STAYS MONOCHROME even though this screen is allowed to be the
 *        app's one warm exception, because the banner already brings every
 *        colour on screen. Tinting the words as well would put type in
 *        competition with an image that is doing that job better. The heart is
 *        the grey one, the same as the billing page — the two places in this
 *        product where it speaks in the operator's own voice should look
 *        related.
 *
 *        BOTH BUTTONS ARE THE SAME SIZE AND SIT TOGETHER. The invitation is
 *        what the screen is FOR, so it carries the inverted fill; but a person
 *        who came here to dictate must never have to hunt for the way out, so
 *        the way out is immediately beside it, plainly labelled, and identical
 *        in weight. Opening the link deliberately does NOT finish onboarding:
 *        the browser takes focus, and closing this window out from under
 *        someone who is reading would lose them the one button they still need.
 * WHERE: Rendered by Onboarding after the hotkey test succeeds. The banner
 *        lives at public/promo/ai-era.jpg.
 */

import { useState } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { AppError } from "@/lib/bindings";
import { ErrorSurface } from "@/components/global";
import { cn } from "@/lib/utils";

/** The plans page, not the community landing page — so the invitation lands
 *  where someone can actually act on it. */
const AI_ERA_PLANS = "https://www.skool.com/ai-era/plans";

export function InviteStep({
  onFinish,
  finishError,
}: {
  onFinish: () => void;
  /** Writing `onboarding_complete` can fail, and this screen owns the button
   *  that writes it. Onboarding's contract is that a failed write KEEPS THE
   *  WINDOW OPEN and says why rather than closing on a promise it did not
   *  keep — so the error has to surface here, where the click happened. */
  finishError: AppError | null;
}) {
  const [bannerFailed, setBannerFailed] = useState(false);

  return (
    // Scrollable, and `my-auto` rather than `justify-center`: this screen owns
    // the only way to FINISH onboarding, so its button has to be reachable in
    // every case. Measured, the content needs 446px of the 522px this window
    // gives it — comfortable — but a finish failure adds an error surface, and
    // centred flex content that overflows clips from the TOP, which would put
    // the button off screen with no way to scroll back to it. Auto margins
    // centre it while it fits and let it scroll when it does not.
    <section className="flex h-full flex-col overflow-y-auto px-8">
      <div className="my-auto flex w-full flex-col items-center gap-5 py-6">
        {bannerFailed ? null : (
          <img
            src="/promo/ai-era.jpg"
            alt="AI Era — build and monetise software with AI, for web developers"
            onError={() => setBannerFailed(true)}
            className="hairline w-full max-w-96 rounded-card object-cover"
          />
        )}

        <div className="flex w-full max-w-96 flex-col gap-3 text-body text-text-secondary">
          <p>
            If you&rsquo;re a web developer looking to build your own software assets to production
            only using AI, and then monetise those assets, I created AI Era for you.
          </p>
          <p>There are a lot of bonuses that come with joining the community.</p>
          <p>
            And if that&rsquo;s not your goal, I hope you still enjoy this product. Thank you so
            much for joining our journey, and I hope I can teach you something in the future.{" "}
            <span aria-label="grey heart">🩶</span>
          </p>
        </div>

        {finishError ? (
          <div className="w-full max-w-96">
            <ErrorSurface size="compact" error={finishError} onRetry={onFinish} />
          </div>
        ) : null}

        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={() => void openUrl(AI_ERA_PLANS)}
            className={cn(
              "h-[var(--control-height)] rounded-input px-4 text-body font-medium transition-opacity",
              "bg-text-primary text-opaque-elevated hover:opacity-90",
            )}
          >
            See the plans
          </button>
          <button
            type="button"
            onClick={onFinish}
            className="hairline h-[var(--control-height)] rounded-input px-4 text-body text-text-primary transition-colors hover:bg-sunken"
          >
            Start using Murmur
          </button>
        </div>
      </div>
    </section>
  );
}
