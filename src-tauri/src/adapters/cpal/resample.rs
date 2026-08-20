/*!
 * SOURCE OF TRUTH KEYWORDS: Resampler16k, push, drain, downmix_to_mono,
 *   rms_of, RESAMPLER_CHUNK
 * WHAT:  Converts a device's native-rate, possibly multi-channel f32 stream
 *        into the 16kHz mono stream Whisper requires.
 * WHY:   Every assumption about audio format is contained here, because getting
 *        one wrong is SILENT — it degrades accuracy rather than failing, and
 *        then presents as a model problem that no amount of prompt tuning
 *        fixes. Three of them matter:
 *
 *        1. Never assume the device rate. Read it from the stream config and
 *           build the resampler from that. Bluetooth headsets renegotiate to
 *           odd rates mid-session, and 44.1kHz is not a clean ratio to 16k.
 *        2. Downmix by AVERAGING channels, not by taking channel 0. Some
 *           interfaces put silence in the first channel, and a transcript of
 *           silence is a very confusing bug.
 *        3. Feed the resampler in the fixed-size chunks it asks for. It is
 *           FixedAsync::Input, so it consumes exactly `input_frames_next()` per
 *           call — handing it an arbitrary buffer either errors or, worse,
 *           silently drops the remainder.
 *
 *        This runs on the drain thread, never in the audio callback, so it is
 *        allowed to allocate.
 * WHERE: Owned by adapters/cpal/source.rs; fed from the ring buffer the
 *        realtime callback writes into.
 */

use audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Async, FixedAsync, PolynomialDegree, Resampler};

use crate::error::{AppError, AppResult, ErrorCode};
use crate::types::TARGET_SAMPLE_RATE;

/// Frames of input consumed per resampler call. ~21ms at 48kHz: small enough
/// that the pill's meter stays responsive, large enough that the per-call
/// overhead is irrelevant.
const RESAMPLER_CHUNK: usize = 1024;

/**
 * SOURCE OF TRUTH KEYWORDS: Resampler16k
 * WHAT:  Stateful converter. Push native-rate interleaved samples, drain 16kHz
 *        mono ones.
 * WHY:   Stateful because a resampler carries filter history across calls —
 *        constructing a fresh one per buffer would put a discontinuity at every
 *        boundary, which is audible as a click and measurable as worse WER.
 * WHERE: One per capture session.
 */
pub struct Resampler16k {
    resampler: Option<Async<f32>>,
    channels: usize,
    /// Mono, native-rate frames not yet consumed by a full resampler chunk.
    pending: Vec<f32>,
    /// Reused so the steady state does not allocate.
    scratch: Vec<f32>,
}

impl Resampler16k {
    /**
     * WHAT:  Builds a converter for one device configuration.
     * WHY:   When the device already runs at 16kHz there is no resampler at
     *        all — running a converter at a 1:1 ratio would add filter delay
     *        and a little distortion in exchange for nothing.
     * WHERE: Called by the audio source when a stream is opened.
     */
    pub fn new(source_rate: u32, channels: u16) -> AppResult<Self> {
        if channels == 0 {
            return Err(AppError::new(
                ErrorCode::AudioFormatUnsupported,
                "That microphone reported no audio channels.",
            ));
        }

        let resampler = if source_rate == TARGET_SAMPLE_RATE {
            None
        } else {
            let ratio = TARGET_SAMPLE_RATE as f64 / source_rate as f64;
            Some(
                Async::<f32>::new_poly(
                    ratio,
                    // No dynamic ratio changes: the device rate is fixed for
                    // the life of the stream, and a device that renegotiates
                    // ends the stream rather than changing under us.
                    1.0,
                    // Cubic is the accuracy/cost point that matters here —
                    // speech at 16k does not benefit from a sinc kernel, and
                    // this runs continuously while inference is also running.
                    PolynomialDegree::Cubic,
                    RESAMPLER_CHUNK,
                    1,
                    FixedAsync::Input,
                )
                .map_err(|err| {
                    AppError::new(
                        ErrorCode::AudioFormatUnsupported,
                        "Murmur could not work with that microphone's sample rate.",
                    )
                    .with_detail(err)
                })?,
            )
        };

        Ok(Self {
            resampler,
            channels: channels as usize,
            pending: Vec::with_capacity(RESAMPLER_CHUNK * 4),
            scratch: Vec::with_capacity(RESAMPLER_CHUNK),
        })
    }

