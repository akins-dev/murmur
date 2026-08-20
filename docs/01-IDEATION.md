# Murmur — Ideation

**One line:** Press a key, talk, press it again — your words are on the clipboard and pasted before you can look up.

**Status:** BUILT. Every feature in §4.1 and §4.2 is implemented. This document
remains the product definition — it says what Murmur is and, just as usefully,
what it deliberately is not. Read it before adding anything, because §4.4 is the
list of things that will otherwise creep back in.

---

## 1. The problem

Typing is the bottleneck between a thought and an AI agent. Prompts are long, iterative, and conversational — exactly the shape of speech, and exactly the wrong shape for a keyboard. Existing fixes all fail on at least one axis:

| Tool | Why it fails |
|---|---|
| macOS built-in dictation | Mediocre accuracy, weak punctuation, no history, no control |
| Wispr Flow / Superwhisper (cloud tiers) | Subscription, word caps, audio leaves the machine |
| Raw whisper.cpp CLI | Not a product — no hotkey, no paste, no history, no UI |
| Browser-based STT | Wrong context — you need it in your terminal, your IDE, everywhere |

**Murmur's position:** the accuracy and polish of the paid tools, running entirely on your machine, free forever, with no cap and no account.

## 2. Non-negotiables

These are the constraints the whole design bends around. If a decision violates one of these, the decision is wrong.

1. **Perceptually instant.** Stop-key to pasted-text must feel like zero. Target **p50 < 300ms, p95 < 600ms**, and — critically — **flat with respect to utterance length**. A 90-second monologue must finalize as fast as a 5-second one.
2. **Fully local.** No network calls in the transcription path, ever. The only network the app makes is a first-run model download and an update check, both visible and both disableable.
3. **Free and uncapped.** No account, no subscription, no telemetry, no usage meter. MIT-licensed.
4. **Cannot lose your words.** Every session is persisted to disk the moment audio exists. If the app crashes, the paste fails, or the target window disappears, the transcript is recoverable from history.
5. **Invisible until summoned.** It lives in the menu bar. The only thing you ever see is a small pill, and only while you are talking.

## 3. Core flow (the MVP)

```
  ⌥Space ──▶ [pill fades in] ──▶ you talk ──▶ ⌥Space ──▶ text pasted
                    │                                        │
                    │                              (also on clipboard,
                    │                               also in history)
                    ▼
                  Esc ──▶ 3s countdown ring ──▶ discarded
                    │
                  Esc again ──▶ countdown aborts, recording resumes
```

**The details that matter:**

- **Pre-roll buffer.** The mic stream is already open and buffering ~500ms *before* you press the key. The first syllable is never clipped — the single most common failure of every dictation tool.
- **Transcription happens while you talk.** Audio is transcribed in chunks at natural silence boundaries during recording, not after it. When you press stop, only the trailing fragment is left to decode. This is the entire reason the latency target is achievable and length-independent.
- **Two hotkey modes.** Toggle (press to start, press to stop) and push-to-talk (hold to record, release to send). Both configurable.
- **Escape semantics.** First Esc arms cancellation and shows a countdown ring in the pill. Audio *keeps recording* during the countdown. A second Esc aborts the cancellation and resumes seamlessly — nothing was lost. Countdown expiry **discards permanently** — the audio, the partial transcript, and the row are all destroyed, and nothing is written to history. Escape means gone. The countdown is the safety net; there is no second one.
- **Paste, don't just copy.** Text lands in the clipboard *and* is pasted into whatever had focus. The previous clipboard contents are restored afterward.

## 4. Features

### 4.1 MUST HAVE — this is the MVP, none of it is optional

| # | Feature | Notes |
|---|---|---|
| M1 | Global hotkey, toggle + push-to-talk | Rebindable; conflict detection against system shortcuts |
| M2 | Local streaming transcription | Chunked during recording; flat latency |
| M3 | Escape-to-cancel with countdown + double-Escape resume | Exactly as specified above |
| M4 | Clipboard + auto-paste with clipboard restore | Falls back to clipboard-only if Accessibility is denied |
| M5 | Grammar, punctuation & capitalization pass | Deterministic, sub-millisecond — see §5 |
| M6 | Multilingual with auto-detect | Hindi, Arabic, Spanish, French, Italian, German, Portuguese, Japanese, Chinese, Korean, Russian + all remaining Whisper languages |
| M7 | Local history, searchable, one-click re-copy | SQLite; survives crashes; recoverable. Delivered and failed sessions only — cancelled sessions are never persisted |
| M8 | Glassmorphic pill overlay | Live waveform, elapsed time, state, cancel ring. Never steals focus |
| M9 | Dashboard: stats + history + settings | The only "real" window |
| M10 | Settings page | Hotkeys, model, language, paste behavior, dictionary, launch-at-login, history retention |
| M11 | First-run onboarding | Permission requests, model download with progress, hotkey test |
| M12 | Menu bar presence | Status glyph, quick toggle, open dashboard, quit |
| M13 | In-app updates | Signed, from GitHub Releases, with release notes and a "later" option |
| M14 | Engine adapter layer | Swap the ASR engine behind one interface — see §6 |
| M15 | Crash-safe session recovery | Orphaned recordings finish transcribing on next launch |

