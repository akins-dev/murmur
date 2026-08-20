# Murmur — Design System

Exact values, so the look is decided once here rather than improvised per component. If a component needs a value that is not in this file, add it here first.

---

## 1. Principles

1. **Real glass, not CSS glass.** Native `NSVisualEffectView` vibrancy is the material. The web layer contributes only the border, the inner highlight, and the noise. `backdrop-filter` is a fallback, and it looks like an imitation of glass — never the primary.
2. **One elevated surface per view, and it has to earn it.** Content sits directly on the material by default. Exactly one card may float on the panel — the one holding the thing the view exists to show — and it is the `glass-elevated` material with `--radius-card`, so the layering reads as depth. A *grid* of cards on glass is what makes this look like a Windows Vista widget, not the existence of a card: repeat the surface and none of them is elevated any more, they are just boxes. Secondary content is grouped by spacing and a hairline, never by giving it a surface of its own.
3. **Silent until you speak.** This is the idea the whole interface is built from, and it is why Murmur looks like itself rather than like every other app. At rest there is NO colour anywhere — the app is warm graphite glass, type and shadow, and nothing else. Saturation exists only while a session is live, and it leaves when the session does. A coloured control on an idle screen is a bug, not a decoration.
4. **Depth comes from layering, not from colour.** Elevation is translucency, radius and shadow: a panel, a card floating on it, a hairline catching light at the top edge. If a surface needs to stand out and the answer reaches for a hue, the answer is wrong.
5. **Motion is physics, not duration.** Springs, not eases. Nothing in the app moves linearly.
6. **The pill has a hard budget: 60fps, always.** It renders while a realtime audio thread and an inference worker are both running. Anything expensive in the pill is wrong by definition.

---

## 2. Color tokens

> **THE APP HAS NO ACCENT HUE.** Ember is gone — the operator rejected it in plain words and asked
> for the minimal black/white/grey the rest of the interface already was. `--accent` survives as a
> NAME, not as a colour: it is ink, and it still means "the emphasised thing", so every call site
> that asks for it by role kept working without an edit. `--success` was already ink before this
> change and is the precedent, not an exception.
>
> **State is carried by FORM, not hue.** A moving waveform means recording; a draining line means
> cancelling; a sentence means failed. Hue was decorating a distinction those shapes had already
> made, which is exactly why removing it cost nothing — and why the old §1.3 rules about "where
> ember is allowed" no longer have anything to govern.
>
> **`--danger` is the one surviving colour**, because destructive and failed are the two cases where
> losing the distinction is a safety regression rather than a style choice: "Delete all history"
> must not look like "Cancel". It was retuned off the old rust tone (hue ≈10°, which reads as
> orange) to a true red (hue ≈4°) so it cannot be mistaken for the theme that was rejected. Use it
> as a MARK — text, icon, dot — never as a wash bigger than the pill's failure tint.


Defined as CSS custom properties on `:root`, redefined under `@media (prefers-color-scheme: dark)`. **Never write a hex value in a component.**

### Light

```css
--surface-glass:      rgba(252, 250, 247, 0.55);   /* warm, never pure white */
--surface-elevated:   rgba(255, 254, 252, 0.72);
--surface-sunken:     rgba(40, 34, 28, 0.04);     /* hover */
--surface-sunken-strong: rgba(40, 34, 28, 0.07); /* press, and a selected nav item */
--border-hairline:    rgba(40, 34, 28, 0.09);
--border-highlight:   rgba(255, 255, 255, 0.70);   /* top inner edge */
--text-primary:       rgba(28, 24, 20, 0.90);
--text-secondary:     rgba(28, 24, 20, 0.54);
--text-tertiary:      rgba(28, 24, 20, 0.32);
--accent:             rgba(28, 24, 20, 0.90);      /* INK. there is no accent hue */
--accent-soft:        rgba(40, 34, 28, 0.06);
--success:            rgba(28, 24, 20, 0.90);      /* no hue: the shape says done */
--success-soft:       rgba(40, 34, 28, 0.06);
--warning:            rgba(28, 24, 20, 0.54);      /* no hue: the line says counting */
--warning-soft:       rgba(40, 34, 28, 0.06);
--danger:             #A8342C;                     /* the ONLY colour in the app */
--danger-soft:        rgba(168, 52, 44, 0.10);
--shadow-pill:        0 12px 40px rgba(28, 24, 20, 0.16);
--shadow-panel:       0 24px 64px rgba(28, 24, 20, 0.14);
--shadow-card:        0 8px 24px rgba(28, 24, 20, 0.10);
```

**Two sunken steps, and they are the app's entire interaction feedback.** `--surface-sunken` is
hover, `--surface-sunken-strong` is press and the selected state of a nav item. Neither carries a
hue, because §1.3 spends the only saturation in the app on a live session and a button under a
cursor is not one. Before these existed, every button in the app hovered to `--accent-soft` — nine
copies of the same mistake, which is how the interface read as jarring while each individual
component looked defensible.