    /// Accepts interleaved native-rate samples and returns whatever 16kHz mono
    /// audio is now complete. An empty result is normal — it means the input
    /// did not yet fill a resampler chunk.
    pub fn push(&mut self, interleaved: &[f32]) -> AppResult<Vec<f32>> {
        downmix_to_mono(interleaved, self.channels, &mut self.pending);

        let Some(resampler) = self.resampler.as_mut() else {
            // Already at the target rate: hand the mono samples straight back.
            return Ok(std::mem::take(&mut self.pending));
        };

        let mut output = Vec::new();
        loop {
            let needed = resampler.input_frames_next();
            if self.pending.len() < needed {
                break;
            }

            self.scratch.clear();
            self.scratch.extend_from_slice(&self.pending[..needed]);
            self.pending.drain(..needed);

            let input = InterleavedSlice::new(self.scratch.as_slice(), 1, needed).map_err(|err| {
                AppError::new(ErrorCode::AudioFormatUnsupported, "Audio buffer mismatch.")
                    .with_detail(err)
            })?;

            let resampled = resampler.process(&input, None).map_err(|err| {
                AppError::new(ErrorCode::AudioFormatUnsupported, "Audio conversion failed.")
                    .with_detail(err)
            })?;

            // Mono, so the interleaved buffer IS the sample sequence and the
            // owned Vec can be taken whole rather than copied frame by frame.
            //
            // Clamped to [-1, 1] because polynomial interpolation OVERSHOOTS.
            // A cubic fitted through four near-full-scale points can land
            // outside the range its inputs occupied, so loud speech can produce
            // samples slightly above 1.0 even though every input was legal.
            // Everything downstream states [-1, 1] as a precondition: the VAD
            // asserts it (and in a release build, where that assert is compiled
            // out, silently mis-reads instead), and Whisper expects normalised
            // input. One clamp here keeps that contract true at the boundary
            // where it is first established.
            output.extend(resampled.take_data().into_iter().map(|s| s.clamp(-1.0, 1.0)));
        }

        Ok(output)
    }

