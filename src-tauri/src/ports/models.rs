/*!
 * SOURCE OF TRUTH KEYWORDS: ModelStore, ModelStatus, ensure_model,
 *   verify_model, DownloadHandle
 * WHAT:  The trait for listing, downloading, verifying and deleting model files.
 * WHY:   Async, because this is the one place the app touches the network and a
 *        574MB download must not block anything. Verification is by SHA-256 and
 *        never by file existence: a half-written model passes an existence
 *        check and then fails deep inside inference, which is indistinguishable
 *        from a corrupt install and impossible to support.
 * WHERE: Implemented by adapters/http_models; driven by onboarding and the
 *        model manager in Settings.
 */

use async_trait::async_trait;
use std::path::PathBuf;

use crate::error::AppResult;
use crate::types::{ModelDescriptor, ModelId, ModelState};

#[derive(Debug, Clone)]
pub struct ModelStatus {
    pub descriptor: ModelDescriptor,
    pub state: ModelState,
    /// Present only once the file exists and its hash has been verified.
    pub path: Option<PathBuf>,
}

#[async_trait]
pub trait ModelStore: Send + Sync {
    /// Every model the app offers, with its local state. Offline-safe.
    async fn list(&self) -> AppResult<Vec<ModelStatus>>;

    async fn status(&self, id: &ModelId) -> AppResult<ModelStatus>;

    /// Download if absent, verify by hash, then return the local path.
    /// Resumable: a partial file is continued with a range request rather than
    /// restarted. Progress is reported through the emitter the adapter was
    /// constructed with.
    async fn ensure(&self, id: &ModelId) -> AppResult<PathBuf>;

    /// Re-hash a file already on disk.
    async fn verify(&self, id: &ModelId) -> AppResult<bool>;

    async fn delete(&self, id: &ModelId) -> AppResult<()>;
}