**Where colour is allowed, exhaustively.** The list used to enumerate where ember was permitted;
there is no ember, so the list collapses to one row and is kept because a one-row list is still
easier to check than re-deriving the rule at each call site:

| Allowed | Because |
|---|---|
| `--danger` on a destructive control, and on the pill's failed state | Losing this distinction is a safety regression, not a style choice. |

Everything else is monochrome, including every case that used to be ember: the pill's waveform and
state dot, the hotkey-capture control while armed, the model download's progress bar, the focus
ring, hover, press, selection, an ON switch, a nav item, a step dot, a primary button and every
chart. A primary action that needs more weight than a secondary one gets it from an inverted fill —
`--text-primary` with `--surface-opaque-elevated` as its label — never from a hue.

### Dark

```css
--surface-glass:      rgba(36, 33, 30, 0.58);   /* warm graphite, not blue-grey */
--surface-elevated:   rgba(52, 48, 44, 0.72);
--surface-sunken:     rgba(252, 250, 247, 0.04);     /* hover */
--surface-sunken-strong: rgba(252, 250, 247, 0.08); /* press, and a selected nav item */
--border-hairline:    rgba(252, 250, 247, 0.10);
--border-highlight:   rgba(252, 250, 247, 0.13);
--text-primary:       rgba(252, 250, 247, 0.92);
--text-secondary:     rgba(252, 250, 247, 0.56);
--text-tertiary:      rgba(252, 250, 247, 0.34);
--accent:             rgba(252, 250, 247, 0.92);   /* INK. there is no accent hue */
--accent-soft:        rgba(252, 250, 247, 0.08);
--success:            rgba(252, 250, 247, 0.92);   /* no hue: the shape says done */
--success-soft:       rgba(252, 250, 247, 0.08);
--warning:            rgba(252, 250, 247, 0.56);   /* no hue: the line says counting */
--warning-soft:       rgba(252, 250, 247, 0.08);
--danger:             #E2685C;                     /* the ONLY colour in the app */
--danger-soft:        rgba(226, 104, 92, 0.14);
--shadow-pill:        0 12px 40px rgba(0, 0, 0, 0.50);
--shadow-panel:       0 24px 64px rgba(0, 0, 0, 0.46);
--shadow-card:        0 8px 24px rgba(0, 0, 0, 0.34);
```

**Two hues exist in this app, and both are earned.** Ember is the live colour: recording, the cancel ring, the waveform. It is warm because the surface is warm, and it is the only thing on screen with saturation while you are speaking. The muted brick is failure, and nothing else. Delivered has NO hue at all — a checkmark at full text weight reads as done without spending a colour on it, and green here would be the third hue that made the old palette feel like a bootstrap theme.

**State → color mapping.** The only permitted uses of non-accent color:

| State | Color |
|---|---|
| Idle | no colour at all |
| Arming / Recording | ink — the waveform's MOTION is the signal |
| Cancel armed | ink — the LINE draining is the signal |
| Failed | `--danger` (true red) |
| Not ready yet (transient) | no colour: it is not a failure |

**A failure the user can simply retry in a moment is not drawn as a failure.** `ENGINE_NOT_READY` — the engine still warming in the seconds after launch — is the single most likely failure a NEW user will ever see, on their very first keypress, and it is temporary by definition. Painting the pill red there says the app is broken; it is not, it is seven seconds old. Transient failures keep `--accent` and read "not yet", not "something is wrong". The distinction is made on `ErrorCode`, which is the field the frontend is meant to branch on — never on message text.

Each has a `-soft` variant at the same alpha as `--accent-soft`. The solid colour is for a mark the eye goes to — a dot, a ring, a checkmark, a line of text. The soft one is for tinting a whole surface, which is what §7 means when it says the pill *turns* `--danger`: at full strength over vibrancy that would be a red box, and the pill is an indicator, not an alert.

---

## 3. Materials

Three, and only three.

| Token | Native material | Used for |
|---|---|---|
| `glass-pill` | `NSVisualEffectMaterial::HudWindow` | The pill overlay |
| `glass-panel` | `NSVisualEffectMaterial::Sidebar` | Dashboard window background |
| `glass-elevated` | `NSVisualEffectMaterial::Popover` | Menus, sheets, dropdowns |

Every glass surface gets the same three-part treatment:

```css
.glass {
  background: var(--surface-glass);
  border: 0.5px solid var(--border-hairline);
  box-shadow: inset 0 0.5px 0 var(--border-highlight), var(--shadow-panel);
}
```

Plus a noise overlay at **3% opacity** — an inline SVG `feTurbulence` at `baseFrequency="0.8"`, `pointer-events: none`, sitting above the background and below content. This is what stops large glass areas from looking flat and plasticky. It is subtle enough that you only notice its absence.

---

## 4. Geometry

