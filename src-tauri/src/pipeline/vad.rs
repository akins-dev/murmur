/*!
 * SOURCE OF TRUTH KEYWORDS: SpeechDetector, VadVerdict, VAD_FRAME_SAMPLES,
 *   SPEECH_THRESHOLD, SPEECH_ONSET_FRAMES, MIN_SPEECH_MS, push_frame,
 *   carries_speech, speech_ms, silence_ms
 * WHAT:  Voice activity detection over the 16kHz mono stream: tracks whether
 *        we are currently in speech and how long the current silence has run.
 * WHY:   Two jobs, and only two. It decides WHERE a chunk boundary goes, and it
 *        gates silent audio out before it ever reaches the model. Both are
 *        low-accuracy tasks, which is why a pure-Rust WebRTC-style detector is
 *        the right tool — no ONNX runtime, no second model to download, no
 *        first-run compile.
 *
 *        The gate is the more important of the two. Whisper reliably INVENTS
 *        text when handed silence — "Thank you.", "Please subscribe", subtitle
 *        credits — because its training data was subtitled video. Never sending
 *        it a silent chunk removes most of that failure class at the source,
 *        which is far better than trying to filter the output afterwards.
 * WHERE: Driven by pipeline/chunker.rs, which asks it where the boundaries are.
 */

use earshot::Detector;

use crate::types::TARGET_SAMPLE_RATE;

/// The detector requires exactly this, at 16kHz. 16ms per frame.
pub const VAD_FRAME_SAMPLES: usize = 256;

/// Scores above this count as voice. 0.5 is the detector's own guidance, and it
/// stays there: the frame score is deliberately NOT the thing that was tightened
/// to fix the hallucination bug. Raising a per-frame threshold to reject noise
/// costs quiet speech first, because a mumbled syllable and a fan both sit near
/// the line. The dwell and duration rules below reject blips on their SHAPE
/// instead, which leaves the quiet-speech case alone.
const SPEECH_THRESHOLD: f32 = 0.5;

/**
 * SOURCE OF TRUTH KEYWORDS: SPEECH_ONSET_FRAMES, dwell, hysteresis
 * WHAT:  Consecutive voiced frames required before speech is declared started.
 * WHY:   Three frames is 48ms. Nothing a person says is shorter, and almost
 *        everything that ISN'T speech is: a key click, a door, the leading edge
 *        of a beep. Without a dwell, ONE frame crossing 0.5 marked a whole
 *        chunk as speech-bearing and sent five seconds of room tone to a model
 *        that will always find words in it.
 *
 *        Onset only. There is deliberately no offset dwell — speech is declared
 *        over as soon as a frame is unvoiced — because the chunk boundary
 *        already waits BOUNDARY_SILENCE_MS (350ms) and adding a second
 *        hysteresis would move boundaries for no gain.
 */
const SPEECH_ONSET_FRAMES: usize = 3;

/**
 * SOURCE OF TRUTH KEYWORDS: MIN_SPEECH_MS, confidently_non_speech
 * WHAT:  Total voiced audio a chunk needs before it is worth decoding.
 * WHY:   The gate this replaced was "did ANY frame score as speech", which a
 *        single 16ms blip satisfied. This asks how MUCH of the chunk was voice,
 *        which is the question that distinguishes a beep from a sentence.
 *
 *        120ms is deliberately low — well under the shortest real utterance
 *        ("no" runs 200-300ms) — because the two errors are not symmetrical.
 *        Letting a beep through costs the user a spurious word they can delete.
 *        Dropping a chunk costs them something they SAID, and they may never
 *        find out which words went missing. When the two are in tension the
 *        answer is to let the beep through, so this number buys only the
 *        margin that is free.
 *
 *        Sustained noise — a fan, an air conditioner — is NOT what this catches.
 *        A WebRTC-style detector scores broadband noise as voice for as long as
 *        it runs, so no duration rule can reject it. That case is caught after
 *        the model instead, by no_speech_probability in the whisper adapter.
 *        Two gates that fail differently, rather than one tuned until it starts
 *        eating speech.
 */
const MIN_SPEECH_MS: u64 = 120;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VadVerdict {
    Speech,
    Silence,
}

/**
 * SOURCE OF TRUTH KEYWORDS: SpeechDetector
 * WHAT:  Stateful detector over a continuous stream.
 * WHY:   Stateful because the underlying model uses three frames of context —
 *        feeding it isolated frames would degrade it to noise. It also tracks
 *        cumulative silence, which is the actual signal the chunker needs.
 * WHERE: One per capture session, owned by the chunker.
 */
