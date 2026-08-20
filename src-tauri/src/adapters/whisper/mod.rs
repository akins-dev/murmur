/*!
 * SOURCE OF TRUTH KEYWORDS: whisper_adapter, WhisperEngine, WHISPER_ENGINE_ID,
 *   audio_ctx_for, is_hallucination, fit_prompt, measure_realtime_factor
 * WHAT:  Barrel for the whisper.cpp implementation of the TranscriptionEngine
 *        port.
 * WHY:   Split by concern rather than kept as one file, because the three
 *        things that decide whether this product works — the encoder context
 *        formula, the parameter table, and the hallucination defence — each
 *        need to be readable and testable without a 574MB model on disk. Every
 *        module here except engine.rs is pure functions or data for exactly
 *        that reason, and the tests that do need the model are quarantined in
 *        live_tests.rs where they self-skip.
 * WHERE: Constructed by adapters/mod.rs::build_engine; consumed through the
 *        port by pipeline/worker.rs.
 */

#[cfg(test)]
mod live_tests;

pub mod benchmark;
pub mod blocklist;
pub mod coreml;
pub mod engine;
pub mod hallucination;
pub mod params;
pub mod prompt;
pub mod state_pool;

pub use benchmark::{measure_realtime_factor, RealtimeMeasurement};
pub use engine::{WhisperEngine, WHISPER_ENGINE_ID};
pub use blocklist::{blocklist_for, BlockedPhrase, DropRule};
pub use coreml::coreml_encoder_path;
pub use hallucination::{is_digital_silence, is_hallucination, rms_dbfs};
pub use params::{audio_ctx_for, DecodeProfile, FULL_AUDIO_CTX};
pub use prompt::{fit_prompt, PROMPT_TOKEN_BUDGET};
pub use state_pool::{StateLease, StatePool};
