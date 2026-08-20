# Murmur — Conventions, and how navigation actually works

`../CLAUDE.md` is the rulebook and it is deliberately terse — "when you see X, do
Y", with no explanation. This document is the explanation. It covers the one
mechanism the whole codebase leans on: a search index written in comments.

---

## 1. The problem this solves

An agent opening this repository has no memory of it. The naive way to find
something is to read the tree until you recognise it, and that has two costs
that compound: it burns the context window on files you did not need, and it
ends with a duplicate anyway, because reading forty files is tiring and writing
a fresh `SessionState` is not.

That is how a codebase ends up with `Session`, `SessionInfo`, `SessionData` and
`SessionRecord`, none of which agree on the field set, and all of which have to
be kept in sync forever by people who do not know the other three exist.

The fix is to make the codebase describe itself, in a format grep can read.

## 2. The header

Every file opens with one. Every non-obvious block gets one too.

```rust
/**
 * SOURCE OF TRUTH KEYWORDS: SessionMachine, SessionState, TransitionError,
 *   CancelPending, Finalizing, transition, can_transition, SessionEvent
 * WHAT:  The finite state machine governing one recording session.
 * WHY:   Recording state lives in exactly one place so illegal states are
 *        unrepresentable and every transition is logged and persisted.
 * WHERE: Owned by session/actor.rs; driven by ipc/commands/session.rs and
 *        the hotkey handler; read by the pill via session:state-changed.
 */
```

Four lines, each doing a different job:

**KEYWORDS** is the index. 5–10 specific symbols this file *owns* — types,
functions, constants, variants. Specific is the whole point: `Session` matches
everything and therefore finds nothing. If a symbol is defined here, it belongs
on this line; if it is merely used here, it does not, or every file becomes a
match for every query.

**WHAT** is one sentence: inputs and outputs. It exists so a reader can rule the
file out without opening it.

**WHY** is the valuable one, and the one people skip. It records the constraint
that forced the choice — the thing that is invisible in the code and expensive
to rediscover. "Uses a lock-free ring buffer because the audio callback runs on
a realtime thread and cannot allocate" is worth a day to whoever reads it next.
"Copies samples into a buffer" is worth nothing; the code already said that.

**WHERE** is the call graph, by hand. It turns a file into a node with edges, so
you can walk the system by following pointers instead of by searching four more
times.

## 3. The two commands

```bash
pnpm sot <keyword>       # → the list of files whose SOT line mentions it
pnpm sot:show <keyword>  # → the same, with each matching header printed
```

`pnpm sot` is `grep -l` with the index applied: it answers *where could this
live*, and it answers it in one screen instead of forty. You then narrow to the
one or two files that look right and read only those. That is the entire
navigation model, and it is why context cost stays flat as the codebase grows.

`pnpm sot:show` is for when the filename is not enough to choose — it prints the
WHAT/WHY/WHERE of each hit so you can pick without opening anything.

These two commands are the only scripts in the repository, and they are the
authorised exception to the no-scripts rule. They are read-only, they add
nothing to a build, and they are load-bearing: the header convention is only
worth writing because something reads it.

## 4. The working loop

1. **Grep before you create.** Always. `pnpm sot SessionState` before writing a
   session type, `pnpm sot AudioChunk` before writing a chunk type.
2. **Read the header of the hits, not the files.** The header tells you whether
   it is the thing you meant.
3. **Follow WHERE** if you need to understand how it is used.
4. **Extend what you found.** If the existing thing is nearly right, make it
   right. A new parallel type is almost never the answer.
5. **If nothing matched, create it — with a header.** Otherwise you have added a
   file that is invisible to everyone after you, and the next agent writes it
   again.

Two files, two minutes, no scrolling.

## 5. Why the rules in CLAUDE.md are shaped the way they are

The header system only pays off if a few other things hold, and each rule in the
rulebook is there to protect it:

**One place per concept** (`registry/`, `types/`, `ports/`, `services/`). The
index is useless if the same idea is authored in three places — grep returns
three files and you have to diff them to find out which one is real. The layer
rules exist so that "where does this go" has exactly one answer, and so that
answer is guessable before you search.

**The registry, and the command factory.** These are where the app's
cross-cutting truth is centralised. A capability entry is a single row that
wires up settings UI, permission preflight, nav, hotkeys and metrics at once;
the factory is a single function every command passes through, so validation,
permissions, tracing, errors and metrics have one implementation instead of one
per handler. Both exist so that adding feature number forty is an entry and a
handler, not a new pattern and six new places to remember.

**Ports and adapters.** Anything third-party or swappable sits behind a trait
that declares its capabilities. Callers branch on what an adapter *can do*, never
on which adapter it is, which is what keeps a second engine from rippling
through the call sites.

**No bypasses.** `any`, `unwrap()`, `expect()`, `@ts-ignore` — each one is a
place where the type system stopped describing the program, and the header index
is only trustworthy while the code is. A codebase that lies in its types will
lie in its comments soon after.

**Generated types are never edited.** Everything crossing IPC is derived from
the Rust type. Editing the generated file makes the two disagree silently, which
is precisely the failure the single-source-of-truth architecture exists to make
impossible.

## 6. What goes where

`../CLAUDE.md` — rules. Terse, no rationale, no technical values. Read every
session.

`docs/` — context. What the product is, why the architecture is what it is, what
the gotchas are, what the values are. Read once at the start, and again before
touching an area you have not touched.

**Docs are not an implementation guide.** They are here so you understand the
project well enough to build it, not so you can transcribe them. If a doc starts
reading like a set of steps to follow, it has drifted — the steps belong in the
code, and the code belongs behind a SOURCE OF TRUTH header where the next agent
can find it.
