/*!
 * SOURCE OF TRUTH KEYWORDS: play_feedback, FeedbackSound, NSSound,
 *   SOUND_START, SOUND_STOP, SOUND_FAILED
 * WHAT:  The three short sounds that confirm a recording started, stopped, or
 *        failed.
 * WHY:   Without them you do not trust that the hotkey registered. Murmur has
 *        no window, no Dock icon and — in the moment you press the key — no
 *        visible pill yet, so the sound is the ONLY immediate confirmation that
 *        anything happened. That is why this is a feature and not decoration.
 *
 *        macOS system sounds are used rather than bundled audio, deliberately:
 *        they are already the sounds this user's machine makes, they respect
 *        the system alert volume, and they add nothing to the bundle. They also
 *        cannot be missing — every macOS install has them.
 *
 *        Playback is fire-and-forget and never blocks. A sound that fails is
 *        not worth an error: the recording is what matters and it is already
 *        underway.
 * WHERE: Called by the session actor on state transitions, gated on the
 *        audio-feedback setting.
 */

use objc2_app_kit::NSSound;
use objc2_foundation::NSString;

/**
 * SOURCE OF TRUTH KEYWORDS: SOUND_START, SOUND_STOP, SOUND_FAILED
 * WHAT:  Which system sound each event uses.
 * WHY:   Chosen against the operator's description — "a more muffled, minimal
 *        sound effect for on and off" — after he reported the old set as
 *        wrong, and specifically that one of them "feels kind of strange to
 *        have an error message sound effect". That was `Basso`, the macOS error
 *        buzz, and it played on any failed dictation.
 *
 *        Start and Stop are a matched pair that FALLS, the way a switch does.
 *        That direction is the whole design and it is now measured rather than
 *        asserted — spectral centroid of the loudest window of each file:
 *
 *            Purr      614 Hz   peak 0.23   audible 0.13s   <- Start
 *            Bottle    309 Hz   peak 0.24   audible 0.27s   <- Stop
 *            Submarine 512 Hz   peak 0.23   audible 0.59s   <- Failed
 *
 *        An octave of descent, from the quietest short sounds macOS ships.
 *        This pair replaces Bottle/Pop, which was chosen on documented
 *        character alone and turned out to do the OPPOSITE: Pop measures
 *        601 Hz against Bottle's 309, so the old "off" ROSE. A rising tone on
 *        the gesture that ends a recording reads as something starting, which
 *        is exactly the confusion the pair exists to prevent. Nobody could hear
 *        the mistake in the code, and the person who wrote it flagged that he
 *        had not been able to measure pitch rather than quoting a number he did
 *        not trust — which is why it was still fixable.
 *
 *        `Tink` is gone because it is a bright tick at 685 Hz — the opposite of
 *        muffled. `Basso` is gone because it is the error buzz he objected to.
 *
 *        Failure stays clearly distinct, because it is the ONLY non-visual
 *        signal that a dictation produced nothing, and a user who cannot hear
 *        the difference between "stopped" and "went wrong" has lost it. It is
 *        separated by LENGTH rather than by harshness: 0.59s against 0.13s and
 *        0.27s, so it is unmistakably longer without being an alert.
 *
 *        Still system sounds rather than bundled audio, for the reasons in the
 *        module WHY — they track the user's alert volume, add nothing to the
 *        bundle, and cannot be missing. If this set is still wrong to his ear,
 *        bundling three custom tones is the next step and only this constant
 *        block changes.
 */
const SOUND_START: &str = "Purr";
const SOUND_STOP: &str = "Bottle";
const SOUND_FAILED: &str = "Submarine";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedbackSound {
    /// Recording has begun and the microphone is live.
    Start,
    /**
     * Recording has ENDED. Played the instant capture stops, not when the text
     * lands.
     * WHY: it used to fire on delivery, which is a second or two later once
     * the model has decoded and the paste has happened. The operator described
     * exactly that: "the turn off sound effect right now happens when the paste
     * takes place. Instead, the second I turn off the recording, the sound
     * effects should play for the off button." A confirmation that arrives
     * after the thing it confirms is not a confirmation.
     */
    Stop,
    /// Something went wrong and there is nothing to paste.
    Failed,
}

impl FeedbackSound {
    fn system_name(self) -> &'static str {
        match self {
            FeedbackSound::Start => SOUND_START,
            FeedbackSound::Stop => SOUND_STOP,
            FeedbackSound::Failed => SOUND_FAILED,
        }
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: play_feedback
 * WHAT:  Plays one system sound, without blocking.
 * WHY:   Errors are swallowed on purpose — see the module WHY. There is no
 *        useful recovery from "the confirmation beep did not play", and
 *        surfacing it would interrupt a working recording to report a
 *        cosmetic problem.
 * WHERE: The session actor, on Recording and on terminal states.
 */
pub fn play_feedback(sound: FeedbackSound) {
    // Safe bindings in objc2 0.6. Playback is asynchronous inside AppKit, so
    // this returns immediately and never blocks the caller.
    let name = NSString::from_str(sound.system_name());
    match NSSound::soundNamed(&name) {
        Some(handle) => {
            handle.play();
        }
        None => tracing::debug!(
            sound = sound.system_name(),
            "system sound not available on this machine"
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_sound_names_a_real_macos_system_sound() {
        // These live in /System/Library/Sounds and ship with every install.
        // If one is ever renamed, this catches it here rather than as silence.
        for sound in [
            FeedbackSound::Start,
            FeedbackSound::Stop,
            FeedbackSound::Failed,
        ] {
            let path = format!("/System/Library/Sounds/{}.aiff", sound.system_name());
            assert!(
                std::path::Path::new(&path).exists(),
                "system sound `{}` is missing",
                sound.system_name()
            );
        }
    }

    #[test]
    fn playing_a_sound_never_panics() {
        // It runs on a hot path; a panic here would take down a recording.
        play_feedback(FeedbackSound::Start);
    }
}
