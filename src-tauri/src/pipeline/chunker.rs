/*!
 * SOURCE OF TRUTH KEYWORDS: Chunker, push, close_tail, ChunkDecision,
 *   MIN_CHUNK_MS, MAX_CHUNK_MS, BOUNDARY_SILENCE_MS, OVERLAP_MS
 * WHAT:  Accumulates captured audio and closes it into chunks at natural
 *        silence boundaries, with a deliberate overlap between them.
 * WHY:   This file is the reason the latency target is achievable, and the
 *        numbers in it are counter-intuitive enough to be worth stating:
 *
 *        **Whisper's encoder always processes a 30-second window.** Shorter
 *        audio is padded up to it. So a 1-second chunk costs almost as much to
 *        encode as a 25-second one, and the instinct — "chunk small for low
 *        latency" — makes the app dramatically SLOWER. Chunking at 1s would be
 *        roughly ten times the total compute of chunking at 10s.
 *
 *        So chunks are LONG (8-15s) and closed at silence. They decode in the
 *        background while the user keeps talking, so their individual latency
 *        is invisible. Only the trailing fragment is on the critical path, and
 *        that is the one the engine shrinks its encoder context for.
 *
 *        Chunks overlap by 200ms so a word spoken across a boundary is not
 *        lost. The duplicate that overlap creates is removed downstream by the
 *        assembler, on TEXT — segment timestamps have no sub-chunk resolution
 *        because timestamp tokens are disabled for speed.
 * WHERE: Fed by pipeline/capture.rs; emits chunks to pipeline/worker.rs.
 */

use crate::pipeline::vad::SpeechDetector;
use crate::types::{AudioChunk, ChunkKind, TARGET_SAMPLE_RATE};

/// Below this a chunk is not worth its own encoder pass — see the module WHY.
const MIN_CHUNK_MS: u64 = 8_000;
/// Above this we close regardless of silence, so a continuous talker still gets
/// background decoding rather than one enormous chunk at the end.
const MAX_CHUNK_MS: u64 = 15_000;
/// Silence long enough to be a natural break rather than a breath.
const BOUNDARY_SILENCE_MS: u64 = 350;
/// Carried into the next chunk so a word across the seam survives.
const OVERLAP_MS: u64 = 200;

fn ms_to_samples(ms: u64) -> usize {
    (ms as usize * TARGET_SAMPLE_RATE as usize) / 1000
}

fn samples_to_ms(samples: usize) -> u64 {
    (samples as u64 * 1000) / TARGET_SAMPLE_RATE as u64
}

/**
 * SOURCE OF TRUTH KEYWORDS: Chunker
 * WHAT:  The rolling buffer and the boundary decision.
 * WHERE: One per session, owned by the capture task.
 */
pub struct Chunker {
    buffer: Vec<f32>,
    detector: SpeechDetector,
    /// Absolute position of buffer[0] within the session, in samples.
    chunk_start_samples: usize,
    /// Total samples seen this session, for absolute timing.
    total_samples: usize,
    /**
     * Loudest sample seen this session, in absolute value.
     * WHY: distinguishes "the user did not speak" from "the microphone is
     * delivering nothing at all". Those produce an identical empty transcript
     * and need opposite responses — one is normal, the other means the capture
     * path is broken and the user must be told.
     */
    peak_amplitude: f32,
}

