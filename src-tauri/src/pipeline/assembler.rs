/*!
 * SOURCE OF TRUTH KEYWORDS: Assembler, push_segments, finish, join_overlapping,
 *   MAX_SEAM_WORDS, normalise_word
 * WHAT:  Joins the segments decoded from successive chunks into one transcript,
 *        removing the duplication the deliberate chunk overlap creates.
 * WHY:   The join has to be done on TEXT, not on timestamps, and that is a
 *        consequence of a decision made elsewhere: timestamp tokens are
 *        disabled for speed, so a chunk returns ONE segment spanning the whole
 *        chunk with no sub-chunk resolution. The spans are truthful but too
 *        coarse to locate a repeated word in, so anything that tried to
 *        de-duplicate by comparing times would find the segments merely
 *        adjacent and silently do nothing.
 *
 *        The overlap is ~200ms, which is at most a word or two. The search is
 *        bounded accordingly — a longer window would start "finding" overlaps
 *        in ordinary repeated speech and delete words the user actually said,
 *        which is far worse than leaving a duplicate in.
 * WHERE: Fed by pipeline/worker.rs as chunks complete; its output goes to the
 *        TextEnhancer.
 */

use crate::types::TranscriptSegment;

/// Upper bound on the seam search. Comfortably covers the 200ms overlap while
/// staying too short to match a genuine repeated phrase.
const MAX_SEAM_WORDS: usize = 6;

/**
 * SOURCE OF TRUTH KEYWORDS: Assembler
 * WHAT:  Accumulates decoded text in chunk order.
 * WHY:   Chunks are decoded in the background and may complete out of order, so
 *        segments are inserted by their start time rather than appended. A
 *        transcript assembled in completion order would scramble under load —
 *        which is exactly when it is hardest to notice.
 * WHERE: One per session, owned by the session actor.
 */
#[derive(Debug, Default)]
pub struct Assembler {
    /// (start_ms, text), kept sorted by start_ms.
    parts: Vec<(u64, String)>,
    language: Option<String>,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the segments decoded from one chunk.
    pub fn push_segments(&mut self, segments: &[TranscriptSegment]) {
        for segment in segments {
            let text = segment.text.trim();
            if text.is_empty() {
                continue;
            }

            // First language wins: auto-detect runs on the first window, and a
            // later chunk changing its mind mid-session is noise, not signal.
            if self.language.is_none() {
                if let Some(language) = &segment.language {
                    self.language = Some(language.as_str().to_string());
                }
            }

            let position = self
                .parts
                .partition_point(|(start, _)| *start <= segment.start_ms);
            self.parts.insert(position, (segment.start_ms, text.to_string()));
        }
    }

    /// The joined transcript, with seam duplication removed.
    pub fn finish(&self) -> String {
        let mut out = String::new();
        for (_, text) in &self.parts {
            out = join_overlapping(&out, text);
        }
        out
    }

