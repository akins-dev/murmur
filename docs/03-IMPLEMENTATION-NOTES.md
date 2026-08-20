# Murmur — Implementation Notes

Everything a builder needs that is not obvious from the plan. Most entries here are things that will otherwise cost a day each to rediscover.

**Confidence marking:** entries marked **[VERIFY]** are things to measure on the actual machine before designing around them. Everything else is settled.

---

## 1. Audio capture (cpal)

### 1.1 The realtime callback is sacred

cpal's input callback runs on a CoreAudio realtime thread. Inside it you may **not**:

- allocate (no `Vec::push` that can grow, no `String`, no `format!`)
- take a lock that another thread can hold
- log, or touch the database
- call anything async

Violating this produces audio glitches and dropped buffers that look like a transcription-accuracy problem. Use a **lock-free SPSC ring buffer** (`ringbuf` crate, MIT/Apache-2.0). The callback does one thing: copy samples in. A separate normal-priority thread drains it.

### 1.2 Device format is not what you want

The default input config is commonly 48kHz, and may be stereo, and may be `i16`/`u16` rather than `f32`. Whisper requires **16kHz, mono, f32, normalized to [-1.0, 1.0]**. So the drain thread must:

1. Convert the sample format to `f32`.
2. Downmix to mono (average channels — do not just take channel 0; some interfaces put silence there).
3. Resample to 16kHz with `rubato` (`FastFixedIn` is enough; `SincFixedIn` is higher quality and still cheap at this rate).

Never assume the rate. Read `SupportedStreamConfig` and build the resampler from the actual value. Bluetooth headsets in particular renegotiate to strange rates.

### 1.3 Device disconnection

AirPods disconnecting mid-sentence is a normal event, not an edge case. cpal's error callback fires; the stream is dead and will not recover on its own. Handle it: transition the FSM to a failure state, keep whatever audio was already captured, transcribe it, and deliver it. Losing a sentence because a headset dropped is exactly the failure mode the history feature exists to prevent.

### 1.4 The always-on microphone tradeoff — read this before designing the pre-roll

The technical plan calls for a 500ms pre-roll buffer so the first syllable is never clipped. That requires the mic stream to already be open. **On macOS, an open mic stream lights the orange privacy indicator in the menu bar and lists the app under "Microphone" in Control Center — continuously, whether or not you are recording.**

Most users will find a permanently-lit mic indicator unacceptable in a background app, and they would be right to.

**Resolution: make it a setting, and do not default to always-on.**

- **Default — On-demand.** The stream opens when the hotkey fires. CoreAudio device-open latency is roughly 20–80ms **[VERIFY on the target machine]**. Mitigate by opening the stream on key *down* and starting the FSM immediately, so the open overlaps with the user's reaction time before they actually begin speaking. In practice people do not start talking for ~200ms after pressing a key, which covers it.
- **Optional — Instant mode.** Keeps the stream warm and gives a true pre-roll. The settings toggle must say plainly that the macOS microphone indicator stays lit. Some users will happily take that trade; it must be their choice.

Measure the real device-open latency in phase 1 and record it in this file. If it lands under ~50ms, on-demand is strictly correct and Instant mode is a power-user nicety. If it is over 150ms, on-demand will clip syllables and Instant mode needs to be offered much more prominently during onboarding.

---

## 2. Whisper — the part that decides whether the latency target is met

### 2.1 The 30-second window, and why naive chunking fails

Whisper's encoder always processes a **30-second mel spectrogram**. whisper.cpp pads shorter audio up to 30 seconds. The consequence is brutal and non-obvious:

> **A 1-second chunk costs almost as much to encode as a 25-second chunk.**

So the instinct — "chunk small for low latency" — makes the app dramatically *slower*, because you pay a near-full encoder pass per chunk. Chunking at 1s would be roughly 10× the total compute of chunking at 10s.

**Two things fix this, and both are required:**

