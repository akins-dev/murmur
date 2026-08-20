/*!
 * SOURCE OF TRUTH KEYWORDS: EngineId, EngineCapabilities, LanguageSupport,
 *   EngineFeature, ModelId, ModelDescriptor, ModelState, DownloadProgress
 * WHAT:  What a transcription engine declares about itself, and what a model
 *        file is.
 * WHY:   Callers branch on declared capability, never on engine identity. This
 *        is the rule that stops the UI shipping a dead button: pin Hindi, pick
 *        an engine that has no Hindi, and Settings can say why up front instead
 *        of failing at 2am when someone actually presses the key. An engine
 *        that lies here is the only way that guarantee breaks, so
 *        realtime_factor is documented as measured rather than claimed.
 * WHERE: Returned by TranscriptionEngine::capabilities; read by the registry's
 *        engine gating and by Settings.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

use super::numeric::TsNumber;

use super::transcript::LanguageCode;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct EngineId(pub String);

impl EngineId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Type)]
pub struct ModelId(pub String);

impl ModelId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ModelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: LanguageSupport
 * WHAT:  Either every language the app knows, or an explicit set.
 * WHY:   `All` is not the same as a long list — Whisper genuinely covers 99
 *        languages and enumerating them at every call site would be noise, but
 *        Parakeet's omission of Hindi and Arabic must be a hard, checkable fact
 *        rather than a footnote.
 * WHERE: Part of EngineCapabilities; checked before a session starts.
 */
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LanguageSupport {
    All,
    Set { languages: Vec<LanguageCode> },
}

impl LanguageSupport {
    pub fn supports(&self, language: &LanguageCode) -> bool {
        match self {
            LanguageSupport::All => true,
            LanguageSupport::Set { languages } => languages.contains(language),
        }
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: EngineFeature
 * WHAT:  Optional abilities a capability entry can require of an engine.
 * WHERE: Declared per capability in registry/; matched against
 *        EngineCapabilities::features to gate UI.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EngineFeature {
    /// Can decode chunks during recording rather than only at the end.
    Streaming,
    /// Can detect the spoken language itself.
    LanguageAutoDetect,
    /// Accepts a vocabulary prompt to bias recognition — what makes the custom
    /// dictionary improve recognition rather than only patch the output.
    InitialPrompt,
    /// Runs with no network access at all.
    Offline,
}

/**
 * SOURCE OF TRUTH KEYWORDS: EngineCapabilities
 * WHAT:  One engine's self-declaration.
 * WHERE: Returned by every TranscriptionEngine implementation.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct EngineCapabilities {
    pub id: EngineId,
    pub display_name: String,
    pub languages: LanguageSupport,
    pub features: Vec<EngineFeature>,
    /// Measured on this machine, not taken from a datasheet.
    pub realtime_factor: f32,
    pub requires_download: bool,
    pub runs_offline: bool,
}

impl EngineCapabilities {
    pub fn has(&self, feature: EngineFeature) -> bool {
        self.features.contains(&feature)
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: ModelDescriptor, ModelState, DownloadProgress
 * WHAT:  A downloadable model, its local status, and download progress.
 * WHY:   `sha256` is mandatory because a half-written model file passes an
 *        existence check and then fails deep inside inference, which is
 *        indistinguishable from a corrupt install. Verify by hash, never by
 *        presence.
 * WHERE: Listed by the ModelStore port; rendered by the model manager.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ModelDescriptor {
    pub id: ModelId,
    pub display_name: String,
    pub description: String,
    pub url: String,
    pub sha256: String,
    #[specta(type = TsNumber)]
    pub size_bytes: u64,
    /// Rough resident memory once loaded, so an 8GB machine can be warned.
    #[specta(type = TsNumber)]
    pub approx_ram_mb: u64,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Type)]
#[serde(tag = "kind", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ModelState {
    NotDownloaded,
    Downloading {
        #[specta(type = TsNumber)]
        received_bytes: u64,
        #[specta(type = TsNumber)]
        total_bytes: u64,
    },
    Verifying,
    /// Downloaded and hash-verified, but the Neural Engine has not compiled it
    /// yet. That compile takes 15-60s once per machine and must never happen on
    /// the hotkey path.
    Optimizing,
    Ready,
    Failed { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct DownloadProgress {
    pub model_id: ModelId,
    #[specta(type = TsNumber)]
    pub received_bytes: u64,
    #[specta(type = TsNumber)]
    pub total_bytes: u64,
    #[specta(type = TsNumber)]
    pub bytes_per_second: u64,
}
