/*!
 * SOURCE OF TRUTH KEYWORDS: is_hallucination, normalise_for_match,
 *   is_digital_silence, SILENCE_PEAK_FLOOR, LIKELY_SILENCE_NO_SPEECH_PROB,
 *   LIKELY_SILENCE_RMS_DBFS, rms_dbfs
 * WHAT:  The two guards that stand between a silent buffer and the words
 *        "Thanks for watching!" appearing in the user's document.
 * WHY:   Whisper invents text from silence, and the inventions are a small,
 *        well-known set of subtitle strings baked in by training-data
 *        contamination. The rule that makes a blocklist safe rather than
 *        destructive is that a phrase is dropped ONLY when it is the entire
 *        segment — "thank you" mid-sentence is a real thing people say, and an
 *        editor that deletes it has broken dictation to fix a cosmetic bug.
 *        The riskiest entries are short ones a person might genuinely utter
 *        alone, so those carry a second condition: the chunk must also have
 *        been too quiet to be speech. That qualifier is measured from the audio
 *        rather than taken from whisper's `no_speech_probability`, because
 *        that field reads 0.000 on this build even for text invented over a
 *        -70 dBFS buffer — see LIKELY_SILENCE_NO_SPEECH_PROB.
 * WHERE: Applied by adapters/whisper/engine.rs to every decoded segment; the
 *        phrase table itself lives in blocklist.rs. The
 *        primary VAD gate that stops most of this lives upstream in
 *        pipeline/vad.rs. Rules from docs/03-IMPLEMENTATION-NOTES.md §2.4.
 */

/**
 * SOURCE OF TRUTH KEYWORDS: SILENCE_PEAK_FLOOR, is_digital_silence
 * WHAT:  Peak amplitude below which a buffer is treated as containing nothing.
 * WHY:   Deliberately set at digital silence (-80 dBFS) rather than at a noise
 *        floor. This is a last-resort backstop, not the VAD: a real microphone
 *        in a quiet room sits far above it, so this can only ever reject a
 *        buffer that is genuinely empty — a dropped device, a muted input, a
 *        zero-filled fragment. Raising it to something that "works better"
 *        would start eating quiet speech, which is the failure this guard is
 *        supposed to be too dumb to cause.
 * WHERE: Checked in engine.rs before the model is invoked at all.
 */
pub const SILENCE_PEAK_FLOOR: f32 = 1.0e-4;

/**
 * SOURCE OF TRUTH KEYWORDS: LIKELY_SILENCE_NO_SPEECH_PROB
 * WHAT:  The no-speech probability above which whisper's own verdict qualifies
 *        a single-word blocklist entry.
 * WHY:   **Measured as inert on this build, and kept anyway.** whisper.cpp
 *        1.8.3 with the docs/03 §2.2 parameter set reports
 *        `no_speech_probability` as exactly 0.000 for every segment — including
 *        text it invented over a -70 dBFS buffer. A defence keyed on it alone
 *        can never fire, which is why the real qualifier below is measured from
 *        the audio instead. This stays so the check improves rather than
 *        breaks if a future whisper.cpp starts populating the field.
 * WHERE: One half of the `WhenLikelySilence` condition in is_hallucination.
 */
pub const LIKELY_SILENCE_NO_SPEECH_PROB: f32 = 0.2;

/**
 * SOURCE OF TRUTH KEYWORDS: LIKELY_SILENCE_RMS_DBFS, rms_dbfs
 * WHAT:  The chunk loudness below which we treat a one-word blocklisted segment
 *        as invented rather than said.
 * WHY:   This is the qualifier that actually fires, and the threshold is
 *        measured rather than guessed. On this machine: whisper invented
 *        "Thank you." over a -70.5 dBFS buffer and again over a -57.0 dBFS one,
 *        while genuine dictation sits at -16 dBFS. -50 dBFS is comfortably
 *        below anything a person talking into a microphone produces and
 *        comfortably above a room's noise floor, so it separates the two cases
 *        without a judgement call.
 *        Note this qualifies ONLY the single-word entries. Nothing here can
 *        discard a sentence, however quiet — a distant speaker is still a
 *        speaker.
 * WHERE: The other half of the `WhenLikelySilence` condition; the level is
 *        measured once per chunk by adapters/whisper/engine.rs.
 */
