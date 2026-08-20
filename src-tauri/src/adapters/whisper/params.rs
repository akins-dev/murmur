/*!
 * SOURCE OF TRUTH KEYWORDS: audio_ctx_for, build_full_params, DecodeProfile,
 *   FULL_AUDIO_CTX, MIN_AUDIO_CTX, AUDIO_CTX_HEADROOM, decode_thread_count,
 *   NO_SPEECH_THOLD, ENTROPY_THOLD
 * WHAT:  Builds the `FullParams` for one decode, and computes the encoder
 *        context size from the fragment length.
 * WHY:   Whisper's encoder always runs over a 30-second mel window, so a 1.2s
 *        tail costs the same as a 25s chunk unless `audio_ctx` is shrunk to fit
 *        the actual audio. That single value is the difference between a ~1.4s
 *        final decode and a sub-200ms one, which is the whole p50 < 300ms
 *        budget. It is applied to the TAIL ONLY: reduced context can cost
 *        accuracy, and interior chunks decode in the background where their
 *        latency is invisible, so they keep the full 1500.
 *        `temperature_inc = 0.0` disables whisper's temperature-fallback loop —
 *        up to six extra decode passes fired without warning, turning a 200ms
 *        decode into 1.4s. `no_context = true` stops decoder state carrying
 *        across chunk boundaries, which is the cause of "it repeated the same
 *        sentence forty times".
 * WHERE: Called by adapters/whisper/engine.rs for every transcribe; the values
 *        come from docs/03-IMPLEMENTATION-NOTES.md §2.1 and §2.2.
 */

use std::ffi::c_int;

use whisper_rs::{FullParams, SamplingStrategy};

use crate::types::{AudioChunk, LanguageHint, TARGET_SAMPLE_RATE};

/// The encoder context that corresponds to whisper's full 30-second window.
pub const FULL_AUDIO_CTX: c_int = 1500;
/// Below this the encoder's convolutional front end has too little to work with
/// and accuracy collapses, so short fragments pay for a floor they do not use.
pub const MIN_AUDIO_CTX: c_int = 256;
/// Extra context beyond the fragment's own length. Whisper's own `stream`
/// example keeps a margin here because the mel window is padded and a context
/// sized exactly to the audio clips the final frames.
pub const AUDIO_CTX_HEADROOM: c_int = 128;

/// Segments whose no-speech probability exceeds this are discarded. Paired with
/// the VAD gate upstream; see docs/03 §2.4.
pub const NO_SPEECH_THOLD: f32 = 0.6;

/**
 * SOURCE OF TRUTH KEYWORDS: ENTROPY_THOLD
 * WHAT:  whisper.cpp's decoder-entropy gate, left at its upstream default.
 * WHY:   docs/03 §2.4 lists this as a hallucination defence, but it only ever
 *        fires inside whisper.cpp's temperature-fallback loop — and §2.2
 *        disables that loop by pinning `temperature_inc` to 0.0. It is
 *        therefore inert by construction. It is set anyway so the value is not
 *        silently inherited if the fallback is ever re-enabled, and it is
 *        documented here so nobody counts it as live protection.
 * WHERE: Applied in build_full_params; the defence that actually fires is the
 *        per-segment no_speech_probability filter in hallucination.rs.
 */
pub const ENTROPY_THOLD: f32 = 2.4;

/**
 * SOURCE OF TRUTH KEYWORDS: DecodeProfile
 * WHAT:  Whether this decode is the latency-critical tail or a background chunk.
 * WHY:   Kept separate from ChunkKind so the accuracy/latency trade is decided
 *        in one place and is legible at the call site, rather than being read
 *        off a boolean four frames deep.
 * WHERE: Derived from AudioChunk::is_tail by `DecodeProfile::for_chunk`.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecodeProfile {
    /// Full encoder context. Latency is hidden behind the user still talking.
    Background,
    /// Encoder context shrunk to the fragment. The only decode the user waits on.
    Tail,
}

impl DecodeProfile {
    /**
     * WHAT:  The profile for a chunk, given whether a Core ML encoder is live.
     * WHY:   `coreml_live` forces Background, and it is not a tuning choice —
     *        it prevents silent data loss. When whisper.cpp has a Core ML
     *        encoder loaded it stops building its own convolution graph and
     *        hands the ANE encoder `2 * audio_ctx` mel frames instead. The
     *        `.mlmodelc` is compiled for a fixed 3000-frame input, so a reduced
     *        context feeds it 512 frames, and whisper.cpp neither guards against
     *        this nor reports it: the decode returns an EMPTY transcript and
     *        looks merely fast. Measured on 1.8.3 — a tail decode "completed" in
     *        5ms having produced nothing at all.
     *        So the two optimisations are mutually exclusive, and correctness
     *        wins: with Core ML live every chunk keeps the full 1500.
     * WHERE: adapters/whisper/engine.rs::transcribe, once per chunk.
     */
    pub fn for_chunk(chunk: &AudioChunk, coreml_live: bool) -> Self {
        if chunk.is_tail() && !coreml_live {
            Self::Tail
        } else {
            Self::Background
        }
    }
}

