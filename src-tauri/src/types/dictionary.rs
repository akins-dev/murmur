/*!
 * SOURCE OF TRUTH KEYWORDS: DictionaryEntry, MatchKind, DictionaryId
 * WHAT:  One custom vocabulary replacement.
 * WHY:   The dictionary is deliberately used twice: its terms are fed to the
 *        engine as a recognition prompt AND applied as a post-hoc replacement.
 *        Replacement alone cannot repair a term the model heard as something
 *        unrelated, and prompting alone does not catch every miss — so the
 *        entry carries what both layers need.
 * WHERE: Stored by services/dictionary.rs; consumed by the whisper adapter's
 *        prompt builder and by the replacement rule in adapters/rules.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

use super::numeric::TsNumber;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct DictionaryId(#[specta(type = TsNumber)] pub i64);

/**
 * SOURCE OF TRUTH KEYWORDS: MatchKind
 * WHAT:  How a dictionary pattern is matched against the transcript.
 * WHY:   Whole-word is the default because substring matching silently corrupts
 *        unrelated words — a replacement of "ai" would rewrite "said".
 * WHERE: On every DictionaryEntry.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MatchKind {
    /// Case-insensitive whole-word match. The default and the safe one.
    Word,
    /// Case-sensitive whole-word match.
    WordCaseSensitive,
    /// Anywhere in the text. Powerful and easy to get wrong.
    Substring,
}

impl MatchKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            MatchKind::Word => "word",
            MatchKind::WordCaseSensitive => "word_cs",
            MatchKind::Substring => "substring",
        }
    }

    /// Parses the value as stored in the database. Named to avoid shadowing
    /// `std::str::FromStr`, which carries different expectations.
    pub fn from_stored(value: &str) -> Option<Self> {
        match value {
            "word" => Some(MatchKind::Word),
            "word_cs" => Some(MatchKind::WordCaseSensitive),
            "substring" => Some(MatchKind::Substring),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DictionaryEntry {
    pub id: DictionaryId,
    /// What the model tends to produce.
    pub pattern: String,
    /// What it should have been. Also fed to the engine as prompt vocabulary.
    pub replacement: String,
    pub match_kind: MatchKind,
    pub enabled: bool,
    /// Bumped on use so the prompt builder can prioritise recent terms when the
    /// dictionary is larger than the engine's prompt budget.
    #[specta(type = Option<TsNumber>)]
    pub used_at: Option<i64>,
}
