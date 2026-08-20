/*!
 * SOURCE OF TRUTH KEYWORDS: AppState, CommandContext, Ports, SessionHandle,
 *   InflightGuard, correlation_id
 * WHAT:  The application state every IPC command is handed, and the guard that
 *        makes a capability non-reentrant.
 * WHY:   Ports are held as trait objects rather than concrete adapters, so the
 *        whole app is written against contracts and only lib.rs ever names an
 *        implementation. That is what makes swapping an engine one new file and
 *        one construction arm.
 *
 *        Everything here is cheap to clone because Tauri hands state out by
 *        reference on every call and a command must never wait to obtain it.
 * WHERE: Constructed once in lib.rs setup, registered as Tauri managed state,
 *        and unwrapped into a CommandContext by ipc/factory.rs.
 */

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use tokio::sync::mpsc;

use crate::config::AppPaths;
use crate::db::Database;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::ports::{
    AudioSource, EventSink, ModelStore, PermissionProvider, TextEnhancer, TextInjector,
    TranscriptionEngine,
};
use crate::registry::CapabilityKey;
use crate::session::SessionEvent;
use crate::types::SessionState;

/**
 * SOURCE OF TRUTH KEYWORDS: Ports
 * WHAT:  Every swappable implementation the app was built with.
 * WHY:   Grouped rather than flattened into AppState so that a test can
 *        construct a full set of fakes in one place, and so the list of things
 *        that are genuinely pluggable stays visible.
 * WHERE: Built by adapters::build in lib.rs setup.
 */
#[derive(Clone)]
pub struct Ports {
    pub engine: Arc<dyn TranscriptionEngine>,
    pub audio: Arc<dyn AudioSource>,
    pub enhancer: Arc<dyn TextEnhancer>,
    pub injector: Arc<dyn TextInjector>,
    pub models: Arc<dyn ModelStore>,
    pub permissions: Arc<dyn PermissionProvider>,
    /// How the domain pushes state outward. See ports/events.rs — this is what
    /// keeps `session/` free of any dependency on the UI framework.
    pub events: Arc<dyn EventSink>,
}

/**
 * SOURCE OF TRUTH KEYWORDS: SessionHandle
 * WHAT:  The only way to reach the session actor.
 * WHY:   A bounded channel to one owning task, deliberately — never an
 *        `Arc<Mutex<SessionState>>` shared between the audio drain, the ASR
 *        worker, the hotkey handler and the IPC layer. That shape is how you
 *        get a deadlock that reproduces once a week at 2am. Here the state has
 *        exactly one owner and everything else sends it messages.
 *
 *        Bounded, because an unbounded queue in front of an actor turns a
 *        transient stall into unbounded memory growth.
 * WHERE: Held in AppState; used by ipc/commands/session.rs and the hotkey
 *        handler.
 */
#[derive(Clone)]
pub struct SessionHandle {
    sender: mpsc::Sender<SessionEvent>,
    /**
     * SOURCE OF TRUTH KEYWORDS: requested_at, stamp, HotkeyDispatch,
     *   TotalFinalize
     * WHAT:  When the user last pressed a key that asks for a state change.
     * WHY:   Two declared metrics measure from the KEYPRESS —
     *        `HotkeyDispatch` ("Hotkey event to FSM transition") and
     *        `TotalFinalize` ("Stop to pasted", the number this product
     *        promises). Neither was recordable, because only the sender knows
     *        when the key was pressed and only the actor holds the recorder.
     *
     *        Carried here rather than on the event because the FSM's vocabulary
     *        is deliberately about WHAT HAPPENED, not about payloads or clocks
     *        — the same argument that keeps `RecordingMode` out of it. Putting
     *        an `Instant` on `StartRequested` would have made every match arm
     *        and every test construct a timestamp to say "the user pressed the
     *        key".
     *
     *        A plain overwrite is correct: a stamp that is never consumed
     *        belongs to a press the FSM rejected, and the next real press
     *        should replace it rather than queue behind it.
     * WHERE: Stamped by the hotkey handler and the session IPC commands;
     *        consumed by SessionActor::dispatch.
     */
    requested_at: Arc<Mutex<Option<Instant>>>,
}

impl SessionHandle {
    pub fn new(sender: mpsc::Sender<SessionEvent>) -> Self {
        Self {
            sender,
            requested_at: Arc::new(Mutex::new(None)),
        }
    }

