/*!
 * SOURCE OF TRUTH KEYWORDS: types, AudioChunk, SessionState, TranscriptSegment,
 *   EngineCapabilities, SettingValue, LatencyStage, DictionaryEntry
 * WHAT:  Barrel for every shared type in the backend.
 * WHY:   One import path, and one place to look before writing a new type. The
 *        rule that types live only here is what keeps a second, subtly
 *        different Session shape from appearing beside the first.
 * WHERE: Imported across the whole crate; mirrored to TypeScript by specta so
 *        the frontend never hand-writes an IPC type.
 */

pub mod audio;
pub mod dictionary;
pub mod engine;
pub mod language;
pub mod metrics;
pub mod numeric;
pub mod session;
pub mod settings;
pub mod transcript;

pub use audio::{AudioChunk, AudioLevel, CaptureMode, ChunkKind, DeviceInfo, TARGET_SAMPLE_RATE};
pub use dictionary::{DictionaryEntry, DictionaryId, MatchKind};
pub use language::{language_label, language_options, LanguageOption, AUTO_LANGUAGE};
pub use engine::{
    DownloadProgress, EngineCapabilities, EngineFeature, EngineId, LanguageSupport,
    ModelDescriptor, ModelId, ModelState,
};
pub use metrics::{
    ActivityDay, LanguageCount, LatencyStage, LatencySummary, MetricSample, StatsSummary,
};
pub use session::{
    DeliveryKind, RecordingMode, SessionId, SessionOutcome, SessionState, SessionSummary,
};
pub use settings::{
    ChoiceSource, HotkeyBinding, KeyModifier, SettingChoice, SettingKind, SettingValue,
};
pub use transcript::{LanguageCode, LanguageHint, Transcript, TranscriptSegment};