```
--radius-pill:   22px      /* = height / 2, fully round */
--radius-panel:  28px
--radius-card:   20px
--radius-input:  12px

--space-1: 4px    --space-2: 8px     --space-3: 12px
--space-4: 16px   --space-5: 24px    --space-6: 32px    --space-8: 48px

--rail-width:        56px    /* the icon-only left rail, §8 */
--control-height:    32px    /* buttons, selects, segmented controls */
--control-height-sm: 28px    /* a control inside a row, §11 */
```

Radii are large on purpose. A 16px panel corner reads as a web card; a 28px one reads as a floating object with physical thickness, which is what the glass is imitating. Card radius stays proportionally inside panel radius so a card sitting on a panel never looks like it is fighting the corner it sits in.

Borders are **0.5px**, not 1px. On a Retina display 0.5px is a true hairline; 1px reads as heavy and immediately un-Apple.

---

## 5. Typography

System font stack (`-apple-system` → SF Pro). Never ship a webfont — it costs a network request or a bundle, and SF is what makes it feel native.

| Role | Size | Weight | Tracking | Line height |
|---|---|---|---|---|
| Display (the big stat number) | 40px | 600 | -0.02em | 44px |
| Title | 22px | 600 | -0.01em | 28px |
| Heading | 16px | 600 | -0.01em | 22px |
| Body | 13px | 400 | 0 | 18px |
| Label | 12px | 500 | 0 | 16px |
| Caption | 11px | 400 | 0.01em | 14px |
| Mono (transcripts, hotkeys) | 12px | 400 | SF Mono | 18px |

Line heights are absolute, not multipliers: a row of glass UI has to sit on the same baseline grid whatever size the text is, and a unitless multiplier drifts off it at 11px and 13px.

Numbers in stats and the pill timer use `font-variant-numeric: tabular-nums`. Without it the timer visibly jitters as digits change — a small detail that reads as cheap.

---

## 6. Motion

Springs only. Reference values (Framer Motion syntax; a CSS spring equivalent is fine):

| Motion | Spring | Note |
|---|---|---|
| Pill appear | none — **instant** | A fade-in delays the only feedback that the keypress registered |
| Pill exit | not a spring — see below | Owned by the WINDOW, not the document |
| State change | `stiffness: 300, damping: 25` | Colour morph. **Not width** — see §7 |
| Panel / sheet | `stiffness: 260, damping: 26` | |
| Waveform bars | no spring | Driven directly by audio RMS at 60fps |
| Countdown line | linear | The one exception — a decreasing timer must be linear or it lies about remaining time |

**The pill's own motion is not in this table, and must never be put back into it.** The pill's glass is a native `NSVisualEffectView` sized to its window, so anything CSS moves — a scale, a translate, a width morph — slides out from under the glass and reveals it. The pill appears instantly and leaves by animating the WINDOW: `--pill-exit-duration-ms` (160) and `--pill-exit-travel` (8), read out of `tokens.css` by `src-tauri/src/tray.rs` so the motion and the design stay one fact. The travel direction carries the outcome — **up for committed, down for cancelled** — which is the whole reason the exit is animated at all, since nothing else on screen distinguishes the two once the pill is gone. Failed rises too: it is not a discarded recording, and dropping it would read as the words being thrown away.

A spring in this table can only drive motion **inside** a surface. Moving a surface is the window's job.

**Respect `prefers-reduced-motion`:** replace every spring with a 120ms opacity fade. Do not simply disable animation — things appearing instantly reads as a glitch.

### 6.1 CSS-side motion

Springs above are for mount/dismiss and morphs, and they run in JS. Hover, press and focus feedback must never take a JS frame — the pill's budget does not allow it — so those are plain CSS transitions with these values:

```
--motion-duration-fast:   120ms   /* hover, press, focus ring; also the reduced-motion fade */
--motion-duration-medium: 240ms   /* enter/exit of non-critical chrome */
--motion-ease-standard:   cubic-bezier(0.32, 0.72, 0, 1)   /* spring-shaped, no overshoot */
--motion-ease-linear:     linear                            /* countdown ring only */
```

`--motion-duration-fast` and the reduced-motion fade are deliberately the same number: under reduced motion every spring collapses to exactly the duration the app already uses for its smallest transition, so nothing feels out of place.

### 6.2 Springs as tokens

Framer needs numbers, and a component may not carry one. Every spring in the table above is published as a pair of custom properties — `--spring-<name>-stiffness` / `--spring-<name>-damping` — and read out of the stylesheet by `src/lib/motion.ts`. A component asks for a spring by name, never for `{ stiffness: 400 }`. `--spring-pill-appear-*` survives for an opacity-only fade of pill CONTENT; it must never be wired to a transform, for the reason in the table above.

---

## 7. The pill — exact specification

```
        ┌─────────────────────────────────┐
        │  ●  ▁▃▅▇▅▃▁▃▅▇▅▃▁    0:07  │     176 × 36 pt
        └─────────────────────────────────┘
          ↑        ↑                  ↑
       state    waveform            elapsed
```

