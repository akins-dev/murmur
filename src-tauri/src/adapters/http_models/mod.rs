/*!
 * SOURCE OF TRUTH KEYWORDS: http_models_adapter, HttpModelStore, MODEL_CATALOG,
 *   DEFAULT_MODEL_ID, FALLBACK_MODEL_ID, hash_file, download_resumable
 * WHAT:  Barrel for the ModelStore implementation over Hugging Face.
 * WHY:   Three files because the catalog is data, the transfer is I/O, and the
 *        state machine over them is policy — and only the middle one needs a
 *        network to test. Keeping them apart is what lets the catalog and the
 *        hashing be covered without a 574MB download.
 * WHERE: Constructed by adapters/mod.rs::build_model_store; consumed through
 *        the ModelStore port by onboarding and the model manager.
 */

pub mod catalog;
pub mod download;
pub mod store;

pub use catalog::{descriptor_for, CatalogEntry, DEFAULT_MODEL_ID, FALLBACK_MODEL_ID, MODEL_CATALOG};
pub use download::{download_resumable, hash_file};
pub use store::HttpModelStore;

/**
 * SOURCE OF TRUTH KEYWORDS: model_store_local_tests
 * WHAT:  Coverage for everything the store decides without a network.
 * WHY:   The offline guarantee in the port's contract is a behaviour, so it is
 *        tested as one: listing must succeed with no connection and no files.
 *        The download itself is exercised by hand against the real host, since
 *        a mock of Hugging Face would only prove the mock resumes.
 * WHERE: `cargo test` on any machine.
 */
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppPaths;
    use crate::ports::ModelStore;
    use crate::types::{ModelId, ModelState};

    fn store() -> Option<HttpModelStore> {
        let paths = AppPaths::resolve().ok()?;
        // NullEventSink rather than a recording fake: these tests assert what
        // `list` reports, not what it announces.
        HttpModelStore::new(paths, std::sync::Arc::new(crate::ports::NullEventSink)).ok()
    }

    #[tokio::test]
    async fn listing_models_needs_no_network() {
        let Some(store) = store() else {
            eprintln!("skipped: no application support directory on this host");
            return;
        };
        let listed = store.list().await.expect("the catalog is local data");
        assert_eq!(listed.len(), MODEL_CATALOG.len());
        assert!(listed.iter().any(|m| m.descriptor.is_default));
    }

    #[tokio::test]
    async fn a_model_that_is_not_installed_reports_no_path() {
        let Some(store) = store() else {
            eprintln!("skipped: no application support directory on this host");
            return;
        };
        for status in store.list().await.expect("lists") {
            match status.state {
                ModelState::Ready => assert!(
                    status.path.is_some(),
                    "a ready model must name the file it verified"
                ),
                _ => assert!(
                    status.path.is_none(),
                    "only a hash-verified model may hand out a path"
                ),
            }
        }
    }

    #[tokio::test]
    async fn an_unknown_model_id_is_refused_everywhere() {
        let Some(store) = store() else {
            eprintln!("skipped: no application support directory on this host");
            return;
        };
        let unknown = ModelId("ggml-not-a-real-model".to_string());

        assert!(store.status(&unknown).await.is_err());
        assert!(store.ensure(&unknown).await.is_err());
        assert!(store.verify(&unknown).await.is_err());
        assert!(store.delete(&unknown).await.is_err());
    }

    /**
     * WHAT:  Drives the real ensure() over whatever is already on disk: the
     *        complete-`.part` resume short-circuit, the SHA-256 check, and the
     *        atomic rename into place.
     * WHY:   Self-skipping on both ends so it never starts a 574MB download on
     *        a machine that has not asked for one — it runs only when a partial
     *        or finished download is already sitting in the models directory.
     *        This is the one path where "verified" has to mean a hash actually
     *        ran, so it is exercised against a real file rather than a mock.
     * WHERE: `cargo test` on a machine where the model has been fetched.
     */
    #[tokio::test]
    async fn ensure_installs_a_downloaded_model_only_after_its_hash_matches() {
        let Some(store) = store() else {
            eprintln!("skipped: no application support directory on this host");
            return;
        };
        let paths = AppPaths::resolve().expect("resolved once already");
        let id = ModelId(DEFAULT_MODEL_ID.to_string());

        let installed = paths.model_file(DEFAULT_MODEL_ID);
        let partial = paths.model_download_file(DEFAULT_MODEL_ID);
        if !installed.is_file() && !partial.is_file() {
            eprintln!("skipped: the default model has not been fetched on this machine");
            return;
        }

        let started = std::time::Instant::now();
        let path = store.ensure(&id).await.expect("a verified model installs");
        eprintln!(
            "ensure() completed in {:.2}s -> {}",
            started.elapsed().as_secs_f64(),
            path.display()
        );

        assert_eq!(path, installed);
        assert!(path.is_file(), "ensure must return a path that exists");
        assert!(
            !partial.is_file(),
            "the .part file must be gone once the model is installed"
        );
        assert!(store.verify(&id).await.expect("re-hashes"));

        let status = store.status(&id).await.expect("status");
        assert!(matches!(status.state, ModelState::Ready));
        assert_eq!(status.path.as_ref(), Some(&installed));
    }

    #[tokio::test]
    async fn verifying_a_model_that_was_never_downloaded_is_false_not_an_error() {
        let Some(store) = store() else {
            eprintln!("skipped: no application support directory on this host");
            return;
        };
        let paths = AppPaths::resolve().expect("resolved once already");
        let fallback = ModelId(FALLBACK_MODEL_ID.to_string());

        if paths.model_file(FALLBACK_MODEL_ID).is_file() {
            eprintln!("skipped: the fallback model is installed on this machine");
            return;
        }
        assert!(!store.verify(&fallback).await.expect("absence is not a failure"));
    }
}