    /// Frames buffered but not yet long enough to resample. Reported so the
    /// caller knows the tail is short rather than missing.
    pub fn pending_frames(&self) -> usize {
        self.pending.len()
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: downmix_to_mono
 * WHAT:  Collapses interleaved multi-channel frames into mono by averaging.
 * WHY:   Averaging, never channel 0 — see the module WHY. A trailing partial
 *        frame is dropped rather than padded, because a frame that is missing
 *        samples would otherwise be averaged against implicit zeros and come
 *        out quieter than the audio around it.
 * WHERE: The first step of every push.
 */
fn downmix_to_mono(interleaved: &[f32], channels: usize, out: &mut Vec<f32>) {
    if channels == 1 {
        out.extend_from_slice(interleaved);
        return;
    }

    for frame in interleaved.chunks_exact(channels) {
        let sum: f32 = frame.iter().sum();
        out.push(sum / channels as f32);
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: rms_of
 * WHAT:  Root-mean-square level of a buffer, for the pill's waveform.
 * WHY:   RMS rather than peak, because peak jumps on a single transient and
 *        makes the waveform look like noise. The pill draws perceived loudness.
 * WHERE: Called on the drain thread, once per delivered buffer.
 */
pub fn rms_of(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_squares: f32 = samples.iter().map(|s| s * s).sum();
    (sum_squares / samples.len() as f32).sqrt()
}

pub fn peak_of(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |acc, s| acc.max(s.abs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_averages_channels_rather_than_taking_the_first() {
        // The failure this guards: an interface that puts silence in channel 0.
        let mut out = Vec::new();
        let stereo = [0.0, 1.0, 0.0, 1.0];
        downmix_to_mono(&stereo, 2, &mut out);

        assert_eq!(out, vec![0.5, 0.5]);
        assert!(
            out.iter().all(|s| *s > 0.0),
            "taking channel 0 would have produced silence here"
        );
    }

    #[test]
    fn downmix_drops_a_partial_trailing_frame() {
        let mut out = Vec::new();
        // Five samples across two channels: the last one has no pair.
        downmix_to_mono(&[1.0, 1.0, 1.0, 1.0, 1.0], 2, &mut out);
        assert_eq!(out.len(), 2, "a half frame must not be averaged with zero");
    }

    #[test]
    fn a_matching_rate_bypasses_the_resampler_entirely() -> AppResult<()> {
        let mut resampler = Resampler16k::new(TARGET_SAMPLE_RATE, 1)?;
        let input: Vec<f32> = (0..100).map(|i| i as f32 / 100.0).collect();
        let out = resampler.push(&input)?;

        assert_eq!(out.len(), input.len(), "16kHz input must pass through");
        assert_eq!(out, input);
        Ok(())
    }

    #[test]
    fn downsampling_48k_to_16k_yields_about_a_third_as_many_frames() -> AppResult<()> {
        let mut resampler = Resampler16k::new(48_000, 1)?;

        // One second of a 440Hz tone at 48kHz.
        let input: Vec<f32> = (0..48_000)
            .map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin())
            .collect();

        let mut total = 0usize;
        for block in input.chunks(2048) {
            total += resampler.push(block)?.len();
        }

        // Allow for the frames still buffered and the resampler's own delay.
        let expected = 16_000;
        assert!(
            total > expected - 2_000 && total <= expected + 100,
            "expected roughly {expected} frames, produced {total}"
        );
        Ok(())
    }

    #[test]
    fn stereo_44100_is_handled_because_it_is_the_common_bluetooth_case() -> AppResult<()> {
        let mut resampler = Resampler16k::new(44_100, 2)?;
        let input: Vec<f32> = (0..44_100 * 2).map(|i| ((i / 2) as f32).sin()).collect();

        let mut total = 0usize;
        for block in input.chunks(4096) {
            total += resampler.push(block)?.len();
        }

        assert!(total > 14_000, "produced only {total} frames from a second");
        Ok(())
    }

    #[test]
    fn rms_is_zero_for_silence_and_positive_for_signal() {
        assert_eq!(rms_of(&[]), 0.0);
        assert_eq!(rms_of(&[0.0; 64]), 0.0);
        assert!(rms_of(&[0.5; 64]) > 0.0);
        // RMS of a constant is that constant.
        assert!((rms_of(&[0.5; 64]) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn peak_tracks_the_loudest_sample_in_either_direction() {
        assert!((peak_of(&[0.1, -0.9, 0.3]) - 0.9).abs() < 1e-6);
    }

    #[test]
    fn resampled_output_never_leaves_the_normalised_range() {
        // Polynomial interpolation overshoots on transients. Everything
        // downstream — the VAD's assert, Whisper's input contract — treats
        // [-1, 1] as guaranteed, so this is where the guarantee is made.
        let mut resampler = Resampler16k::new(48_000, 1).expect("resampler");

        // Full-scale square wave: the worst case for overshoot, because every
        // transition asks the interpolator to fit a step.
        let input: Vec<f32> = (0..48_000)
            .map(|i| if (i / 8) % 2 == 0 { 1.0 } else { -1.0 })
            .collect();

        for block in input.chunks(2048) {
            for sample in resampler.push(block).expect("push") {
                assert!(
                    (-1.0..=1.0).contains(&sample),
                    "sample {sample} escaped the normalised range"
                );
                assert!(sample.is_finite(), "resampling produced {sample}");
            }
        }
    }

    #[test]
    fn zero_channels_is_rejected_rather_than_dividing_by_zero() {
        assert!(Resampler16k::new(48_000, 0).is_err());
    }
}
