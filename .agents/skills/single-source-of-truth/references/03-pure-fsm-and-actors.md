# Pillar 3 Reference: Pure FSMs & The Actor Pattern

/**
 * SOURCE OF TRUTH KEYWORDS: PureFSM, StateMachine, EffectsAsData,
 *   ActorPattern, ZeroMockTesting, EventPushing
 * WHAT:  Architecture and implementation guide for deterministic finite state
 *        machines that return effects as pure data.
 * WHY:   Decouples business state transitions from async side-effects, making
 *        complex lifecycle logic 100% testable without mocks.
 * WHERE: Referenced by SKILL.md Pillar 3; implemented in domain/ / session/ modules.
 */

## 1. The Anti-Pattern: Tangled State and Side-Effects

In conventional codebases, state transitions are peppered with inline side-effects:

```rust
// BAD: Impossible to test deterministically without mocks
impl Session {
    pub async fn stop(&mut self) -> Result<()> {
        self.state = State::Finalizing;
        self.audio_driver.stop().await?;       // Side effect 1
        self.db.update_status("done").await?;  // Side effect 2
        self.ipc.emit("state-changed").await?; // Side effect 3
        self.sound.play("beep.wav").await?;    // Side effect 4
        Ok(())
    }
}
```

Testing this method requires mocking the audio driver, database, IPC emitter, and audio playback system. If the database times out, the audio driver is left in an unknown state.

---

## 2. The Solution: Pure Transitions Returning Effects as Data

In the SOT Architecture, state transitions are **pure mathematical functions**:

```rust
pub fn transition(
    state: &SessionState,
    event: SessionEvent,
) -> Result<(SessionState, Vec<Effect>), TransitionError>
```

### The Rules of Pure FSMs:
1. **Zero Side-Effects Inside Transition**: The transition function does not touch disk, network, timers, or threads.
2. **Effects are Data Enums**: Side-effects are represented as serializable data variants:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    StopAudioCapture,
    PlayFeedbackSound(SoundId),
    PersistSessionState { id: SessionId, status: SessionStatus },
    EmitClientEvent { event: &'static str, payload: StatePayload },
    ScheduleTimeout { id: TimerId, duration_ms: u64 },
}
```

---

## 3. 100% Deterministic Testing (Zero Mocks)

Because `transition` is a pure function, unit tests are fast, clean, and completely reliable:

```rust
#[test]
fn stopping_an_active_session_transitions_to_finalizing_with_effects() {
    let initial_state = SessionState::Recording {
        session_id: SessionId("test-123"),
        started_at: 1000,
    };

    let (next_state, effects) = transition(&initial_state, SessionEvent::Stop).unwrap();

    // 1. Assert exact state
    assert_eq!(next_state, SessionState::Finalizing { session_id: SessionId("test-123") });

    // 2. Assert exact effects generated as pure data
    assert_eq!(
        effects,
        vec![
            Effect::StopAudioCapture,
            Effect::PlayFeedbackSound(SoundId::Chime),
            Effect::EmitClientEvent {
                event: "session:state-changed",
                payload: StatePayload::Finalizing,
            }
        ]
    );
}
```
*No mocks. No async delays. No flakes. Tests run in microseconds.*

---

## 4. The Actor Pattern: The Side-Effect Executor

The **Actor** is the state machine's single asynchronous owner:
- Maintains an event mailbox channel (`tokio::sync::mpsc` or similar).
- Receives events sequentially.
- Passes `(state, event)` to the pure state machine.
- Iterates over the returned `Vec<Effect>` and dispatches each effect to concrete ports.

```rust
pub struct SessionActor {
    mailbox: mpsc::Receiver<SessionEvent>,
    state: SessionState,
    ports: Arc<Ports>,
}

impl SessionActor {
    pub async fn run(mut self) {
        while let Some(event) = self.mailbox.recv().await {
            match transition(&self.state, event) {
                Ok((next_state, effects)) => {
                    self.state = next_state;
                    for effect in effects {
                        self.execute_effect(effect).await;
                    }
                }
                Err(err) => {
                    tracing::error!(error = ?err, "Illegal state transition attempted");
                }
            }
        }
    }

    async fn execute_effect(&self, effect: Effect) {
        match effect {
            Effect::StopAudioCapture => self.ports.audio.stop().await.ok(),
            Effect::PlayFeedbackSound(id) => self.ports.sound.play(id).await.ok(),
            Effect::EmitClientEvent { event, payload } => {
                self.ports.events.push(event, payload).await.ok()
            }
            // ...
        };
    }
}
```

---

## 5. Pushed Events vs Client State Polling

In SOT architecture, clients (React, Svelte, Vue, mobile) **never poll** and **never duplicate domain state**:
- Backend pushes typed events: `session:state-changed`.
- Frontend store subscribes to the stream and updates its local reactive display:
```ts
useEvent("session:state-changed", (payload) => {
  setSessionState(payload);
});
```
- There is only ever **one true state** in existence — on the backend state machine.
