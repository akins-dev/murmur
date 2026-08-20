/*!
 * SOURCE OF TRUTH KEYWORDS: SessionMachine, SessionEvent, Transition, Effect,
 *   TransitionError, handle, current_state, cancel_countdown_ms, CancelPending,
 *   CancelArmed, CancelAborted, CancelExpired, DestroySession
 * WHAT:  The finite state machine governing one recording session. Pure: it
 *        takes an event, returns the new state plus a list of effects, and
 *        performs none of them.
 * WHY:   Recording state lives in exactly one place, so illegal states are
 *        unrepresentable and every transition is logged. Purity is the point —
 *        the machine touches no database, no audio device and no clock, which
 *        is what makes "kill the process in state X and assert recovery"
 *        something a unit test can express rather than something you find out
 *        about from a user.
 *
 *        Effects are DATA, returned for the actor to perform. That separation
 *        is what lets the ordering guarantee be checked: the persist effect is
 *        emitted before the state is considered live, so a crash mid-transition
 *        leaves a recoverable row rather than a state nobody wrote down.
 *
 *        The one asymmetry worth knowing: cancellation does NOT stop capture.
 *        Audio keeps flowing through CancelPending, because nothing being torn
 *        down is exactly what lets a second Escape resume with no gap.
 * WHERE: Owned by session/actor.rs, which is the only thing allowed to drive
 *        it. Its state is pushed to the pill as a typed event.
 */

use crate::error::{AppError, ErrorCode};
use crate::types::{SessionId, SessionState};

/**
 * SOURCE OF TRUTH KEYWORDS: SessionEvent
 * WHAT:  Everything that can move a session.
 * WHY:   Named for what HAPPENED, not for what should follow — `CancelArmed`
 *        rather than `GoToCancelPending`. The machine decides the consequence,
 *        so a caller cannot drive it into a state by naming one.
 * WHERE: Sent to the actor over its command channel by the hotkey handler, the
 *        capture pipeline and the finalize task.
 */
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// The dictation hotkey fired while idle.
    StartRequested,
    /// The microphone is open and delivering samples.
    ArmingComplete,
    ArmingFailed(AppError),
    /// The hotkey fired again, or push-to-talk was released.
    StopRequested,
    /// Escape, once.
    CancelArmed,
    /// Escape again during the countdown.
    CancelAborted,
    /// The countdown ran out. The recording is destroyed.
    CancelExpired,
    /// Elapsed-time update while capturing.
    Tick { elapsed_ms: u64, remaining_ms: u64 },
    /// The trailing fragment decoded and the text was delivered.

    /// A background delivery failed. Surfaced only when nothing else is
    /// happening — see the Idle arm.
    DeliveryFailed(AppError),
    /**
     * A handed-off recording's trailing decode never came back. Handled
     * entirely by the actor — it does not reach the FSM, because by now the
     * FSM has moved on and this recording is no longer its business. It is a
     * SessionEvent only because the actor's inbox is the one place that can
     * safely touch the pending map.
     */
    DeliveryTimedOut(SessionId),
    /// The device disappeared. Whatever was captured is still worth delivering.
    DeviceLost(AppError),
    /// Return to Idle from a terminal state, after the pill has dismissed.
    Reset,
}

/**
 * SOURCE OF TRUTH KEYWORDS: Effect
 * WHAT:  A side effect the actor must perform for a transition to be real.
 * WHY:   Returned as data rather than executed, so the machine stays pure and
 *        the ORDER of effects is part of what a test can assert. `PersistRow`
 *        appearing before `StartCapture` is the crash-recovery guarantee
 *        written down where it can be checked.
 * WHERE: Interpreted by session/actor.rs, in the order returned.
 */
#[derive(Debug, Clone, PartialEq)]
pub enum Effect {
    /// Write the in-flight session row. Always emitted BEFORE capture starts.
    PersistRow { session_id: SessionId },
    /// Open the microphone and begin buffering.
    StartCapture,
    /// Close the microphone. Never emitted on the cancel-armed path.
    StopCapture,
    /**
     * SOURCE OF TRUTH KEYWORDS: HandOffToDelivery
     * WHAT:  Detach everything captured so far and let it become pasted text on
     *        its own time. Capture is over the instant this is emitted.
     * WHY:   Replaced `BeginFinalize`, which kept the session in a Finalizing
     *        state until the model came back. The difference is the whole point
     *        of the split: the FSM returns to Idle in the same transition that
     *        emits this, so the next recording can start while the previous one
     *        is still decoding.
     */
    HandOffToDelivery { session_id: SessionId },
    /// Start the cancel countdown timer for this many milliseconds.
    StartCountdown { duration_ms: u64 },
    /// Stop the countdown; the recording continues.
    AbortCountdown,
    /// Delete the row, drop the audio buffer, discard decoded segments. Escape
    /// means gone — there is no tombstone and no purge job to trust.
    DestroySession { session_id: SessionId },
    /// Write the terminal row: outcome, text, timings.
    PersistOutcome { session_id: SessionId },
    /// Push the new state to the pill.
    EmitState,
}

