/*!
 * SOURCE OF TRUTH KEYWORDS: ModifierTap, TapDetector, TapEvent, TapOutcome,
 *   watch_modifier_tap, DOUBLE_TAP_WINDOW, TAP_MAX_HOLD
 * WHAT:  Lets a bare modifier — Option on its own — be the dictation hotkey,
 *        by watching flagsChanged through a CGEventTap and recognising a
 *        deliberate double-tap.
 * WHY:   The global-shortcut plugin cannot express this. Its API is
 *        `Shortcut::new(modifiers, code)` — a chord — so there is no key for a
 *        lone modifier to be. The operator asked for Option on its own in plain
 *        words, so the mechanism has to come from somewhere else, and an event
 *        tap is the only thing on macOS that reports a modifier changing state
 *        without a key accompanying it.
 *
 *        **This is the most dangerous code in the app and the danger is
 *        specific: a false positive starts recording the user and pastes text
 *        into whatever they are working in.** Option is held constantly — every
 *        accented character, every menu peek, every half-typed shortcut. So the
 *        recogniser is deliberately hard to trigger:
 *
 *          1. TAPS_REQUIRED taps, currently ONE. This started as a double-tap,
 *             on the reasoning that a single tap fires whenever someone presses
 *             Option to peek at a menu and changes their mind. The operator
 *             overruled it and answered the objection himself: "Single click to
 *             toggle is more than enough. And if the user doesn't want it,
 *             they'll just shut off the app." It is his machine and his
 *             workflow, and he is the one who knows how often he peeks at a
 *             menu. Changing it back is one constant.
 *
 *             The rules below are NOT preferences and did not change with it. A
 *             tap firing while he holds Option for a chord would be a
 *             regression, not a matter of taste.
 *          2. Any other key pressed between the taps CANCELS. Typing "café" is
 *             Option-down, E-down, Option-up — the keystroke invalidates it.
 *          3. Any other MODIFIER held cancels. ⌥⇧ is the start of a chord.
 *          4. Holding the modifier longer than TAP_MAX_HOLD is not a tap. Held
 *             Option means the user is reading a menu, not summoning dictation.
 *
 *        The recognition logic is a PURE state machine — `TapDetector` — and
 *        the tap thread below only feeds it events. That is what makes the
 *        rules above testable at all: proving "typing an accented character
 *        never fires" needs a unit test, not a person at a keyboard.
 *
 *        Listen-only. The tap never consumes an event, so even a total failure
 *        of this logic cannot swallow a keystroke from another app.
 * WHERE: Started by bootstrap when the dictation binding is modifier-only;
 *        replaced by the global-shortcut plugin for every ordinary chord.
 */

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crate::types::KeyModifier;

/**
 * SOURCE OF TRUTH KEYWORDS: TAP_MAX_HOLD
 * WHAT:  Longest a modifier may be held and still count as a tap.
 * WHY:   A tap is a gesture, a hold is a mode. 400ms is comfortably longer than
 *        a deliberate tap and comfortably shorter than the time anyone holds
 *        Option to look at the alternate menu items it reveals.
 */
const TAP_MAX_HOLD: Duration = Duration::from_millis(400);

/**
 * SOURCE OF TRUTH KEYWORDS: DOUBLE_TAP_WINDOW
 * WHAT:  How long the second tap has to arrive.
 * WHY:   500ms, matching the platform's own double-click and dictation feel. Too
 *        short and a deliberate gesture fails and the user concludes the feature
 *        is broken; too long and two unrelated Option presses a beat apart start
 *        a recording.
 */
const DOUBLE_TAP_WINDOW: Duration = Duration::from_millis(500);

/**
 * SOURCE OF TRUTH KEYWORDS: TAPS_REQUIRED
 * WHAT:  How many clean taps make the gesture.
 * WHY:   One, at the operator's explicit request. Two is meaningfully safer
 *        against accidental triggering and this is the only line that needs to
 *        change to go back — the pairing logic below is written for a count
 *        rather than hardcoded to a pair, precisely so that reversal stays
 *        cheap.
 */
pub const TAPS_REQUIRED: usize = 1;

/// What the tap thread observed, normalised so the detector never touches
/// CoreGraphics types and can therefore be tested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapEvent {
    /// The watched modifier went down, with no other modifier held.
    ModifierDown,
    /// The watched modifier went up.
    ModifierUp,
    /// Any other modifier changed state. Starts a chord; cancels a gesture.
    OtherModifierChanged,
    /// A non-modifier key was pressed. Cancels a gesture.
    KeyPressed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TapOutcome {
    /// Nothing yet.
    Waiting,
    /// A complete double-tap. Fire the hotkey.
    Fire,
}

