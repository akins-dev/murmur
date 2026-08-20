# Murmur — Technical Plan

Companion to `01-IDEATION.md`. Implementation-level gotchas live in `03-IMPLEMENTATION-NOTES.md`; visual values in `04-DESIGN-SYSTEM.md`; the file tree in `05-PROJECT-STRUCTURE.md`. That document says what to build. This one says how, and why each choice is the one that survives contact with feature #40.

---

## 1. Tech stack

Every line is free, permissively licensed, and self-hosted. No API keys, no accounts, no recurring cost.

### Shell & core

| Concern | Choice | License | Why this and not the alternative |
|---|---|---|---|
| Desktop shell | **Tauri v2** | MIT / Apache-2.0 | ~10MB bundle vs Electron's ~150MB, uses the system WebView, Rust core. Confirmed as your preference and it is the correct one. |
| Core language | **Rust (stable)** | — | Audio + ASR is a realtime path. GC pauses in that path are audible as latency jitter. |
| UI | **React 19 + TypeScript + Vite** | MIT | You already work this way. Vite HMR keeps the frontend loop fast. |
| Styling | **Tailwind CSS v4** | MIT | v4's CSS-variable-native theming is exactly what a token-driven glass design needs. |
| Components | **shadcn/ui (Radix primitives)** | MIT | Copy-in, not a dependency. Matches your existing convention. |
| Client state | **Zustand** | MIT | The frontend holds almost no state — Rust is the source of truth and pushes events. A reducer framework here would be ceremony. |
| Charts | **Recharts** | MIT | Dashboard only, lazy-loaded, off the hot path. |
| Typed IPC | **tauri-specta v2 + specta** | MIT | **This is the tRPC analog.** Rust command signatures generate `bindings.ts` at build time. One source of truth, no DTOs, no hand-written IPC types, and a renamed Rust field breaks the TypeScript build. |

### Audio & inference

| Concern | Choice | License | Why |
|---|---|---|---|
| Capture | **cpal** | Apache-2.0 | The Rust standard for cross-platform audio input. CoreAudio on macOS. |
| Resampling | **rubato** | MIT | Device rate (44.1/48kHz) → 16kHz mono, which is what Whisper requires. |
| Ring buffer | **ringbuf** | MIT/Apache-2.0 | Lock-free SPSC. The cpal realtime callback cannot allocate or lock — see `03 §1.1`. |
| VAD | **earshot** (pure-Rust WebRTC VAD) | Apache-2.0/MIT | Used only for chunk boundaries and silence trimming — a low-accuracy job. Pure Rust means no ONNX runtime and no second model to download. Silero can be swapped in behind the port if it ever proves necessary. |
| ASR runtime | **whisper-rs 0.16** → whisper.cpp | MIT/Unlicense | Mature bindings, Metal + Core ML feature flags. Metal alone gives ~9× realtime for large-v3-turbo on M3; Core ML moves the encoder to the Neural Engine for a further 2–3×. |
| Default model | **ggml-large-v3-turbo-q5_0** (~574MB) | MIT | Best accuracy-per-millisecond available locally. ~3.7% WER, 99 languages including Hindi and Arabic. |
| Fallback model | **ggml-small-q5_1** (~190MB) | MIT | For ≤8GB machines. Selectable in Settings. |
| Storage | **rusqlite** (bundled SQLite) | MIT | Single file, zero-config, ACID. `bundled` means no system SQLite dependency. |
| Clipboard | **arboard** | MIT/Apache-2.0 | Reliable, handles the restore-previous-contents dance. |
| Keystroke injection | **core-graphics** (CGEvent) | MIT/Apache-2.0 | Direct synthetic ⌘V. Lower level and more reliable than a cross-platform wrapper for the one gesture we need. |

### Platform integration