- **Size:** 176×36pt for every live state — arming, recording and cancel-armed alike. **Nothing resizes during a dictation.** The operator asked for the pill to be smaller and more minimal, and the height is where that reads: 44→36 is the change you feel. The contents were retuned to match — 20 waveform bars rather than 24, a 16pt bar height, 12pt padding and an 8pt gap — so the pill got smaller without anything inside it getting cramped.
- **The window is the pill.** The NSPanel is sized to `--pill-width`/`--pill-height` exactly and its vibrancy carries `--radius-pill`, so every pixel of the window is painted glass in the pill's shape. Two consequences bind everything else here: the pill fills the window and **never animates its own size or transform** (object motion — arriving, resizing, leaving — belongs to the window, in Rust; only motion *inside* the pill belongs to CSS), and the pill has **no CSS drop shadow**, because a shadow has nowhere to fall and is clipped by the window bounds. Its shadow is the native window shadow. `src-tauri/src/tray.rs` `include_str!`s `tokens.css` and parses these values at startup, so the window has no second copy of them to disagree with. **Change a token and the window follows — there is no Rust number to edit and no test to re-green.** Delete a token the pill needs and it is a build-time panic naming it.
- **Position:** bottom-center, 96pt above the bottom edge of the screen containing the cursor. Multi-monitor: follow the active screen. Position is decided **once per appearance** — and that is enforced by an `is_visible()` gate in `set_pill_visible`, not by intent. State is emitted several times a second, so positioning unconditionally makes the pill chase the pointer between displays mid-sentence. A comment describing that intent is not a mechanism; the gate is.
- **State dot:** 8pt, ink (`--danger` only on a real failure), with a 2s breathing pulse (opacity 0.6→1) while recording. Boxed to one line-height so it sits on the *first line* of a two-line failure and dead-centre in every single-line state.
- **Waveform:** 20 bars, 2pt wide, 2pt gap, max height 16pt, driven by windowed RMS — 78pt intrinsic, which is the number the pill width is built around. Bars scroll right-to-left. Minimum bar height 2pt so it never looks dead during a pause.
- **Waveform input scaling.** The RMS arriving from the audio thread is linear 0..1, and real speech sits between roughly 0.05 and 0.3 — so a bar drawn at `height = rms` is a flat line for the entire recording. Two values map the audio range onto the visual one:

  ```
  --waveform-input-ceiling: 0.35   /* the RMS treated as a full-height bar */
  --waveform-input-gamma:   0.6    /* < 1 lifts quiet speech into view */
  ```

  `level = clamp((rms / ceiling) ^ gamma)`. The ceiling sits just above normal speech so the loud end has somewhere to go before it pins; the gamma is there because loudness is perceived logarithmically, and a linear map spends most of the bar's height on a range no one ever speaks in. Together they put 0.05 at roughly a third height and 0.3 near full — a visible, moving waveform across the range a person actually talks at.
- **Timer:** `M:SS`, tabular numerals, `--text-secondary`, in a **reserved 34pt box** rather than a measured one. A minute rolling over must not steal width from the waveform mid-recording.
- **Arming** draws *exactly* what recording draws — resting waveform, unpulsed dot, `0:00`. It is normally under 100ms, and a distinct label for a tenth of a second is not information, it is a flicker: the pill would appear, flash a word, and reshape before the eye resolved any of it. The pill arrives already in its working shape and comes alive as sound reaches it. A genuine stall surfaces as Failed.
- **Cancel armed:** the waveform is replaced by a **countdown line** in the same slot — full width, `--countdown-line-height` (3pt), starting full and draining to zero. Not a ring: the operator asked for a line, and the ring it replaces was genuinely broken, not merely ugly. **There is no label beside it.** A countdown the user armed himself with a keypress does not need a caption telling him he armed it, and dropping it is what lets the line occupy the whole middle slot so the swap reads as the waveform changing mode rather than a new widget appearing next to one. A second Escape refills the line with a spring and returns the waveform.
- **The line is driven by `remaining_ms`, never by a local timer, and the animation is created ONCE.** The ring failed because its effect depended on `durationMs` — which *was* `remaining_ms`, a value Rust re-emits several times a second — so every tick cancelled the running animation and restarted a fresh full-to-empty drain. It never progressed and it flickered: two clocks disagreeing. Later ticks may only SEEK the running animation, and only when they disagree with it by more than 120ms, so ordinary tick jitter costs nothing and a real correction still lands. **Never read the countdown's duration as a constant** — it is configurable, and a skip-countdown setting exists.
- **Failed:** 360×56pt, same `--radius-pill`. The pill turns `--danger` (the soft variant — at full strength over vibrancy this is a red box, and the pill is an indicator, not an alert) and shows the reason **on up to two lines, never truncated**. Failure is a sentence, not a status: the corpus it must seat runs to 100 characters (*"Murmur heard nothing at all. Check that the right input device is selected and that it is not muted."*). Measured at `--text-label`, every message in that corpus wraps to at most two lines at 340pt; 360pt is that threshold plus room for the copy to grow a few words without a redesign. Height is two line-boxes plus 12pt above and below — at 16pt it reads as a dialog rather than as the pill grown, which is visible the moment the two are put side by side. Content is **centred, not top-aligned**, or a one-line message floats at the top of the box. **The whole point of this state is that he finds out why nothing arrived, and a truncated sentence is a failure that failed twice.** The size is fixed rather than fitted to the line count, because only the webview can measure text and only Rust can size the window — a fitted height would need a round trip and would show a frame at the wrong size.
- **Not ready yet:** a failure with a transient code — currently `ENGINE_NOT_READY` — keeps `--accent` and shows its reason plainly. Same layout as Failed, none of the alarm. See the note under §2.
- **There is no Finalizing and no Delivered.** Releasing the hotkey ends capture instantly; transcription and pasting continue in the background with nothing on screen. The pill is gone the moment the key is up. It does not report a word count — the operator asked for that removal specifically — and it does not linger to congratulate itself. **Failed re-appears** a second or two later if the background half could not finish, which is the only reason the pill is ever seen again after a release.
- **IDLE never blanks the view.** Rust hides the window on IDLE and the same transition also reaches the webview over IPC; those do not land on the same frame. A view that cleared on IDLE would leave an empty glass capsule on screen until the native hide caught up — a grey blob flashing at the end of every dictation. The view renders the last state that had something to say and lets the window take it away.

