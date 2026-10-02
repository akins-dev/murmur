# Murmur (Windows)

Press a key, talk, press it again — your words are on the clipboard and pasted before you can look up.

Local-first, blazing-fast speech-to-text for Windows. No account, no subscription, no word cap, and no audio ever leaves your machine.

---

> **Attribution & Lineage**:
> Murmur was initially created by [Web Prodigies](https://github.com/webprodigies/murmur) for macOS.
> This repository is a clone and port of that original project, focused on bringing full native Windows compatibility and ongoing improvements on top of it.

---

## What it does

- **Global hotkey** — `Alt+Space` anywhere (configurable). Toggle, or hold-to-talk (push-to-talk).
- **Transcribes while you speak**, not after you stop, so finishing is as fast after a five-minute monologue as after a five-second one.
- **Pastes into whatever had focus**, and restores your previous clipboard content afterwards.
- **Escape to cancel**, with a three-second countdown. Press Escape again and the recording resumes with nothing lost.
- **99 languages**, auto-detected or pinned.
- **A custom dictionary** for names, acronyms, and jargon the model gets wrong — used both to bias recognition and correct output.
- **Searchable local history**, so a failed paste is never a lost thought.
- **Honest stats**, including the p50/p95 latency the app actually achieves.

Everything runs locally on your PC. The only network requests Murmur ever makes are the first-run model download and an optional update check — both are visible and both can be turned off.

---

## Requirements

- **Operating System**: Windows 10 or Windows 11 (64-bit)
- **Disk Space**: ~600 MB of free disk space for the default Whisper model
- **Audio Input**: Any working microphone or headset

---

## Running it

### Prerequisites

- [Node.js](https://nodejs.org/) (v18+) and [pnpm](https://pnpm.io/)
- [Rust](https://rustup.rs/) (`x86_64-pc-windows-msvc` toolchain)
- Visual Studio C++ Build Tools / Desktop development with C++

### Development

```powershell
pnpm install
pnpm tauri dev
```

First launch opens setup:
1. Verify microphone access
2. Download the local Whisper model
3. Test your hotkey (`Alt+Space`)

After setup, Murmur minimizes quietly into your **Windows System Tray** (Notification Area). You will not see a window again unless you open the dashboard or settings.

---

## Building a release

To build a production Windows installer:

```powershell
pnpm tauri build
```

The bundle lands in `src-tauri/target/release/bundle/nsis/` as a standalone Windows NSIS setup installer (`.exe`).

---

## Windows Architecture & Compatibility

Murmur was re-engineered for Windows native support:

- **Audio Capture**: Uses [CPAL](https://github.com/RustAudio/cpal) (Cross-Platform Audio Library) with WASAPI for low-latency, glitch-free microphone capture on Windows.
- **Transcription**: Powered by [whisper.rs](https://github.com/tazz4843/whisper-rs) (GGML Whisper engine), executing CPU/GPU inference entirely on your hardware.
- **Input Injection**: Employs Windows `SendInput` API and the Windows clipboard (`arboard`) to paste transcription output directly into the active foreground window without requiring macOS accessibility grants.
- **System Tray**: Native Windows Taskbar Notification Area integration.
- **Permissions**: Desktop apps on Windows have direct access to audio devices and keyboard hooks. If microphone permissions are disabled at the OS level, Murmur provides direct links to the Windows Settings app (`ms-settings:privacy-microphone`).

---

## For developers

The architecture is documented in `docs/`, and `CLAUDE.md` is the project rulebook.
Start with `docs/00-START-HERE.md` and read `docs/06-CONVENTIONS-AND-GREP.md`.

Navigate the codebase with the source-of-truth grep index:

```bash
pnpm sot SessionState      # which files own this symbol
pnpm sot:show AudioChunk   # the same, with each file's header
```

### TypeScript Bindings & Typechecks

Types cross to TypeScript through `tauri-specta`. The TypeScript definitions in `src/lib/bindings.ts` are generated from Rust code:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml   # regenerates bindings
pnpm typecheck
```

---

## Acknowledgments & Credits

- Originally conceived and created by [Web Prodigies](https://github.com/webprodigies/murmur).
- Windows compatibility, audio layer, and Windows platform adapter improvements by [Akins-dev](https://github.com/akins-dev).

---

## License

MIT License. See [LICENSE](LICENSE).