/**
 * WHAT:  The encoder context size for a fragment of `sample_count` samples.
 * WHY:   `ceil(seconds / 30 * 1500) + headroom`, clamped. The ratio is exact —
 *        1500 context positions cover 30 seconds — so this is the smallest
 *        encoder that still spans the audio, plus a margin for the padded mel
 *        window. Background decodes never call this; they use FULL_AUDIO_CTX.
 * WHERE: adapters/whisper/engine.rs, tail decodes only.
 */
pub fn audio_ctx_for(sample_count: usize) -> c_int {
    let seconds = sample_count as f64 / f64::from(TARGET_SAMPLE_RATE);
    let scaled = (seconds / 30.0 * f64::from(FULL_AUDIO_CTX)).ceil();

    // The cast is bounded by the clamp below, so it cannot wrap.
    let requested = if scaled >= f64::from(FULL_AUDIO_CTX) {
        FULL_AUDIO_CTX
    } else if scaled <= 0.0 {
        0
    } else {
        scaled as c_int
    };

    requested
        .saturating_add(AUDIO_CTX_HEADROOM)
        .clamp(MIN_AUDIO_CTX, FULL_AUDIO_CTX)
}

/**
 * WHAT:  Threads for one decode: available parallelism minus two, floor of one.
 * WHY:   docs/03 §2.2 asks for physical cores minus two, leaving headroom for
 *        the CoreAudio realtime thread and the UI. On Apple Silicon there is no
 *        SMT, so `available_parallelism` already reports physical cores — which
 *        is why this is correct here and would not be on an x86 host.
 * WHERE: Read once when the engine is constructed.
 */
pub fn decode_thread_count() -> c_int {
    let logical = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4);
    let usable = logical.saturating_sub(2).max(1);
    c_int::try_from(usable).unwrap_or(1)
}

/**
 * SOURCE OF TRUTH KEYWORDS: build_full_params
 * WHAT:  Every whisper parameter from docs/03 §2.2, plus the profile-dependent
 *        `audio_ctx` and the optional vocabulary prompt.
 * WHY:   One function so the table has exactly one implementation — a parameter
 *        set assembled at two call sites is a parameter set that disagrees with
 *        itself the first time one is edited. `language` is borrowed for `'a`
 *        because whisper-rs ties the params to the string's lifetime.
 * WHERE: Called per decode by adapters/whisper/engine.rs.
 */
