# Pillar 2 Reference: The Command Execution Factory

/**
 * SOURCE OF TRUTH KEYWORDS: CommandFactory, OperationalSOT, ReentrancyLock,
 *   PermissionPreflight, AppErrorMapping, TracingPipeline
 * WHAT:  Architecture and implementation guide for the centralized boundary
 *        command execution factory.
 * WHY:   Eliminates repetitive validation, permission, concurrency, tracing,
 *        and error mapping boilerplate across IPC and API command handlers.
 * WHERE: Referenced by SKILL.md Pillar 2; implemented in ipc/factory.rs.
 */

## 1. The Anti-Pattern: Fat Handlers with Leaky Concerns

In standard backends or desktop IPC layers, handlers frequently look like this:

```rust
// BAD: 40 lines of boilerplate, 3 lines of actual work
async fn start_recording(payload: Payload) -> Result<Response, String> {
    // 1. Manual payload validation
    if payload.device_id.is_empty() { return Err("Invalid device".into()); }
    // 2. Permission check
    if !check_mic_permission().await { return Err("No mic permission".into()); }
    // 3. Race condition / lock
    let lock = MUTEX.lock().await;
    // 4. Tracing setup
    let span = info_span!("start_recording");
    // 5. Work
    let res = domain_start(payload.device_id).await;
    // 6. Error conversion
    res.map_err(|e| e.to_string())
}
```

When 50 commands repeat this pattern:
- 10 commands will forget to check permissions.
- 15 commands will fail to log duration or trace errors properly.
- Error shapes vary between endpoints, confusing client consumers.
- Race conditions occur on un-guarded mutating actions.

---

## 2. The Operational SOT Pipeline

The Command Execution Factory (`ipc/factory.rs`) funnels **every** boundary command through a single higher-order pipeline:

```
[Incoming Command]
       │
       ▼
 1. Schema Validation (Input::validate(&self))
       │
       ▼
 2. Permission Preflight (Ports::permissions.ensure_granted(cap.permissions))
       │
       ▼
 3. Reentrancy Guard (Named Lock acquisition)
       │
       ▼
 4. Tracing Span Initiation (info_span! with correlation ID)
       │
       ▼
 5. Thin Handler Execution (Pure domain call)
       │
       ▼
 6. AppError Mapping (Unified status, safe user message, internal logging)
       │
       ▼
[Response to Client]
```

---

## 3. Reference Implementation

```rust
// ipc/factory.rs
pub async fn execute<I, O, F, Fut>(
    ctx: &AppContext,
    cmd_name: &'static str,
    input: I,
    handler: F,
) -> Result<O, AppError>
where
    I: Validate + std::fmt::Debug,
    F: FnOnce(AppContext, I) -> Fut,
    Fut: Future<Output = Result<O, DomainError>>,
{
    // 1. Validation
    if let Err(val_err) = input.validate() {
        tracing::warn!(command = cmd_name, error = ?val_err, "Validation failed");
        return Err(AppError::validation(val_err));
    }

    // 2. Permission Preflight
    if let Some(cap) = ctx.registry.find_capability(cmd_name) {
        for perm in cap.permissions {
            if !ctx.ports.permissions.is_granted(*perm).await? {
                tracing::error!(command = cmd_name, permission = ?perm, "Permission denied");
                return Err(AppError::permission_denied(*perm));
            }
        }
    }

    // 3. Reentrancy & Concurrency Guarding
    let _guard = if is_mutating(cmd_name) {
        Some(ctx.acquire_command_lock(cmd_name).await)
    } else {
        None
    };

    // 4. Tracing Span
    let _span = tracing::info_span!("command", name = cmd_name).entered();
    let start_time = std::time::Instant::now();

    // 5. Handler Execution
    let result = handler(ctx.clone(), input).await;

    // 6. Canonical Error Mapping & Latency
    let elapsed = start_time.elapsed();
    tracing::info!(command = cmd_name, duration_ms = elapsed.as_millis(), "Command finished");

    result.map_err(|dom_err| {
        tracing::error!(command = cmd_name, error = ?dom_err, "Command error");
        AppError::from(dom_err)
    })
}
```

---

## 4. The Result: Pristine, Thin Handlers

Individual command handlers shrink to their essential essence:

```rust
// ipc/commands/session.rs
#[tauri::command]
pub async fn start_recording(
    ctx: State<'_, AppContext>,
    input: StartRecordingInput,
) -> Result<SessionId, AppError> {
    execute(&ctx, "session.start_recording", input, |ctx, input| async move {
        ctx.session_actor.send(SessionEvent::Start(input.device_id)).await
    }).await
}
```

Notice what is absent:
- No manual permission checks.
- No payload boundary checks.
- No reentrancy mutex code.
- No ad-hoc error string conversions.
Adding a new command is effortless, robust, and completely protected.