| Concern | Plugin | Note |
|---|---|---|
| Global hotkey | `tauri-plugin-global-shortcut` | Escape is registered **only while recording** and unregistered immediately after, so it is never stolen from other apps. |
| Floating pill | Tauri window APIs + `window-vibrancy` | **Changed during the build.** The plan called for `tauri-nspanel`; it is not published on crates.io and would have meant depending on a git branch for the app's most visible surface. The same guarantees come from the platform directly: activation policy `Accessory` (no Dock icon, the app never activates), `always_on_top` + `visible_on_all_workspaces`, and — the one that actually settles it — `set_ignore_cursor_events(true)`. A window that cannot be clicked cannot steal a click meant for the app underneath, and cannot take focus by click. The window is created once at launch and only shown and hidden, never recreated, so no hotkey press ever pays a webview's parse cost. |
| Launch at login | `tauri-plugin-autostart` | Off by default; onboarding offers it. |
| Updates | `tauri-plugin-updater` | Minisign-signed manifests, GitHub Releases as host. Both free. |
| Logging | `tracing` + `tracing-appender` | Rolling local files, never transmitted. |

**Total recurring cost: $0.** The only optional cost is Apple notarization ($99/yr) for warning-free distribution to third parties — see §12.

---

## 2. The architecture in one page

Two ideas carry the whole codebase, mirroring the guardrail boilerplate.

**One: a single file describes every feature.** `src-tauri/src/registry/mod.rs` is the analog of `resources.ts`. It declares every capability, setting, hotkey, permission requirement, dashboard nav item, and metric the app has.

**Two: a single factory builds every IPC command.** `src-tauri/src/ipc/factory.rs` is the analog of `protectedProcedure`. It reads the registry and wires up validation, permission preflight, error mapping, tracing, and metrics. Handlers contain only business logic.

```
┌─────────────────────────────────────────────────────────────┐
│ UI          React · Zustand · generated bindings.ts          │
│             <CapabilityGate> reads the registry              │
├─────────────────────────────────────────────────────────────┤
│ IPC         ipc/commands/*.rs — business logic only          │
│             ipc/factory.rs   — validation, permissions,      │
│                                errors, tracing, metrics      │
├─────────────────────────────────────────────────────────────┤
│ Domain      session/machine.rs — the recording FSM           │
│             pipeline/*.rs      — capture → vad → asr →       │
│                                  enhance → inject            │
├─────────────────────────────────────────────────────────────┤
│ Ports       ports/*.rs — TranscriptionEngine, TextEnhancer,  │
│             AudioSource, TextInjector, ModelStore            │
├─────────────────────────────────────────────────────────────┤
│ Adapters    adapters/whisper/, adapters/rules/,              │
│             adapters/cpal/, adapters/macos/                  │
├─────────────────────────────────────────────────────────────┤
│ Services    services/*.rs — pure SQLite access, one verb     │
│                             per function, no business rules  │
├─────────────────────────────────────────────────────────────┤
│ Infra       registry/, config/, error/, db/, telemetry/      │
└─────────────────────────────────────────────────────────────┘
```

Dependencies point **downward only**. A service never calls a port. A port never knows an adapter exists. This is enforced mechanically — see §10.

---

## 3. The registry — the single source of truth

`src-tauri/src/registry/mod.rs`. One `CAPABILITIES` table. Adding an entry wires up eight things at once, exactly as one `RESOURCES` entry does in the guardrail.

```rust
pub struct Capability {
    pub key: CapabilityKey,
    pub name: &'static str,
    pub description: &'static str,

    /// Settings this capability owns. Drives the Settings UI — form
    /// controls are generated from these, never hand-written per feature.
    pub settings: &'static [SettingDef],

    /// OS permissions required before it can run. Drives onboarding,
    /// the preflight in the command factory, and the "needs setup" badge.
    pub requires: &'static [OsPermission],

    /// Engine features it depends on. Reading this is what stops the UI
    /// offering Hindi on an engine that cannot speak it.
    pub engine_needs: &'static [EngineFeature],

    /// Dashboard placement. Present ⇒ it appears in the nav.
    pub nav: Option<NavDef>,

    /// Metrics it emits. Declared here so the dashboard cannot read a
    /// metric nothing writes, and nothing can write a metric that is
    /// never surfaced.
    pub metrics: &'static [MetricDef],

    /// Default hotkey, if it binds one. Conflict detection reads this.
    pub hotkey: Option<HotkeyDef>,
}
```

