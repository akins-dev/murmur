/*!
 * SOURCE OF TRUTH KEYWORDS: adapters, build_engine, build_model_store,
 *   available_engines, WHISPER_ENGINE_ID, default_engine_id
 * WHAT:  Barrel for every third-party integration, and the factory that
 *        constructs exactly one implementation per port.
 * WHY:   Construction is centralised so "only the selected adapter is ever
 *        built" is enforceable rather than aspirational — a second engine
 *        loaded speculatively would hold a second 1.1GB model resident, which
 *        on an 8GB machine is the whole app. The factory returns the trait
 *        object and nothing else: callers branch on `capabilities()`, never on
 *        which arm was taken, which is what keeps adding an engine to one arm
 *        here and one new directory below.
 * WHERE: Called from the app bootstrap in lib.rs; the trait objects it returns
 *        are consumed by pipeline/ and ipc/.
 */

use std::path::PathBuf;
use std::sync::Arc;

use crate::config::AppPaths;
use crate::error::{AppError, AppResult, ErrorCode};
use crate::ports::{EventSink, ModelStore, TranscriptionEngine};
use crate::types::EngineId;

pub mod cpal;
pub mod events;
pub mod http_models;
pub mod macos;
pub mod rules;
pub mod whisper;

pub use events::TauriEventSink;
pub use http_models::HttpModelStore;
pub use whisper::{WhisperEngine, WHISPER_ENGINE_ID};

/// The engine used unless a setting says otherwise.
pub fn default_engine_id() -> EngineId {
    EngineId(WHISPER_ENGINE_ID.to_string())
}

/// Every engine this build can construct. One entry per factory arm below.
pub fn available_engines() -> Vec<EngineId> {
    vec![default_engine_id()]
}

/**
 * SOURCE OF TRUTH KEYWORDS: build_engine
 * WHAT:  Constructs the one engine named by `id`, around an already-verified
 *        model file.
 * WHY:   The model path is an input rather than something the engine resolves,
 *        so the hash check in the ModelStore is the only gate a model file can
 *        pass through on its way into inference. An engine that could open a
 *        path of its own choosing would be a second, unverified route.
 *        The returned engine is NOT prepared: `prepare()` is a blocking ~1.5s
 *        model load and the caller decides which thread wears it.
 * WHERE: App bootstrap; the handle goes to pipeline/worker.rs.
 */
pub fn build_engine(
    id: &EngineId,
    model_path: PathBuf,
) -> AppResult<Arc<dyn TranscriptionEngine>> {
    match id.as_str() {
        WHISPER_ENGINE_ID => Ok(Arc::new(WhisperEngine::new(model_path))),
        other => Err(AppError::new(
            ErrorCode::EngineNotReady,
            "That transcription engine is not available in this version of Murmur.",
        )
        .with_detail(format!("unknown engine id {other}"))),
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: build_model_store
 * WHAT:  Constructs the model store, wired to the app's event sink.
 * WHY:   It takes the EventSink PORT rather than a Tauri handle, so the
 *        download path still never imports Tauri and the dependency arrow
 *        still points downward — but it is one seam instead of two. The store
 *        pushes both download progress and model state, and a bespoke closure
 *        per event would be a second mechanism sitting beside a port built for
 *        exactly this.
 * WHERE: App bootstrap; the handle goes to onboarding and the model manager.
 */
pub fn build_model_store(
    paths: AppPaths,
    events: Arc<dyn EventSink>,
) -> AppResult<Arc<dyn ModelStore>> {
    Ok(Arc::new(HttpModelStore::new(paths, events)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_engine_is_one_the_factory_can_build() {
        let id = default_engine_id();
        assert!(available_engines().contains(&id));
        assert!(build_engine(&id, PathBuf::from("/tmp/ggml-unused.bin")).is_ok());
    }

    #[test]
    fn an_unknown_engine_is_refused_rather_than_defaulted() {
        let built = build_engine(&EngineId("parakeet".into()), PathBuf::from("/tmp/x.bin"));
        match built {
            Ok(_) => panic!("this build has no parakeet"),
            Err(err) => assert_eq!(err.code, ErrorCode::EngineNotReady),
        }
    }

    #[test]
    fn building_an_engine_does_not_load_the_model() {
        // prepare() is where the ~1.5s load happens; construction must not be
        // able to block the caller that is only wiring the app up.
        let engine = build_engine(&default_engine_id(), PathBuf::from("/tmp/ggml-unused.bin"))
            .expect("builds");
        assert!(!engine.is_ready());
    }
}