### 4.2 SHOULD HAVE — I strongly recommend these for v1.0; they are what separate a demo from a product

| # | Feature | Why it earns its place |
|---|---|---|
| S1 | **Custom dictionary / replacements** | Every user has 20 terms the model gets wrong — names, product names, jargon. A replacement table fixes 90% of perceived accuracy problems for near-zero engineering cost. Highest value-per-line-of-code feature in the entire app. |
| S2 | **Spoken formatting commands** | "new line", "new paragraph", "comma", "period", "quote". Essential for dictating structured prompts to an agent. Toggleable. |
| S3 | **Filler word removal** | Strips "um", "uh", "like", "you know". Makes voice output read like writing. Toggleable. |
| S4 | **Per-app profiles** | Different behavior in Terminal vs Slack vs an IDE — e.g. filler removal always on in Slack, formatting commands on in the terminal. Detects the frontmost app. |
| S5 | **Audio feedback** | Two subtle sounds (start / delivered). Without them you don't trust that the hotkey registered. Toggleable. |
| S6 | **Pinned language vs auto-detect** | Auto-detect costs a beat of latency and occasionally misfires between similar languages. Pinning your primary language is faster and more accurate. Both supported. |
| S7 | **Model manager** | Download / delete / switch models from Settings with disk usage shown. Users on 8GB machines need the small model; users on an M4 Max want the big one. |
| S8 | **History export** | JSON / Markdown / plaintext. It's your data — no lock-in, and it costs a day. **An export always covers everything, never the current search.** An export that silently honoured a filter hands someone a partial archive they believe is complete, which is worse than no export at all — this is the one feature whose whole purpose is that their transcripts are not trapped in here. For the same reason the write goes through a native save dialog in Rust rather than a webview download: a sandboxed download that fails does so silently, and a person who believes they have a backup and does not is worse off than one who knows they have none. |
| S9 | **Retention policy** | Auto-purge history after N days, plus one-click "delete everything". Privacy tool, not a feature. |
| S10 | **Latency telemetry (local)** | p50/p95 shown in the dashboard. This is how you *prove* the speed claim rather than assert it, and how you catch a regression. |

### 4.3 NICE TO HAVE — post-1.0, explicitly out of MVP scope

| # | Feature | Note |
|---|---|---|
| N1 | Local LLM cleanup pass | Rewrites rambling speech into clean prose via a small local model (Qwen3-0.6B class). Adds 200–500ms, so it must be opt-in and per-profile. The adapter seam for this ships in the MVP; the implementation does not. |
| N2 | Live partial text in the pill | Words appear as you speak. Beautiful demo, real complexity, zero functional value since you only need the final text. Deliberately deferred. |
| N3 | Voice commands | "Murmur, clear that" without touching the keyboard. |
| N4 | Snippets / macros | "insert my signature" expands a stored block. |
| N5 | Meeting / long-form mode | Record for an hour, transcribe with speaker turns, save as a document. Different product shape — separate mode, not the pill. |
| N6 | Windows + Linux | **Decided: out of MVP scope.** The architecture is portable by design; only the injection and overlay layers are platform-specific. See technical plan §14. |
| N7 | Apple Speech + Parakeet engines | Extra engines behind the existing adapter. Parakeet is faster on English/European but has **no Hindi or Arabic**, so it can never be the default. |
| N8 | Cloud engine adapters | Deliberately last, deliberately opt-in, deliberately off. The adapter exists so this is possible; the philosophy says don't. |

### 4.4 EXPLICIT NON-GOALS

Naming these now prevents scope drift later.

- No account, no login, no sync, no cloud backup.
- No paywall, no tiers, no license keys, no usage counting.
- No analytics or crash reporting that leaves the machine.
- No text-to-speech. No translation. No speaker diarization in v1.
- No mobile app. No browser extension. No web version.
- No "AI assistant" features. It transcribes. That's the product.