**What one entry buys you, for free:**

1. A typed `CapabilityKey` constant, usable from Rust and TypeScript.
2. Generated Settings UI (control type, label, validation, default, reset).
3. Permission preflight in the command factory — no per-handler check.
4. A dashboard nav entry.
5. Registered + conflict-checked hotkey.
6. Declared metric names, wired to the stats queries.
7. Engine-compatibility gating in the UI.
8. A TypeScript mirror through specta, so the frontend reads the same table.

**The rule:** if you find yourself writing a `match` on a feature name anywhere outside `registry/`, the branch belongs in the registry instead.

---

## 4. The command factory — the analog of `protectedProcedure`

Every IPC command is built through one function. Nothing calls `#[tauri::command]` directly.

```rust
command!(
    name = "start_recording",
    capability = CapabilityKey::Dictation,
    input = StartRecordingInput,   // validated before the handler runs
    handler = |ctx, input| async move {
        ctx.session.start(input.mode).await
    }
);
```

What the factory does on every call, so no handler ever does it again:

| Step | Behavior |
|---|---|
| 1. Deserialize + validate | Input schema is the source of truth. Invalid input never reaches a handler. |
| 2. Permission preflight | Reads `capability.requires`. Missing mic or Accessibility returns a typed, actionable error, never a panic. |
| 3. Reentrancy guard | Per-capability. A double-fired hotkey cannot start two recordings — a whole class of race conditions eliminated in one place. |
| 4. Tracing span | Structured, with a correlation id that follows the session end-to-end. |
| 5. Handler | Your logic. Nothing else. |
| 6. Error mapping | Every error becomes `AppError { code, message, recoverable, action }`. The UI renders it uniformly. `unwrap()` is banned by lint. |
| 7. Metric recording | Duration + outcome, against the metrics the registry declared. |

`AppError` is the only error type crossing the boundary. There is one error surface in the UI, not forty.

---

## 5. Ports and adapters

Five traits are the entire third-party contract. Everything swappable lives behind one of them.

```rust
#[async_trait]
pub trait TranscriptionEngine: Send + Sync {
    fn capabilities(&self) -> EngineCapabilities;
    async fn prepare(&self) -> Result<()>;                      // warm the model
    async fn transcribe(&self, chunk: AudioChunk, hint: LanguageHint)
        -> Result<TranscriptSegment>;
}

pub struct EngineCapabilities {
    pub id: EngineId,
    pub languages: LanguageSupport,      // All | Set(&[Lang])
    pub streaming: bool,
    pub realtime_factor: f32,            // measured, not claimed
    pub requires_download: bool,
    pub runs_offline: bool,
}
```

Plus `TextEnhancer`, `AudioSource`, `TextInjector`, `ModelStore`.

**Three rules, taken directly from the guardrail's provider seam:**

1. **Never branch on an engine's name.** Branch on `capabilities()`. `engine.id == "whisper"` hard-wires you to one engine; `caps.streaming` does not.
2. **Declare capabilities, don't assume them.** Parakeet has no Hindi. The UI must learn that from the declaration, not from a failed transcription at 2am.
3. **Only the selected adapter is constructed.** Adapters are behind Cargo features and built by a factory function. An unselected engine's model never loads and never occupies RAM.

**Ships in the MVP:** `WhisperEngine`, `RuleEnhancer`, `CpalAudioSource`, `MacosInjector`, `HttpModelStore`.
**The seam exists for:** Apple Speech, Parakeet, a local-LLM enhancer, and any cloud engine — each of which becomes a new file plus one `match` arm, and touches nothing else.

---

## 6. The session state machine — where "no failure opportunity" is won

Recording state is **one** explicit FSM in `session/machine.rs`. Not booleans scattered across modules. Illegal states are unrepresentable, and every transition is logged.