**(a) Chunk at 8–15 seconds, at VAD silence boundaries.** Not at 1s. The chunks are transcribed in the background while the user keeps talking, so their individual latency is invisible. Only the *tail* matters.

**(b) Use `audio_ctx` to shrink the encoder for the tail.** `whisper_full_params.audio_ctx` truncates the encoder's context window. The default 1500 corresponds to the full 30 seconds. Setting it proportional to the actual audio length gives a large, roughly linear speedup — this is the same trick whisper.cpp's own `stream` example uses. Scale it to the fragment duration with headroom:

```
audio_ctx ≈ clamp(ceil(duration_seconds / 30 * 1500) + 128, 256, 1500)
```

**Without `audio_ctx`, the final decode is a full 30-second encoder pass (~1.4s on an M2 Pro) and the p50 < 300ms target is unreachable.** This is the single most important implementation detail in the entire project.

**[VERIFY]** Reduced `audio_ctx` can cost accuracy on longer audio. Use the reduced value **only for the trailing fragment**, and keep the full 1500 for the main background chunks where latency is hidden anyway. Measure the WER delta in phase 2 and record it here.

### 2.2 Whisper parameters — get these wrong and it is slow, wrong, or both

| Parameter | Value | Why |
|---|---|---|
| `translate` | **`false`** | Defaults can bite here. If true, every language is translated to English — which silently destroys the entire multilingual requirement. |
| `language` | pinned code, or `auto` | Auto-detect runs on the first window and costs a beat. Pinning is faster and more accurate. |
| `no_context` | **`true`** | Between chunks. Carrying decoder context across chunk boundaries causes repetition loops and hallucination drift. This is the #1 cause of "it repeated the same sentence 40 times". |
| `temperature` | `0.0` | Deterministic. |
| `temperature_inc` | **`0.0`** | **Critical for latency.** By default, whisper retries failed decodes at progressively higher temperatures — up to 6 extra passes. That is a latency bomb: a normal 200ms decode becomes 1.4s with no warning. Disabling it means an occasional lower-quality segment instead of an occasional catastrophic stall. Correct trade for this product. |
| `suppress_blank` | `true` | Reduces empty-output artifacts. |
| `no_timestamps` | `true` | We do not use word timings. Skipping them is free speed. |
| `n_threads` | physical cores − 2 | Leave headroom for the UI and audio threads. Do **not** use logical core count. |
| `print_progress` / `print_realtime` / `print_special` / `print_timestamps` | `false` | Otherwise whisper.cpp writes to stdout on every call. |
| `no_speech_thold` | ~0.6 | Works with the VAD gate against hallucination. |

### `no_timestamps` — the table above was WRONG, and the fix is `false`

**Measured correction. `no_timestamps` must be `false`.** The table's claim that
disabling timestamp tokens is "free speed" is not true, and it is not
half-true — on real speech it is both slower and catastrophically wrong.

Same utterance, same machine:

| `no_timestamps` | Tail decode | Output |
|---|---|---|
| `true` | 340 ms | **1019 characters — the sentence repeated 24 times** |
| `false` | **113 ms** | 41 characters, correct |

**Why**, and this is the part worth carrying to any other whisper.cpp project:
the timestamp tokens are how the decoder knows when to STOP. Remove them and it
runs on, re-emitting the same sentence until it hits the token limit. Normally
whisper.cpp's temperature-fallback loop would notice the repetition and retry —
but §2.2 correctly pins `temperature_inc = 0.0` to kill that latency bomb, so
there is nothing left to break the loop. **The two settings interact, and the
combination that looks fastest on paper is the one that ships a 24× repeated
paste.** Turning one off made the other dangerous, which is exactly the kind of
coupling a parameter table hides.

The consequence downstream is a good one: the assembler DOES get real
per-segment spans, so seam de-duplication can use them. Ours de-duplicates on
text as well, over a bounded window, because that also catches genuine spoken
stutters — but the timestamps are real and available if wanted.

### 2.3 The `initial_prompt` trick — this is how the custom dictionary earns its keep

