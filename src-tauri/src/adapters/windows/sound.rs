/*!
 * SOURCE OF TRUTH KEYWORDS: play_feedback, FeedbackSound, sound
 * WHAT:  Audio cues confirming recording start, stop, or failure on Windows.
 * WHY:   Provides immediate auditory confirmation that the global hotkey registered.
 * WHERE: Called by the session actor on state transitions.
 */

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackSound {
    Start,
    Stop,
    Failed,
}

pub fn play_feedback(_sound: FeedbackSound) {
    // Non-blocking subtle notification or no-op on Windows
}