    /// Marks "the user just pressed the key". Call immediately before sending
    /// a StartRequested or StopRequested, and nowhere else — a stamp from a
    /// timer or an internal event would report a latency no user experienced.
    pub fn stamp_request(&self) {
        *self.requested_at.lock() = Some(Instant::now());
    }

    /// Takes the pending stamp, if a user press produced one.
    pub fn take_request_stamp(&self) -> Option<Instant> {
        self.requested_at.lock().take()
    }

    /// Delivers an event to the actor. A full channel means the actor is wedged,
    /// which is a real failure and not something to silently drop.
    pub async fn send(&self, event: SessionEvent) -> AppResult<()> {
        self.sender.send(event).await.map_err(|_| {
            AppError::new(
                ErrorCode::Internal,
                "Murmur's recorder stopped responding. Restarting the app will fix it.",
            )
        })
    }

    /// Non-blocking send, for callers that cannot await — the audio callback
    /// path and the countdown timer. Drops rather than blocking a hot path.
    pub fn try_send(&self, event: SessionEvent) -> bool {
        self.sender.try_send(event).is_ok()
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: AppState
 * WHAT:  Everything a command can reach.
 * WHERE: Tauri managed state; the single argument every command takes.
 */
#[derive(Clone)]
pub struct AppState {
    pub paths: AppPaths,
    pub db: Database,
    pub ports: Ports,
    pub session: SessionHandle,
    /// Held so any layer can emit a typed event. Rust owns domain state and
    /// PUSHES it — the frontend never polls — so an emit handle is not a
    /// convenience here, it is how the UI learns anything.
    pub app: tauri::AppHandle,
    /// Capabilities with a command currently executing. See InflightGuard.
    inflight: Arc<Mutex<HashSet<CapabilityKey>>>,
    /// Last state the actor published. See `current_state`.
    published_state: Arc<Mutex<SessionState>>,
}

impl AppState {
    pub fn new(
        paths: AppPaths,
        db: Database,
        ports: Ports,
        session: SessionHandle,
        app: tauri::AppHandle,
    ) -> Self {
        Self {
            paths,
            db,
            ports,
            session,
            app,
            inflight: Arc::new(Mutex::new(HashSet::new())),
            published_state: Arc::new(Mutex::new(SessionState::Idle)),
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: current_state, publish_state
     * WHAT:  The last state the session actor published.
     * WHY:   This is a read-through cache, NOT a second source of truth. The
     *        FSM inside the actor remains the only thing that decides state,
     *        and the actor is the only writer here — it publishes immediately
     *        after each transition, in the same step that emits the event.
     *
     *        It exists for exactly one case: a window that opens mid-session
     *        needs something to paint before the next event arrives. The
     *        alternative is an actor round trip on every mount, which puts a
     *        channel hop on the pill's first frame. Nothing polls this, and
     *        nothing writes to it except `publish_state`.
     * WHERE: Written by session/actor.rs on EmitState; read once on mount by
     *        get_session_state.
     */
    pub fn current_state(&self) -> SessionState {
        self.published_state.lock().clone()
    }

    /// Actor-only. Called in the same step that emits SessionStateChanged, so
    /// the cache and the event can never disagree.
    pub fn publish_state(&self, state: SessionState) {
        *self.published_state.lock() = state;
    }

    /// The narrowed view the session actor runs on. Shares this state's Arcs.
    pub fn session_context(&self) -> SessionContext {
        SessionContext {
            db: self.db.clone(),
            ports: self.ports.clone(),
            session: self.session.clone(),
            published_state: Arc::clone(&self.published_state),
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: InflightGuard, begin_exclusive
     * WHAT:  Claims a capability, or reports that it is already busy.
     * WHY:   This is the reentrancy guard, and it exists in ONE place so that
     *        an entire class of race disappears at once. A hotkey that fires
     *        twice — which happens on key repeat and on a sticky modifier —
     *        cannot start two recordings, because the second call never reaches
     *        a handler.
     * WHERE: Called by ipc/factory.rs before every guarded command.
     */
    pub fn begin_exclusive(&self, capability: CapabilityKey) -> Option<InflightGuard> {
        let mut inflight = self.inflight.lock();
        if !inflight.insert(capability) {
            return None;
        }
        Some(InflightGuard {
            capability,
            inflight: Arc::clone(&self.inflight),
        })
    }
}

/// Releases its capability when dropped, including on an early return or a
/// panic inside a handler — which is why it is a guard and not a pair of calls.
pub struct InflightGuard {
    capability: CapabilityKey,
    inflight: Arc<Mutex<HashSet<CapabilityKey>>>,
}

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.inflight.lock().remove(&self.capability);
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: SessionContext, session_context
 * WHAT:  Exactly what the session actor needs, and nothing else.
 * WHY:   The actor is domain code. Handing it the whole AppState would hand it
 *        a `tauri::AppHandle` it does not use — which drags the UI framework
 *        into the one module where the interesting correctness lives
 *        (cancellation, seams, delivery) and makes it impossible to test
 *        without constructing a Tauri app.
 *
 *        Every field is a clone of the SAME Arc AppState holds, so the
 *        published state stays coherent between them. This is a narrowing of
 *        access, not a second copy of the truth.
 * WHERE: Built by AppState::session_context; consumed by session/actor.rs and
 *        by the end-to-end tests, which construct one directly from fakes.
 */
#[derive(Clone)]
pub struct SessionContext {
    pub db: Database,
    pub ports: Ports,
    pub session: SessionHandle,
    published_state: Arc<Mutex<SessionState>>,
}

impl SessionContext {
    /// For tests, which build one from fakes rather than from an AppState.
    pub fn new(db: Database, ports: Ports, session: SessionHandle) -> Self {
        Self {
            db,
            ports,
            session,
            published_state: Arc::new(Mutex::new(SessionState::Idle)),
        }
    }

    pub fn current_state(&self) -> SessionState {
        self.published_state.lock().clone()
    }

    pub fn publish_state(&self, state: SessionState) {
        *self.published_state.lock() = state;
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: CommandContext
 * WHAT:  What a handler receives: the state, plus the correlation id for this
 *        call.
 * WHY:   The correlation id is generated by the factory and threaded through
 *        the tracing span, so one string ties a log line to a session row and
 *        to whatever the user reported.
 *
 *        It OWNS its AppState rather than borrowing one. A borrowed context
 *        cannot be tied to the lifetime of the future a handler returns without
 *        a higher-ranked bound that async closures do not express, and every
 *        field of AppState is an Arc or a channel sender — so owning it costs a
 *        handful of refcount bumps and removes the problem entirely.
 * WHERE: The first argument of every command handler.
 */
pub struct CommandContext {
    pub state: AppState,
    pub correlation_id: String,
}

impl CommandContext {
    pub fn db(&self) -> &Database {
        &self.state.db
    }

    pub fn ports(&self) -> &Ports {
        &self.state.ports
    }

    pub fn paths(&self) -> &AppPaths {
        &self.state.paths
    }

    pub fn session(&self) -> &SessionHandle {
        &self.state.session
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inflight_set() -> Arc<Mutex<HashSet<CapabilityKey>>> {
        Arc::new(Mutex::new(HashSet::new()))
    }

    /// Builds just enough of AppState to exercise the guard.
    fn guard_for(
        inflight: &Arc<Mutex<HashSet<CapabilityKey>>>,
        capability: CapabilityKey,
    ) -> Option<InflightGuard> {
        let mut set = inflight.lock();
        if !set.insert(capability) {
            return None;
        }
        Some(InflightGuard {
            capability,
            inflight: Arc::clone(inflight),
        })
    }

    #[test]
    fn a_capability_cannot_be_entered_twice() {
        let inflight = inflight_set();
        let first = guard_for(&inflight, CapabilityKey::Dictation);
        assert!(first.is_some(), "the first caller claims the capability");

        let second = guard_for(&inflight, CapabilityKey::Dictation);
        assert!(
            second.is_none(),
            "a double-fired hotkey must not start a second recording"
        );
    }

    #[test]
    fn different_capabilities_do_not_block_each_other() {
        let inflight = inflight_set();
        let _dictation = guard_for(&inflight, CapabilityKey::Dictation);
        assert!(
            guard_for(&inflight, CapabilityKey::History).is_some(),
            "browsing history while recording must still work"
        );
    }

    #[test]
    fn dropping_the_guard_releases_the_capability() {
        let inflight = inflight_set();
        {
            let _guard = guard_for(&inflight, CapabilityKey::Dictation);
        }
        assert!(
            guard_for(&inflight, CapabilityKey::Dictation).is_some(),
            "an early return must not wedge the capability forever"
        );
    }
}