```
                ┌──────┐
       hotkey   │ Idle │◀──────────────────────┐
          │     └──┬───┘                       │
          ▼        │                           │
      ┌────────┐   │ (preflight fails)         │
      │ Arming ├───┘                           │
      └───┬────┘                               │
          │ mic live, pre-roll attached        │
          ▼                                    │
    ┌───────────┐   Esc    ┌───────────────┐   │
    │ Recording │─────────▶│ CancelPending │   │
    │           │◀─────────┤  (countdown)  │   │
    └─────┬─────┘  Esc     └───────┬───────┘   │
          │ hotkey                 │ expiry    │
          ▼                        ▼           │
    ┌────────────┐           ┌───────────┐     │
    │ Finalizing │           │ Destroyed ├─────┤
    └─────┬──────┘           └───────────┘     │
          ▼                                    │
    ┌───────────┐  paste ok / fallback         │
    │ Delivered ├──────────────────────────────┘
    └───────────┘
```

**Guarantees this buys:**

- **Audio never stops during `CancelPending`.** Double-Escape resumes with zero gap because nothing was ever torn down.
- **Every state change writes to SQLite before it takes effect.** A crash in any state leaves a recoverable row — except `Destroyed`, whose whole job is to remove it.
- **`Finalizing` has a hard timeout** (default 8s). On expiry it delivers whatever chunks are complete rather than hanging. Partial delivery beats a spinner.
- **`Delivered` is reached even when paste fails.** Clipboard-only is a successful outcome with a different notification, not an error.
- **Only one session can exist.** Enforced by the type system, not by a flag.

---

## 7. Latency engineering — the part everything else serves

The naive design transcribes after you stop, so latency scales with utterance length. A 60-second recording takes ~2.8s to decode on an M2 Pro. That is a completely different product from the one specified.

**The fix: transcribe during recording.**

```
audio in ──▶ ring buffer ──▶ VAD finds a silence boundary
                                      │
                                      ▼
                         chunk closed, queued to the ASR worker
                                      │
                    (decoded on a background thread while you keep talking)
                                      ▼
                              segments accumulate

  you press stop ──▶ only the trailing fragment is left (typically <1.5s)
                     ──▶ decode it ──▶ join ──▶ enhance ──▶ paste
```

**Budget, from stop-keypress to pasted text:**

| Stage | Target |
|---|---|
| Hotkey event → FSM transition | < 5ms |
| Trailing fragment decode (warm context, `audio_ctx` scaled to fragment length — see `03 §2.1`) | 80–250ms |
| Segment join + seam de-duplication | < 2ms |
| Enhancement pass (deterministic rules) | < 1ms |
| Clipboard write | < 5ms |
| Synthetic ⌘V + target app render | 30–80ms |
| **Total** | **p50 < 300ms, p95 < 600ms** |

**And it is flat.** A 5-second utterance and a 5-minute one finalize in the same time, because the only work left at stop is the tail.

**The five things that make or break this:**

1. **The model is warm before you press the key.** Loaded and its Metal context initialized at app start. Cold-loading large-v3-turbo costs ~1.5s — that must never be on the hotkey path. The Core ML/ANE compile (15–60s, once per machine) happens during onboarding, never lazily — see `03 §2.6`.
2. **The mic is opened without clipping the first syllable.** Note the conflict resolved in `03-IMPLEMENTATION-NOTES.md §1.4`: an always-open stream gives a true pre-roll buffer, but it also keeps the macOS orange microphone indicator lit permanently, which is unacceptable as a default for a background app. Resolution: **on-demand by default** (stream opens on key-down, overlapping the ~200ms before a person actually starts speaking), with an opt-in "Instant mode" that keeps the stream warm and says plainly that the indicator stays on.
3. **The ASR worker is a dedicated thread with a bounded queue.** Never on the UI thread, never on Tauri's async runtime.
4. **Chunk seams overlap by 200ms** and are de-duplicated on join, so a word split across a boundary is not lost or doubled.
5. **Latency is measured, not assumed.** Every stage timestamps into `session_metrics`. The dashboard shows p50/p95. A regression is visible the day it lands, not six months later.

**Measured at every build.** A benchmark over a fixed audio corpus runs in CI; a p95 regression over 15% fails the build.

---

## 8. Data model