impl Chunker {
    pub fn new() -> Self {
        Self {
            buffer: Vec::with_capacity(ms_to_samples(MAX_CHUNK_MS)),
            detector: SpeechDetector::new(),
            chunk_start_samples: 0,
            total_samples: 0,
            peak_amplitude: 0.0,
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: push
     * WHAT:  Adds captured audio, returning a chunk when one is ready.
     * WHY:   Two independent close conditions, and both are needed. The silence
     *        boundary is the preferred one because cutting mid-word costs
     *        accuracy; the hard maximum exists because someone who talks
     *        without pausing would otherwise never trigger the first, and would
     *        get no background decoding at all — turning a flat-latency design
     *        back into a linear one.
     * WHERE: Called by pipeline/capture.rs for every buffer from the device.
     */
    pub fn push(&mut self, samples: &[f32]) -> Option<AudioChunk> {
        self.buffer.extend_from_slice(samples);
        self.total_samples += samples.len();
        for sample in samples {
            let magnitude = sample.abs();
            if magnitude > self.peak_amplitude {
                self.peak_amplitude = magnitude;
            }
        }
        self.detector.push(samples);

        let buffered_ms = samples_to_ms(self.buffer.len());

        let at_silence_boundary =
            buffered_ms >= MIN_CHUNK_MS && self.detector.silence_ms() >= BOUNDARY_SILENCE_MS;
        let at_hard_limit = buffered_ms >= MAX_CHUNK_MS;

        if !at_silence_boundary && !at_hard_limit {
            return None;
        }

        self.close(ChunkKind::Interior)
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: close_tail
     * WHAT:  Closes whatever is left when the user presses stop.
     * WHY:   Marked as Tail, which is what tells the engine to shrink its
     *        encoder context for this one decode. That single flag is the
     *        difference between a ~1.4s final pass and one under 250ms.
     *        Returns None when the remainder holds no speech, so a stop pressed
     *        during silence does not hand the model something to invent from.
     * WHERE: Called once per session, on the transition into Finalizing.
     */
    pub fn close_tail(&mut self) -> Option<AudioChunk> {
        self.close(ChunkKind::Tail)
    }

    fn close(&mut self, kind: ChunkKind) -> Option<AudioChunk> {
        if self.buffer.is_empty() {
            return None;
        }

        // The hallucination gate: audio that is confidently not speech never
        // reaches the model. It measures HOW MUCH voice the chunk holds, not
        // whether any single frame scored as voice — see
        // SpeechDetector::carries_speech.
        //
        // Note what is NOT done here: the buffer is passed on WHOLE. Nothing
        // trims the samples around detected speech to tighten the gate. A
        // quiet or mumbled first syllable often falls below the detector's
        // threshold while being perfectly audible, and trimming to the VAD's
        // idea of where speech starts is how you deliver a sentence with its
        // first word clipped off.
        if !self.detector.carries_speech() {
            tracing::debug!(
                duration_ms = samples_to_ms(self.buffer.len()),
                speech_ms = self.detector.speech_ms(),
                "dropping a chunk that carries too little speech to decode"
            );
            self.discard_to_overlap(kind);
            return None;
        }

        let start_ms = samples_to_ms(self.chunk_start_samples);
        let end_ms = samples_to_ms(self.chunk_start_samples + self.buffer.len());
        let samples = self.buffer.clone();

        self.discard_to_overlap(kind);

        Some(AudioChunk {
            samples,
            start_ms,
            end_ms,
            kind,
        })
    }

    /**
     * WHAT:  Retains the trailing overlap and drops the rest.
     * WHY:   The overlap is what protects a word spoken across a boundary. It
     *        is NOT retained after a tail chunk — the session is over, and
     *        keeping it would leave the buffer holding audio nothing will ever
     *        decode.
     */
    fn discard_to_overlap(&mut self, kind: ChunkKind) {
        self.detector.reset_chunk();

        if matches!(kind, ChunkKind::Tail) {
            self.chunk_start_samples += self.buffer.len();
            self.buffer.clear();
            return;
        }

        let overlap = ms_to_samples(OVERLAP_MS).min(self.buffer.len());
        let dropped = self.buffer.len() - overlap;
        self.buffer.drain(..dropped);
        self.chunk_start_samples += dropped;
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: heard_nothing_at_all, digital_silence
     * WHAT:  True when the microphone delivered essentially pure zeros for the
     *        whole session.
     * WHY:   A room is never digitally silent — even a quiet one carries
     *        preamp noise well above this floor. A peak this low across seconds
     *        of audio means no signal reached us at all, which is a broken
     *        capture path rather than a quiet user. The two are otherwise
     *        indistinguishable: both end with an empty transcript.
     * WHERE: Checked by the session actor when a session produces no text.
     */
    pub fn heard_nothing_at_all(&self) -> bool {
        // -80 dBFS. Below any real microphone's noise floor, above the exact
        // zero that would make this trivially true for a single dead sample.
        const SILENCE_FLOOR: f32 = 0.0001;
        self.total_samples > 0 && self.peak_amplitude < SILENCE_FLOOR
    }

    /// Loudest sample seen, for diagnostics.
    pub fn peak_amplitude(&self) -> f32 {
        self.peak_amplitude
    }

    /// Total audio seen this session.
    pub fn duration_ms(&self) -> u64 {
        samples_to_ms(self.total_samples)
    }

    /// Milliseconds currently buffered and undecoded. On the critical path this
    /// is what the tail decode will cost.
    pub fn pending_ms(&self) -> u64 {
        samples_to_ms(self.buffer.len())
    }

    /// Drops everything. Used when a session is destroyed by Escape.
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.detector.reset_chunk();
        self.chunk_start_samples = 0;
        self.total_samples = 0;
        self.peak_amplitude = 0.0;
    }
}

impl Default for Chunker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * SOURCE OF TRUTH KEYWORDS: loud, real_speech_fixture
     * WHAT:  `ms` of actual recorded speech, tiled to length.
     * WHY:   This used to be a sum of three sine waves, on the theory that
     *        "broadband-ish content" gives the detector something voice-like to
     *        respond to. It does not. Measured on this detector, a tone mixture
     *        produces a handful of voiced frames per second — enough to satisfy
     *        the OLD gate, which asked only whether ANY frame scored as voice,
     *        and nothing like enough to satisfy a gate that asks how MUCH voice
     *        a chunk holds.
     *
     *        So the fixture was quietly testing the boundary logic against audio
     *        the pipeline would now correctly refuse to transcribe. Using real
     *        speech means these tests exercise the path a user actually takes,
     *        and it means tightening the speech gate too far breaks them —
     *        which is the alarm that should ring.
     * WHERE: Every chunker test that expects a chunk to be produced.
     */
    fn loud(ms: u64) -> Option<Vec<f32>> {
        let source = crate::testing::synthesise_speech(
            "The quick brown fox jumps over the lazy dog, again and again.",
            "chunker",
        )?;
        let wanted = ms_to_samples(ms);
        Some(source.iter().copied().cycle().take(wanted).collect())
    }

    /// Skips the test when `say` is unavailable rather than failing for a
    /// reason that has nothing to do with the code under test.
    macro_rules! speech {
        ($ms:expr) => {
            match loud($ms) {
                Some(samples) => samples,
                None => {
                    eprintln!("skipped: `say` is unavailable on this host");
                    return;
                }
            }
        };
    }

    fn quiet(ms: u64) -> Vec<f32> {
        vec![0.0; ms_to_samples(ms)]
    }

    #[test]
    fn short_audio_does_not_close_a_chunk() {
        // The counter-intuitive core: small chunks are expensive, so we wait.
        let mut chunker = Chunker::new();
        assert!(chunker.push(&speech!(1_000)).is_none());
        assert!(chunker.push(&speech!(2_000)).is_none());
        assert!(chunker.pending_ms() >= 2_900);
    }

    #[test]
    fn a_long_talker_still_gets_chunks_at_the_hard_limit() {
        // Without this, someone who never pauses gets no background decoding
        // and the whole flat-latency design collapses.
        let mut chunker = Chunker::new();
        let mut closed = None;

        for _ in 0..20 {
            if let Some(chunk) = chunker.push(&speech!(1_000)) {
                closed = Some(chunk);
                break;
            }
        }

        let chunk = closed.expect("a chunk must close at the hard limit");
        assert_eq!(chunk.kind, ChunkKind::Interior);
        assert!(
            chunk.duration_ms() >= MIN_CHUNK_MS,
            "chunk was {}ms, below the minimum",
            chunk.duration_ms()
        );
    }

    #[test]
    fn consecutive_chunks_overlap_rather_than_abutting() {
        // The 200ms overlap is what stops a word on a seam being lost.
        let mut chunker = Chunker::new();
        let mut chunks = Vec::new();

        for _ in 0..40 {
            if let Some(chunk) = chunker.push(&speech!(1_000)) {
                chunks.push(chunk);
            }
            if chunks.len() == 2 {
                break;
            }
        }

        assert_eq!(chunks.len(), 2, "expected two chunks");
        let overlap = chunks[0].end_ms.saturating_sub(chunks[1].start_ms);
        assert!(
            (150..=250).contains(&overlap),
            "chunks overlapped by {overlap}ms, expected about {OVERLAP_MS}ms"
        );
    }

    #[test]
    fn a_silent_chunk_is_never_emitted() {
        // The single most effective hallucination defence: whisper is never
        // handed silence in the first place.
        let mut chunker = Chunker::new();

        for _ in 0..20 {
            assert!(
                chunker.push(&quiet(1_000)).is_none(),
                "silence must never be emitted as a chunk"
            );
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: a_beep_is_never_transcribed
     * WHAT:  A chunk whose only non-silence is a beep is never emitted.
     * WHY:   The operator's bug, at the level it has to be fixed. He turned the
     *        microphone on, said nothing, and got the word "beep". The chunk was
     *        emitted because ONE frame of the beep scored as voice, and once
     *        whisper is handed five seconds of anything it writes words for it.
     *
     *        This asserts the whole chunk is withheld, which is the only
     *        defence that works — the beep cannot be trimmed out and the rest
     *        sent, because trimming around detected speech is how a first
     *        syllable gets clipped off a real sentence.
     */
    #[test]
    fn a_chunk_containing_only_a_beep_is_never_emitted() {
        let mut chunker = Chunker::new();

        // A 1kHz beep in an otherwise quiet room, then long enough for every
        // close condition to have been reached.
        let beep: Vec<f32> = (0..ms_to_samples(200))
            .map(|i| {
                (i as f32 * 1000.0 * std::f32::consts::TAU / TARGET_SAMPLE_RATE as f32).sin() * 0.3
            })
            .collect();

        assert!(chunker.push(&quiet(2_000)).is_none());
        assert!(chunker.push(&beep).is_none());
        for _ in 0..20 {
            assert!(
                chunker.push(&quiet(1_000)).is_none(),
                "a beep in a quiet room must never be sent to the model"
            );
        }
        assert!(
            chunker.close_tail().is_none(),
            "stopping after a beep must not queue a decode either"
        );
    }

    #[test]
    fn a_silent_tail_produces_nothing_to_transcribe() {
        let mut chunker = Chunker::new();
        chunker.push(&quiet(2_000));
        assert!(
            chunker.close_tail().is_none(),
            "stopping during silence must not queue a decode"
        );
    }

    #[test]
    fn the_tail_is_marked_so_the_engine_can_shrink_its_context() {
        // This flag is the difference between a 1.4s final decode and <250ms.
        let mut chunker = Chunker::new();
        chunker.push(&speech!(3_000));

        let tail = chunker.close_tail().expect("a tail with speech in it");
        assert_eq!(tail.kind, ChunkKind::Tail);
        assert!(tail.is_tail());
    }

    #[test]
    fn a_tail_leaves_no_audio_behind() {
        let mut chunker = Chunker::new();
        chunker.push(&speech!(3_000));
        chunker.close_tail();

        assert_eq!(
            chunker.pending_ms(),
            0,
            "a tail must not retain overlap — the session is over"
        );
    }

    #[test]
    fn duration_tracks_everything_seen_not_just_what_is_buffered() {
        let mut chunker = Chunker::new();
        for _ in 0..20 {
            chunker.push(&speech!(1_000));
        }
        assert!(
            chunker.duration_ms() >= 19_000,
            "session duration must survive chunk closes, got {}",
            chunker.duration_ms()
        );
    }

    #[test]
    fn clearing_discards_everything_for_a_cancelled_session() {
        let mut chunker = Chunker::new();
        chunker.push(&speech!(5_000));
        chunker.clear();

        assert_eq!(chunker.pending_ms(), 0);
        assert_eq!(chunker.duration_ms(), 0);
        assert!(chunker.close_tail().is_none());
    }

    #[test]
    fn the_chunk_window_matches_the_documented_range() {
        // Guards against someone "optimising" toward small chunks, which is the
        // intuitive change and the wrong one.
        assert_eq!(MIN_CHUNK_MS, 8_000);
        assert_eq!(MAX_CHUNK_MS, 15_000);
        // Deliberately compared through variables: clippy correctly points out
        // that two constants compare at compile time, but the value of this
        // assertion is that it fails loudly if someone edits the constants into
        // an inverted range.
        let (min, max) = (MIN_CHUNK_MS, MAX_CHUNK_MS);
        assert!(min < max, "the chunk window must not be inverted");
    }
}
