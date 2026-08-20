/*!
 * SOURCE OF TRUTH KEYWORDS: TranscriptionEngine, TranscribeRequest,
 *   EngineHandle, transcribe, prepare, capabilities
 * WHAT:  The trait every speech-to-text engine implements.
 * WHY:   Deliberately BLOCKING, not async. Inference occupies a core for
 *        hundreds of milliseconds; wrapping that in a future invites someone to
 *        await it on the tokio runtime, where it stalls every IPC command in
 *        the app. A blocking signature makes the dedicated ASR thread the only
 *        place this can be called, and makes the wrong thing hard to write.
 *        Callers branch on `capabilities()`, never on engine identity.
 * WHERE: Implemented by adapters/whisper; the seam for Apple Speech, Parakeet
 *        and any future local model. Called only from pipeline/worker.rs.
 */

use crate::error::AppResult;
use crate::types::{AudioChunk, EngineCapabilities, LanguageHint, TranscriptSegment};

/**
 * SOURCE OF TRUTH KEYWORDS: TranscribeRequest
 * WHAT:  Everything an engine needs for one chunk beyond the audio itself.
 * WHY:   `prompt` carries the user's dictionary terms so the engine can bias
 *        recognition toward them. That is what makes a custom dictionary fix
 *        the cause rather than patch the symptom — a post-hoc replacement
 *        cannot repair a term the model heard as something unrelated.
 * WHERE: Built by pipeline/worker.rs from settings and the dictionary service.
 */
#[derive(Debug, Clone, Default)]
pub struct TranscribeRequest {
    pub language: LanguageHint,
    /// Vocabulary bias. Ignored by engines without EngineFeature::InitialPrompt.
    pub prompt: Option<String>,
}

pub trait TranscriptionEngine: Send + Sync {
    /// What this engine can actually do. Read by the UI to gate options.
    fn capabilities(&self) -> EngineCapabilities;

    /// Load weights and warm any accelerator context. Blocking and slow — call
    /// once at startup, never on the hotkey path.
    fn prepare(&self) -> AppResult<()>;

    /// True once `prepare` has completed successfully.
    fn is_ready(&self) -> bool;

    /// Decode one chunk. Blocking; call only from the ASR worker thread.
    fn transcribe(
        &self,
        chunk: &AudioChunk,
        request: &TranscribeRequest,
    ) -> AppResult<Vec<TranscriptSegment>>;
}