## 5. Accuracy: what "grammatically correct" actually means

The requirement is that output reads like writing, not like a transcript. Three layers deliver it, in ascending cost:

**Layer 1 — the model (free, already happening).** Whisper large-v3-turbo emits punctuation and casing natively. It is trained on punctuated text; this is not a bolt-on. This alone gets you ~85% of the way.

**Layer 2 — deterministic post-processing (<1ms, always on).** A pure Rust pipeline of ordered, individually-toggleable rules: sentence-start capitalization, terminal punctuation, whitespace and quote normalization, custom dictionary replacement, filler removal, spoken-command expansion, number/unit normalization, de-duplication of stutters at chunk seams. Deterministic means testable — every rule gets a table test, so accuracy never silently regresses.

**Layer 3 — local LLM rewrite (200–500ms, opt-in, post-MVP).** For when you want rambling turned into prose. The seam ships in the MVP as a `TextEnhancer` port; the implementation is N1.

**The honest note:** Layer 2 is where the perceived quality lives, and the custom dictionary (S1) is the single highest-leverage item in it. A model that writes "clod code" instead of "Claude Code" feels broken no matter how good its WER is.

## 6. Why an adapter layer, concretely

Today: one engine (Whisper via whisper.cpp), one enhancer (rules), one injector (macOS paste).

Tomorrow, any of these plugs in without touching a single caller:

- **Apple Speech** — free, on-device, instant, weaker punctuation.
- **NVIDIA Parakeet** — faster than Whisper on English and 25 European languages, **no Hindi, no Arabic**. Good as an opt-in speed engine, disqualified as a default.
- **A future local model that doesn't exist yet** — the actual reason to build the seam.
- **A cloud engine** — for someone who wants it, off by default, and physically incapable of running unless explicitly selected.

Each engine **declares its own capabilities** — supported languages, streaming support, measured real-time factor, whether it needs a download, whether it runs offline. The UI reads those declarations. This is what stops the app from shipping a dead button: if you pin Hindi and select Parakeet, the settings page tells you why it's unavailable instead of failing silently at record time.

## 7. The dashboard

Not a big app. A single window with three sections, all frosted glass.

**Stats** — the proof the product works:
- Time saved (words ÷ your typing speed − time actually spoken). Configurable baseline WPM, defaults to 40.
- Total transcriptions, total words, total minutes spoken — lifetime and this week.
- Your speaking WPM vs your typing WPM, as a single satisfying number.
- Activity heatmap by day, streak counter.
- Language breakdown.
- Latency p50/p95 — the honest engineering metric.

**History** — searchable list, keyboard-navigable, click to copy, showing duration, language, word count, and timestamp. Failed and undelivered sessions appear too, flagged — that is the entire point of having history. Cancelled sessions do not, by design.

**Settings** — §4.1 M10.

## 8. Design language

Minimal, glassmorphic, Apple-native in feel. Real `NSVisualEffectView` vibrancy — actual system blur, not a CSS approximation. Thin hairline borders, generous whitespace, one accent color, SF-family typography, spring physics on every transition, full light/dark support driven by the system.

**The pill** is the signature object: a small floating capsule, roughly 180×44pt, bottom-center. Live waveform, elapsed timer, a state dot. It floats over fullscreen apps, it never takes focus, and it never appears in the app switcher or the Dock. When cancellation is armed, the waveform is replaced by a countdown ring that drains — and a second Escape refills it.

## 9. The one place "free" has a limit

Everything in this stack is free: Tauri, Rust, whisper.cpp, the model weights, SQLite, GitHub Releases for distribution and updates. Zero recurring cost, zero API keys, zero infrastructure.

**The exception is Apple notarization.** To distribute to *other people* without them seeing a Gatekeeper warning, Apple requires a Developer Program membership at $99/year. Without it, the app builds and runs perfectly on your machine, and other people can run it — but the first launch requires a right-click → Open, and the download carries an "unidentified developer" warning.

This does not affect building or using the app. It affects distribution polish only.

**Decided: take the free path.** Ship ad-hoc signed with minisign-signed updates. Adding Apple credentials later is two GitHub secrets and one workflow flag — the pipeline is built so nothing needs restructuring when you choose to.

## 10. What success looks like

- You reach for the hotkey instead of the keyboard without deciding to.
- You never once open the history — but the one time something fails, it's there.
- The latency number in the dashboard stays under 300ms after six months of features.
- Someone else installs it and doesn't ask you how to use it.