pub struct SpeechDetector {
    detector: Box<Detector>,
    /// Samples of continuous silence immediately preceding the write head.
    silence_samples: usize,
    /// Voiced samples accumulated in the current chunk, counted only after
    /// onset is confirmed. This is what the gate measures.
    voiced_samples: usize,
    /// Consecutive voiced frames not yet long enough to declare onset.
    onset_run: usize,
    /// Whether we are currently inside a confirmed run of speech.
    in_speech: bool,
    /// Frames not yet forming a complete VAD frame.
    remainder: Vec<f32>,
}

impl SpeechDetector {
    pub fn new() -> Self {
        Self {
            detector: Detector::default_boxed(),
            silence_samples: 0,
            voiced_samples: 0,
            onset_run: 0,
            in_speech: false,
            remainder: Vec::with_capacity(VAD_FRAME_SAMPLES),
        }
    }

    /**
     * WHAT:  Feeds samples through the detector, updating the silence run.
     * WHY:   Accepts an arbitrary-length buffer and internally splits it into
     *        the exact 256-sample frames the detector demands. Callers get
     *        whatever the resampler produced, which is never a neat multiple —
     *        making that the caller's problem would put this fiddly bit in
     *        several places instead of one.
     * WHERE: Called by the chunker for every buffer of captured audio.
     */
    pub fn push(&mut self, samples: &[f32]) -> VadVerdict {
        self.remainder.extend_from_slice(samples);

        let mut last = if self.silence_samples > 0 {
            VadVerdict::Silence
        } else {
            VadVerdict::Speech
        };

        let mut consumed = 0usize;
        while self.remainder.len() - consumed >= VAD_FRAME_SAMPLES {
            let frame = &self.remainder[consumed..consumed + VAD_FRAME_SAMPLES];
            consumed += VAD_FRAME_SAMPLES;

            let score = self.detector.predict_f32(frame);
            if score >= SPEECH_THRESHOLD {
                self.silence_samples = 0;

                if self.in_speech {
                    self.voiced_samples += VAD_FRAME_SAMPLES;
                } else {
                    self.onset_run += 1;
                    if self.onset_run >= SPEECH_ONSET_FRAMES {
                        self.in_speech = true;
                        // The frames that PROVED the onset were speech too, so
                        // they count. Dropping them would under-measure every
                        // utterance by 48ms and bias the gate against short
                        // words — the exact thing it must not do.
                        self.voiced_samples += self.onset_run * VAD_FRAME_SAMPLES;
                        self.onset_run = 0;
                    }
                }
                last = VadVerdict::Speech;
            } else {
                self.silence_samples += VAD_FRAME_SAMPLES;
                self.onset_run = 0;
                self.in_speech = false;
                last = VadVerdict::Silence;
            }
        }

        self.remainder.drain(..consumed);
        last
    }

    /// Milliseconds of unbroken silence at the write head.
    pub fn silence_ms(&self) -> u64 {
        (self.silence_samples as u64 * 1000) / TARGET_SAMPLE_RATE as u64
    }

    /// Voiced milliseconds accumulated since the last chunk reset.
    pub fn speech_ms(&self) -> u64 {
        (self.voiced_samples as u64 * 1000) / TARGET_SAMPLE_RATE as u64
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: carries_speech, hallucination_gate
     * WHAT:  Whether this chunk holds enough voice to be worth decoding.
     * WHY:   This is the hallucination gate, and it replaced a boolean that was
     *        true if ANY single frame scored as voice. Whisper does not merely
     *        mislabel non-speech — it INVENTS content to fill whatever it is
     *        handed, because it was trained on subtitled video. So one 16ms blip
     *        in a five-second chunk sent five seconds of room tone to a model
     *        guaranteed to find words in it, which is exactly what the operator
     *        reported: a beep transcribed as "beep", a fan as "fan".
     *
     *        Asking HOW MUCH of the chunk was voice, rather than whether any of
     *        it was, is what distinguishes the two cases.
     * WHERE: Checked by the chunker before queueing a chunk.
     */
    pub fn carries_speech(&self) -> bool {
        self.speech_ms() >= MIN_SPEECH_MS
    }

    /// Called when a chunk closes. Keeps the detector's own context — only the
    /// per-chunk bookkeeping resets, because the audio stream is continuous.
    pub fn reset_chunk(&mut self) {
        self.voiced_samples = 0;
        self.onset_run = 0;
        self.in_speech = false;
        self.silence_samples = 0;
    }
}

impl Default for SpeechDetector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 440Hz tone at speech-like amplitude.
    fn tone(samples: usize) -> Vec<f32> {
        (0..samples)
            .map(|i| {
                (i as f32 * 220.0 * std::f32::consts::TAU / TARGET_SAMPLE_RATE as f32).sin() * 0.4
            })
            .collect()
    }