One SQLite file at `~/Library/Application Support/com.murmur.app/murmur.db`, WAL mode, versioned migrations applied by `PRAGMA user_version`.

```sql
sessions (
  id TEXT PRIMARY KEY,                -- correlation id, shared with logs
  started_at INTEGER NOT NULL,
  ended_at INTEGER,
  state TEXT NOT NULL,                -- delivered | failed | orphaned
  duration_ms INTEGER,
  language TEXT,
  engine_id TEXT NOT NULL,
  model_id TEXT NOT NULL,
  raw_text TEXT,                      -- pre-enhancement, for debugging accuracy
  final_text TEXT,
  word_count INTEGER,
  app_bundle_id TEXT,                 -- which app had focus, powers per-app profiles
  delivery TEXT,                      -- pasted | clipboard_only | none
  error_code TEXT
);

session_metrics (session_id, stage TEXT, duration_ms INTEGER);
settings        (key TEXT PRIMARY KEY, value TEXT NOT NULL);  -- JSON, registry-validated
dictionary      (id, pattern, replacement, match_kind, enabled);
app_profiles    (bundle_id, settings_json);

CREATE VIRTUAL TABLE sessions_fts USING fts5(final_text, content=sessions);
```

**Decisions worth stating:**

- **Audio is not retained by default.** Transcribe, then discard the samples. A setting can keep the last N recordings for debugging; it is off, and it is clearly labelled.
- **`raw_text` is kept alongside `final_text`.** Without it you cannot tell whether a bad result came from the model or from your own rules.
- **Cancelled sessions are destroyed immediately.** *(Decided.)* On countdown expiry the FSM deletes the in-flight row, drops the audio buffer, and discards every decoded segment in the same transition — there is no tombstone, no soft-delete, no purge job to trust. Escape means gone. This is why the countdown ring exists and why double-Escape resumes losslessly: the confirmation window is the only safety net, so it has to be real.
- **FTS5 for search**, because scanning grows unusably slow past a few thousand rows and it costs one table.

---

## 9. Frontend architecture

```
src/
  app/
    dashboard/            # the real window
      stats/  history/  settings/
    pill/                 # the overlay — its own entry point, its own tiny bundle
    onboarding/
  components/
    global/               # reusable anywhere: GlassPanel, StatCard, Waveform,
                          # SettingControl, DataTable, CountdownRing
    ui/                   # shadcn primitives
  lib/
    bindings.ts           # GENERATED by tauri-specta — never hand-edited
    registry.ts           # generated mirror of the Rust registry
    events.ts             # typed subscriptions to Rust-emitted events
  stores/                 # Zustand: UI state only
```

**Rules, carried over from your existing conventions:**

- **The pill is a separate entry point.** Its bundle contains no charts, no tables, no router — it must paint in under one frame.
- **Rust owns all domain state and pushes it.** The frontend subscribes to typed events. It never polls, and it never holds a second copy of the truth.
- **Settings controls are generated from the registry.** Adding a setting is a registry entry, not a new form component.
- **No hardcoded colors, ever.** Semantic tokens only (`bg-surface`, `text-muted`, `border-hairline`). Glass materials are tokens too: `glass-pill`, `glass-panel`, `glass-elevated`.
- **`any` is a build error.** Every IPC type comes from `bindings.ts`.

**Glass implementation:** real `NSVisualEffectView` vibrancy underneath (via `window-vibrancy`), with the web layer transparent and contributing only a hairline border, a subtle inner highlight, and a noise texture at ~3% opacity. CSS `backdrop-filter` is the fallback on platforms without native vibrancy — it is a fallback, not the primary, because it looks like a simulation of glass rather than glass.

---

## 10. Conventions — how this stays clean at feature #40