Whisper accepts an initial prompt (up to ~224 tokens) that biases decoding toward that vocabulary. Feeding the user's dictionary terms in as a prompt makes the model **recognize** them correctly, rather than mis-hearing them and relying on a post-hoc string replacement.

This is far better than replacement alone, because replacement cannot fix "clod code" → "Claude Code" if the model heard something that does not match the pattern at all.

**Implement both layers:**
1. Dictionary terms injected as `initial_prompt` → improves recognition.
2. Post-hoc replacement table → catches what the prompt missed.

Keep the prompt under the token budget — prioritize the most recently used terms if the dictionary is large. A prompt that overflows is silently truncated, which is worse than choosing deliberately.

### 2.4 Hallucination on silence

Whisper reliably invents text when fed silence or noise. The outputs are well-known and repetitive: "Thank you.", "Thanks for watching!", "Please subscribe", subtitle-credit strings, and their equivalents in other languages (this is training-data contamination from subtitled video).

**Three defenses, all needed:**
1. **VAD gate.** Never send a chunk with no detected speech to the model at all. This eliminates most of it.
2. **Blocklist.** A per-language list of known hallucination phrases, dropped when they are the *entire* segment content. Never drop them mid-sentence — "thank you" is a real thing people say.
3. **Thresholds.** A per-segment `no_speech_probability` filter, applied by us on the returned segments.

**Correction, measured during the build — `entropy_thold` is inert here.** This
section originally listed it beside `no_speech_thold` as a third threshold. It
is not one: inside whisper.cpp, `entropy_thold` is only ever consulted as a
trigger for the temperature-fallback loop, and §2.2 disables that loop by
pinning `temperature_inc = 0.0`. With the parameters this app ships, the value
can never fire, whatever it is set to.

It is left set, and documented as inert, so nobody re-derives it as a fix later.
But it must not be counted as a defence — a check that cannot fail reads as
protection on the page and provides none in the product, which is how you end up
believing you have three defences when you have two. The two that actually fire
are the VAD gate and the blocklist, plus our own `no_speech_probability` filter.

### 2.5 Context and state lifecycle

`WhisperContext` holds the model weights and is expensive to create (~1.5s for turbo). Create **one**, at app start, and keep it for the process lifetime. `WhisperState` is per-inference working memory — create one per worker thread and reuse it. Do not create a context per transcription; that is the single easiest way to destroy the latency target.

### 2.6 Core ML — and the first-run trap

whisper.cpp uses the Apple Neural Engine for the encoder when a matching `ggml-<model>-encoder.mlmodelc` sits beside the model file. Prebuilt Core ML encoders including `large-v3-turbo` are published in the `ggerganov/whisper.cpp` Hugging Face repo, so nothing needs converting.

**The trap:** on first load on a given machine, the ANE service compiles the model to a device-specific format. This takes **15–60 seconds**, once. If that happens on the user's first hotkey press, their first impression of a "blazing fast" app is a minute-long hang.

**Therefore:** run a warm-up inference on a short silent buffer **during onboarding**, immediately after download, with honest progress UI ("Optimizing for your Mac — one time, about a minute"). Do the same after any model switch. Never let the ANE compile happen lazily.

Enable `whisper-rs` with both the `metal` and `coreml` features. If the Core ML encoder is missing or fails to load, whisper.cpp falls back to Metal automatically — so treat Core ML as an enhancement, not a hard requirement.

### 2.7 Model assets

Base URL: `https://huggingface.co/ggerganov/whisper.cpp/resolve/main/`

| File | Approx size | Role |
|---|---|---|
| `ggml-large-v3-turbo-q5_0.bin` | ~574MB | Default model |
| `ggml-large-v3-turbo-encoder.mlmodelc.zip` | ~?MB | Core ML encoder for the above |
| `ggml-small-q5_1.bin` | ~190MB | Low-RAM alternative |

