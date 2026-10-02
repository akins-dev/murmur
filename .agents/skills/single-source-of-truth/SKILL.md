---
name: single-source-of-truth
description: Single Source of Truth (SOT) and Vertical Slice Matrix Architecture. Enforces centralized declarative registries, cross-cutting command execution factories, pure finite state machines (effects as data), ports & adapters (hexagonal), pure data-access services, and comment-based search indexing with compile-time reachability and upward-import guardrails. Use when designing, architecting, refactoring, or navigating codebases to eliminate duplicate state, prevent structural rot, and keep AI navigation context cost flat.
---

# Single Source of Truth (SOT) & Vertical Slice Matrix Architecture

This skill defines the universal **Single Source of Truth (SOT) and Vertical Slice Matrix Architecture** — a battle-tested engineering blueprint designed to eliminate duplicate state, eradicate cross-cutting boilerplate, prevent structural rot, and keep AI/developer context consumption flat ($O(1)$) as codebases grow to hundreds of thousands of lines.

---

## 1. Core Axioms & The Problems Solved

Traditional software architectures frequently decay into a **"ball of mud"**:
1. **Type Proliferation & Inconsistency**: As features expand, developers and AI agents create parallel types (`Session`, `SessionInfo`, `SessionData`, `SessionRecord`), none of which agree on field sets, causing endless synchronization bugs.
2. **Scattered Cross-Cutting Concerns**: Input validation, permission preflights, reentrancy guards, error mapping, and telemetry get copy-pasted across dozens of endpoints. Over time, half the endpoints miss permission checks or use incompatible error representations.
3. **Ghost Features**: Settings, metrics, or capabilities are declared in UI forms or configs, but no active runtime code ever reads or acts upon them. The user toggles a feature, gets a success message, but application behavior never changes.
4. **Fragile Mock-Heavy Tests**: Side-effects are tangled with business decisions, forcing tests to construct elaborate mocks of networks, databases, and OS APIs that break upon the slightest refactoring.
5. **Context Window Exhaustion**: An AI agent opening a codebase with dozens of directories must read 40+ files just to deduce where a concept lives, burning token context and frequently giving up to write duplicate types.

### The Guiding Axioms

- **Axiom 1: Exactly One Place Per Concept.** Every feature metadata, data schema, domain state, SQL query, and capability contract has exactly one owner in the codebase.
- **Axiom 2: Declarative Over Imperative.** Capabilities, permissions, settings schemas, and telemetry metrics are declared in static data structures, not scattered in conditional code.
- **Axiom 3: State Decisions Return Effects as Data.** Business logic is pure. Given an input event and state, it returns the next state and a list of effect descriptions. No side-effects inside decision branches.
- **Axiom 4: The Boundary Is A Factory.** All boundary requests (IPC, HTTP, CLI, RPC) pass through a single uniform pipeline executing validation, permissions, locking, tracing, and error mapping.
- **Axiom 5: Third-Party Dependencies Are Trait Ports.** Third-party libraries and OS primitives never penetrate business logic. They are hidden behind capability traits. Exactly one bootstrap file instantiates concrete adapters.
- **Axiom 6: The Codebase Indexes Itself.** Every file carries a standardized 4-part comment header. A zero-dependency script indexes symbols, making codebase navigation instant and context-flat.
- **Axiom 7: Architecture Is Enforced In CI, Not In Code Reviews.** Import directions and capability reachability are verified by automated tests. A rule claimed in documentation and unenforced in CI is technical debt.

---

## 2. The Six Architectural Pillars

