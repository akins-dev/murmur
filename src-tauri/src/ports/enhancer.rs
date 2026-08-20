/*!
 * SOURCE OF TRUTH KEYWORDS: TextEnhancer, EnhanceContext, EnhanceRule,
 *   RuleId, enhance
 * WHAT:  The trait that turns a raw transcript into text that reads like
 *        writing.
 * WHY:   Blocking and expected to be sub-millisecond, because the deterministic
 *        rule pipeline runs inside the finalize budget. The seam exists so a
 *        local-LLM rewrite can be dropped in later as a different implementation
 *        — that one costs 200-500ms, which is exactly why it must be opt-in and
 *        must not be able to sneak onto this path unnoticed.
 * WHERE: Implemented by adapters/rules; called by pipeline/deliver.rs after the
 *        segments are joined.
 */

use crate::error::AppResult;
use crate::types::{DictionaryEntry, LanguageCode};

/**
 * SOURCE OF TRUTH KEYWORDS: EnhanceContext
 * WHAT:  Everything the rules need besides the text.
 * WHY:   Passed in rather than read from settings inside the enhancer, so every
 *        rule is a pure function of its inputs and therefore table-testable.
 *        That test suite is the cheapest accuracy guard in the project.
 * WHERE: Built by pipeline/deliver.rs from settings, the active app profile,
 *        and the dictionary service.
 */
#[derive(Debug, Clone, Default)]
pub struct EnhanceContext {
    pub language: Option<LanguageCode>,
    pub dictionary: Vec<DictionaryEntry>,
    pub strip_fillers: bool,
    pub expand_spoken_commands: bool,
    pub normalise_punctuation: bool,
    pub capitalise_sentences: bool,
    /**
     * SOURCE OF TRUTH KEYWORDS: apply_corrections
     * WHAT:  Apply spoken self-corrections — "Tuesday, sorry, I meant
     *        Wednesday" becomes "Wednesday".
     * WHY:   Off by default, on the operator's own condition: he asked for it
     *        only if it does not cost delivery time, and any pass that can
     *        rewrite words is one more thing between speaking and pasting. It
     *        is rule-based and costs microseconds, but the default stays off
     *        because the failure mode — deleting something he meant to keep —
     *        is silent, and a silent failure should be opt-in.
     */
    pub apply_corrections: bool,
}

pub trait TextEnhancer: Send + Sync {
    fn id(&self) -> &'static str;
    /// Pure and fast. Same input plus same context must give the same output.
    fn enhance(&self, raw: &str, context: &EnhanceContext) -> AppResult<String>;
}