1. **SOURCE OF TRUTH KEYWORDS header on every module**, in the format your guardrail already uses (`KEYWORDS / WHAT / WHY / WHERE`), so the next agent greps `-l` instead of reading the tree. This is the single practice that keeps context cost flat as the codebase grows.
2. **Grep before you create.** Types, helpers, and components almost certainly already exist.
3. **Types live in one place per side** — `src-tauri/src/types/` and `src/lib/bindings.ts` (generated). Never inline, never duplicated.
4. **Services are pure.** One verb, one table, no business rules, no orchestration.
5. **Business logic lives in commands and the pipeline**, never in services, never in components.
6. **No `unwrap()`, no `expect()`, no `panic!` outside tests.** Clippy-enforced at deny level.
7. **`any`, `as any`, and `@ts-ignore` are build errors.**
8. **Layer boundaries are enforced mechanically**, by a `cargo-deny`-style import check in CI. A service importing a port fails the build rather than being caught in review.
9. **No scripts, no seed files, no scratch files in the repo.** Nothing that could not be shipped.
10. **Barrel exports** (`mod.rs`, `index.ts`) at every folder boundary.
11. **One file, one responsibility** — and a hard ceiling: if a module exceeds ~400 lines, it is doing two things.

---

## 11. Build phases

Each phase ends in something runnable. Nothing is stubbed forward.

**All twelve phases are complete.** The table is kept as the record of what was
built in what order and why that order was chosen — a later feature that wants
to skip a step should have to argue with it. Where reality corrected the plan,
the correction is recorded in `03-IMPLEMENTATION-NOTES.md` rather than being
quietly edited into the plan, so the disagreement stays visible.

| # | Phase | Deliverable | Definition of done |
|---|---|---|---|
| **0** | Foundation | Tauri v2 + React + Tailwind v4 scaffold, specta bindings pipeline, SQLite + migrations, error type, tracing, registry skeleton, layer-boundary lint | `pnpm tauri dev` opens a glass window; `bindings.ts` regenerates on Rust change; CI green |
| **1** | Audio spine | cpal capture, resampling, ring buffer, pre-roll, VAD chunking, `AudioSource` port | A command records 5s and reports correct duration, RMS, and chunk boundaries |
| **2** | ASR core | whisper-rs + Metal, model download with progress + checksum, warm-load at start, `TranscriptionEngine` port, chunked streaming worker | End-to-end transcription of a fixed corpus, with a latency benchmark printed |
| **3** | Session FSM | The full state machine, SQLite persistence at every transition, crash recovery, hard timeouts | Every transition unit-tested including kill-during-each-state recovery |
| **4** | Hotkeys & injection | Global shortcut, toggle + push-to-talk, dynamic Escape registration, clipboard + ⌘V injection with restore, Accessibility fallback | Hotkey → speak → hotkey → text appears in TextEdit, Slack, and Terminal |
| **5** | Enhancement | Rule pipeline, custom dictionary, spoken commands, filler removal, seam de-duplication, `TextEnhancer` port | Table tests per rule; measured under 1ms end-to-end |
| **6** | The pill | NSPanel overlay, vibrancy, waveform, timer, countdown ring, state animations | Floats over fullscreen, never steals focus, absent from Dock and app switcher |
| **7** | Dashboard | Stats with real aggregates, history with FTS search + copy, activity heatmap, latency panel | Every number traced to a real query — no placeholder data anywhere |
| **8** | Settings & onboarding | Registry-generated settings, model manager, hotkey capture with conflict detection, permission flow, launch-at-login | Fresh-machine install to first transcription with no documentation |
| **9** | Multilingual | Language pinning + auto-detect, per-language config, engine capability gating in the UI | Hindi, Arabic, Spanish, French verified against reference audio |
| **10** | Hardening | Failure-injection tests, memory-leak soak, latency regression gate, error-surface audit | 30-minute soak with 200 sessions: no leak, no orphan, no unhandled error |
| **11** | Release | DMG, minisign keys, updater manifest, GitHub Actions release workflow, README, LICENSE (MIT) | Install a build, ship a change, receive the update in-app |

Phase 2 is the risk concentration. If measured latency there misses the target, the design decision to revisit is chunk size and model choice — both isolated behind the port, so it is a tuning problem, not a rewrite.

---

## 12. Updates & distribution