```
   ┌─────────────────────────────────────────────────────────────┐
   │                  BOUNDARY (IPC / HTTP / CLI)                │
   │  ┌───────────────────────────────────────────────────────┐  │
   │  │   PILLAR 2: Operational SOT (Command Factory)         │  │
   │  │   Validate → Permissions → Reentrancy → Trace → Map   │  │
   │  └──────────────────────────┬────────────────────────────┘  │
   └─────────────────────────────┼───────────────────────────────┘
                                 │
   ┌─────────────────────────────▼───────────────────────────────┐
   │                        DOMAIN LAYER                         │
   │  ┌─────────────────────────────┐  ┌──────────────────────┐  │
   │  │ PILLAR 3: Pure FSM          │  │ PILLAR 1: Registry   │  │
   │  │ (State, Event) -> (S, [Eff])│  │ Capabilities, Schemas│  │
   │  └──────────────┬──────────────┘  │ Reachability Check   │  │
   │                 │ [Effects]       └──────────────────────┘  │
   │                 ▼                                           │
   │  ┌─────────────────────────────┐                            │
   │  │ Actor Execution Loop        │                            │
   │  └──────────────┬──────────────┘                            │
   └─────────────────┼───────────────────────────────────────────┘
                     │
   ┌─────────────────▼───────────────────────────────────────────┐
   │           PILLAR 4: PORTS & ADAPTERS (Hexagonal)            │
   │  Ports (Pure Traits) ◄─── Adapters (Whisper, CPAL, HTTP)    │
   │  Bootstrap: The ONLY place naming concrete adapters         │
   └─────────────────┬───────────────────────────────────────────┘
                     │
   ┌─────────────────▼───────────────────────────────────────────┐
   │           PILLAR 5: PURE SERVICES (Data Layer)              │
   │  Pure SQL: 1 Verb, 1 Table. Parameters in, rows out.        │
   │  No business logic. No inter-service calls.                 │
   └─────────────────────────────────────────────────────────────┘
   
   ┌─────────────────────────────────────────────────────────────┐
   │  PILLAR 6: NAVIGATION SOT (Comment Search Index sot.mjs)    │
   │  KEYWORDS: 5-10 symbols | WHAT: 1 line | WHY: Constraints   │
   │  WHERE: Call graph | Fast O(1) agent navigation             │
   └─────────────────────────────────────────────────────────────┘
```

### Pillar 1: Declarative Central Registry (Functional SOT)
All feature definitions, permissions, setting schemas, engine requirements, and telemetry metrics are declared in a single static data structure (`registry/mod.rs` or `registry/capabilities.ts`).
- **Zero Conditional Feature Strings**: Components and handlers do not `match` or `if/else` on hardcoded feature strings. They query the registry.
- **Dynamic Client Generation**: UI settings views, navigation bars, and hotkey listeners are rendered dynamically by iterating over registry declarations. Adding feature #40 is a single entry in the registry table; zero UI code is written.
- **Reachability Verification**: An automated test scans the entire codebase to assert that every setting declared is actively consumed by domain logic, and every metric declared is actively recorded.

### Pillar 2: Command Execution Factory (Operational SOT)
All entry points (IPC commands, HTTP endpoints, CLI actions) pass through a single higher-order execution function (`execute<I, O>()`):
1. **Schema Validation**: Validates payload structure and constraints before business logic runs.
2. **Permission Preflight**: Checks required OS/application permissions against the registry.
3. **Reentrancy & Concurrency Guarding**: Applies named locks or mutexes to prevent race conditions on critical operations.
4. **Distributed Tracing & Telemetry**: Starts structured tracing spans with correlation IDs and measures execution duration.
5. **Handler Execution**: Invokes the thin domain handler.
6. **Canonical Error Mapping**: Intercepts domain errors, logs root causes securely, and translates them into a single typed `AppError` contract crossing the boundary.
*Handlers contain ZERO validation, permission, or error serialization boilerplate.*

### Pillar 3: Pure State Machine & Event-Driven Domain (Domain SOT)
Domain state is governed by a pure Finite State Machine (FSM):
```
transition(State, Event) -> Result<(NextState, Vec<Effect>), TransitionError>
```
- **Effects as Data**: Side-effects (disk writes, network calls, audio recording, IPC notifications) are returned as **data structures** (`Effect::PlaySound(SoundId)`, `Effect::PersistTranscript(id)`), NEVER executed inside the transition.
- **100% Deterministic Testing**: Unit tests pass events into states and assert the exact next state and list of effects. Zero mocks, zero timeouts, zero async flakes.
- **The Actor Pattern**: An asynchronous Actor owns the state machine, listens on an event channel, performs the returned effects sequentially or concurrently, and pushes typed state-change events out to clients.
- **No Client State Duplication**: Clients never duplicate or predict domain state. The backend pushes typed events (`session:state-changed`). Clients are reactive renderers.

### Pillar 4: Ports & Adapters Boundary (Hexagonal Architecture)
Third-party libraries, OS drivers, hardware interfaces, and network services are strictly isolated:
- **Ports (`ports/`)**: Pure traits/interfaces declaring capability contracts (`AudioCapturePort`, `SpeechEnginePort`, `KeyInjectionPort`). Contains zero implementation code and zero third-party dependencies.
- **Adapters (`adapters/`)**: Concrete implementations of ports (`CPALAudioAdapter`, `WhisperCppAdapter`, `MacOsKeyInjector`). Isolated in their own folders.
- **Single Bootstrap Point (`bootstrap.rs` / `bootstrap.ts`)**: Exactly one file in the entire application names concrete adapters and wires them to ports. Swapping Whisper for Deepgram or CPAL for CoreAudio requires touching exactly one file.