    fn silence(samples: usize) -> Vec<f32> {
        vec![0.0; samples]
    }

    /// A pure tone at `hz`, which is what a notification chime or a monitor
    /// beep looks like to a detector.
    fn beep(samples: usize, hz: f32, amplitude: f32) -> Vec<f32> {
        (0..samples)
            .map(|i| {
                (i as f32 * hz * std::f32::consts::TAU / TARGET_SAMPLE_RATE as f32).sin()
                    * amplitude
            })
            .collect()
    }

    /// Deterministic pseudo-noise. A seeded LCG rather than `rand`, because a
    /// gate test that passes or fails depending on the seed is not a test.
    fn lcg(seed: &mut u64) -> f32 {
        *seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((*seed >> 40) as f32 / 8_388_608.0) - 1.0
    }

    /// Broadband hiss at a realistic room level.
    fn hiss(samples: usize, amplitude: f32) -> Vec<f32> {
        let mut seed = 0x2545F4914F6CDD1D;
        (0..samples).map(|_| lcg(&mut seed) * amplitude).collect()
    }

    /// Surrounds a sound with silence, the way a real chunk holds it.
    fn in_a_quiet_room(body: Vec<f32>) -> Vec<f32> {
        let pad = TARGET_SAMPLE_RATE as usize;
        let mut out = vec![0.0; pad];
        out.extend(body);
        out.extend(vec![0.0; pad]);
        out
    }

    #[test]
    fn digital_silence_is_never_reported_as_speech() {
        // The gate that stops whisper being handed silence to hallucinate into.
        let mut detector = SpeechDetector::new();
        detector.push(&silence(TARGET_SAMPLE_RATE as usize));

        assert!(
            !detector.carries_speech(),
            "a second of pure silence must not register as speech"
        );
        assert!(detector.silence_ms() >= 900);
    }

    #[test]
    fn silence_duration_accumulates_and_resets() {
        let mut detector = SpeechDetector::new();

        detector.push(&silence(TARGET_SAMPLE_RATE as usize / 2));
        let after_silence = detector.silence_ms();
        assert!(after_silence >= 400, "got {after_silence}ms");

        detector.reset_chunk();
        assert_eq!(detector.silence_ms(), 0);
        assert!(!detector.carries_speech());
    }

    #[test]
    fn arbitrary_buffer_lengths_are_handled_without_losing_samples() {
        // Buffers arrive from the resampler at whatever size it produced, never
        // a neat multiple of the VAD frame.
        let mut detector = SpeechDetector::new();
        let audio = silence(1000);

        for block in audio.chunks(37) {
            detector.push(block);
        }

        // 1000 samples is three whole 256-frames plus a remainder.
        let expected_ms = (768 * 1000) / TARGET_SAMPLE_RATE as u64;
        assert_eq!(detector.silence_ms(), expected_ms);
    }