**Mechanism.** `tauri-plugin-updater` checks a `latest.json` manifest on GitHub Releases at launch and every 24h. Bundles are signed with a minisign keypair — the private key lives in GitHub Actions secrets, the public key is compiled into the app. An unsigned or mis-signed bundle is refused. Updates download in the background, and the user is prompted with release notes and a "later" that is genuinely a later.

**Release flow.** Tag `v1.2.3` → GitHub Actions builds universal (arm64 + x86_64), signs, publishes the DMG and manifest. Free on public repositories.

**Decided: free path now.** Ship ad-hoc signed, add Apple credentials whenever you want them. The workflow already passes every Apple secret through and Tauri skips signing and notarization while they are empty, so turning it on is adding secrets — no new step, no flag, no restructure.

**Two things must be settled BEFORE the first public tag. Both become permanent afterwards.**

1. **The updater private key has no passphrase, and it exists on exactly one machine.** `~/.murmur-updater.key`, chmod 600, gitignored. Its public half is compiled into every binary that ships, so after the first release the keypair can never be rotated for installs already in the wild — losing this file strands every existing installation on whatever version it has, permanently, with no recovery path. It belongs in a password manager, and adding a passphrase is free today and impossible later.

2. **`plugins.updater.endpoints` is a guess.** It points at `github.com/webprodigies/murmur`, inferred rather than read, because the project has no git remote to read one from. If the real slug differs, the update check will silently find nothing — no error, no prompt, just an app that never updates and never says why. One value, and it has to be right before the first tag rather than after it.

Releases are published as **drafts**, so a broken build is never what every install auto-downloads.

**The notarization wall, stated plainly.** Distributing to third parties *without* Gatekeeper warnings requires an Apple Developer membership ($99/yr) for signing and notarization. Without it:

- On your own machine: no impact whatsoever.
- For others: first launch needs right-click → Open, and the download shows an "unidentified developer" warning.
- Updates still work — the updater's minisign signing is independent of Apple's and stays free.

The build pipeline is written so that adding credentials later is two GitHub secrets and one workflow flag. Nothing needs restructuring, and nothing needs deciding today.

---

## 13. Risks, and what is already done about them

| Risk | Mitigation |
|---|---|
| Latency misses the target | Measured from phase 2, not phase 11. CI gate at 15% p95 regression. Chunk size, model, and engine are all tunable behind the port. |
| Model download is 574MB | Resumable, checksum-verified, with a small model offered as an alternative. Shown in onboarding with honest progress. |
| Accessibility permission denied | Clipboard-only is a first-class success path, not an error. The app is fully useful without it. |
| Hotkey conflicts with another app | Conflict detection at bind time against known system shortcuts, with a clear rebind prompt. |
| Whisper hallucinates on silence | VAD gates the input — silent chunks are never sent to the model, which is where hallucinated text comes from. |
| Memory growth over a long session | Bounded ring buffer, bounded worker queue, explicit context reuse. Soak-tested in phase 10. |
| macOS version drift (NSPanel, vibrancy) | The overlay is behind `TextInjector`/window abstractions; a CSS-blur fallback path exists. |
| Feature creep collapsing the architecture | The registry plus the layer-boundary lint. New features are registry entries, not new patterns. |

---

## 14. Cross-platform, for later

**Decided: macOS-only MVP.** Everything above `ports/` is platform-neutral. Windows and Linux need exactly three new adapters — `TextInjector` (SendInput / XTest), the overlay window, and the permission model — plus swapping Metal for CUDA/Vulkan or CPU in whisper.cpp. That is roughly a week's work *because* the seam exists, and it is not in the MVP.

---

## 15. Decisions locked

| Decision | Choice | Consequence |
|---|---|---|
| Platform scope | **macOS only** | Metal, NSPanel, and CGEvent paths taken directly. Ports still keep Windows/Linux a ~1-week adapter job, not a rewrite. |
| Code signing | **Free path now** | Ad-hoc signed DMG, minisign-signed updates. Workflow written so Apple credentials drop in as two secrets and one flag. |
| Cancelled sessions | **Destroyed immediately** | No tombstone, no purge job. The countdown ring is the only safety net, so it is built to be genuinely reliable. |

Anything not listed here I decide during the build and record in this document as it lands.