### Pillar 5: Pure Data Services (Data Access Layer)
The database layer (`services/`) adheres to strict transactional boundaries:
- **Rule of Simplicity**: One verb, one table. Parameters in, rows out.
- **No Business Logic**: Services contain zero branching on business rules, zero permission logic, and zero orchestration.
- **No Inter-Service Calls**: `users_service` NEVER calls `orders_service`. Orchestration belongs strictly in the domain layer or pipeline.

### Pillar 6: Comment-Based Search Index (`sot.mjs` & SOT Headers)
Every authored file opens with a standardized 4-part SOT header comment block:
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
- **CLI Navigation Tool**: A zero-dependency script (`sot.mjs`) parses headers across the project.
  - `sot <keyword>`: Returns only the files that declare ownership of the symbol.
  - `sot --show <keyword>`: Prints the WHAT/WHY/WHERE blocks directly to the terminal.
- **Flat Context Scaling**: When navigating or adding features, developers and AI agents do not search entire codebases. They run `sot <symbol>`, inspect 1-2 headers, and touch only the exact files required.

---

## 3. The Vertical Slice Matrix & Layer Hierarchy

The architecture balances horizontal technical layers with vertical feature slices:

```
               VERTICAL FEATURE SLICES
             [Dictation]  [History]  [Settings]  [Models]
HORIZONTAL    ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌─────────┐
LAYERS        │         │ │         │ │         │ │         │
Boundary      │ IPC Cmd │ │ IPC Cmd │ │ IPC Cmd │ │ IPC Cmd │
   │          ├─────────┤ ├─────────┤ ├─────────┤ ├─────────┤
Domain        │ Actor/FSM││ Pipeline│ │ Logic   │ │ Worker  │
   │          ├─────────┤ ├─────────┤ ├─────────┤ ├─────────┤
Registry      │ Cap Def │ │ Cap Def │ │ Cap Def │ │ Cap Def │
   │          ├─────────┤ ├─────────┤ ├─────────┤ ├─────────┤
Adapters      │ Concrete│ │ Concrete│ │ Concrete│ │ Concrete│
   │          ├─────────┤ ├─────────┤ ├─────────┤ ├─────────┤
Ports         │ Trait   │ │ Trait   │ │ Trait   │ │ Trait   │
   │          ├─────────┤ ├─────────┤ ├─────────┤ ├─────────┤
Infrastructure│ DB/Types│ │ DB/Types│ │ DB/Types│ │ DB/Types│
              └─────────┘ └─────────┘ └─────────┘ └─────────┘
```

### Downward-Only Dependency Flow
Code in any layer may only import from its own layer or layers **strictly below** it. Never above.

| Order | Layer | Purpose | Permitted Imports |
|:---:|:---|:---|:---|
| **6** | **Boundary** (`ipc`, `controllers`, `cli`, `bootstrap`) | Entry points, command factory, transport wiring | Domain, Registry, Adapters, Ports, Infrastructure |
| **5** | **Domain** (`session`, `pipeline`, `services`, `actors`) | State machines, business workflows, SQL services | Registry, Ports, Infrastructure |
| **4** | **Registry** (`registry`) | Declarative capabilities, settings, metrics schemas | Ports, Infrastructure |
| **3** | **Adapters** (`adapters`) | Concrete third-party implementations of port traits | Ports, Infrastructure |
| **2** | **Ports** (`ports`) | Pure interfaces, capability traits | Infrastructure |
| **1** | **Infrastructure** (`config`, `db`, `types`, `error`, `telemetry`) | Low-level drivers, shared serializable types, AppError | Only external/stdlib libraries |

---

## 4. Automated Architectural Guardrails

Architectural rules must be defended by automated tests in CI. Two primary tests protect the system:

### Guardrail 1: Upward Import Enforcement (`verify-layering`)
An automated test scans all source file imports and validates that no file imports from a layer above itself.
- **Self-Cleaning Exception List**: If a temporary architectural violation is permitted during migration, it must be declared in an explicit allowlist with a mandatory reason string.
- **Fail on Cleanup**: If code is refactored and an exception is eliminated, the test *fails* until the entry is removed from the exception list, ensuring exceptions never outlive the problem.