/**
 * SOURCE OF TRUTH KEYWORDS: TransitionError
 * WHAT:  The event was not legal in the current state.
 * WHY:   An error rather than a silent no-op, because an illegal transition
 *        means two parts of the app disagree about what is happening, and
 *        swallowing it hides the disagreement until it produces a lost
 *        recording.
 * WHERE: Returned by `handle`; logged by the actor and surfaced as an AppError
 *        only if a user action caused it.
 */
#[derive(Debug, Clone, PartialEq)]
pub struct TransitionError {
    pub state: &'static str,
    pub event: &'static str,
}

impl std::fmt::Display for TransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`{}` is not legal while {}", self.event, self.state)
    }
}

impl From<TransitionError> for AppError {
    fn from(err: TransitionError) -> Self {
        AppError::new(ErrorCode::IllegalTransition, "That is not possible right now.")
            .recoverable()
            .with_detail(err)
    }
}

#[derive(Debug, Clone)]
pub struct Transition {
    pub from: SessionState,
    pub to: SessionState,
    pub effects: Vec<Effect>,
}

/**
 * SOURCE OF TRUTH KEYWORDS: SessionMachine
 * WHAT:  The machine itself. One exists; it is never cloned.
 * WHY:   Only one session can exist at a time, and that is enforced by there
 *        being one machine owned by one actor — not by a flag someone has to
 *        remember to check.
 * WHERE: Constructed once by session/actor.rs.
 */
#[derive(Debug)]
pub struct SessionMachine {
    state: SessionState,
    session_id: Option<SessionId>,
    cancel_countdown_ms: u64,
    /**
     * SOURCE OF TRUTH KEYWORDS: discard_on_escape
     * WHAT:  Escape destroys the recording at once instead of arming a
     *        countdown.
     * WHY:   Held on the MACHINE rather than read at the edge, because this is
     *        a setting that changes which TRANSITION an event takes — not one
     *        that some handler consults afterwards. Deciding it in the hotkey
     *        handler would mean two different events for one keypress and a
     *        machine whose vocabulary is about the user's preferences rather
     *        than about what happened.
     *
     *        Snapshotted per session alongside cancel_countdown_ms, so toggling
     *        it mid-recording cannot change the rules halfway through a
     *        sentence.
     */
    discard_on_escape: bool,
}

