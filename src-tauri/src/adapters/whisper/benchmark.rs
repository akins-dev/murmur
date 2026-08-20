/*!
 * SOURCE OF TRUTH KEYWORDS: measure_realtime_factor, probe_signal,
 *   PROBE_SECONDS, WARMUP_SECONDS, RealtimeMeasurement
 * WHAT:  Measures, on this machine, how many seconds of audio the loaded model
 *        decodes per second of wall clock, and warms the accelerator first.
 * WHY:   `EngineCapabilities::realtime_factor` gates which engines the app will
 *        offer, so an adapter that copies a number off a datasheet breaks the
 *        gating for everyone on slower hardware — this is the one field where a
 *        plausible lie is worse than no value. Measuring it here also does the
 *        job docs/03 §2.6 requires for a different reason: the Core ML compile
 *        takes 15-60 seconds the first time a model is loaded on a machine, and
 *        that must be paid during prepare(), never lazily on the hotkey path.
 *        The probe is a synthetic harmonic tone rather than silence, because
 *        silence gives the decoder nothing to do and would report a speed the
 *        engine cannot reach on real audio.
 * WHERE: Called once from adapters/whisper/engine.rs::prepare, after the
 *        WhisperContext is created.
 */

use std::time::Instant;

use whisper_rs::WhisperState;

use crate::error::{AppError, AppResult, ErrorCode};
use crate::types::TARGET_SAMPLE_RATE;

use super::params::{build_full_params, DecodeProfile};

/// Long enough for the encoder pass to dominate the measurement and short
/// enough that startup does not visibly stall.
pub const PROBE_SECONDS: f32 = 10.0;
/// A throwaway pass whose only job is to trigger the Core ML compile and the
/// first Metal allocation, so neither lands inside the timed run.
pub const WARMUP_SECONDS: f32 = 1.0;

/**
 * SOURCE OF TRUTH KEYWORDS: RealtimeMeasurement
 * WHAT:  The measured result: the ratio, and the raw numbers behind it.
 * WHY:   The elapsed time is kept alongside the ratio so a suspicious factor
 *        can be traced to a slow run rather than to arithmetic, and so the
 *        number can be logged in the shape docs/03 §9 asks for.
 * WHERE: Returned by measure_realtime_factor; the ratio lands in
 *        EngineCapabilities and the rest is traced.
 */
#[derive(Debug, Clone, Copy)]
pub struct RealtimeMeasurement {
    /// Seconds of audio decoded per second of wall clock. Above 1.0 is faster
    /// than realtime. 0.0 means never measured.
    pub factor: f32,
    pub audio_seconds: f32,
    pub elapsed_seconds: f32,
}

/**
 * SOURCE OF TRUTH KEYWORDS: probe_signal
 * WHAT:  A deterministic voiced-speech-like buffer: a 120Hz harmonic stack
 *        shaped by three formants and amplitude-modulated at a syllable rate.
 * WHY:   Deterministic so the measurement is reproducible across runs, and
 *        harmonic rather than noise or silence so the mel spectrogram is
 *        non-trivial and the decoder actually emits tokens. A probe the encoder
 *        finds empty measures the encoder alone and overstates the engine.
 * WHERE: Used only by measure_realtime_factor.
 */
pub fn probe_signal(seconds: f32) -> Vec<f32> {
    let sample_count = (seconds * TARGET_SAMPLE_RATE as f32).max(0.0) as usize;
    let rate = TARGET_SAMPLE_RATE as f32;
    let fundamental = 120.0_f32;
    // Roughly the first three formants of a neutral vowel.
    let formants = [730.0_f32, 1090.0, 2440.0];

    (0..sample_count)
        .map(|index| {
            let t = index as f32 / rate;
            // Syllable-rate envelope, never fully closing, so the VAD-style
            // guards elsewhere see continuous speech.
            let envelope = 0.35 + 0.3 * (std::f32::consts::TAU * 4.0 * t).sin();
            let harmonics: f32 = (1..=12)
                .map(|n| {
                    let freq = fundamental * n as f32;
                    // Weight each harmonic by its distance to the nearest formant.
                    let gain = formants
                        .iter()
                        .map(|f| 1.0 / (1.0 + ((freq - f) / 120.0).powi(2)))
                        .sum::<f32>();
                    gain * (std::f32::consts::TAU * freq * t).sin()
                })
                .sum();
            (envelope * harmonics * 0.15).clamp(-1.0, 1.0)
        })
        .collect()
}

/**
 * SOURCE OF TRUTH KEYWORDS: measure_realtime_factor
 * WHAT:  Runs a warm-up decode then a timed decode, returning the measurement.
 * WHY:   Two passes, not one. The first load of a model on a machine pays the
 *        Neural Engine compile and the first Metal buffer allocation; folding
 *        that into the timing would report a factor an order of magnitude below
 *        the truth and gate the engine off on hardware that runs it fine.
 *        Background-profile parameters are used deliberately: the full 1500
 *        encoder context is the honest, worst-case cost, and quoting the
 *        tail's reduced-context speed as the engine's general capability would
 *        be the flattering lie this function exists to avoid.
 * WHERE: adapters/whisper/engine.rs::prepare.
 */
pub fn measure_realtime_factor(
    state: &mut WhisperState,
    n_threads: std::ffi::c_int,
) -> AppResult<RealtimeMeasurement> {
    let warmup = probe_signal(WARMUP_SECONDS);
    let warmup_params = build_full_params(
        DecodeProfile::Background,
        warmup.len(),
        Some("en"),
        None,
        n_threads,
    );
    state.full(warmup_params, &warmup).map_err(|err| {
        AppError::new(
            ErrorCode::EngineNotReady,
            "Murmur could not warm up the transcription model.",
        )
        .with_detail(format!("{err:?}"))
    })?;

    let probe = probe_signal(PROBE_SECONDS);
    let params = build_full_params(
        DecodeProfile::Background,
        probe.len(),
        Some("en"),
        None,
        n_threads,
    );

    let started = Instant::now();
    state.full(params, &probe).map_err(|err| {
        AppError::new(
            ErrorCode::EngineNotReady,
            "Murmur could not measure transcription speed on this Mac.",
        )
        .with_detail(format!("{err:?}"))
    })?;
    let elapsed_seconds = started.elapsed().as_secs_f32();

    // A zero elapsed time is not a real result; report it as unmeasured rather
    // than as an infinitely fast engine.
    let factor = if elapsed_seconds > 0.0 {
        PROBE_SECONDS / elapsed_seconds
    } else {
        0.0
    };

    Ok(RealtimeMeasurement {
        factor,
        audio_seconds: PROBE_SECONDS,
        elapsed_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_is_the_length_it_claims_and_stays_in_range() {
        let probe = probe_signal(2.0);
        assert_eq!(probe.len(), (TARGET_SAMPLE_RATE * 2) as usize);
        assert!(probe.iter().all(|s| (-1.0..=1.0).contains(s)));
    }

    #[test]
    fn the_probe_is_not_silence_and_is_deterministic() {
        let a = probe_signal(0.5);
        let b = probe_signal(0.5);
        assert_eq!(a, b);
        assert!(!super::super::hallucination::is_digital_silence(&a));
    }
}
