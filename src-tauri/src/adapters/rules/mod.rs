/*!
 * SOURCE OF TRUTH KEYWORDS: RuleEnhancer, enhance, RULE_ORDER, text
 * WHAT:  The deterministic TextEnhancer: applies the rules in text.rs in a
 *        fixed order.
 * WHY:   The ORDER is the design, and it is not arbitrary:
 *          1. whitespace first, so every later rule can assume single spaces;
 *          2. spoken commands next, because they INSERT punctuation that later
 *             rules then need to tidy and capitalise after;
 *          3. fillers before the dictionary, so a filler cannot sit inside a
 *             phrase the dictionary is trying to match, and spoken corrections
 *             straight after them so a stray "um" cannot separate a slip from
 *             the correction that replaces it;
 *          4. the dictionary before stutter removal, so a corrected term is
 *             what gets compared for repetition;
 *          5. punctuation, then casing, then the terminal stop — casing depends
 *             on sentence boundaries existing, and the terminal stop must come
 *             last or it would be capitalised as a new sentence.
 *
 *        Every rule is individually toggleable through EnhanceContext, and the
 *        whole pass is measured in microseconds — it runs inside the finalize
 *        budget, so anything expensive here is spending the product's headline
 *        promise.
 * WHERE: Implements the TextEnhancer port; called by pipeline/deliver.rs.
 */

pub mod text;

use crate::error::AppResult;
use crate::ports::enhancer::{EnhanceContext, TextEnhancer};

pub struct RuleEnhancer;

impl RuleEnhancer {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RuleEnhancer {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEnhancer for RuleEnhancer {
    fn id(&self) -> &'static str {
        "rules"
    }

    fn enhance(&self, raw: &str, context: &EnhanceContext) -> AppResult<String> {
        let language = context.language.as_ref();

        // 1. Normalise first — see the module WHY for why the order is fixed.
        let mut out = text::normalise_whitespace(raw);

        if out.is_empty() {
            return Ok(out);
        }

        // 2. Spoken formatting: inserts punctuation and newlines.
        if context.expand_spoken_commands {
            out = text::expand_spoken_commands(&out, language);
        }

        // 3. Fillers, before the dictionary can try to match across one.
        if context.strip_fillers {
            out = text::strip_fillers(&out, language);
        }

        /*
         * 3b. Spoken self-corrections, AFTER fillers and BEFORE the dictionary.
         * After fillers, so an "um" between the slip and the correction cannot
         * push the replaced span out of reach. Before the dictionary, so the
         * text the dictionary fixes is the text that survives the correction
         * rather than a phrase that is about to be replaced.
         */
        if context.apply_corrections {
            out = text::apply_spoken_corrections(&out, language);
        }

        // 4. The user's vocabulary. Always on — it is never a downgrade.
        if !context.dictionary.is_empty() {
            out = text::apply_dictionary(&out, &context.dictionary);
        }

        // 5. Chunk seams and genuine stutters. Always on: a doubled word at a
        //    seam is an artefact of our own chunking, not something the user said.
        out = text::dedupe_stutters(&out);

        // 6. Spacing and quote characters.
        if context.normalise_punctuation {
            out = text::normalise_punctuation(&out);
        }

        // 7. Casing, which needs sentence boundaries to already exist.
        if context.capitalise_sentences {
            out = text::capitalise_sentences(&out);
        }

        // 8. The terminal stop, last, so nothing capitalises after it.
        if context.normalise_punctuation {
            out = text::ensure_terminal_punctuation(&out);
        }

        Ok(text::normalise_whitespace(&out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DictionaryEntry, DictionaryId, LanguageCode, MatchKind};

    fn context() -> EnhanceContext {
        EnhanceContext {
            language: Some(LanguageCode("en".into())),
            dictionary: vec![],
            strip_fillers: true,
            expand_spoken_commands: true,
            normalise_punctuation: true,
            capitalise_sentences: true,
            apply_corrections: true,
        }
    }

    #[test]
    fn the_full_pass_turns_speech_into_writing() {
        let enhancer = RuleEnhancer::new();
        let raw = "um  so i was thinking we should ship it comma today";

        let out = enhancer.enhance(raw, &context()).expect("enhance");

        assert_eq!(out, "So i was thinking we should ship it, today.");
    }

    #[test]
    fn the_dictionary_runs_before_stutter_removal() {
        // Ordering guard: if these swapped, the corrected term would not be
        // what gets compared for repetition.
        let enhancer = RuleEnhancer::new();
        let mut ctx = context();
        ctx.dictionary = vec![DictionaryEntry {
            id: DictionaryId(1),
            pattern: "clod".into(),
            replacement: "Claude".into(),
            match_kind: MatchKind::Word,
            enabled: true,
            used_at: None,
        }];

        let out = enhancer.enhance("clod clod is great", &ctx).expect("enhance");
        assert_eq!(out, "Claude is great.");
    }

    #[test]
    fn every_rule_can_be_turned_off_independently() {
        let enhancer = RuleEnhancer::new();
        let ctx = EnhanceContext {
            language: Some(LanguageCode("en".into())),
            dictionary: vec![],
            strip_fillers: false,
            expand_spoken_commands: false,
            normalise_punctuation: false,
            capitalise_sentences: false,
            apply_corrections: false,
        };

        let raw = "um hello comma world";
        let out = enhancer.enhance(raw, &ctx).expect("enhance");

        // Only whitespace normalisation and seam de-duplication remain, and
        // neither should have changed anything here.
        assert_eq!(out, raw);
    }

    #[test]
    fn a_non_english_transcript_passes_through_the_language_rules_untouched() {
        let enhancer = RuleEnhancer::new();
        let mut ctx = context();
        ctx.language = Some(LanguageCode("hi".into()));

        // English filler and command words must not be treated as such here.
        let out = enhancer.enhance("um like comma", &ctx).expect("enhance");
        assert!(
            out.to_lowercase().contains("um") && out.to_lowercase().contains("comma"),
            "language-specific rules must not fire on another language: got {out:?}"
        );
    }

    #[test]
    fn empty_and_whitespace_only_input_produce_nothing_rather_than_a_lone_full_stop() {
        let enhancer = RuleEnhancer::new();
        assert_eq!(enhancer.enhance("", &context()).expect("enhance"), "");
        assert_eq!(enhancer.enhance("   ", &context()).expect("enhance"), "");
    }

    #[test]
    fn enhancement_is_deterministic() {
        // The property the table tests depend on.
        let enhancer = RuleEnhancer::new();
        let raw = "um so like we should probably ship this comma today";
        let first = enhancer.enhance(raw, &context()).expect("enhance");
        let second = enhancer.enhance(raw, &context()).expect("enhance");
        assert_eq!(first, second);
    }

    #[test]
    fn the_pass_stays_well_inside_its_millisecond_budget() {
        // It runs inside the finalize budget; a slow rule here spends the
        // product's headline promise.
        let enhancer = RuleEnhancer::new();
        let raw = "um so like ".repeat(400);

        let started = std::time::Instant::now();
        let _ = enhancer.enhance(&raw, &context()).expect("enhance");
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(20),
            "enhancement took {elapsed:?} on a long transcript"
        );
    }
}