Downloads must be **resumable** (HTTP range requests), **checksum-verified** before use, and written to a temp path then atomically renamed. A half-written model file that passes an existence check is a support nightmare — verify by hash, never by presence.

---

## 3. macOS integration — where the unpleasant surprises live

### 3.1 Secure Input will break your paste, silently

When any application has secure text entry enabled — a password field in 1Password, Terminal at a `sudo` prompt, a login screen — macOS sets a system-wide flag and **synthetic keyboard events are blocked**. Your `CGEventPost` returns success and nothing happens.

The user experiences this as "sometimes the paste just doesn't work" and will report it as a random bug.

**Handle it explicitly:** call `IsSecureEventInputEnabled()` before attempting injection. If true, skip the keystroke, deliver to the clipboard only, and tell the user why: *"Text copied — another app is blocking keystrokes (a password field is focused somewhere)."* One line of code, one class of bug report eliminated.

### 3.2 Paste timing — two races, both real

```
write clipboard
   ↓  wait ~30–50ms       ← without this, the target app pastes STALE clipboard content
post ⌘V (keydown + keyup)
   ↓  wait ~100–150ms     ← without this, restoring clobbers the paste in flight
restore previous clipboard
```

Both delays are required. The exact values need measuring **[VERIFY]** — they vary by target app, and Electron apps are the slowest.

`CGEventPost` details that matter:
- Set `CGEventFlags::CGEventFlagCommand` on **both** the keydown and the keyup event. Setting it only on keydown works in some apps and not others.
- Use `CGEventTapLocation::HID` (not `Session` or `AnnotatedSession`) for the widest compatibility.
- The keycode for `V` is `9`.

**Clipboard restore is a setting, defaulting to on.** Some applications read the clipboard lazily, after the paste event — restoring too eagerly breaks those. If a user reports pastes coming through empty in a specific app, turning restore off is the fix.

### 3.3 TCC permissions

**Microphone.** `NSMicrophoneUsageDescription` must be in `Info.plist`. Without it the app does not get denied — it **crashes** on first mic access. Bundled Tauri apps need this added via `tauri.conf.json`'s macOS plist entries.

**Accessibility.** Required for keystroke injection. Check with `AXIsProcessTrustedWithOptions` (passing the prompt option shows the system dialog **once**). Critically: **you cannot re-prompt.** Once denied, the dialog never appears again. The only recovery is deep-linking the user to the settings pane:

```
x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility
```

Build that into the onboarding flow and into the error state, because a meaningful fraction of users will click "Don't Allow" the first time.

### 3.4 TCC will drive you mad during development

TCC grants are keyed to the **bundle identifier plus the code signature**. Ad-hoc signing produces a new signature hash on every build, so **every rebuild resets the Accessibility grant** and the app silently loses permission.

**Mitigation:** create one self-signed code-signing certificate in Keychain Access and sign every dev build with that same identity. The signature stays stable, and TCC stops forgetting. Do this at the start of phase 4, not after losing an afternoon.

### 3.5 The pill window

For an overlay that floats over everything, including fullscreen apps, and never steals focus:

- Style mask: `NSWindowStyleMask::NonactivatingPanel`
- Subclass returning `false` from `canBecomeKeyWindow`
- Window level: `NSStatusWindowLevel` (above normal floating windows, below system alerts)
- Collection behavior: `CanJoinAllSpaces | FullScreenAuxiliary | Stationary`
- `hidesOnDeactivate = false`

`tauri-nspanel`'s `PanelBuilder` handles most of this, including a `no_activate(true)` option that temporarily sets the activation policy to Prohibited during window creation — necessary because the underlying window briefly exists as a normal window and would otherwise steal focus at creation time.

**Ignore mouse events when idle** so the pill never intercepts a click meant for the app underneath.

### 3.6 Dock and activation policy

The app should not appear in the Dock or the ⌘-Tab switcher. Set `LSUIElement` / activation policy `.accessory`.