pub fn build_full_params<'a>(
    profile: DecodeProfile,
    sample_count: usize,
    language: Option<&'a str>,
    prompt: Option<&str>,
    n_threads: c_int,
) -> FullParams<'a, 'a> {
    // Greedy with best_of = 1: temperature is pinned to 0.0, so sampling more
    // candidates would cost time and return identical text.
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });

    params.set_n_threads(n_threads);

    // Never translate. The default bites here: with translate on, every
    // language silently becomes English and the multilingual product is gone.
    params.set_translate(false);

    // NEVER set detect_language here, in either arm. whisper-rs documents it as
    // "the same effect as setting the language to auto or None". In whisper.cpp
    // 1.8.3 it is not: `whisper_full_with_state` detects the language and then
    // does `if (params.detect_language) { return 0; }` — it returns ZERO
    // segments and never transcribes. Setting it alongside a null language,
    // which reads as harmlessly stating the intent twice, silently turns every
    // auto-language dictation into an empty transcript. A null language is the
    // whole instruction: whisper.cpp detects from it and then carries on.
    params.set_language(language);

    // The #1 cause of "it repeated the same sentence forty times".
    params.set_no_context(true);

    params.set_temperature(0.0);
    // The latency bomb. See the module WHY.
    params.set_temperature_inc(0.0);
    params.set_entropy_thold(ENTROPY_THOLD);
    params.set_no_speech_thold(NO_SPEECH_THOLD);
    params.set_suppress_blank(true);

    // ── THE ONE DELIBERATE DEPARTURE FROM THE docs/03 §2.2 TABLE ──────────
    //
    // The table says `no_timestamps = true`, on the reasoning that we do not
    // use word timings so skipping them is free speed. Measured on whisper.cpp
    // 1.8.3 against real speech, it is the opposite of free and the opposite of
    // fast. Same 3s utterance, same everything else:
    //
    //     no_timestamps = true   ->  340 ms, 1019 characters
    //     no_timestamps = false  ->  113 ms,   41 characters
    //
    // The 1019 characters are the user's sentence repeated twenty-four times.
    // Timestamp tokens are what let the decoder decide it has reached the end
    // of the audio; without them it keeps decoding into the 30-second silent
    // padding whisper always pads to, and loops. `temperature_inc = 0.0` —
    // correctly pinned above for latency — removes the fallback that would
    // otherwise break the loop, so the two settings are individually defensible
    // and together produce a paste of the same sentence two dozen times.
    // `single_segment` does not help: measured at 318 ms and still looping.
    //
    // Keeping them also gives pipeline/assembler.rs real per-segment spans to
    // de-duplicate the 200ms chunk overlap with, which it otherwise could not.
    params.set_no_timestamps(false);

    // whisper.cpp writes to stdout on every call unless all four are off.
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_special(false);
    params.set_print_timestamps(false);

    params.set_audio_ctx(match profile {
        DecodeProfile::Background => FULL_AUDIO_CTX,
        DecodeProfile::Tail => audio_ctx_for(sample_count),
    });

    if let Some(prompt) = prompt {
        params.set_initial_prompt(prompt);
    }

    params
}

/**
 * WHAT:  The whisper language code for a hint, or None to auto-detect.
 * WHERE: Called by engine.rs before build_full_params, so the borrowed code
 *        outlives the params.
 */
pub fn language_code(hint: &LanguageHint) -> Option<&str> {
    match hint {
        LanguageHint::Auto => None,
        LanguageHint::Pinned { language } => Some(language.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_context_scales_with_duration_and_respects_the_floor() {
        // A 1.2s tail: ceil(1.2 / 30 * 1500) = 60, + 128 = 188, floored to 256.
        assert_eq!(audio_ctx_for(TARGET_SAMPLE_RATE as usize * 12 / 10), 256);

        // 10 seconds: ceil(10 / 30 * 1500) = 500, + 128 = 628.
        assert_eq!(audio_ctx_for(TARGET_SAMPLE_RATE as usize * 10), 628);

        // 30 seconds and beyond can never exceed the model's own context.
        assert_eq!(audio_ctx_for(TARGET_SAMPLE_RATE as usize * 30), FULL_AUDIO_CTX);
        assert_eq!(
            audio_ctx_for(TARGET_SAMPLE_RATE as usize * 600),
            FULL_AUDIO_CTX
        );
    }

    #[test]
    fn an_empty_fragment_still_yields_a_legal_context() {
        assert_eq!(audio_ctx_for(0), MIN_AUDIO_CTX);
    }

    #[test]
    fn a_live_coreml_encoder_forces_the_full_context_on_every_chunk() {
        let tail = AudioChunk {
            samples: vec![0.0; 16_000],
            start_ms: 0,
            end_ms: 1000,
            kind: crate::types::ChunkKind::Tail,
        };
        assert_eq!(DecodeProfile::for_chunk(&tail, false), DecodeProfile::Tail);
        // Core ML live: reducing the context would return an empty transcript.
        assert_eq!(
            DecodeProfile::for_chunk(&tail, true),
            DecodeProfile::Background
        );
    }

    #[test]
    fn thread_count_never_reaches_zero() {
        assert!(decode_thread_count() >= 1);
    }
}