pub const LIKELY_SILENCE_RMS_DBFS: f32 = -50.0;

use super::blocklist::{blocklist_for, DropRule};

/**
 * SOURCE OF TRUTH KEYWORDS: normalise_for_match
 * WHAT:  Lowercases, drops everything that is not alphanumeric or whitespace,
 *        and collapses runs of whitespace.
 * WHY:   Whisper's punctuation and capitalisation of an invented phrase are not
 *        stable — "Thank you.", "thank you!", " Thank you " are the same
 *        artifact. Matching on raw text would catch one spelling and miss the
 *        rest, which is how a blocklist ends up looking like it works.
 * WHERE: Used on both sides of every comparison in is_hallucination.
 */
pub fn normalise_for_match(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut pending_space = false;
    for ch in text.chars().flat_map(char::to_lowercase) {
        if ch.is_whitespace() {
            pending_space = !out.is_empty();
        } else if ch.is_alphanumeric() {
            if pending_space {
                out.push(' ');
                pending_space = false;
            }
            out.push(ch);
        }
        // Punctuation and symbols are dropped without breaking the word.
    }
    out
}

/**
 * SOURCE OF TRUTH KEYWORDS: is_hallucination
 * WHAT:  True when this segment's ENTIRE text is a known invention and should
 *        be discarded.
 * WHY:   Whole-segment only. This function deliberately has no way to edit a
 *        segment, because the moment a blocklist can rewrite the middle of a
 *        sentence it will eventually delete something the user said.
 * WHERE: adapters/whisper/engine.rs, per decoded segment.
 */
pub fn is_hallucination(
    text: &str,
    language: Option<&str>,
    no_speech_prob: f32,
    rms_dbfs: f32,
) -> bool {
    let normalised = normalise_for_match(text);
    if normalised.is_empty() {
        return true;
    }

    // Whisper's own verdict on the audio, which would fire regardless of
    // wording. Measured inert on 1.8.3; see LIKELY_SILENCE_NO_SPEECH_PROB.
    if no_speech_prob > super::params::NO_SPEECH_THOLD {
        return true;
    }

    let likely_silence =
        rms_dbfs < LIKELY_SILENCE_RMS_DBFS || no_speech_prob > LIKELY_SILENCE_NO_SPEECH_PROB;

    blocklist_for(language).any(|phrase| {
        phrase.text == normalised
            && match phrase.rule {
                DropRule::Always => true,
                DropRule::WhenLikelySilence => likely_silence,
            }
    })
}

/**
 * SOURCE OF TRUTH KEYWORDS: rms_dbfs
 * WHAT:  The root-mean-square level of a buffer, in dBFS.
 * WHY:   RMS rather than peak, because peak is set by a single click or a
 *        keyboard tap and says nothing about whether anyone spoke. RMS over the
 *        whole chunk is what separates "a room" from "a person talking in a
 *        room", which is the distinction the blocklist needs.
 * WHERE: Computed once per chunk in adapters/whisper/engine.rs and passed into
 *        is_hallucination for every segment of that chunk.
 */
pub fn rms_dbfs(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return f32::NEG_INFINITY;
    }
    let mean_square = samples.iter().map(|s| s * s).sum::<f32>() / samples.len() as f32;
    // The floor keeps a digitally silent buffer from producing -inf, which
    // would compare strangely and print worse.
    20.0 * mean_square.sqrt().max(1.0e-9).log10()
}

/**
 * SOURCE OF TRUTH KEYWORDS: is_digital_silence
 * WHAT:  True when no sample in the buffer rises above SILENCE_PEAK_FLOOR.
 * WHY:   Cheaper than a decode by four orders of magnitude, and it closes the
 *        one case the blocklist cannot: an invention we have never seen before.
 *        See SILENCE_PEAK_FLOOR for why the threshold is set where it is.
 * WHERE: adapters/whisper/engine.rs, before the model is touched.
 */
