/*!
 * SOURCE OF TRUTH KEYWORDS: TranscriptSegment, LanguageHint, LanguageCode,
 *   Transcript, join_segments
 * WHAT:  What the engine returns for a chunk, and how those pieces name their
 *        language.
 * WHY:   A segment keeps its own time span so the assembler can de-duplicate
 *        the deliberate 200ms overlap between chunks. Without the spans the
 *        only way to join is string matching, which either loses a word split
 *        across a boundary or doubles it.
 * WHERE: Returned by TranscriptionEngine::transcribe; consumed by
 *        pipeline/assembler.rs and then the TextEnhancer.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

use super::numeric::TsNumber;

/// A BCP-47-ish code as Whisper uses it: "en", "hi", "ar", "zh".
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct LanguageCode(pub String);

impl LanguageCode {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for LanguageCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: LanguageHint
 * WHAT:  Whether to auto-detect the language or pin it.
 * WHY:   Auto-detect costs a beat on the first window and occasionally misfires
 *        between similar languages, so pinning is both faster and more accurate
 *        for someone who always dictates in one language. Both are supported
 *        because neither is right for everyone.
 * WHERE: Passed into every transcribe call; sourced from settings, optionally
 *        overridden per app profile.
 */
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LanguageHint {
    #[default]
    Auto,
    Pinned { language: LanguageCode },
}


/**
 * SOURCE OF TRUTH KEYWORDS: TranscriptSegment
 * WHAT:  One decoded span of speech.
 * WHERE: Accumulated per session, then joined by pipeline/assembler.rs.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct TranscriptSegment {
    pub text: String,
    #[specta(type = TsNumber)]
    pub start_ms: u64,
    #[specta(type = TsNumber)]
    pub end_ms: u64,
    pub language: Option<LanguageCode>,
}

/**
 * SOURCE OF TRUTH KEYWORDS: Transcript
 * WHAT:  The assembled result of a session, before and after enhancement.
 * WHY:   Carrying both is what makes an accuracy complaint diagnosable: if raw
 *        is right and final is wrong, the bug is in our rules, not the model.
 * WHERE: Built by pipeline/assembler.rs, enhanced by the TextEnhancer port, and
 *        persisted whole by services/sessions.rs.
 */
#[derive(Debug, Clone, Default, Serialize, Deserialize, Type)]
pub struct Transcript {
    pub raw_text: String,
    pub final_text: String,
    pub language: Option<LanguageCode>,
    pub word_count: u32,
}
