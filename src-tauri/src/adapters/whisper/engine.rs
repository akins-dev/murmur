/*!
 * SOURCE OF TRUTH KEYWORDS: WhisperEngine, WHISPER_ENGINE_ID, LoadedModel,
 *   StatePool, prepare, transcribe, capabilities, coreml_in_use, absolute_ms
 * WHAT:  The TranscriptionEngine implementation over whisper.cpp: loads the
 *        model once, measures its speed, and decodes chunks into segments.
 * WHY:   One WhisperContext for the whole process. Creating one costs ~1.5s for
 *        large-v3-turbo, so a context per transcription would put that 1.5s on
 *        the hotkey path and there would be no latency story left to tell.
 *        WhisperState is the per-inference scratch space; it is pooled and
 *        reused rather than held in a thread_local, because a thread_local
 *        cannot be scoped to an engine instance and would silently alias
 *        between two engines the moment a test or a model switch created a
 *        second one. There is exactly one ASR worker, so the pool never
 *        contends.
 * WHERE: Constructed by adapters/mod.rs::build_engine with a verified model
 *        path from the ModelStore; called only from pipeline/worker.rs on the
 *        dedicated ASR thread.
 */

use std::ffi::c_int;
use std::path::PathBuf;

use parking_lot::RwLock;
use whisper_rs::{WhisperContext, WhisperContextParameters};

use crate::error::{AppError, AppResult, ErrorAction, ErrorCode};
use crate::ports::{TranscribeRequest, TranscriptionEngine};
use crate::types::{
    AudioChunk, EngineCapabilities, EngineFeature, EngineId, LanguageCode, LanguageSupport,
    TranscriptSegment, TARGET_SAMPLE_RATE,
};

use super::benchmark::measure_realtime_factor;
use super::coreml::coreml_encoder_path;
use super::hallucination::{is_digital_silence, is_hallucination, rms_dbfs};
use super::params::{build_full_params, decode_thread_count, language_code, DecodeProfile};
use super::prompt::{fit_prompt, PROMPT_TOKEN_BUDGET, TOKENIZE_CAPACITY};
use super::state_pool::StatePool;

/// The engine's stable identity. Callers must never branch on it — it exists
/// for the factory, for tracing and for the settings row.
pub const WHISPER_ENGINE_ID: &str = "whisper";

/**
 * SOURCE OF TRUTH KEYWORDS: LoadedModel
 * WHAT:  Everything that only exists after prepare() succeeded.
 * WHY:   Grouped in one struct behind one lock so "loaded" is a single
 *        observable fact. A model path plus a separate `is_ready` flag is the
 *        shape that eventually reports ready while holding no context.
 * WHERE: Held by WhisperEngine::inner.
 */
struct LoadedModel {
    /**
     * Reusable inference scratch space. See state_pool.rs for why it is a pool.
     *
     * DECLARED BEFORE `context`, and that is load-bearing: Rust drops struct
     * fields in declaration order, and every WhisperState holds Metal buffers
     * allocated from the context's device. Freeing the context first leaves
     * those states pointing at a torn-down device. Reordering these two fields
     * for tidiness reintroduces that, silently.
     */
    states: StatePool,
    context: WhisperContext,
}

/**
 * SOURCE OF TRUTH KEYWORDS: WhisperEngine
 * WHAT:  The whisper.cpp engine: model path in, transcript segments out.
 * WHERE: Behind the TranscriptionEngine port; see the module WHERE.
 */
pub struct WhisperEngine {
    model_path: PathBuf,
    n_threads: c_int,
    inner: RwLock<Option<LoadedModel>>,
    /// Filled in by prepare(); realtime_factor stays 0.0 until then, which is
    /// how a caller can tell "not measured" from "measured and slow".
    measured_factor: RwLock<f32>,
}

impl WhisperEngine {
    /**
     * WHAT:  Builds the engine around a model file that has already been
     *        hash-verified by the ModelStore.
     * WHY:   Takes a path rather than a ModelId so the adapter never reaches
     *        for the network or the filesystem layout — the store owns both,
     *        and an engine that could download its own model would be a second
     *        place where a half-written file gets loaded.
     * WHERE: adapters/mod.rs::build_engine.
     */
    pub fn new(model_path: PathBuf) -> Self {
        Self {
            model_path,
            n_threads: decode_thread_count(),
            inner: RwLock::new(None),
            measured_factor: RwLock::new(0.0),
        }
    }