**How long Failed stays on screen is enforced in Rust, not in CSS**, because the FSM owns whether the pill is visible — a window whose visibility is owned by the state machine and whose dismissal is owned by the view will disagree eventually. It is deliberately absent from `tokens.css`: it was there once, unused, and an unused token that looks authoritative is worse than no token, because the next person changes it and expects something to happen.

**Never:** a close button, a title bar, a shadow-casting border, or any control. It is an indicator, not a window. Every interaction is via keyboard.

---

## 8. Dashboard layout

### The update notice

A slim bar docked BELOW the scroll area, on every dashboard page, absent entirely until there is a
newer version.

- **It must never take the screen.** The person using this app is usually dictating into something
  else and the dashboard may not even be open. An update prompt that interrupts that is worse than
  one found late, because the product's whole promise is "talk and the words appear" and anything
  stealing focus mid-sentence breaks it. Not a modal, not a toast, not an overlay — docked below the
  content so it covers nothing and moves nothing.
- **It asks as well as listens**, and that is what makes the feature visible at all. The backend
  checks at launch and every 24h and emits an event, but the dashboard opens from the menu bar and
  may not exist when that event fires — a listener alone would miss nearly every one and the feature
  would appear broken while working perfectly. So: `check_for_update` once on mount for the answer
  that already exists, plus the event for a check that lands while it is open. Same shape as the
  pill — a command for first paint, events after that.
- **The restart is in the button label**, not in a sentence beside it. Installing swaps the running
  app, and the label is the last thing read before committing.
- The button is a bordered secondary. The inverted primary fill is reserved for the app's one
  primary action, and a bar whose entire purpose is to stay quiet must not carry the loudest control
  in the product.

### Registry-declared icons

`NavDef.icon` is a STRING that crosses from Rust into TypeScript and is resolved through a
hand-maintained map in `src/lib/icons.ts`. A name with no entry does not fail to build, does not
warn and does not throw — it renders the fallback. Two shipped that way (`CreditCard`,
`ChartNoAxesColumn`) and sat in the sidebar wearing a plausible placeholder until the operator
reported it.

- **The fallback is deliberately ugly.** It was `CircleDot`, which looks like a designed icon and is
  exactly why the bug survived. It is now `CircleHelp`: a fallback's job is not to look acceptable,
  it is to be read as "this is missing" in about a second.
- **The guarantee is a test, not a glyph.** `src-tauri/src/registry/icons.rs` reads `icons.ts` and
  asserts every declared nav icon resolves. It lives on the side that declares the name because
  TypeScript cannot know what strings Rust will send — the same arrangement as `reachability.rs`
  and as `tray.rs` reading `tokens.css`. Adding a capability with an unmapped icon now fails
  `cargo test` with the route and the icon name in the message.


### The page header, the scroll fades, and no scrollbars

- **One header component, every page** (`PageShell`). The page title lives here, not inline in each
  view, and it is read from the capability registry's `NavDef.label` — a page added to the registry
  arrives titled with no frontend edit.
