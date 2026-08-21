/**
 * SOURCE OF TRUTH KEYWORDS: BillingView, billing-route, free-forever
 * WHAT:  The Billing page. There is no billing.
 * WHY:   The operator wrote this one himself and the words are his, so they are
 *        reproduced exactly rather than tidied into something more corporate —
 *        "lol" is the joke, and a page that opens with "Murmur is free to use"
 *        is a different, worse page.
 *
 *        It is built like a real page rather than dropped in as a stub, because
 *        a half-hearted joke reads as an unfinished feature: someone clicking
 *        Billing expecting a paywall should land somewhere that is obviously
 *        deliberate and be pleased, not wonder whether the page failed to load.
 *        So it gets the same centred composition and the same type scale as
 *        every other terminal state in the app, and the punchline is set in the
 *        display size normally reserved for the hero stat — the loudest type in
 *        the product, spent on the shortest word in it.
 *
 *        The heart is the grey one. It is the only emoji in the app and the
 *        palette has no colour left in it by design (docs/04 §2), so a red one
 *        would be the single saturated pixel in the entire interface, on the
 *        one page that exists to be charming rather than to be noticed.
 * WHERE: Routed from the capability registry's Billing nav entry.
 */

import { ScrollArea } from "@/components/global";

export function BillingView() {
  return (
    <ScrollArea contentClassName="flex min-h-full flex-col items-center justify-center gap-3 px-[var(--page-padding-x)] pb-8 text-center">
      <p className="text-display text-text-primary">lol</p>
      <p className="max-w-96 text-body text-text-secondary">
        This is absolutely free. Enjoy <span aria-label="grey heart">🩶</span>
      </p>
    </ScrollArea>
  );
}