impl SessionMachine {
    pub fn new(cancel_countdown_ms: u64) -> Self {
        Self {
            state: SessionState::Idle,
            session_id: None,
            cancel_countdown_ms,
            discard_on_escape: false,
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn session_id(&self) -> Option<&SessionId> {
        self.session_id.as_ref()
    }

    pub fn set_cancel_countdown_ms(&mut self, duration_ms: u64) {
        self.cancel_countdown_ms = duration_ms;
    }

    /// See the field WHY. Set once per session, from SessionSettings.
    pub fn set_discard_on_escape(&mut self, discard: bool) {
        self.discard_on_escape = discard;
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: handle
     * WHAT:  Applies one event, returning the transition and its effects.
     * WHY:   The single entry point, so every state change goes through one
     *        `match` that the compiler checks for exhaustiveness. A new state
     *        or a new event forces every path to be reconsidered deliberately —
     *        which is the review this design exists to force.
     * WHERE: Called only by session/actor.rs.
     */
    pub fn handle(&mut self, event: SessionEvent) -> Result<Transition, TransitionError> {
        let from = self.state.clone();

        let (to, effects) = match (&self.state, &event) {
            // ── Starting ─────────────────────────────────────────────────
            (SessionState::Idle, SessionEvent::StartRequested) => {
                let session_id = SessionId::new();
                self.session_id = Some(session_id.clone());
                (
                    SessionState::Arming,
                    // Persist FIRST. A crash between here and the first sample
                    // must still leave a row that recovery can find.
                    vec![
                        Effect::PersistRow { session_id },
                        Effect::StartCapture,
                        Effect::EmitState,
                    ],
                )
            }
            (SessionState::Arming, SessionEvent::ArmingComplete) => (
                SessionState::Recording { elapsed_ms: 0 },
                vec![Effect::EmitState],
            ),
            (SessionState::Arming, SessionEvent::ArmingFailed(err)) => (
                SessionState::Failed {
                    code: err.code,
                    message: err.message.clone(),
                },
                self.terminal_effects(),
            ),

            // ── Recording ────────────────────────────────────────────────
            (SessionState::Recording { .. }, SessionEvent::Tick { elapsed_ms, .. }) => (
                SessionState::Recording {
                    elapsed_ms: *elapsed_ms,
                },
                vec![Effect::EmitState],
            ),
            /*
             * SOURCE OF TRUTH KEYWORDS: stop_returns_to_idle
             * The transition the whole redesign exists for. Releasing the key
             * ends the session as far as the user is concerned, so the machine
             * says Idle immediately and the words catch up separately.
             *
             * Idle rather than a "delivering" state is deliberate and is the
             * part that must not be softened later: Idle is the only state
             * StartRequested is legal from, so anything else here — however
             * briefly — is a window in which the next recording is refused.
             */
            (SessionState::Recording { .. }, SessionEvent::StopRequested) => {
                let session_id = self.take_session_id();
                (
                    SessionState::Idle,
                    vec![
                            Effect::StopCapture,
                            Effect::HandOffToDelivery { session_id },
                            Effect::EmitState,
                        ],
                )
            }
            /*
             * SOURCE OF TRUTH KEYWORDS: escape_discards_immediately
             * Escape with `discard_on_escape` on: gone, now. No CancelPending,
             * no countdown, nothing to change your mind with — which is exactly
             * what the setting promises, and why it is off by default.
             *
             * The effects are the SAME ones CancelExpired emits, in the same
             * order, because this is that path with the wait removed. Sharing
             * the shape rather than writing a second teardown is what stops the
             * two drifting into deleting different things.
             */
            (SessionState::Recording { .. }, SessionEvent::CancelArmed)
                if self.discard_on_escape =>
            {
                let session_id = self.take_session_id();
                (
                    SessionState::Idle,
                    vec![
                        Effect::StopCapture,
                        Effect::DestroySession { session_id },
                        Effect::EmitState,
                    ],
                )
            }
            (SessionState::Recording { elapsed_ms }, SessionEvent::CancelArmed) => (
                SessionState::CancelPending {
                    elapsed_ms: *elapsed_ms,
                    remaining_ms: self.cancel_countdown_ms,
                },
                // Note the absence of StopCapture. Audio keeps flowing, which
                // is what makes a second Escape lossless.
                vec![
                    Effect::StartCountdown {
                        duration_ms: self.cancel_countdown_ms,
                    },
                    Effect::EmitState,
                ],
            ),
            /*
             * A device that vanishes mid-sentence is a normal event. Finalize
             * what we already have rather than discarding the session.
             *
             * StopCapture is NOT redundant here just because the device is
             * gone. "Lost" means cpal stopped delivering samples; it does not
             * mean our stream handle was closed, and until this effect existed
             * the handle stayed open through Finalizing and was then DROPPED
             * without `stop()` when the next session assigned over it — which
             * never sets the drain thread's stop flag. The visible consequence
             * is the one that matters: the macOS microphone indicator stays
             * lit after recording has ended, and in a local-first app that
             * reads as "it is still listening". Every other exit from a
             * capturing state stops the capture; these two were the omission.
             */
            (SessionState::Recording { .. }, SessionEvent::DeviceLost(_)) => {
                let session_id = self.take_session_id();
                (
                    SessionState::Idle,
                    vec![
                            Effect::StopCapture,
                            Effect::HandOffToDelivery { session_id },
                            Effect::EmitState,
                        ],
                )
            }

            // ── Cancel armed ─────────────────────────────────────────────
            (
                SessionState::CancelPending { .. },
                SessionEvent::Tick {
                    elapsed_ms,
                    remaining_ms,
                },
            ) => (
                SessionState::CancelPending {
                    elapsed_ms: *elapsed_ms,
                    remaining_ms: *remaining_ms,
                },
                vec![Effect::EmitState],
            ),
            (SessionState::CancelPending { elapsed_ms, .. }, SessionEvent::CancelAborted) => (
                SessionState::Recording {
                    elapsed_ms: *elapsed_ms,
                },
                vec![Effect::AbortCountdown, Effect::EmitState],
            ),
            // Pressing the stop hotkey while the countdown runs means "keep it"
            // — the user is reaching for delivery, not for the bin.
            (SessionState::CancelPending { .. }, SessionEvent::StopRequested) => {
                let session_id = self.take_session_id();
                (
                    SessionState::Idle,
                    vec![
                            Effect::AbortCountdown,
                            Effect::StopCapture,
                            Effect::HandOffToDelivery { session_id },
                            Effect::EmitState,
                        ],
                )
            }
            (SessionState::CancelPending { .. }, SessionEvent::CancelExpired) => {
                let session_id = self.take_session_id();
                (
                    SessionState::Idle,
                    vec![
                        Effect::StopCapture,
                        Effect::DestroySession { session_id },
                        Effect::EmitState,
                    ],
                )
            }
            // Same omission as the Recording arm above, and the same fix — see
            // the WHY there.
            (SessionState::CancelPending { .. }, SessionEvent::DeviceLost(_)) => {
                let session_id = self.take_session_id();
                (
                    SessionState::Idle,
                    vec![
                            Effect::AbortCountdown,
                            Effect::StopCapture,
                            Effect::HandOffToDelivery { session_id },
                            Effect::EmitState,
                        ],
                )
            }

            /*
             * SOURCE OF TRUTH KEYWORDS: DeliveryFailed, background_failure
             * WHAT:  A recording that finished capturing cleanly and then went
             *        wrong on its way to becoming text.
             * WHY:   Legal only from Idle, and that restriction is the design.
             *        Delivery now runs in the background, so its failure can
             *        arrive while the user is part-way through the NEXT
             *        recording — and interrupting a live recording to report a
             *        previous one would take words the user is speaking right
             *        now. The actor only raises this when nothing is in
             *        progress; otherwise the failure goes to History and the
             *        log, which is where it can be read later without costing
             *        anything in the moment.
             *
             *        Note the absence of the whole Finalizing block that used
             *        to live here — FinalizeComplete, FinalizeFailed and the
             *        deadline that rescued a wedged actor. None of them are
             *        reachable any more, because there is no state to be stuck
             *        in. The wedge is not fixed; it is unrepresentable.
             */
            (SessionState::Idle, SessionEvent::DeliveryFailed(err)) => (
                SessionState::Failed {
                    code: err.code,
                    message: err.message.clone(),
                },
                vec![Effect::EmitState],
            ),

            // ── Leaving a terminal state ─────────────────────────────────
            (
                SessionState::Failed { .. },
                SessionEvent::Reset,
            ) => {
                self.session_id = None;
                (SessionState::Idle, vec![Effect::EmitState])
            }
            (SessionState::Idle, SessionEvent::Reset) => {
                (SessionState::Idle, vec![])
            }

            // ── Everything else is a disagreement, not a no-op ────────────
            (state, event) => {
                return Err(TransitionError {
                    state: state_name(state),
                    event: event_name(event),
                })
            }
        };

        tracing::info!(
            from = state_name(&from),
            to = state_name(&to),
            event = event_name(&event),
            session_id = self.session_id.as_ref().map(|id| id.as_str()),
            "session transition"
        );

        self.state = to.clone();
        Ok(Transition { from, to, effects })
    }

    /// Terminal states persist their outcome and then show themselves.
    fn terminal_effects(&self) -> Vec<Effect> {
        match &self.session_id {
            Some(session_id) => vec![
                Effect::PersistOutcome {
                    session_id: session_id.clone(),
                },
                Effect::EmitState,
            ],
            None => vec![Effect::EmitState],
        }
    }

    /// Cancellation consumes the id — nothing may refer to the row afterwards.
    fn take_session_id(&mut self) -> SessionId {
        self.session_id.take().unwrap_or_default()
    }
}

fn state_name(state: &SessionState) -> &'static str {
    match state {
        SessionState::Idle => "Idle",
        SessionState::Arming => "Arming",
        SessionState::Recording { .. } => "Recording",
        SessionState::CancelPending { .. } => "CancelPending",
        SessionState::Failed { .. } => "Failed",
    }
}

fn event_name(event: &SessionEvent) -> &'static str {
    match event {
        SessionEvent::StartRequested => "StartRequested",
        SessionEvent::ArmingComplete => "ArmingComplete",
        SessionEvent::ArmingFailed(_) => "ArmingFailed",
        SessionEvent::DeliveryFailed(_) => "DeliveryFailed",
        SessionEvent::DeliveryTimedOut(_) => "DeliveryTimedOut",
        SessionEvent::StopRequested => "StopRequested",
        SessionEvent::CancelArmed => "CancelArmed",
        SessionEvent::CancelAborted => "CancelAborted",
        SessionEvent::CancelExpired => "CancelExpired",
        SessionEvent::Tick { .. } => "Tick",
        SessionEvent::DeviceLost(_) => "DeviceLost",
        SessionEvent::Reset => "Reset",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COUNTDOWN_MS: u64 = 3000;

    fn machine() -> SessionMachine {
        SessionMachine::new(COUNTDOWN_MS)
    }

    /// Drives a machine to Recording, which most tests need as a starting point.
    fn recording() -> SessionMachine {
        let mut m = machine();
        m.handle(SessionEvent::StartRequested).expect("start");
        m.handle(SessionEvent::ArmingComplete).expect("armed");
        m
    }

    #[test]
    fn the_row_is_persisted_before_capture_ever_starts() {
        // This ordering IS the crash-recovery guarantee. If it inverts, a crash
        // during arming loses the session with no trace.
        let mut m = machine();
        let transition = m.handle(SessionEvent::StartRequested).expect("start");

        let persist_at = transition
            .effects
            .iter()
            .position(|e| matches!(e, Effect::PersistRow { .. }))
            .expect("a row must be persisted on start");
        let capture_at = transition
            .effects
            .iter()
            .position(|e| matches!(e, Effect::StartCapture))
            .expect("capture must start");

        assert!(
            persist_at < capture_at,
            "the session row must be written before the microphone opens"
        );
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: stop_is_instant, snappy
     * WHAT:  Releasing the hotkey returns to Idle in ONE transition, and the
     *        next recording can start immediately.
     * WHY:   THE test for the whole split. It used to go
     *        Recording -> Finalizing -> Delivered -> Reset -> Idle, and the
     *        user sat through all of it: the pill stayed up for as long as the
     *        model took, then reported a word count nobody asked for, and every
     *        hotkey press in between was refused as an illegal transition.
     *
     *        The second half is the half that matters. Reaching a terminal
     *        state quickly is worth nothing if the app still will not record —
     *        so this asserts StartRequested succeeds on the very next call,
     *        with no Reset and no waiting, which is the property the operator
     *        actually asked for.
     */
    #[test]
    fn releasing_the_hotkey_frees_the_app_immediately() {
        let mut m = recording();
        assert!(m.state().is_capturing());

        let transition = m.handle(SessionEvent::StopRequested).expect("stop");

        assert!(
            matches!(m.state(), SessionState::Idle),
            "capture is over the moment the key comes up, so the machine is free"
        );
        assert!(
            transition
                .effects
                .iter()
                .any(|e| matches!(e, Effect::HandOffToDelivery { .. })),
            "the words must be handed off, not dropped"
        );
        assert!(transition.effects.contains(&Effect::StopCapture));

        // No Reset, no linger, no waiting: straight into the next one.
        m.handle(SessionEvent::StartRequested)
            .expect("the next recording must start immediately");
        m.handle(SessionEvent::ArmingComplete).expect("armed");
        assert!(m.state().is_capturing());
    }

    /**
     * WHAT:  Two recordings back to back each hand off their own session.
     * WHY:   The hand-off carries the id that routes decode results back to the
     *        right transcript. If both hand-offs named the same session — or if
     *        the second reused the first's id because take_session_id had
     *        already emptied it — one recording's words would be appended to
     *        the other's, which reads as the model mishearing and is nearly
     *        impossible to diagnose from the outside.
     */
    #[test]
    fn back_to_back_recordings_hand_off_distinct_sessions() {
        fn handed_off(t: &Transition) -> SessionId {
            t.effects
                .iter()
                .find_map(|e| match e {
                    Effect::HandOffToDelivery { session_id } => Some(session_id.clone()),
                    _ => None,
                })
                .expect("every stop hands off")
        }

        let mut m = recording();
        let first = handed_off(&m.handle(SessionEvent::StopRequested).expect("stop"));

        m.handle(SessionEvent::StartRequested).expect("start again");
        m.handle(SessionEvent::ArmingComplete).expect("armed");
        let second = handed_off(&m.handle(SessionEvent::StopRequested).expect("stop"));

        assert_ne!(
            first, second,
            "each recording must hand off under its own id or their words will merge"
        );
    }

    #[test]
    fn cancel_armed_does_not_stop_capture() {
        // The whole reason double-Escape can resume losslessly.
        let mut m = recording();
        let transition = m.handle(SessionEvent::CancelArmed).expect("cancel armed");

        assert!(
            !transition.effects.contains(&Effect::StopCapture),
            "audio must keep flowing while the countdown runs"
        );
        assert!(m.state().is_capturing(), "CancelPending is still capturing");
    }

    #[test]
    fn a_second_escape_resumes_recording_where_it_left_off() {
        let mut m = recording();
        m.handle(SessionEvent::Tick {
            elapsed_ms: 4200,
            remaining_ms: 0,
        })
        .expect("tick");
        m.handle(SessionEvent::CancelArmed).expect("armed");
        m.handle(SessionEvent::CancelAborted).expect("aborted");

        match m.state() {
            // Elapsed time is carried through, so nothing was lost or reset.
            SessionState::Recording { elapsed_ms } => assert_eq!(*elapsed_ms, 4200),
            other => panic!("expected Recording, got {other:?}"),
        }
    }

    #[test]
    fn countdown_expiry_destroys_the_session_and_returns_to_idle() {
        let mut m = recording();
        m.handle(SessionEvent::CancelArmed).expect("armed");
        let transition = m.handle(SessionEvent::CancelExpired).expect("expired");

        assert!(matches!(m.state(), SessionState::Idle));
        assert!(
            transition
                .effects
                .iter()
                .any(|e| matches!(e, Effect::DestroySession { .. })),
            "escape must destroy the row, not mark it"
        );
        assert!(
            !transition
                .effects
                .iter()
                .any(|e| matches!(e, Effect::PersistOutcome { .. })),
            "a cancelled session must never be written to history"
        );
        assert!(
            m.session_id().is_none(),
            "the id must be consumed so nothing can refer to the deleted row"
        );
    }

    #[test]
    fn stopping_during_the_countdown_keeps_the_recording() {
        let mut m = recording();
        m.handle(SessionEvent::CancelArmed).expect("armed");
        let transition = m.handle(SessionEvent::StopRequested).expect("stop");

        assert!(matches!(m.state(), SessionState::Idle));
        assert!(transition.effects.contains(&Effect::AbortCountdown));
        assert!(
            !transition
                .effects
                .iter()
                .any(|e| matches!(e, Effect::DestroySession { .. })),
            "reaching for stop means keep it"
        );
    }

    #[test]
    fn a_lost_device_delivers_what_was_captured_rather_than_failing() {
        let mut m = recording();
        let err = AppError::new(ErrorCode::AudioDeviceLost, "AirPods disconnected");
        m.handle(SessionEvent::DeviceLost(err)).expect("device lost");

        assert!(
            matches!(m.state(), SessionState::Idle),
            "a disconnected headset must not discard the sentence"
        );
        // The sentence survives because it was handed off, not because the
        // machine held on to it.
        let mut m2 = recording();
        let t = m2
            .handle(SessionEvent::DeviceLost(AppError::new(
                ErrorCode::AudioDeviceLost,
                "gone",
            )))
            .expect("device lost");
        assert!(t
            .effects
            .iter()
            .any(|e| matches!(e, Effect::HandOffToDelivery { .. })));
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: device_lost_stops_capture, microphone_indicator
     * WHAT:  Every exit from a capturing state emits StopCapture — including
     *        the two DeviceLost arms, which did not.
     * WHY:   "Lost" means cpal stopped delivering samples. It does NOT mean our
     *        stream handle was closed, so without this effect the handle
     *        survived into Finalizing and was dropped without `stop()` when the
     *        next session assigned over it. The macOS microphone indicator
     *        stayed lit after recording ended, and in a local-first app a user
     *        reads that as "it is still listening" — which no amount of correct
     *        behaviour elsewhere argues them out of.
     *
     *        Written as a sweep over every capturing state rather than two
     *        hand-written cases, so a state added later cannot quietly reopen
     *        the same hole.
     */
    /**
     * SOURCE OF TRUTH KEYWORDS: escape_discards_immediately
     * WHAT:  With the setting on, Escape destroys the recording in one step.
     * WHY:   The setting changes which TRANSITION Escape takes, not what some
     *        handler does afterwards — so this is the assertion that it is
     *        wired at all. It also pins the effects to the same set
     *        CancelExpired emits: the two are the same teardown with and
     *        without a wait, and if they ever delete different things one of
     *        them is leaking a session row or an open microphone.
     */
    #[test]
    fn escape_discards_at_once_when_the_setting_is_on() {
        let mut m = recording();
        m.set_discard_on_escape(true);

        let transition = m.handle(SessionEvent::CancelArmed).expect("escape");

        assert!(
            matches!(m.state(), SessionState::Idle),
            "immediate discard must land in Idle, not CancelPending"
        );
        assert!(transition.effects.contains(&Effect::StopCapture));
        assert!(
            transition
                .effects
                .iter()
                .any(|e| matches!(e, Effect::DestroySession { .. })),
            "the recording must actually be destroyed"
        );
        assert!(
            !transition
                .effects
                .iter()
                .any(|e| matches!(e, Effect::StartCountdown { .. })),
            "there must be no countdown to change your mind with"
        );
    }

    #[test]
    fn escape_still_arms_a_countdown_when_the_setting_is_off() {
        // The default, and the behaviour everyone already has.
        let mut m = recording();
        m.set_discard_on_escape(false);

        let transition = m.handle(SessionEvent::CancelArmed).expect("escape");
        assert!(matches!(m.state(), SessionState::CancelPending { .. }));
        assert!(transition
            .effects
            .iter()
            .any(|e| matches!(e, Effect::StartCountdown { .. })));
        assert!(
            !transition.effects.contains(&Effect::StopCapture),
            "audio must keep flowing so a second Escape is lossless"
        );
    }

    /**
     * WHAT:  Immediate discard tears down exactly what the countdown path does.
     * WHY:   Two teardown paths that delete different things is how a session
     *        row survives a cancel, or a microphone stays open after one.
     */
    #[test]
    fn both_discard_paths_tear_down_the_same_things() {
        let mut immediate = recording();
        immediate.set_discard_on_escape(true);
        let fast = immediate.handle(SessionEvent::CancelArmed).expect("escape");

        let mut waited = recording();
        waited.set_discard_on_escape(false);
        waited.handle(SessionEvent::CancelArmed).expect("escape");
        let slow = waited.handle(SessionEvent::CancelExpired).expect("expired");

        let shape = |effects: &[Effect]| -> Vec<&'static str> {
            effects
                .iter()
                .map(|e| match e {
                    Effect::StopCapture => "StopCapture",
                    Effect::DestroySession { .. } => "DestroySession",
                    Effect::EmitState => "EmitState",
                    _ => "other",
                })
                .collect()
        };

        assert_eq!(
            shape(&fast.effects),
            shape(&slow.effects),
            "immediate discard and countdown expiry must tear down identically"
        );
    }

    #[test]
    fn losing_the_device_always_stops_the_capture() {
        for start_in_cancel_pending in [false, true] {
            let mut m = recording();
            if start_in_cancel_pending {
                m.handle(SessionEvent::CancelArmed).expect("armed cancel");
            }

            let err = AppError::new(ErrorCode::AudioDeviceLost, "AirPods disconnected");
            let transition = m.handle(SessionEvent::DeviceLost(err)).expect("device lost");

            assert!(
                transition.effects.contains(&Effect::StopCapture),
                "DeviceLost from {:?} left the microphone open",
                if start_in_cancel_pending {
                    "CancelPending"
                } else {
                    "Recording"
                }
            );
            assert!(
                transition
                    .effects
                    .iter()
                    .any(|e| matches!(e, Effect::HandOffToDelivery { .. })),
                "the words captured so far are still worth delivering"
            );
        }
    }

    /**
     * WHAT:  No transition out of a capturing state forgets StopCapture.
     * WHY:   The general form of the bug above. CancelArmed is the ONE
     *        deliberate exception — it keeps the audio flowing so a second
     *        Escape is lossless — and stating that exception here is what stops
     *        someone "fixing" it later.
     */
    #[test]
    fn leaving_a_capturing_state_releases_the_microphone() {
        let err = || AppError::new(ErrorCode::AudioDeviceLost, "gone");
        let exits: Vec<(&str, SessionEvent)> = vec![
            ("StopRequested", SessionEvent::StopRequested),
            ("DeviceLost", SessionEvent::DeviceLost(err())),
        ];

        for (name, event) in exits {
            let mut m = recording();
            let transition = m.handle(event).expect(name);
            assert!(
                transition.effects.contains(&Effect::StopCapture),
                "`{name}` left the microphone open"
            );
        }

        // The exception, asserted so it stays deliberate.
        let mut m = recording();
        let armed = m.handle(SessionEvent::CancelArmed).expect("cancel armed");
        assert!(
            !armed.effects.contains(&Effect::StopCapture),
            "arming a cancel must keep recording — a second Escape has to be lossless"
        );
    }

    /**
     * WHAT:  A background delivery failure is shown only when the pill is free.
     * WHY:   Delivery now runs while the user may be recording again, and
     *        taking the screen to report a recording they finished ten seconds
     *        ago would interrupt words they are speaking right now. The FSM
     *        enforces that by refusing the event outside Idle — a rule that has
     *        to live here rather than in the reporting code, because there is
     *        no other place that knows what is on screen.
     */
    #[test]
    fn a_background_failure_never_interrupts_a_live_recording() {
        let failure = || {
            SessionEvent::DeliveryFailed(AppError::new(
                ErrorCode::TranscriptionFailed,
                "decode failed",
            ))
        };

        let mut m = recording();
        assert!(
            m.handle(failure()).is_err(),
            "a previous recording's failure must not take the screen mid-sentence"
        );
        assert!(m.state().is_capturing(), "and must not disturb the recording");

        // Once nothing is in progress, the same failure is worth showing.
        let mut idle = machine();
        idle.handle(failure()).expect("idle can show it");
        assert!(matches!(idle.state(), SessionState::Failed { .. }));
    }

    #[test]
    fn illegal_events_are_rejected_rather_than_ignored() {
        let mut m = machine();
        // Nothing is recording, so there is nothing to stop.
        assert!(m.handle(SessionEvent::StopRequested).is_err());
        assert!(m.handle(SessionEvent::CancelArmed).is_err());
        assert!(m.handle(SessionEvent::CancelExpired).is_err());
        assert!(matches!(m.state(), SessionState::Idle));

        // And a second start while already recording must not open a second
        // microphone — the reentrancy guarantee, at the state level.
        let mut m = recording();
        assert!(m.handle(SessionEvent::StartRequested).is_err());
    }

    #[test]
    fn reset_returns_a_terminal_state_to_idle_and_clears_the_id() {
        let mut m = machine();
        m.handle(SessionEvent::DeliveryFailed(AppError::new(
            ErrorCode::TranscriptionFailed,
            "decode failed",
        )))
        .expect("failed");

        m.handle(SessionEvent::Reset).expect("reset");
        assert!(matches!(m.state(), SessionState::Idle));
        assert!(m.session_id().is_none());
    }

    #[test]
    fn every_state_emits_its_change_so_the_pill_can_never_go_stale() {
        let mut m = machine();
        for event in [
            SessionEvent::StartRequested,
            SessionEvent::ArmingComplete,
            SessionEvent::CancelArmed,
            SessionEvent::CancelAborted,
            SessionEvent::StopRequested,
        ] {
            let transition = m.handle(event).expect("legal transition");
            assert!(
                transition.effects.contains(&Effect::EmitState),
                "transition to {:?} did not emit its state",
                transition.to
            );
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: no_state_can_wedge, unrepresentable
     * WHAT:  Every state the machine can reach either captures, or accepts a
     *        new recording, or can be left by an event the app actually sends.
     * WHY:   This replaces a regression test for a specific wedge — a decode
     *        that never returned left the FSM in Finalizing forever, and every
     *        later hotkey press was rejected as illegal, so dictation was dead
     *        until restart. That state no longer exists: capture hands the work
     *        off and returns to Idle, so there is nothing to be stuck in.
     *
     *        The specific test was deleted rather than kept, because a test for
     *        a bug that has been designed out asserts nothing and quietly
     *        becomes decoration. What is worth guarding is the PROPERTY it was
     *        protecting, which is that a user is never one bad decode away from
     *        an app that will not record.
     */
    #[test]
    fn no_reachable_state_can_refuse_recording_forever() {
        // Idle and every capturing state are fine by construction. Failed is
        // the only other reachable state, and Reset is what leaves it — sent by
        // a timer the actor arms whenever it enters one.
        let mut m = machine();
        m.handle(SessionEvent::DeliveryFailed(AppError::new(
            ErrorCode::TranscriptionFailed,
            "decode failed",
        )))
        .expect("failed");
        assert!(matches!(m.state(), SessionState::Failed { .. }));

        m.handle(SessionEvent::Reset).expect("failed states are left");
        m.handle(SessionEvent::StartRequested)
            .expect("and the app records again");

        // The stop path is the one that used to wedge, and it now returns to a
        // state that takes a new recording with no intervening event at all.
        let mut m = recording();
        m.handle(SessionEvent::StopRequested).expect("stop");
        m.handle(SessionEvent::StartRequested)
            .expect("stopping can never cost the next recording");
    }
}