    pub fn model_path(&self) -> &PathBuf {
        &self.model_path
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: coreml_available, coreml_encoder_path
     * WHAT:  Whether a Core ML encoder sits where whisper.cpp will look for it.
     * WHY:   Reported, never required — whisper.cpp falls back to Metal on its
     *        own when the encoder is absent, so treating this as a precondition
     *        would turn an optional accelerator into a startup failure.
     *        It must use whisper.cpp's own path rule and not a plausible-looking
     *        one, because this value is a claim about what the engine is doing.
     *        Derived from the model path by `coreml_encoder_path`; getting that
     *        rule wrong is how an adapter reports "Core ML active" while
     *        whisper.cpp logs "failed to load" and quietly runs on Metal, which
     *        is precisely the lie EngineCapabilities exists to prevent.
     * WHERE: Traced during prepare and read by the model manager.
     */
    pub fn coreml_available(&self) -> bool {
        coreml_encoder_path(&self.model_path).is_some_and(|path| path.is_dir())
    }

    /**
     * WHAT:  Fits the prompt to whisper's own token count rather than a
     *        word-count guess that overflows on a long term.
     * WHY:   The byte-length check is not defensive tidiness — it is the only
     *        thing standing between a large dictionary and an immediate process
     *        abort. See TOKENIZE_CAPACITY in prompt.rs for what whisper-rs does
     *        with an overflowing tokenize call. A string too long to measure
     *        safely is reported as "does not fit", which is also true.
     * WHERE: Called once per transcribe, before build_full_params.
     */
    fn fit_prompt_for(&self, model: &LoadedModel, prompt: &str) -> Option<String> {
        fit_prompt(prompt, PROMPT_TOKEN_BUDGET, |text| {
            if text.len() > TOKENIZE_CAPACITY {
                return None;
            }
            model
                .context
                .tokenize(text, TOKENIZE_CAPACITY)
                .ok()
                .map(|tokens| tokens.len())
        })
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: coreml_in_use
     * WHAT:  True when this build can use Core ML AND an encoder is installed
     *        where whisper.cpp will find it.
     * WHY:   Both halves are needed. The cargo feature alone says the code is
     *        compiled in; the encoder alone does nothing without it. Only when
     *        both hold does whisper.cpp route the encoder through the ANE — and
     *        that is exactly when a reduced `audio_ctx` silently produces an
     *        empty transcript. See DecodeProfile::for_chunk.
     *        Deliberately conservative: if the encoder is present but fails to
     *        load, whisper.cpp falls back to Metal and we give up the tail
     *        optimisation we could have had. Losing 240ms is a bad trade for a
     *        rare case; losing the user's sentence is not a trade at all.
     * WHERE: Read once per transcribe and logged during prepare.
     */
    fn coreml_in_use(&self) -> bool {
        cfg!(feature = "coreml") && self.coreml_available()
    }

    fn not_ready() -> AppError {
        AppError::new(
            ErrorCode::EngineNotReady,
            "The transcription model is still loading.",
        )
        .recoverable()
        .with_action(ErrorAction::Retry)
    }
}

impl TranscriptionEngine for WhisperEngine {
    fn capabilities(&self) -> EngineCapabilities {
        EngineCapabilities {
            id: EngineId(WHISPER_ENGINE_ID.to_string()),
            display_name: "Whisper".to_string(),
            // Whisper genuinely covers the full set; see LanguageSupport's WHY.
            languages: LanguageSupport::All,
            features: vec![
                EngineFeature::Streaming,
                EngineFeature::LanguageAutoDetect,
                EngineFeature::InitialPrompt,
                EngineFeature::Offline,
            ],
            // 0.0 until prepare() has measured it on this machine.
            realtime_factor: *self.measured_factor.read(),
            requires_download: true,
            runs_offline: true,
        }
    }

    /**
     * WHAT:  Loads the model, creates the first state, and measures speed.
     * WHY:   Blocking and slow on purpose — this is the ~1.5s model load plus,
     *        on a machine that has never run this model, the 15-60s Core ML
     *        compile. Both are paid here, once, at startup or immediately after
     *        a download, so neither can ever surface as a hang on the hotkey.
     * WHERE: Called once by the session bootstrap; never on the hotkey path.
     */
    fn prepare(&self) -> AppResult<()> {
        if self.inner.read().is_some() {
            return Ok(());
        }

        // whisper.cpp writes its own progress and decoder trace straight to
        // stderr, and the four print_* params do not cover it. Routing its log
        // through whisper-rs's hook is the only thing that silences it, and it
        // must happen before a context exists. Idempotent by contract.
        static SILENCE_WHISPER_LOGS: std::sync::Once = std::sync::Once::new();
        SILENCE_WHISPER_LOGS.call_once(whisper_rs::install_logging_hooks);

        if !self.model_path.is_file() {
            return Err(AppError::new(
                ErrorCode::ModelMissing,
                "The transcription model has not been downloaded yet.",
            )
            .recoverable()
            .with_detail(format!("missing {}", self.model_path.display())));
        }

        let mut params = WhisperContextParameters::default();
        // Metal. The build enables it as a cargo feature; asking for the GPU
        // here is what actually routes the encoder onto it.
        params.use_gpu(true);
        // Flash attention is left off: it is mutually exclusive with DTW in
        // whisper-rs and interacts badly with a reduced audio_ctx, which is the
        // one optimisation this engine cannot give up.
        params.flash_attn(false);

        let context = WhisperContext::new_with_params(&self.model_path, params).map_err(|err| {
            AppError::new(
                ErrorCode::EngineNotReady,
                "Murmur could not load the transcription model.",
            )
            .with_action(ErrorAction::Retry)
            .with_detail(format!("{err:?} loading {}", self.model_path.display()))
        })?;

        let mut state = context.create_state().map_err(|err| {
            AppError::new(
                ErrorCode::EngineNotReady,
                "Murmur could not load the transcription model.",
            )
            .with_detail(format!("{err:?}"))
        })?;

        let measurement = measure_realtime_factor(&mut state, self.n_threads)?;

        tracing::info!(
            model = %self.model_path.display(),
            threads = self.n_threads,
            coreml_encoder_installed = self.coreml_available(),
            coreml_in_use = self.coreml_in_use(),
            realtime_factor = measurement.factor,
            probe_seconds = measurement.audio_seconds,
            elapsed_seconds = measurement.elapsed_seconds,
            "whisper engine prepared"
        );

        if self.coreml_in_use() {
            tracing::warn!(
                "Core ML encoder is live, so every chunk decodes at the full \
                 encoder context: whisper.cpp returns an empty transcript when \
                 audio_ctx is reduced with Core ML loaded. Tail latency will be \
                 several times higher than on Metal alone."
            );
        }

        *self.measured_factor.write() = measurement.factor;
        *self.inner.write() = Some(LoadedModel {
            context,
            states: StatePool::seeded(state),
        });

        Ok(())
    }

    fn is_ready(&self) -> bool {
        self.inner.read().is_some()
    }

    /**
     * WHAT:  Decodes one chunk into segments, already filtered and re-based
     *        onto the session's absolute timeline.
     * WHY:   Blocking; see the port's WHY. The order of the guards matters: the
     *        silence check runs before the model is touched at all, because the
     *        cheapest way to not hallucinate over an empty buffer is to never
     *        show it to the model.
     * WHERE: pipeline/worker.rs, on the ASR thread.
     */
    fn transcribe(
        &self,
        chunk: &AudioChunk,
        request: &TranscribeRequest,
    ) -> AppResult<Vec<TranscriptSegment>> {
        let guard = self.inner.read();
        let model = guard.as_ref().ok_or_else(Self::not_ready)?;

        // Defence 1 of docs/03 §2.4, as a backstop behind pipeline/vad.rs.
        if chunk.samples.is_empty() || is_digital_silence(&chunk.samples) {
            return Ok(Vec::new());
        }

        let language = language_code(&request.language);
        let prompt = request
            .prompt
            .as_deref()
            .and_then(|prompt| self.fit_prompt_for(model, prompt));

        let params = build_full_params(
            DecodeProfile::for_chunk(chunk, self.coreml_in_use()),
            chunk.samples.len(),
            language,
            prompt.as_deref(),
            self.n_threads,
        );

        let mut lease = model.states.acquire(&model.context)?;
        let state = lease.get()?;

        state.full(params, &chunk.samples).map_err(|err| {
            AppError::new(
                ErrorCode::TranscriptionFailed,
                "Murmur could not turn that recording into text.",
            )
            .recoverable()
            .with_detail(format!("{err:?}"))
        })?;

        // Whisper reports the detected language on the state, not per segment,
        // so it is resolved once for the chunk. A pinned hint
        // wins over the report: whisper answers -1 for "not detected" when the
        // language was never in question, and letting that become `None` would
        // silently downgrade the blocklist to its universal entries for the one
        // case where we know exactly which list applies.
        let detected = match language {
            Some(pinned) => Some(LanguageCode(pinned.to_string())),
            None => whisper_rs::get_lang_str(state.full_lang_id_from_state())
                .map(|code| LanguageCode(code.to_string())),
        };

        // Measured once for the chunk, not per segment: it describes the audio
        // that was decoded, and it is what qualifies the riskier blocklist
        // entries. See hallucination.rs::LIKELY_SILENCE_RMS_DBFS.
        let level_dbfs = rms_dbfs(&chunk.samples);

        let mut segments = Vec::new();
        for segment in state.as_iter() {
            // Lossy rather than strict: whisper can split a multi-byte
            // character across a segment boundary, and losing one glyph is a
            // better outcome than losing the sentence to a UTF-8 error.
            let text = match segment.to_str_lossy() {
                Ok(text) => text.trim().to_string(),
                Err(err) => {
                    tracing::warn!(?err, "unreadable whisper segment discarded");
                    continue;
                }
            };

            let no_speech = segment.no_speech_probability();
            if is_hallucination(
                &text,
                detected.as_ref().map(LanguageCode::as_str),
                no_speech,
                level_dbfs,
            ) {
                tracing::debug!(%text, no_speech, level_dbfs, "segment dropped as hallucination");
                continue;
            }

            segments.push(TranscriptSegment {
                text,
                // Whisper's spans are centiseconds relative to the buffer; the
                // assembler needs them on the session's clock to de-duplicate
                // the 200ms chunk overlap.
                start_ms: absolute_ms(chunk, segment.start_timestamp()),
                end_ms: absolute_ms(chunk, segment.end_timestamp()),
                language: detected.clone(),
            });
        }

        Ok(segments)
    }
}

/**
 * WHAT:  Converts a whisper segment timestamp into session-absolute ms.
 * WHY:   whisper.cpp reports centiseconds from the start of the buffer it was
 *        given, so the value is meaningless to the assembler until the chunk's
 *        own offset is added. Clamped to the chunk because with no_timestamps
 *        whisper reports the whole buffer and rounding must not push a segment
 *        past the chunk it came from.
 * WHERE: adapters/whisper/engine.rs only.
 */
fn absolute_ms(chunk: &AudioChunk, centiseconds: i64) -> u64 {
    let within = u64::try_from(centiseconds.max(0)).unwrap_or(0).saturating_mul(10);
    let chunk_len_ms = (chunk.samples.len() as u64 * 1000) / u64::from(TARGET_SAMPLE_RATE);
    chunk.start_ms.saturating_add(within.min(chunk_len_ms))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::ChunkKind;

    fn silent_chunk(seconds: u64) -> AudioChunk {
        let samples = vec![0.0_f32; (TARGET_SAMPLE_RATE as u64 * seconds) as usize];
        AudioChunk {
            samples,
            start_ms: 0,
            end_ms: seconds * 1000,
            kind: ChunkKind::Tail,
        }
    }

    #[test]
    fn an_unprepared_engine_refuses_rather_than_pretending() {
        let engine = WhisperEngine::new(PathBuf::from("/nonexistent/ggml-does-not-exist.bin"));
        assert!(!engine.is_ready());

        let err = engine
            .transcribe(&silent_chunk(1), &TranscribeRequest::default())
            .expect_err("an unloaded engine cannot transcribe");
        assert_eq!(err.code, ErrorCode::EngineNotReady);
        assert!(err.recoverable);
    }

    #[test]
    fn prepare_reports_a_missing_model_as_a_missing_model() {
        let engine = WhisperEngine::new(PathBuf::from("/nonexistent/ggml-does-not-exist.bin"));
        let err = engine.prepare().expect_err("there is no model there");
        assert_eq!(err.code, ErrorCode::ModelMissing);
    }

    #[test]
    fn capabilities_report_no_speed_until_it_has_been_measured() {
        let engine = WhisperEngine::new(PathBuf::from("/nonexistent/ggml-does-not-exist.bin"));
        let caps = engine.capabilities();
        assert_eq!(caps.realtime_factor, 0.0);
        assert!(caps.has(EngineFeature::InitialPrompt));
        assert!(caps.has(EngineFeature::Streaming));
        assert!(caps.has(EngineFeature::LanguageAutoDetect));
        assert!(caps.has(EngineFeature::Offline));
        assert!(matches!(caps.languages, LanguageSupport::All));
    }

    #[test]
    fn a_segment_timestamp_is_rebased_and_clamped_to_its_chunk() {
        let mut chunk = silent_chunk(2);
        chunk.start_ms = 8_000;
        chunk.end_ms = 10_000;

        assert_eq!(absolute_ms(&chunk, 0), 8_000);
        assert_eq!(absolute_ms(&chunk, 50), 8_500);
        // Past the end of the buffer, and negative, are both survivable.
        assert_eq!(absolute_ms(&chunk, 9_999), 10_000);
        assert_eq!(absolute_ms(&chunk, -5), 8_000);
    }
}