- **It is the alignment fix for the traffic lights, and there are two halves to it.** macOS places
  the lights at a fixed inset from the window's TOP-LEFT, spanning roughly 20–72pt. They know
  nothing about our sidebar and cannot be moved from CSS, so the header accommodates them, never the
  reverse.
  - *Vertically:* the title sits in `--traffic-light-band` (28pt, the standard macOS title bar,
    which puts the button centres at 14pt) — **not** centred in the taller `--page-header-height`,
    which dropped it a few pixels below them. `--text-title-line` is also 28pt, so the title fits
    the band exactly. **If the title still sits off the lights on device, that token is the one
    number to change.**
  - *Horizontally:* the rail's divider now starts BELOW the header band. The lights are wider than
    the 56pt rail, so a full-height divider ran straight through the middle of the button cluster
    and made them read as something trying and failing to fit inside the sidebar. Above the band
    there is no seam at all — the top of the window is one continuous surface across rail and
    content, which is what a title bar looks like and what lets the lights simply sit in it.
- **The header is the drag region**, and it is deliberately generous. The old one lived *inside*
  each page's scroller, so it slid away the moment anyone scrolled and took the draggable surface
  with it — which is why the window was reported as hard to grab. Interactive children opt out with
  `data-tauri-drag-region={false}` or they drag the window instead of firing.
- **The header OVERLAYS the scroll area** rather than sitting above it, so content tucks under it
  and fades. `ScrollArea`'s `headerInset` pads the content down past the bar — that padding is the
  guarantee that content is never hidden, and it lives in one place so no page has to remember it.
- **The fades are a MASK, not a painted gradient.** The window is transparent with native vibrancy
  behind it; a gradient in a background colour would be an opaque slab lying on the glass. Masking
  makes the content itself dissolve, which is both what was asked for and the only version that is
  correct over a translucent surface.
- **Smart means each edge is present only when something is actually hidden behind it.** A fade with
  nothing behind it is not neutral — it reads as a rendering artefact, a smudge at the bottom of a
  short page. Driven by `useScrollEdges`, which watches scroll position, viewport size AND content
  size: content changing height (a list finishing loading, a banner appearing) changes the answer
  without any scroll event ever firing, and a fade that is only correct until the window is resized
  is the same bug as no fade at all.
- **No scrollbars anywhere.** The fade is the affordance that replaces them. This hides the BAR,
  never the scrolling — wheel, trackpad, keyboard paging and `scrollTop` are untouched.
- A scroller nested below its own chrome (DataList, under its toolbar) overrides `--fade-size-top`
  so it fades at its own edge instead of reaching up into somebody else's header.


```
+--------------------------------------------------+
| o o o                                            |   traffic lights inset,
| _-#-_ |                                          |   hidden title bar
|       |  Stats                                   |
|  [#]  |  +------------------------------------+  |   960 x 640 default
|  (o)  |  |  4h 12m       the one loud number  |  |   min 840 x 560
|  {*}  |  |    /\__/\_    thin unadorned line  |  |
|       |  +------------------------------------+  |
|       |  quiet label/value rows below            |
+-------+------------------------------------------+
   56pt
```

**The window IS the floating panel.** Native vibrancy fills the whole window rect and the
window's own corner radius rounds it, so there is no inset margin and no second panel drawn
inside one — a glass panel inside a glass window is two seams where the design wants one object.

- Hidden title bar with inset traffic lights (`titleBarStyle: Overlay`), so the glass runs to the top edge.
- **The left rail is icon-only, `--rail-width` wide**, and it is not a surface: no fill, no card, one
  `hairline` down its right edge separating it from the content. A 180pt labelled sidebar spends a
  fifth of a 960pt window on three words that never change, and the icon plus its tooltip say the
  same thing in a third of the space. The mark sits at the top of the rail — it is the only place in
  the app that says whose product this is.
- **Nav selection is monochrome.** The active item is `--surface-sunken-strong` with `--text-primary`;
  it is never `--accent-soft`. A selected rail item is a place, not a live session, and
  because you clicked Settings is the same lie as a logo that looks live at rest (§12). The same rule
  removes every `hover:accent-soft` in the app: rest, hover and press are transparent,
  `--surface-sunken` and `--surface-sunken-strong` — elevation, not colour.
- **Stats:** the hero number and the activity chart share the one elevated card allowed by §1.2 —
  they are the thing the view exists to show. The 2x3 secondary grid, the language breakdown and the
  latency rows sit directly on the panel below it, separated by spacing and hairlines.
- **History:** a virtualized list — it will reach tens of thousands of rows. Row shows first line of text, then duration · language · word count · relative time. `⌘F` focuses search, `↑`/`↓` navigate, `⏎` copies.
- **Settings:** sectioned, generated from the registry. Each row is label + description + control, right-aligned control, hairline dividers.

---

## 9. Empty and error states

Every list and stat has a designed empty state. A blank panel reads as broken.

- **No transcriptions yet:** the hotkey rendered as a keycap, and one line: "Press ⌥Space anywhere to start."
- **Model downloading:** progress with real bytes and a real estimate. Never an indeterminate spinner for a 574MB download.
- **Permission missing:** a single sentence naming what is missing and one button that deep-links to the right settings pane.
- **Failed transcription:** the reason in plain language, and the audio duration that was captured, so the user knows what was lost.

---

## 10. Accessibility