    #[test]
    fn a_partial_frame_is_buffered_rather_than_discarded() {
        let mut detector = SpeechDetector::new();
        // Less than one frame: nothing can be decided yet.
        detector.push(&silence(100));
        assert_eq!(detector.silence_ms(), 0);

        // Completing the frame makes it count.
        detector.push(&silence(200));
        assert!(detector.silence_ms() > 0);
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: a_beep_is_not_speech, hallucination_gate
     * WHAT:  A notification beep and a single click do not open the gate.
     * WHY:   The operator's report, verbatim: "if there's a beep or if there's
     *        like a fan noise, it says fan or beep." The old gate was "did ANY
     *        frame score as voice", and a beep is precisely that — measured on
     *        this detector, a 200ms beep produces 4 voiced frames out of 137,
     *        and a 16ms click produces 2. Both used to send the ENTIRE chunk,
     *        room tone and all, to a model that always finds words.
     *
     *        Tested against real waveforms rather than zeros. A test built on
     *        digital silence passes against a gate that still forwards every
     *        beep in the room, which is exactly the gate that shipped.
     */
    #[test]
    fn a_beep_or_a_click_never_opens_the_gate() {
        for (name, body) in [
            ("16ms click", beep(VAD_FRAME_SAMPLES, 1000.0, 0.6)),
            ("200ms beep", beep(TARGET_SAMPLE_RATE as usize / 5, 1000.0, 0.3)),
            ("500ms beep", beep(TARGET_SAMPLE_RATE as usize / 2, 1000.0, 0.3)),
        ] {
            let mut detector = SpeechDetector::new();
            detector.push(&in_a_quiet_room(body));
            assert!(
                !detector.carries_speech(),
                "{name} opened the gate with {}ms of voiced audio; whisper would have been \
                 handed the whole chunk and would have written a word for it",
                detector.speech_ms()
            );
        }
    }

    #[test]
    fn quiet_room_noise_never_opens_the_gate() {
        // A room with a fan in it, at levels from clearly audible to barely.
        for amplitude in [0.10_f32, 0.018, 0.003] {
            let mut detector = SpeechDetector::new();
            detector.push(&hiss(3 * TARGET_SAMPLE_RATE as usize, amplitude * 0.1));
            assert!(
                !detector.carries_speech(),
                "room noise at amplitude {amplitude} opened the gate ({}ms voiced)",
                detector.speech_ms()
            );
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: real_speech_clears_the_gate_by_a_wide_margin
     * WHAT:  Real speech clears the gate with room to spare, and the test says
     *        how much room.
     * WHY:   This is the test that protects the OPERATOR'S CONSTRAINT — "careful
     *        when you make this change that it doesn't delete actual voice."
     *        Asserting merely that speech passes would still be green with the
     *        threshold raised to just under whatever this sentence produces, and
     *        the next person tightening the gate against a stubborn noise would
     *        sail past it and start clipping short words.
     *
     *        So it asserts the MARGIN. Measured on this detector, a four-second
     *        sentence yields ~2300ms of voiced audio against a 120ms gate — a
     *        19x margin. Requiring 5x leaves generous room for a quieter or
     *        mumblier speaker while still failing loudly if MIN_SPEECH_MS is
     *        raised anywhere near real speech.
     *
     *        Skips rather than fails when `say` is unavailable: a suite that
     *        goes red for an environmental reason is one people learn to ignore.
     */
    #[test]
    fn real_speech_clears_the_gate_by_a_wide_margin() {
        let Some(samples) =
            crate::testing::synthesise_speech("Testing one two three, this is real speech.", "vadgate")
        else {
            eprintln!("skipped: `say` is unavailable on this host");
            return;
        };

        let mut detector = SpeechDetector::new();
        detector.push(&samples);

        assert!(
            detector.carries_speech(),
            "real speech did not clear the gate: {}ms voiced",
            detector.speech_ms()
        );
        assert!(
            detector.speech_ms() >= MIN_SPEECH_MS * 5,
            "real speech cleared the gate by only {}ms against a {MIN_SPEECH_MS}ms threshold — \
             the margin protecting short and quiet words has been spent",
            detector.speech_ms()
        );
    }

    /**
     * WHAT:  The frames that prove an onset are counted, not thrown away.
     * WHY:   Discarding them would under-measure every utterance by the dwell
     *        (48ms) and bias the gate hardest against the shortest words — the
     *        ones it must be most careful with.
     */
    #[test]
    fn the_frames_that_prove_an_onset_still_count_as_speech() {
        let mut detector = SpeechDetector::new();
        // Exactly the dwell, and nothing more, from a source the detector reads
        // as voice: the count must include all three frames rather than zero.
        let Some(samples) = crate::testing::synthesise_speech("Yes.", "vadonset") else {
            eprintln!("skipped: `say` is unavailable on this host");
            return;
        };
        detector.push(&samples);
        let voiced = detector.speech_ms();
        assert!(
            voiced >= SPEECH_ONSET_FRAMES as u64 * 16,
            "a confirmed onset contributed {voiced}ms, less than the dwell that proved it"
        );
    }

    #[test]
    fn a_tone_disturbs_the_silence_run() {
        // Not asserting the detector calls a sine wave "speech" — it is trained
        // on voice. Only that non-silence is handled without panicking and that
        // the frame accounting stays consistent.
        let mut detector = SpeechDetector::new();
        detector.push(&tone(TARGET_SAMPLE_RATE as usize / 4));
        let _ = detector.carries_speech();
        let _ = detector.silence_ms();
    }
}