pub fn is_digital_silence(samples: &[f32]) -> bool {
    !samples
        .iter()
        .any(|sample| sample.abs() > SILENCE_PEAK_FLOOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Measured level of genuine dictation on the target machine.
    const SPEAKING: f32 = -16.0;
    /// Measured level of a buffer whisper invented "Thank you." over.
    const NEAR_SILENT: f32 = -70.0;

    #[test]
    fn a_zero_filled_buffer_is_silence() {
        assert!(is_digital_silence(&vec![0.0_f32; 16_000]));
        assert!(is_digital_silence(&[]));
    }

    #[test]
    fn a_quiet_room_is_not_silence() {
        // -60 dBFS, well below speech and well above the floor.
        let noise: Vec<f32> = (0..16_000)
            .map(|i| if i % 2 == 0 { 1.0e-3 } else { -1.0e-3 })
            .collect();
        assert!(!is_digital_silence(&noise));
    }

    #[test]
    fn punctuation_and_case_do_not_change_the_verdict() {
        for spelling in ["Thanks for watching!", "thanks for watching", "THANKS, FOR WATCHING."] {
            assert!(
                is_hallucination(spelling, Some("en"), 0.0, SPEAKING),
                "{spelling} should be dropped"
            );
        }
    }

    #[test]
    fn a_blocklisted_phrase_inside_a_sentence_is_never_touched() {
        assert!(!is_hallucination(
            "Thank you for sending that over this morning.",
            Some("en"),
            0.0,
            SPEAKING
        ));
        assert!(!is_hallucination(
            "I want to say thanks for watching the recording before we meet.",
            Some("en"),
            0.0,
            SPEAKING
        ));
    }

    #[test]
    fn a_risky_single_word_survives_when_the_audio_was_confidently_speech() {
        // Somebody genuinely saying "You." into a microphone.
        assert!(!is_hallucination("You.", Some("en"), 0.0, SPEAKING));
        // The same word over a buffer far too quiet to have contained it.
        assert!(is_hallucination("You.", Some("en"), 0.0, NEAR_SILENT));
        // And via whisper's own verdict, for the day it starts reporting one.
        assert!(is_hallucination("You.", Some("en"), 0.45, SPEAKING));
    }

    #[test]
    fn whisper_own_no_speech_verdict_drops_anything() {
        assert!(is_hallucination(
            "an otherwise perfectly ordinary sentence",
            Some("en"),
            0.9,
            SPEAKING
        ));
    }

    #[test]
    fn an_empty_segment_is_dropped() {
        assert!(is_hallucination("   ", Some("en"), 0.0, SPEAKING));
        assert!(is_hallucination("...", Some("en"), 0.0, SPEAKING));
    }

    #[test]
    fn a_language_with_no_list_still_gets_the_universal_entries() {
        assert!(is_hallucination("[MUSIC]", Some("sv"), 0.0, SPEAKING));
        // ...but not English's single-word guesses.
        assert!(!is_hallucination("you", Some("sv"), 0.0, NEAR_SILENT));
    }

    #[test]
    fn other_languages_drop_their_own_credits() {
        assert!(is_hallucination("ご視聴ありがとうございました", Some("ja"), 0.0, SPEAKING));
        assert!(is_hallucination("Gracias por ver el video.", Some("es"), 0.0, SPEAKING));
        assert!(is_hallucination("感谢观看", Some("zh"), 0.0, SPEAKING));
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;

    #[test]
    fn rms_reports_the_levels_the_thresholds_were_set_from() {
        assert!(rms_dbfs(&[]).is_infinite());
        // Digital silence floors rather than returning -inf.
        assert!(rms_dbfs(&[0.0; 128]) < -170.0);

        let hiss: Vec<f32> = (0..16_000)
            .map(|i| if i % 2 == 0 { 3.0e-4 } else { -3.0e-4 })
            .collect();
        let level = rms_dbfs(&hiss);
        assert!(
            (-71.0..-70.0).contains(&level),
            "hiss measured {level} dBFS, expected about -70.5"
        );
        assert!(level < LIKELY_SILENCE_RMS_DBFS);

        // A full-scale square wave is 0 dBFS by definition.
        let loud: Vec<f32> = (0..16_000)
            .map(|i| if i % 2 == 0 { 1.0 } else { -1.0 })
            .collect();
        assert!(rms_dbfs(&loud).abs() < 0.01);
        assert!(rms_dbfs(&loud) > LIKELY_SILENCE_RMS_DBFS);
    }
}