- Every interactive element reachable by keyboard, with a visible `--accent` focus ring.
- The pill is marked as a status/live region so VoiceOver announces state changes rather than staying silent.
- Text contrast meets 4.5:1 against the *effective* background — glass over a bright desktop is the worst case, which is why `--surface-glass` sits at 55% rather than lower. **Verify against a white desktop background, not a neutral gray one.**
- Full `prefers-reduced-motion` and `prefers-reduced-transparency` support. Under reduced transparency, glass becomes an opaque solid surface — that is a supported look, not a degraded one.

---

## 11. Component metrics

Values the primitives need that the sections above imply but do not state. Same rule: if a component needs a number that is not here, it goes here first.

### Surfaces

```
--border-width-hairline:  0.5px           /* §4 — the hairline, stated once */
--noise-opacity:          0.03            /* §3 */
--noise-frequency:        0.8             /* feTurbulence baseFrequency, §3 */
--glass-backdrop-blur:    24px            /* the non-vibrancy fallback only */
```

### Reduced transparency

Under `prefers-reduced-transparency` glass becomes an opaque solid — a supported look, not a degraded one (§10), which means the opaque colours have to be chosen rather than derived from an alpha at runtime.

```
Light:  --surface-opaque: #F6F6F8    --surface-opaque-elevated: #FFFFFF
Dark:   --surface-opaque: #1E1E20    --surface-opaque-elevated: #2C2C30
```

The dark values are the §2 dark surfaces at full alpha. The light ones are not white: pure white under a hairline border reads as a print layout rather than a window, so the base surface sits a shade below it and elevation is what reaches white.

### Focus

```
--focus-ring-width:   2px
--focus-ring-offset:  2px
```

Colour is `--accent` (§10). Offset rather than inset, so the ring never sits on top of a hairline border and muddies it.

### List rows

```
--row-height:      56px    /* history row: first line of text + a metadata line */
--row-overscan:    6       /* rows rendered outside the viewport, each side */
```

A uniform row height is a constraint, not a limitation: it is what lets a list of tens of thousands of rows resolve its scroll position by arithmetic instead of by measurement. A list that needs variable-height rows needs a different component and a reason.

### Charts

**A chart in this app is data drawn on glass, with no chrome around it.** Default chart-library styling is the single fastest way to make a product look like every other product, because it is literally the same styling every other product ships. So:

- **No gridlines.** None. Not faint ones.
- **No axis lines, no boxed frame, no tick marks.** Labels sit directly on the glass in `--text-tertiary`.
- **No coloured fills and no legend chrome.** A line is a line: 1.5px, `--text-primary` at reduced opacity, drawn on nothing.
- **No animation on load.** The number is the point; a chart that grows into place is a toy.
- **Numbers are tabular** everywhere, so a value does not reflow as it updates.
- Colour appears in a chart only where it is carrying state, which for stats is nowhere.

```
--chart-line-width:    1.5px
--chart-row-height:    32px   /* one categorical row — a bar plus its breathing room */
--chart-label-gutter:  96px   /* left gutter for category labels */
--chart-ink:           the line and bar colour — --text-primary at --chart-ink-opacity
--chart-ink-opacity:   0.72
--chart-height:        96px   /* the activity line's drawing area, labels excluded */
--chart-point-size:    3px    /* radius of the one dot: the most recent value */
--chart-inset:         4px    /* so a point at the top or bottom is not clipped */
```

**No chart library.** Once the rules above are applied there is nothing left for one to do: a bar is
a div with a percentage width and a line is one `<path>`. Shipping 109KB of charting to draw that
buys the default styling every other product also ships, which is the look we are trying to escape.
`recharts` was a dependency for exactly that reason and is no longer used by anything.

### The activity chart

Sessions per day, as **one unadorned line** — the shape of the habit, which is what the number
cannot show. Not a calendar heatmap: a lattice of tinted squares is a borrowed form that says
GitHub before it says anything about this product, and tinting it needs a hue, which at rest the app
does not have (§1.3).

- One `<path>`, `--chart-line-width`, `--chart-ink`, `stroke-linejoin: round`, no fill of any kind.
- **A single dot on the most recent point**, `--chart-point-size`. It is the only mark, and it is
  there because a line's right edge is otherwise ambiguous about whether it ended or ran out.
- Two labels, at the two ends, `--text-tertiary` at caption size. No axis, so no ticks to align to.
- The range is chosen with a segmented control, never a dropdown — the options are three, they are
  mutually exclusive, and a select would hide two of them behind a click.
- **Vertical scale is 0-to-busiest, and it is not stated.** A y-axis label on a chart with no axis is
  the chrome coming back in through the caption.

### Segmented control

```
--segment-height:    28px   /* = --control-height-sm */
--segment-radius:    var(--radius-input)
--segment-padding:   2px    /* the track's inset, so the thumb clears the track edge */
```

