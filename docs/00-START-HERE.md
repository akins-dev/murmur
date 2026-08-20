# Murmur — start here

A local-first, free, blazing-fast speech-to-text app for macOS. Press a hotkey, talk, press it again, and your words are pasted where you were typing.

## Two kinds of file, and they are not interchangeable

`../CLAUDE.md` is the **rulebook**. Terse, no rationale, no technical values —
"when you see X, do Y". Read it every session; it is what you are held to.

`docs/` is **context**. It exists so you understand the project well enough to
build it. It is not an implementation guide and it is not a checklist — nothing
here is a set of steps to transcribe. Read it once at the start, and again
before you touch an area you have not touched before.

## Read order

| Doc | What it answers |
|---|---|
| `01-IDEATION.md` | What the product is, every feature, what is deliberately not built |
| `02-TECHNICAL-PLAN.md` | The stack, the architecture, the build phases, locked decisions |
| `03-IMPLEMENTATION-NOTES.md` | **The gotchas.** Whisper params, macOS TCC, paste races, audio threading. Read before writing code in those areas. |
| `04-DESIGN-SYSTEM.md` | Exact tokens, materials, motion, the pill spec |
| `05-PROJECT-STRUCTURE.md` | The full tree, where things go, naming, hard limits |
| `06-CONVENTIONS-AND-GREP.md` | The SOURCE OF TRUTH header system and how navigation works. **Read before writing your first file.** |

## Five-minute version

Read `06` in full — it is short, and it is the mechanism everything else relies
on. Then `02` §2 (the architecture in one page) and `02` §7 (latency
engineering). Those two sections explain why every other decision is what it is.

## The three things that decide whether this succeeds

1. **Transcribe during recording, not after.** Latency must be flat with utterance length. `02` §7.
2. **`audio_ctx` scaled to the tail fragment.** Without it, p50 < 300ms is unreachable. `03` §2.1.
3. **The registry and the command factory.** One place for feature metadata, one place for cross-cutting concerns. Everything else is a plug-in. `02` §3–4.

## Locked decisions

- macOS only for the MVP
- Free signing path (ad-hoc + minisign updates); Apple credentials drop in later as two secrets
- Escape destroys a recording immediately — no tombstone, no purge job