### Guardrail 2: Reachability Scanner (`verify-reachability`)
Prevents "ghost features" and decorative dead code:
- Scans `registry/` for every declared setting key and telemetry metric.
- Inspects the entire codebase for real consumption sites (excluding declaration tables, display-ordering lists, and tests).
- Fails build/test if any setting is written but never read, or any metric is declared but never recorded.

---

## 5. Developer & AI Agent Working Workflow

When prompted to add a feature, refactor code, or fix a bug, follow the **Five-Step SOT Loop**:

```
1. GREP BEFORE YOU CREATE
   Run `node scripts/sot.mjs <SymbolName>`
   Check if a canonical type, port, or service already exists.
         │
         ▼
2. READ HEADERS, NOT FILES
   Examine the WHAT, WHY, and WHERE lines of matches.
   Identify the single canonical owner in seconds.
         │
         ▼
3. FOLLOW THE WHERE CALL GRAPH
   Follow documented edges to understand upstream callers and downstream consumers.
         │
         ▼
4. EXTEND, NEVER DUPLICATE
   Add fields or variants to existing canonical types.
   Never create parallel types (`SessionInfo`, `UserData2`).
         │
         ▼
5. AUTHOR OR UPDATE SOT HEADERS
   Every new or modified file must update its SOT header.
   Verify layering and reachability tests pass before committing.
```

---

## 6. Decision Matrix: Where Things Go

| If you are writing... | It goes in... | NEVER in... |
|:---|:---|:---|
| A new feature declaration or capability | `registry/` | Scattered constants or inline configs |
| Cross-cutting behavior (auth, validation, tracing) | Command Factory (`ipc/factory`) | Individual command handlers or controllers |
| Domain decision logic / state transitions | Pure FSM (`domain/machine`) | UI components, command handlers, services |
| Side-effect execution (audio, files, network) | Actor or Adapter (`domain/actor`, `adapters/`) | Inside state machine transitions |
| A database query | Pure Service (`services/`) | Handlers, pipeline, or UI components |
| A contract for a swappable dependency | Pure Port (`ports/`) | Inside concrete adapter packages |
| A third-party library integration | Concrete Adapter (`adapters/`) | Directly in domain logic or ports |
| Concrete adapter instantiation | Bootstrap (`bootstrap`) | Anywhere else in the codebase |
| Shared serializable types | `types/` | Inline in domain or command modules |
| Transient UI state (sidebar open, tab index) | Frontend store (`stores/`) | Backend state or React context drilling |
| Canonical domain state | Backend pushed events | Frontend local cache / duplicate stores |

---

## 7. Hard Limits & Prohibitions

1. **No Bypasses**: Never use `any`, `@ts-ignore`, `unwrap()`, or `expect()` in production code. A codebase that lies in its types will quickly lie in its documentation.
2. **File Size Limit**: Files over **~400 lines** are doing two things. Split them.
3. **Function Size Limit**: Functions over **~50 lines** must be refactored into smaller, focused helpers.
4. **Generated Types Are Read-Only**: Client types generated from backend definitions (e.g. Specta, OpenAPI, Prisma, Protobuf) are NEVER edited by hand.
5. **No Upward Imports**: Strictly enforced by automated layering tests.
6. **Pure SQL in Services**: No joins across unrelated domains, no business rule evaluation, no inter-service queries.

---

## 8. Detailed References & Tooling

To explore implementation details, code templates, and testing harnesses, refer to the accompanying skill references:

- [Registry Pattern & Reachability](references/01-registry-and-reachability.md): Schema blueprints, dynamic client UI generation, and reachability scanners.
- [Command Execution Factory](references/02-command-factory-pipeline.md): Pipeline lifecycle, reentrancy guards, validation, and AppError mapping.
- [Pure FSM & Actors](references/03-pure-fsm-and-actors.md): Deterministic state machines, effects as data, zero-mock testing, and event streams.
- [Ports, Adapters & Layering](references/04-ports-adapters-and-layering.md): Hexagonal boundaries, dependency inversion, bootstrap factories, and layering tests.
- [SOT Comment Header Indexing](references/05-sot-header-indexing.md): Comment syntax, semantic grep rules, and `sot.mjs` tooling.

### Included Automation Scripts
- `scripts/sot.mjs`: Fast keyword search across SOT headers.
- `scripts/verify-layering.mjs`: Layer dependency direction validator.
- `scripts/verify-reachability.mjs`: Registry setting and metric reachability scanner.
