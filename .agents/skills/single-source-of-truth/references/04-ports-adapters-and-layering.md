# Pillar 4 Reference: Ports, Adapters & Layering Enforcement

/**
 * SOURCE OF TRUTH KEYWORDS: PortsAndAdapters, HexagonalArchitecture,
 *   BootstrapFactory, LayeringEnforcement, PureServices
 * WHAT:  Architecture and implementation guide for hexagonal boundary isolation,
 *        pure database services, and compile-time downward-only layer testing.
 * WHY:   Prevents circular dependencies, isolates third-party libraries, and
 *        guarantees that replacing an external dependency is a one-file change.
 * WHERE: Referenced by SKILL.md Pillar 4 & 5; enforced by layering tests.
 */

## 1. Ports vs Adapters (Hexagonal Core)

The codebase isolates external tools behind abstract interfaces:

### Ports (`ports/`)
- Pure traits / abstract interfaces.
- Zero implementation code.
- Zero third-party library dependencies (only stdlib or shared domain types).

```rust
// ports/speech.rs
#[async_trait]
pub trait TranscriptionEnginePort: Send + Sync {
    async fn transcribe(&self, audio: AudioBuffer) -> Result<Transcript, EngineError>;
    fn capabilities(&self) -> EngineCapabilities;
}
```

### Adapters (`adapters/`)
- Concrete implementations of ports.
- Encapsulates third-party crates, C bindings, network SDKs, or OS APIs.
- Completely isolated in individual directories (`adapters/whisper`, `adapters/deepgram`, `adapters/cpal`).

---

## 2. The Single Bootstrap Point (`bootstrap.rs`)

**Hard Rule**: Concrete adapters are named in **exactly one file** in the entire application: `bootstrap.rs`.

```rust
// bootstrap.rs
pub fn create_runtime(config: &AppConfig) -> AppRuntime {
    // 1. Concrete adapters instantiated ONLY here
    let audio_adapter = Arc::new(CpalAudioAdapter::new());
    let engine_adapter = Arc::new(WhisperEngineAdapter::load(&config.model_path));
    let injector_adapter = Arc::new(MacOsKeyInjector::new());

    // 2. Wired into the Ports container
    let ports = Arc::new(Ports {
        audio: audio_adapter,
        engine: engine_adapter,
        injector: injector_adapter,
    });

    AppRuntime::new(ports)
}
```

### Benefit:
If you replace `WhisperEngineAdapter` with `CloudAsrAdapter`, you create a new adapter folder and change **one line in `bootstrap.rs`**. Not a single domain handler, service, or state machine changes.

---

## 3. Pure Services (`services/`)

The persistence layer (`services/`) adheres strictly to simplicity:
- **Rule**: One verb, one table. Parameters in, rows out.
- **No Business Logic**: Services do not evaluate business decisions or compute status transitions.
- **No Cross-Service Calls**: `sessions_service` never imports or queries `dictionary_service`.

```rust
// services/sessions.rs
pub async fn insert_session(db: &SqlitePool, id: &str, title: &str) -> Result<(), DbError> {
    sqlx::query!("INSERT INTO sessions (id, title) VALUES (?, ?)", id, title)
        .execute(db)
        .await?;
    Ok(())
}

pub async fn get_session_by_id(db: &SqlitePool, id: &str) -> Result<Option<SessionRow>, DbError> {
    sqlx::query_as!(SessionRow, "SELECT * FROM sessions WHERE id = ?", id)
        .fetch_optional(db)
        .await
        .map_err(Into::into)
}
```

---

## 4. Layering Enforcement (`nothing_imports_upward`)

To prevent the architecture from decaying into circular dependencies, an automated test scans imports and enforces downward-only dependencies.

```rust
const LAYER_ORDER: &[&[&str]] = &[
    // 1. Infrastructure (Lowest: knows nothing above)
    &["config", "db", "error", "telemetry", "types"],
    // 2. Contracts
    &["ports"],
    // 3. Implementations
    &["adapters"],
    // 4. Registry
    &["registry"],
    // 5. Domain
    &["pipeline", "services", "session"],
    // 6. Boundary (Highest: can see everything)
    &["bootstrap", "ipc", "tray"],
];
```

### Self-Cleaning Allowlist:
```rust
const KNOWN_UPWARD_IMPORTS: &[(&str, &str, &str)] = &[
    (
        "session/actor.rs",
        "ipc",
        "SessionContext is currently in ipc/; migrating to domain context"
    ),
];
```
- Every exception MUST provide a documented reason.
- If someone refactors the code and removes the upward import, **the test fails until the allowlist line is deleted**. The allowlist is permanently self-cleaning.
