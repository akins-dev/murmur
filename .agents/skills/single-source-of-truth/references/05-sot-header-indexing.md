# Pillar 6 Reference: SOT Header Indexing & Semantic Navigation

/**
 * SOURCE OF TRUTH KEYWORDS: SotHeader, SotIndex, SemanticGrep,
 *   HeaderConvention, ContextOptimization, AgentNavigation
 * WHAT:  Specification and guidelines for the 4-part SOT file header and the
 *        `sot.mjs` keyword navigation CLI tool.
 * WHY:   Keeps AI agent and developer context costs flat (O(1)) by making the
 *        codebase self-indexing without external dependencies.
 * WHERE: Referenced by SKILL.md Pillar 6; executed via scripts/sot.mjs.
 */

## 1. The Context Problem in Agentic Engineering

When an AI agent or human opens a codebase with hundreds of files:
- Searching for generic terms (`Session`, `Handler`, `Config`) returns 80+ files.
- Reading 40 files burns half the context window before writing a single line of code.
- Exhausted agents frequently stop searching and create duplicate parallel types (`SessionData`, `UserContext2`), creating technical debt.

---

## 2. The 4-Part SOT Header Specification

Every authored file **must** begin with a standardized 4-part header:

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

### Anatomy of the 4 Parts:

1. **SOURCE OF TRUTH KEYWORDS**:
   - 5 to 10 specific symbols that this file **owns** (types, structs, functions, enums, constants).
   - **Rule**: Only list symbols *defined* in this file. Never list symbols merely imported or used, or every file matches every query.

2. **WHAT**:
   - Exactly one sentence explaining inputs and outputs.
   - Purpose: Allows a reader or agent to rule the file out without reading its code.

3. **WHY**:
   - The architectural constraint, trade-off, or non-obvious engineering rationale.
   - Example: *"Uses a lock-free ring buffer because the audio callback runs on a realtime thread and cannot allocate."*
   - Avoid stating the obvious (*"Stores session information"*).

4. **WHERE**:
   - The manual call graph and dependency pointers.
   - Identifies:
     - Upstream owners (who calls or instantiates this).
     - Downstream consumers (who reads or reacts to this).

---

## 3. The Navigation CLI (`sot.mjs`)

The codebase includes a zero-dependency script (`scripts/sot.mjs`) to query the index:

### Commands:
```bash
# 1. Print matching files that claim ownership of the keyword
node scripts/sot.mjs SessionState

# 2. Print matching files along with their complete WHAT / WHY / WHERE headers
node scripts/sot.mjs --show SessionState
```

### The 5-Step Navigation Loop:
1. **Grep before you create**: Run `node scripts/sot.mjs <SymbolName>`.
2. **Read the header hits, not the code**: Determine the owner in 5 lines.
3. **Follow WHERE**: Walk the call graph directly.
4. **Extend existing types**: Never create a parallel type.
5. **Always author a header**: If you create a new file, add the SOT header so the next agent finds it.