    pub fn language(&self) -> Option<&str> {
        self.language.as_deref()
    }

    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }

    pub fn clear(&mut self) {
        self.parts.clear();
        self.language = None;
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: join_overlapping
 * WHAT:  Concatenates two pieces of transcript, dropping the repeated words
 *        where they overlap.
 * WHY:   Longest match first, so "I want to / I want to go" collapses on the
 *        full three words rather than only the first. Comparison is on
 *        normalised words — lowercased, punctuation stripped — because the two
 *        decodes of the same audio routinely differ in casing and in whether a
 *        comma landed.
 * WHERE: Used by Assembler::finish for every seam.
 */
fn join_overlapping(left: &str, right: &str) -> String {
    if left.is_empty() {
        return right.to_string();
    }
    if right.is_empty() {
        return left.to_string();
    }

    let left_words: Vec<&str> = left.split_whitespace().collect();
    let right_words: Vec<&str> = right.split_whitespace().collect();

    let max_span = MAX_SEAM_WORDS.min(left_words.len()).min(right_words.len());

    for span in (1..=max_span).rev() {
        let tail = &left_words[left_words.len() - span..];
        let head = &right_words[..span];

        if tail
            .iter()
            .zip(head.iter())
            .all(|(a, b)| normalise_word(a) == normalise_word(b) && !normalise_word(a).is_empty())
        {
            let remainder = right_words[span..].join(" ");
            if remainder.is_empty() {
                return left.to_string();
            }
            return format!("{left} {remainder}");
        }
    }

    format!("{left} {right}")
}

/// Lowercased, letters and digits only. Two decodes of the same word differ in
/// punctuation and casing far more often than in spelling.
fn normalise_word(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::LanguageCode;

    fn segment(text: &str, start_ms: u64, end_ms: u64) -> TranscriptSegment {
        TranscriptSegment {
            text: text.into(),
            start_ms,
            end_ms,
            language: Some(LanguageCode("en".into())),
        }
    }

    #[test]
    fn a_single_chunk_passes_straight_through() {
        let mut assembler = Assembler::new();
        assembler.push_segments(&[segment("hello there", 0, 1000)]);
        assert_eq!(assembler.finish(), "hello there");
    }

    #[test]
    fn a_word_duplicated_across_a_seam_appears_once() {
        // The exact artefact the 200ms chunk overlap creates.
        let mut assembler = Assembler::new();
        assembler.push_segments(&[segment("we should ship it", 0, 8000)]);
        assembler.push_segments(&[segment("it today", 7800, 12000)]);

        assert_eq!(assembler.finish(), "we should ship it today");
    }

    #[test]
    fn a_multi_word_seam_collapses_on_the_longest_match() {
        let mut assembler = Assembler::new();
        assembler.push_segments(&[segment("the plan is to", 0, 8000)]);
        assembler.push_segments(&[segment("is to ship on friday", 7800, 12000)]);

        assert_eq!(assembler.finish(), "the plan is to ship on friday");
    }

    #[test]
    fn casing_and_punctuation_do_not_hide_a_seam_duplicate() {
        // Two decodes of the same audio routinely disagree about both.
        let mut assembler = Assembler::new();
        assembler.push_segments(&[segment("we should ship it,", 0, 8000)]);
        assembler.push_segments(&[segment("It today", 7800, 12000)]);

        assert_eq!(assembler.finish(), "we should ship it, today");
    }

    #[test]
    fn genuine_repetition_further_apart_is_preserved() {
        // The bound on the seam search exists so this text survives intact.
        let mut assembler = Assembler::new();
        assembler.push_segments(&[segment("go to the shop and then", 0, 8000)]);
        assembler.push_segments(&[segment("walk to the shop again", 7800, 12000)]);

        let out = assembler.finish();
        assert!(
            out.contains("go to the shop") && out.contains("walk to the shop"),
            "a repeated phrase that is not a seam must survive: {out}"
        );
    }

    #[test]
    fn chunks_completing_out_of_order_still_assemble_in_time_order() {
        // Background decodes finish whenever they finish. Order must come from
        // the audio, not from completion.
        let mut assembler = Assembler::new();
        assembler.push_segments(&[segment("second part", 8000, 16000)]);
        assembler.push_segments(&[segment("first part", 0, 8000)]);

        assert_eq!(assembler.finish(), "first part second part");
    }

    #[test]
    fn empty_segments_are_ignored_rather_than_creating_double_spaces() {
        let mut assembler = Assembler::new();
        assembler.push_segments(&[segment("hello", 0, 1000)]);
        assembler.push_segments(&[segment("   ", 1000, 2000)]);
        assembler.push_segments(&[segment("world", 2000, 3000)]);

        assert_eq!(assembler.finish(), "hello world");
    }

    #[test]
    fn the_first_detected_language_is_the_one_reported() {
        let mut assembler = Assembler::new();
        assembler.push_segments(&[segment("hello", 0, 1000)]);

        let mut later = segment("bonjour", 1000, 2000);
        later.language = Some(LanguageCode("fr".into()));
        assembler.push_segments(&[later]);

        assert_eq!(
            assembler.language(),
            Some("en"),
            "a later chunk changing its mind is noise, not signal"
        );
    }

    #[test]
    fn a_fully_contained_repeat_does_not_duplicate_or_truncate() {
        // The pathological case: the whole of the second piece is overlap.
        assert_eq!(join_overlapping("ship it now", "now"), "ship it now");
    }

    #[test]
    fn clearing_resets_everything_for_a_cancelled_session() {
        let mut assembler = Assembler::new();
        assembler.push_segments(&[segment("something", 0, 1000)]);
        assembler.clear();

        assert!(assembler.is_empty());
        assert_eq!(assembler.finish(), "");
        assert_eq!(assembler.language(), None);
    }
}