**Consequence:** when the user opens the dashboard, the window will not come to the front on its own and cannot receive keyboard focus properly. You must explicitly call `NSApp.activate(ignoringOtherApps: true)` when showing it. Without this the dashboard opens behind other windows and feels broken.

---

## 4. Tauri v2 specifics

- **`macOSPrivateApi: true`** in `tauri.conf.json` is required for transparent windows. Vibrancy does not work without it.
- **Transparency needs all three:** the Tauri window `transparent: true`, `macOSPrivateApi: true`, and `html, body { background: transparent }`. Missing any one produces an opaque window and a confusing hour.
- **Tauri v2 uses an ACL/capabilities system.** Every plugin permission must be declared in `src-tauri/capabilities/*.json` or calls fail at runtime with a permission error. This is new in v2 and catches people coming from v1.
- **Multiple windows need multiple Vite entry points.** Configure `build.rollupOptions.input` with separate HTML entries for the dashboard, the pill, and onboarding, so the pill does not ship the dashboard's bundle.
- **tauri-specta v2** uses `Builder::new().commands(collect_commands![...])` and exports bindings from a test or build step. Wire the export so it runs automatically — bindings that are regenerated manually will drift.
- **Updater signing:** the public key goes in `tauri.conf.json`; the private key and its password go in CI as `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Generate with `pnpm tauri signer generate`. **Back the private key up** — losing it means no existing installation can ever be updated again.

---

## 5. Concurrency model — pick this deliberately, once

Three thread domains, and they must not blur:

| Domain | What runs there | Rule |
|---|---|---|
| CoreAudio realtime thread | cpal input callback | Copy into ring buffer, nothing else (§1.1) |
| ASR worker (dedicated `std::thread`) | whisper inference | **Never** on tokio. A 200ms blocking inference on a tokio worker stalls every IPC command. |
| Tokio runtime | IPC commands, DB, events | Never blocks |

**Own the session state in an actor, not a mutex.** A single task owns the FSM and receives commands over a bounded `mpsc` channel; everything else sends messages. `Arc<Mutex<SessionState>>` shared across the audio drain, the ASR worker, the hotkey handler and the IPC layer is how you get a deadlock at 2am that reproduces once a week. The actor pattern makes that structurally impossible and costs nothing here.

Use **bounded** channels everywhere. An unbounded queue in front of an ASR worker turns a transient slowdown into unbounded memory growth.

---

## 6. SQLite notes

- Enable `PRAGMA journal_mode = WAL` and `PRAGMA busy_timeout = 5000` on connection.
- At this scale a single connection behind a mutex is genuinely fine — do not build a pool for a single-user local app.
- Migrations: `PRAGMA user_version`, applied in order, each in a transaction. No migration framework needed.
- FTS5 with `content=sessions` requires **triggers** to stay in sync on insert/update/delete. An external-content FTS table that silently drifts is worse than no search.
- Write the session row **before** transcription starts, then update it. That is what makes crash recovery possible.

---

## 7. Testing

- **Reference corpus.** Mozilla Common Voice clips are CC0 and cover Hindi, Arabic, Spanish, French, Italian and more. Keep a small fixed set (2–3 clips per language) with expected transcripts, for accuracy regression tests. Do not commit large audio to git — fetch on demand into a gitignored fixtures directory.
- **Rule pipeline: table tests.** Every enhancement rule gets input/output pairs. This is the cheapest, highest-value test suite in the project — rules are pure functions.
- **FSM: exhaustive transition tests**, including "kill the process in state X, relaunch, assert recovery".
- **Latency benchmark** over the fixed corpus, printing p50/p95 per stage. Runs in CI, fails on a 15% p95 regression.
- **Do not write tests for whisper's accuracy itself.** Test *your* pipeline. Model accuracy is the model's business.

---

## 8. Development environment prerequisites

- Xcode Command Line Tools (`xcode-select --install`)
- Rust stable (`rustup`)
- Node 20+ and pnpm
- CMake (for whisper.cpp) — `brew install cmake`

**The first `cargo build` compiles whisper.cpp and takes several minutes.** This is normal and not a hang. Subsequent builds are incremental. Do not add a build timeout shorter than ~10 minutes in CI.

---

## 9. Open questions — answers as they are measured

Measured on the build machine: **M4 Max, macOS 15.7.3, `large-v3-turbo-q5_0`, Metal.**

**2. Tail-decode time with reduced `audio_ctx` — ANSWERED. The trick works, and it is worth 5.5×.**

| `audio_ctx` | Tail decode |
|---|---|
| 256 (scaled to the fragment) | **53 ms** |
| 1500 (full 30s window) | 294 ms |

Measured realtime factor: **34.8×**. Both numbers assume the language is pinned — see below, because that turns out to matter more than `audio_ctx` does.

**2b. Pin the language before the tail. This is the single biggest latency finding.**

| Tail decode, after an interior chunk | |
|---|---|
| Language pinned | **54 ms** |
| `LanguageHint::Auto` | **333 ms** |

`Auto` on its own misses the p50 < 300 ms budget on the tail decode alone — and `Auto` is the `Default` impl, so the naive wiring is the slow one.

**Cause**, and it is not guessable: whisper.cpp assigns `state->exp_n_audio_ctx` **after** it runs the language-detection encoder pass. So detection reuses the *previous* call's `audio_ctx` — and after a full-context interior chunk, that is 1500. The tail pays a full 30-second encode purely to work out what language it is in, discarding the entire benefit above.

**Therefore: detect once on the first interior chunk, then pin for the rest of the session, including the tail.** `TranscriptSegment.language` already carries the detected code, so this costs nothing but the sequencing. Auto-detect stays available; it just stops being paid for on the critical path.

**5. Core ML — ANSWERED: it is a net LOSS for this model. Keep it off.**

Not "not worth it" — actively harmful, for a reason that has nothing to do with the Neural Engine being slow:

- The `.mlmodelc` is compiled for a **fixed 3000-frame input**, so it is **incompatible with a reduced `audio_ctx`** — which is the entire latency strategy. Worse, it does not error: it returns an **empty transcript**, with no guard and no warning.
- Falling back to full context to use it puts the tail at **469 ms**, versus 53 ms on Metal alone.
- And the encoder is a **1.17 GB** download on top of the model.

It stays behind an off-by-default cargo feature. Anyone who turns it on gets a slower app and a silent failure mode, so the flag exists to be *findable*, not to be used.

**1. CoreAudio device-open latency** — still to measure, and it needs a human. It is measurable in-process (the `DeviceOpen` stage is already timed into `session_metrics`), so the honest way to answer it is to use the app for a day and read the number off the dashboard's latency panel rather than to synthesise it. Drives the §1.4 on-demand vs Instant default: under ~50 ms on-demand is strictly correct; over ~150 ms and Instant mode needs to be offered prominently during onboarding.

**3. Optimal chunk length** — 8–15s in use and behaving; not yet swept for the compute/tail balance.

**4. Real paste-timing delays** across TextEdit, Terminal, Slack, VS Code and a browser — still to measure, and this one CANNOT be automated: it requires dictating into each app and watching what arrives. Defaults are 40 ms / 150 ms, which is the middle of the range that works across native apps, Terminal and Electron. Both are exposed as advanced settings precisely because no single number fits every target, so a user who hits it can fix their own case immediately. The symptom to watch for is a paste arriving with the PREVIOUS clipboard contents (raise `output.paste_delay_ms`) or arriving empty in one specific app (turn off `output.restore_clipboard`).

**What could not be verified without a person.** The end-to-end tests replay synthesised speech through a fake `AudioSource`, which exercises the chunker, the VAD, the engine, the assembler, the enhancement rules, the database and the FSM. What they do NOT exercise is the CoreAudio driver itself and the real synthetic-paste path into a third-party app. Those two need a human with a microphone, and no amount of test scaffolding substitutes for it.