/**
 * SOURCE OF TRUTH KEYWORDS: TapDetector
 * WHAT:  The pure recogniser. Feed it events and monotonic timestamps; it says
 *        when a double-tap has happened.
 * WHY:   Pure so the rules that keep it from firing during normal typing are
 *        unit-testable. Takes the elapsed time as a parameter rather than
 *        reading a clock, for the same reason.
 * WHERE: Driven by the tap thread; exercised directly by the tests below.
 */
#[derive(Debug, Default)]
pub struct TapDetector {
    /// When the modifier went down, if it is currently down and still clean.
    down_at_ms: Option<u64>,
    /// A key or another modifier intervened; this press cannot be a tap.
    dirty: bool,
    /// When the first completed tap landed, and how many have accumulated.
    first_tap_ms: Option<u64>,
    taps: usize,
}

impl TapDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /**
     * WHAT:  Applies one event at `now_ms`, returning whether to fire.
     * WHY:   Every cancelling condition clears BOTH the in-progress press and
     *        the pending first tap. Half-cancelling — forgetting the press but
     *        keeping the first tap — is how a stray Option during typing gets
     *        paired with a later deliberate one and fires a recording nobody
     *        asked for.
     */
    pub fn on_event(&mut self, event: TapEvent, now_ms: u64) -> TapOutcome {
        match event {
            TapEvent::KeyPressed | TapEvent::OtherModifierChanged => {
                self.dirty = true;
                self.down_at_ms = None;
                self.first_tap_ms = None;
                self.taps = 0;
            }
            TapEvent::ModifierDown => {
                self.dirty = false;
                self.down_at_ms = Some(now_ms);
            }
            TapEvent::ModifierUp => {
                let Some(down_at) = self.down_at_ms.take() else {
                    // Up without a clean down: the press was invalidated, or we
                    // started listening mid-hold.
                    self.dirty = false;
                    return TapOutcome::Waiting;
                };
                if self.dirty || now_ms.saturating_sub(down_at) > TAP_MAX_HOLD.as_millis() as u64 {
                    self.first_tap_ms = None;
                    self.taps = 0;
                    self.dirty = false;
                    return TapOutcome::Waiting;
                }

                // A clean tap. It extends the run only if it arrived in time.
                let in_time = self
                    .first_tap_ms
                    .is_some_and(|first| {
                        now_ms.saturating_sub(first) <= DOUBLE_TAP_WINDOW.as_millis() as u64
                    });
                self.taps = if in_time { self.taps + 1 } else { 1 };
                self.first_tap_ms = Some(now_ms);

                if self.taps >= TAPS_REQUIRED {
                    self.taps = 0;
                    self.first_tap_ms = None;
                    return TapOutcome::Fire;
                }
            }
        }
        TapOutcome::Waiting
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: ModifierTap, watch_modifier_tap
 * WHAT:  A running event tap. Dropping it stops the thread.
 * WHERE: Held by bootstrap for as long as a modifier-only binding is active.
 */
pub struct ModifierTap {
    stop: Arc<AtomicBool>,
}

impl Drop for ModifierTap {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The CGEventFlags bit for each modifier we can watch.
fn flag_for(modifier: KeyModifier) -> u64 {
    // Values from CGEventFlags; named constants rather than the crate's enum so
    // the bit test below is a plain integer operation.
    match modifier {
        KeyModifier::Option => 0x0008_0000,  // kCGEventFlagMaskAlternate
        KeyModifier::Shift => 0x0002_0000,   // kCGEventFlagMaskShift
        KeyModifier::Control => 0x0004_0000, // kCGEventFlagMaskControl
        KeyModifier::Command => 0x0010_0000, // kCGEventFlagMaskCommand
    }
}

/// Every modifier bit, so "any other modifier" is one mask test.
const ALL_MODIFIER_FLAGS: u64 = 0x0002_0000 | 0x0004_0000 | 0x0008_0000 | 0x0010_0000;

/**
 * SOURCE OF TRUTH KEYWORDS: watch_modifier_tap
 * WHAT:  Starts watching for a double-tap of `modifier`, calling `on_trigger`
 *        each time one happens.
 * WHY:   Runs on its OWN thread with its own CFRunLoop, because an event tap
 *        only delivers while a run loop is running and the main thread's belongs
 *        to Tauri. Listen-only, so a bug here can slow the event stream but can
 *        never eat a keystroke.
 *
 *        Requires Accessibility. Without it `CGEventTap::with_enabled` fails and
 *        this returns None rather than pretending to work — a hotkey that
 *        silently never fires is the failure this whole batch exists to remove.
 * WHERE: bootstrap::register_hotkeys, for a modifier-only binding.
 */
pub fn watch_modifier_tap(
    modifier: KeyModifier,
    // `Sync` as well as `Send`: the handler is shared across tap re-installs
    // via an Arc. Every caller passes a closure over an AppHandle, which is
    // already both.
    on_trigger: impl Fn() + Send + Sync + 'static,
) -> Option<ModifierTap> {
    use core_foundation::runloop::CFRunLoop;
    use core_graphics::event::{
        CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
        CallbackResult,
    };

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = Arc::clone(&stop);
    let started = Arc::new(AtomicBool::new(false));
    let thread_started = Arc::clone(&started);

    let watched = flag_for(modifier);
    // Shared because the tap is re-installed in a loop below and each install
    // needs its own copy of the handler.
    let on_trigger = Arc::new(on_trigger);

    std::thread::Builder::new()
        .name("murmur-modifier-tap".into())
        .spawn(move || {
            /*
             * SOURCE OF TRUTH KEYWORDS: reinstall_loop, TapDisabledByTimeout,
             *   secure_input_kills_taps, permission_without_restart
             * WHY THIS IS A LOOP AND NOT A SINGLE INSTALL:
             *
             * macOS DISABLES event taps behind your back, and tells you by
             * delivering one of two synthetic events — TapDisabledByTimeout if
             * a callback was slow, TapDisabledByUserInput when something takes
             * exclusive control of input, which INCLUDES every Secure Input
             * session: a password field, a sudo prompt, a keychain dialog, and
             * the permission prompts our own onboarding puts on screen.
             *
             * A callback that ignores those events leaves the tap dead
             * permanently. That is precisely what the operator hit: the hotkey
             * worked, he granted a permission, and it never fired again until
             * he restarted the app. There is no error and nothing in the log —
             * the tap simply stops being delivered events.
             *
             * The same loop fixes a second complaint with the same shape. If
             * Accessibility is not granted yet, installation FAILS; without a
             * retry, granting it later does nothing until the app is restarted,
             * which is exactly what he described having to do. Retrying means
             * the grant takes effect on its own.
             */
            // The tap callback is `Fn`, not `FnMut`, so the recogniser's state
            // lives behind a lock. Uncontended in practice: only the run-loop
            // thread of this function ever touches it.
            let mut reported_failure = false;

            while !thread_stop.load(Ordering::Relaxed) {
            // Re-created per install: a tap that was disabled mid-gesture must
            // not resume with half a gesture remembered.
            let state = parking_lot::Mutex::new((TapDetector::new(), 0u64));
            let start = std::time::Instant::now();

            let reinstall = Arc::new(AtomicBool::new(false));
            let callback_reinstall = Arc::clone(&reinstall);
            let on_trigger = Arc::clone(&on_trigger);
            let loop_stop = Arc::clone(&thread_stop);

            let installed = CGEventTap::with_enabled(
                CGEventTapLocation::HID,
                CGEventTapPlacement::HeadInsertEventTap,
                // Listen-only: we observe and never modify or drop.
                CGEventTapOptions::ListenOnly,
                vec![CGEventType::FlagsChanged, CGEventType::KeyDown],
                move |_proxy, event_type, event| {
                    let now_ms = start.elapsed().as_millis() as u64;
                    let mut guard = state.lock();
                    let (detector, previous_flags) = &mut *guard;

                    let observed = match event_type {
                        // The system just switched us off. Ask the thread to
                        // install a fresh tap; re-enabling this one from inside
                        // its own callback would need the port we do not hold.
                        CGEventType::TapDisabledByTimeout
                        | CGEventType::TapDisabledByUserInput => {
                            tracing::warn!(
                                ?event_type,
                                "the modifier tap was disabled by macOS; reinstalling"
                            );
                            callback_reinstall.store(true, Ordering::Relaxed);
                            return CallbackResult::Keep;
                        }
                        CGEventType::KeyDown => Some(TapEvent::KeyPressed),
                        CGEventType::FlagsChanged => {
                            let flags = event.get_flags().bits();
                            let changed = flags ^ *previous_flags;
                            *previous_flags = flags;

                            if changed & watched != 0 {
                                // Our modifier moved. Down only counts as the
                                // start of a gesture when it is alone.
                                if flags & watched != 0 {
                                    if flags & ALL_MODIFIER_FLAGS & !watched != 0 {
                                        Some(TapEvent::OtherModifierChanged)
                                    } else {
                                        Some(TapEvent::ModifierDown)
                                    }
                                } else {
                                    Some(TapEvent::ModifierUp)
                                }
                            } else if changed & ALL_MODIFIER_FLAGS != 0 {
                                Some(TapEvent::OtherModifierChanged)
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };

                    if let Some(observed) = observed {
                        if detector.on_event(observed, now_ms) == TapOutcome::Fire {
                            on_trigger();
                        }
                    }

                    CallbackResult::Keep
                },
                || {
                    thread_started.store(true, Ordering::Relaxed);
                    // Wakes every 250ms so the stop flag and a reinstall
                    // request are both noticed promptly.
                    while !loop_stop.load(Ordering::Relaxed)
                        && !reinstall.load(Ordering::Relaxed)
                    {
                        CFRunLoop::run_in_mode(
                            unsafe { core_foundation::runloop::kCFRunLoopDefaultMode },
                            Duration::from_millis(250),
                            false,
                        );
                    }
                },
            );

            if installed.is_err() {
                // Logged once, not every retry: Accessibility may simply not be
                // granted yet, and a line every two seconds forever would bury
                // everything else in the file.
                if !reported_failure {
                    reported_failure = true;
                    tracing::error!(
                        "could not install the modifier event tap; Accessibility is probably \
                         not granted. Retrying, so granting it takes effect without a restart."
                    );
                }
                std::thread::sleep(Duration::from_secs(2));
            } else {
                reported_failure = false;
            }
            }
        })
        .ok()?;

    // Give the thread a moment to report that the tap installed, so a failure
    // is logged at startup rather than discovered when the hotkey does nothing.
    std::thread::sleep(Duration::from_millis(50));
    if !started.load(Ordering::Relaxed) {
        tracing::warn!("the modifier tap thread has not reported ready yet");
    }

    Some(ModifierTap { stop })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Milliseconds, for readability in the sequences below.
    const TAP: u64 = 60;

    fn detector() -> TapDetector {
        TapDetector::new()
    }

    /// A clean gesture fires. Written against TAPS_REQUIRED rather than a
    /// hardcoded pair, so flipping that constant does not invalidate the test.
    fn perform_gesture(d: &mut TapDetector, from_ms: u64) -> TapOutcome {
        let mut outcome = TapOutcome::Waiting;
        for tap in 0..TAPS_REQUIRED {
            let t = from_ms + tap as u64 * 200;
            d.on_event(TapEvent::ModifierDown, t);
            outcome = d.on_event(TapEvent::ModifierUp, t + TAP);
        }
        outcome
    }

    #[test]
    fn a_clean_gesture_fires() {
        let mut d = detector();
        assert_eq!(perform_gesture(&mut d, 0), TapOutcome::Fire);
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: typing_never_fires
     * WHAT:  Typing an accented character does not start a recording.
     * WHY:   THE test for this module. On a Mac, "café" is Option-down, E-down,
     *        Option-up — a modifier press that begins and ends with no other
     *        modifier involved, which is exactly the shape of a tap. The
     *        intervening keystroke is the only thing that distinguishes them,
     *        and if this rule ever breaks, the app starts recording people
     *        mid-sentence and pasting into their document.
     */
    #[test]
    fn typing_an_accented_character_never_fires() {
        let mut d = detector();
        for round in 0..4 {
            let t = round * 300;
            assert_eq!(d.on_event(TapEvent::ModifierDown, t), TapOutcome::Waiting);
            assert_eq!(d.on_event(TapEvent::KeyPressed, t + 20), TapOutcome::Waiting);
            assert_eq!(
                d.on_event(TapEvent::ModifierUp, t + 40),
                TapOutcome::Waiting,
                "typing produced a tap on round {round}"
            );
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: a_dirtied_tap_never_fires
     * WHAT:  A press with a keystroke inside it never fires, however clean it
     *        looks from the outside.
     * WHY:   The invariant that survives any TAPS_REQUIRED. Option-down,
     *        E-down, Option-up is the shape of typing an accented character AND
     *        the shape of a tap; the intervening keystroke is the only thing
     *        that tells them apart, so it must invalidate the press outright
     *        rather than merely reset a counter.
     */
    #[test]
    fn a_press_containing_a_keystroke_never_fires() {
        let mut d = detector();
        d.on_event(TapEvent::ModifierDown, 0);
        d.on_event(TapEvent::KeyPressed, 20);
        assert_eq!(d.on_event(TapEvent::ModifierUp, 40), TapOutcome::Waiting);
    }

    #[test]
    fn holding_the_modifier_to_read_a_menu_never_fires() {
        let mut d = detector();
        // Held well past TAP_MAX_HOLD, twice.
        d.on_event(TapEvent::ModifierDown, 0);
        assert_eq!(d.on_event(TapEvent::ModifierUp, 1_500), TapOutcome::Waiting);
        d.on_event(TapEvent::ModifierDown, 1_600);
        assert_eq!(
            d.on_event(TapEvent::ModifierUp, 3_000),
            TapOutcome::Waiting,
            "a hold is a mode, not a gesture"
        );
    }



    #[test]
    fn an_up_with_no_down_is_ignored() {
        // Happens when the tap starts while the user already holds the key.
        let mut d = detector();
        assert_eq!(d.on_event(TapEvent::ModifierUp, 10), TapOutcome::Waiting);
    }
}
