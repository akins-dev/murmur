/**
 * SOURCE OF TRUTH KEYWORDS: TourStep, TourSlide, TOUR_SLIDES, onboarding-tour,
 *   tour-media, onboarding-gif
 * WHAT:  The three-slide tour that opens onboarding: what the app does, in the
 *        order a new user needs it. Logo, a media slot, one line of copy, dots.
 * WHY:   THIS TEACHES, IT DOES NOT SET UP. The setup steps that follow are
 *        derived from backend state — permission granted, model ready — and
 *        that is right for them, because a returning user must not repeat work
 *        they already did. A tour has no backend state to derive from: it is
 *        three pages of reading, so it is the one part of this flow that is
 *        honestly a counter, and pretending otherwise would be architecture for
 *        its own sake.
 *
 *        THE THIRD SLIDE IS THE PRODUCT. Murmur transcribes on the user's own
 *        machine, and nothing in the app said so anywhere — a person could use
 *        it for a week believing their voice was being uploaded. That is the
 *        single most valuable sentence here and it gets its own slide rather
 *        than a clause at the end of another one.
 *
 *        THE MEDIA SLOT IS RESERVED BUT NOT REQUIRED. The operator is supplying
 *        GIFs later, so the box is sized and positioned now and the file is the
 *        only thing missing: drop `step-1.gif`, `step-2.gif`, `step-3.gif` into
 *        `public/onboarding/` and they appear, no code change. Until then the
 *        slot renders nothing at all until a file is there — an empty bordered
 *        rectangle on the first screen anyone ever sees reads as broken, not as
 *        pending, and this flow ships before the media does. Logo, heading and
 *        one line is a finished minimal screen without it.
 * WHERE: Rendered by Onboarding before the permission/model/hotkey steps.
 */

import { useState } from "react";
import { ChevronLeft } from "lucide-react";
import type { HotkeyBinding } from "@/lib/bindings";
import { glyphsForBinding } from "@/lib/hotkey";
import { cn } from "@/lib/utils";
import { Keycap, Mark } from "@/components/global";

interface TourSlide {
  title: string;
  /** One line. Anything longer stops being read on a first-run screen. */
  body: (hotkey: HotkeyBinding | null) => React.ReactNode;
  media: string;
}

const TOUR_SLIDES: readonly TourSlide[] = [
  {
    title: "Hold to talk",
    body: (hotkey) => (
      <>
        Hold{" "}
        {hotkey ? (
          <Keycap keys={glyphsForBinding(hotkey)} size="sm" className="mx-0.5 inline-flex align-middle" />
        ) : (
          "your shortcut"
        )}{" "}
        anywhere and say it. Let go when you are done.
      </>
    ),
    media: "/onboarding/step-1.gif",
  },
  {
    title: "It lands where you type",
    body: () => <>Your words paste straight into whatever app you were already in.</>,
    media: "/onboarding/step-2.gif",
  },
  {
    title: "Nothing leaves your Mac",
    body: () => <>Every word is transcribed on your own machine. No account, no upload.</>,
    media: "/onboarding/step-3.gif",
  },
];

export const TOUR_LENGTH = TOUR_SLIDES.length;

export function TourStep({
  hotkey,
  onDone,
}: {
  hotkey: HotkeyBinding | null;
  onDone: () => void;
}) {
  const [index, setIndex] = useState(0);
  const slide = TOUR_SLIDES[index]!;
  const isLast = index === TOUR_SLIDES.length - 1;

  return (
    <section className="flex h-full flex-col items-center justify-center gap-6 px-8">
      <Mark size="lg" label="Murmur" />

      <TourMedia key={slide.media} src={slide.media} title={slide.title} />

      <header className="flex flex-col items-center gap-2 text-center">
        <h1 className="text-title text-text-primary">{slide.title}</h1>
        <p className="max-w-96 text-body text-text-secondary">
          {slide.body(hotkey)}
        </p>
      </header>

      <div className="flex items-center gap-3">
        <button
          type="button"
          onClick={() => setIndex((current) => current - 1)}
          disabled={index === 0}
          aria-label="Previous"
          className="flex size-[var(--control-height)] items-center justify-center rounded-input text-text-secondary transition-opacity hover:bg-sunken disabled:pointer-events-none disabled:opacity-0"
        >
          <ChevronLeft className="size-4" />
        </button>

        <button
          type="button"
          onClick={() => (isLast ? onDone() : setIndex((current) => current + 1))}
          className="h-[var(--control-height)] rounded-input bg-text-primary px-4 text-body font-medium text-opaque-elevated transition-opacity hover:opacity-90"
        >
          {isLast ? "Set up Murmur" : "Next"}
        </button>

        {/* Balances the back button so the primary action stays optically
            centred on slide one, where back is hidden. Without it the button
            drifts sideways as you page, which is the kind of movement a
            "no distraction" screen cannot afford. */}
        <span aria-hidden="true" className="size-[var(--control-height)] shrink-0" />
      </div>

      {/* The slider's only progress indicator. Clickable, because a tour the
          user cannot page back through is a slideshow they are trapped in. */}
      <div className="flex items-center gap-2">
        {TOUR_SLIDES.map((item, dotIndex) => (
          <button
            key={item.title}
            type="button"
            aria-label={`Go to ${item.title}`}
            aria-current={dotIndex === index}
            onClick={() => setIndex(dotIndex)}
            className={cn(
              "size-[var(--pill-dot-size)] rounded-pill transition-colors",
              dotIndex === index ? "bg-text-primary" : "bg-sunken hover:bg-sunken-strong",
            )}
          />
        ))}
      </div>
    </section>
  );
}

/**
 * The reserved slot. Sized and positioned now so that dropping a file into
 * public/onboarding/ is the only change needed — but it renders NOTHING until
 * a file is actually there.
 *
 * An empty bordered rectangle on the first screen a person ever sees does not
 * read as "media pending", it reads as broken, and this flow ships before the
 * GIFs do. Collapsing is not a fallback, it is the correct state for a build
 * that has no media: logo, one line, one heading is a finished minimal screen
 * on its own. The layout only changes between builds, never in front of a user.
 */
function TourMedia({ src, title }: { src: string; title: string }) {
  const [loaded, setLoaded] = useState(false);
  const [failed, setFailed] = useState(false);

  if (failed) return null;

  return (
    <div
      className={cn(
        "hairline aspect-[16/10] w-full max-w-80 overflow-hidden rounded-card bg-sunken",
        !loaded && "hidden",
      )}
    >
      <img
        src={src}
        alt={title}
        onLoad={() => setLoaded(true)}
        onError={() => setFailed(true)}
        className="size-full object-cover"
      />
    </div>
  );
}