Low contrast on purpose: the track is `--surface-sunken`, the selected segment is
`--surface-sunken-strong` with `--text-primary`, and the unselected labels are `--text-secondary`.
Nothing here is accented — picking a date range is not a live session.

### Interaction timings

Not motion — these are how long the interface waits before acting, and they belong here for the same reason a duration does: improvised per view, the app feels inconsistently responsive.

```
--search-debounce:  180ms   /* keystroke to query dispatch */
--feedback-hold:   1200ms   /* how long a confirmation glyph stays after an action */
--titlebar-height:  38px    /* inset traffic lights clear this; drag region fills it */
```

180ms is under the ~250ms at which a person perceives a pause, and long enough that a typed word costs one query rather than five.

### Keycap

```
--keycap-height-sm: 18px   --keycap-height-md: 24px
--keycap-min-width-sm: 18px --keycap-min-width-md: 24px
--keycap-radius: var(--radius-input)
```

Square at minimum so a single glyph (`⌥`) reads as a key rather than as a letter, and widening with content so a word (`Space`) stays one key.

### Spacing, as Tailwind sees it

The §4 scale is a 4px grid, so it is published as Tailwind's spacing base (`--spacing: 4px`) rather than as seven named steps. `p-1 p-2 p-3 p-4 p-6 p-8 p-12` are exactly `--space-1` … `--space-8`. Naming them twice would let two names for 24px both be correct, which is how a scale stops being one.

---

## 12. Identity

The app had no identity, and that is why it read as an MVP: nothing on screen was specific to *this* product.

### The mark

**Onboarding shows the mark on the tour and nowhere else.** The three tour slides introduce the
product; the setup steps that follow do not repeat it. A fourth consecutive screen carrying the logo
turns an identity into a watermark — the "first screen only" rule did not change, the first screen
did. The setup steps also carry **no progress dots**: they are derived from backend state rather than
a fixed-length sequence, so a "1 of 3" was announcing a length that varies per machine, and a second
three-dot row immediately after the tour's read as progress resetting.

**One mark, two renderings, and they are asserted to match by geometry rather than by memory.**
`assets/mark.svg` is the file everything outside the webview consumes — the menu-bar glyph and the
`.icns`/`.ico` bundle icons. The web layer does NOT import it; it draws the same shape from tokens
(`mark-bars`, five bars at ratios 0.25 / 0.5 / 0.75) so a size change stays a token change. The SVG
is 2pt bars, 2pt gaps, heights 6/12/18/12/6 on a 24 grid — the same ratios. Every logo in the
running app is the `Mark` component; there is no second drawing and no image file in `src/`.


**The waveform at rest.** Five rounded vertical bars, centre-weighted, in the proportions the pill already draws while listening — tall in the middle, short at the edges. It is the app's signature object, so the identity costs nothing to introduce and is recognised immediately once the user has seen the pill once.

```
--mark-bar-width:      2px
--mark-bar-gap:        2px
--mark-size-md:        24px    /* the rail, an empty state */
--mark-size-lg:        40px    /* onboarding's first screen */
--mark-ratio-outer:    0.25    /* 6/24  — the two end bars */
--mark-ratio-mid:      0.5     /* 12/24 — the two shoulders */
--mark-ratio-peak:     0.75    /* 18/24 — the centre bar */
```

The heights are published as **ratios of `--mark-size`, not as pixels**, so the mark is one shape at
every size rather than five numbers that have to be re-picked each time it is drawn larger. Bar width
and gap stay fixed: scaling the stroke with the height is what turns a mark into a blob at 40px and a
smear at 16px.

Rendered in `--text-primary`. It was never allowed to be ember, and now nothing is: a logo that looks live when nothing is happening is the same lie as a toggle that says ON while nothing works.

It appears in exactly three places: the **top of the dashboard rail**, **onboarding's first screen** at `--mark-size-lg`, and — as `assets/mark.svg`, the same geometry — the **menu-bar glyph and the bundle icon**. It does NOT go on the "no transcriptions yet" empty state, even though that is the first screen a new user sees: that state's whole job is to say which key to press, and putting a logo above the keycap makes the user read a brand before they read the instruction. An identity repeated on every surface is a watermark, not an identity.

`assets/mark.svg` is `fill="currentColor"` on purpose. As a macOS template image the system recolours it for light, dark and the pressed menu-bar state, so the glyph must carry no colour of its own.

### Voice

Sentence case everywhere — never Title Case, which reads as a marketing site rather than a system tool. Labels are quiet: `--text-secondary` at label size, and never bold. The only heavy weight on a screen is the one number that screen exists to show.

Numbers are the loudest thing in the interface, because the product's whole claim is a measurement. That is why Display size exists and why almost nothing else is above Heading.

### What "premium" means here, concretely

It is not more effects. It is: fewer hues, larger radii, softer shadows, quieter labels, one loud number per screen, and generous spacing that is still grouped tightly enough to read as deliberate. Spread everything out evenly and it stops looking minimal and starts looking unfinished.
