/*!
 * SOURCE OF TRUTH KEYWORDS: HttpModelStore, ensure, verify, status, list,
 *   delete, ProgressEmitter, verified_cache, CONNECT_TIMEOUT
 * WHAT:  The ModelStore implementation: a static catalog plus whatever is on
 *        disk, with resumable, hash-verified, atomically-installed downloads.
 * WHY:   No path leaves this file without a hash behind it — a truncated 574MB
 *        model passes an existence check and then fails deep inside inference,
 *        where the error is indistinguishable from a corrupt install.
 *
 *        But the GATE and the QUERIES are not the same job, and conflating
 *        them cost 4.5 seconds of the first screen a user ever sees. `ensure`
 *        is the gate: it hashes, always, because it hands a path to the engine.
 *        `list` and `status` are UI queries — they answer "what have we got",
 *        they never hand out a path, and they must never be where half a
 *        gigabyte is read. They report from the cached verdict plus cheap
 *        filesystem evidence, and a present file whose hash has not run is
 *        `Verifying`, never `Ready`: reporting a guarantee before checking it
 *        is the exact failure the hash exists to prevent.
 *
 *        The cache holds the VERDICT, not just the successes. A file that
 *        hashed wrong is as much a fact as one that hashed right, and without
 *        the negative a corrupt file of exactly the right size would report
 *        `Verifying` forever, with nothing left to verify it.
 *        Install is a rename from `.part` in the same directory, which is
 *        atomic on APFS: there is no instant at which the model path holds a
 *        partially written file.
 * WHERE: Constructed by adapters/mod.rs::build_model_store; driven by
 *        onboarding and the model manager through the ModelStore port.
 */

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use parking_lot::Mutex;

use crate::config::AppPaths;
use crate::error::{AppError, AppResult, ErrorAction, ErrorCode};
use crate::ports::{EventSink, ModelStatus, ModelStore};
use crate::types::{DownloadProgress, ModelDescriptor, ModelId, ModelState};

use super::catalog::{descriptor_for, CatalogEntry, MODEL_CATALOG};
use super::download::{download_resumable, hash_file, resume_offset};

/// Long enough for a slow handshake, short enough that an unreachable host
/// fails while the user is still looking at the screen.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

pub struct HttpModelStore {
    paths: AppPaths,
    client: reqwest::Client,
    /**
     * SOURCE OF TRUTH KEYWORDS: events, EventSink
     * WHAT:  Where download progress and model state go.
     * WHY:   The EventSink PORT, never a Tauri handle. The adapter must not
     *        know the event system exists — that is an upward import, and it
     *        would make every test of this file need a running Tauri app.
     *        One seam for both events rather than a closure per event: the
     *        store now reports state as well as progress, and a second bespoke
     *        callback beside a port built for this would be a parallel
     *        mechanism nobody asked for.
     * WHERE: Supplied by adapters/mod.rs::build_model_store.
     */
    events: Arc<dyn EventSink>,
    /// The result of every hash that has actually run this process: true when
    /// the file matched the catalog, false when it did not. Never written
    /// without a hash having run, so it can be stale-empty and never stale-true.
    verified: Mutex<HashMap<ModelId, bool>>,
    /**
     * SOURCE OF TRUTH KEYWORDS: in_flight, per_model_lock, download_race
     * WHAT:  One async lock per model id, held for the whole of `ensure`.
     * WHY:   `ensure` is reachable from two places that know nothing about each
     *        other: `download_model`, which the command factory serialises with
     *        an Exclusive reentrancy guard, and the engine warm-up thread that
     *        bootstrap spawns at launch, which touches no guard at all. On a
     *        first run both want the same model within seconds of each other —
     *        the warm-up starts during `setup`, and onboarding's Download button
     *        appears immediately after.
     *
     *        Two concurrent `download_resumable` calls compute their resume
     *        offset from the same `.part` length and then both append to it. The
     *        bytes interleave, the SHA-256 fails, the part file is deleted, and
     *        the user is told "The downloaded model was damaged in transit" —
     *        for a download that was never damaged. They retry, and if the
     *        timing repeats, so does the message.
     *
     *        The lock lives HERE rather than being fixed by reordering
     *        bootstrap, because the factory's guard is per-process-per-command
     *        and cannot see a background thread. A resource that two unrelated
     *        callers can open has to defend itself. This is also what makes the
     *        second caller cheap rather than merely safe: it waits, then finds
     *        the file installed and verified, and returns immediately.
     *
     *        A tokio Mutex, not parking_lot: the critical section awaits a
     *        multi-minute download, and holding a blocking lock across that
     *        would stall the runtime thread.
     * WHERE: Taken by `ensure`, which is the only mutating entry point.
     */
    in_flight: Mutex<HashMap<ModelId, Arc<tokio::sync::Mutex<()>>>>,
}

impl HttpModelStore {
    /**
     * SOURCE OF TRUTH KEYWORDS: gate_for
     * WHAT:  The lock for one model id, creating it on first use.
     * WHY:   Keyed per model rather than one lock for the store, so downloading
     *        a small model does not queue behind a 574MB one. The map only ever
     *        grows by the number of models in the catalog, which is single
     *        digits — not worth reaping.
     * WHERE: `ensure` and `delete`, the two mutating entry points.
     */
    fn gate_for(&self, id: &ModelId) -> Arc<tokio::sync::Mutex<()>> {
        let mut in_flight = self.in_flight.lock();
        Arc::clone(
            in_flight
                .entry(id.clone())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
    }

    /**
     * WHAT:  Builds the store around the app's resolved paths and the app's
     *        event sink.
     * WHY:   Takes AppPaths rather than building paths itself, because a model
     *        downloaded to one location and looked for in another presents as a
     *        574MB re-download and is diagnosed as a network bug.
     * WHERE: adapters/mod.rs::build_model_store.
     */
    pub fn new(paths: AppPaths, events: Arc<dyn EventSink>) -> AppResult<Self> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("murmur/", env!("CARGO_PKG_VERSION")))
            .build()?;

        Ok(Self {
            paths,
            client,
            events,
            verified: Mutex::new(HashMap::new()),
            in_flight: Mutex::new(HashMap::new()),
        })
    }

    fn entry(&self, id: &ModelId) -> AppResult<&'static CatalogEntry> {
        descriptor_for(id).ok_or_else(|| {
            AppError::new(
                ErrorCode::NotFound,
                "Murmur does not offer that transcription model.",
            )
            .with_detail(format!("unknown model id {id}"))
        })
    }

    fn model_file(&self, entry: &CatalogEntry) -> PathBuf {
        self.paths.model_file(entry.id)
    }

    fn part_file(&self, entry: &CatalogEntry) -> PathBuf {
        self.paths.model_download_file(entry.id)
    }

    /**
     * WHAT:  True when the installed file's SHA-256 equals the catalog's.
     * WHY:   Short-circuits on the cached result and on a size mismatch, in
     *        that order. The size check is not a substitute for the hash — it
     *        is the cheap way to reject a file that cannot possibly match
     *        before spending a read of the whole thing on proving it.
     * WHERE: Used by status, ensure and verify.
     */
    async fn is_installed_and_verified(&self, entry: &CatalogEntry) -> AppResult<bool> {
        let id = ModelId(entry.id.to_string());
        if let Some(verdict) = self.verified.lock().get(&id).copied() {
            return Ok(verdict);
        }

        let path = self.model_file(entry);
        let Ok(meta) = tokio::fs::metadata(&path).await else {
            return Ok(false);
        };
        if !meta.is_file() || meta.len() != entry.size_bytes {
            // Not a hash verdict — there is nothing here to have an opinion
            // about — so this is deliberately NOT cached.
            return Ok(false);
        }

        // A hash that already ran, on a file that has not changed since. This
        // is the 6.5 seconds: re-reading 574MB at every launch re-proves
        // something we proved about a byte-identical file, and it does it on
        // the path that must have the engine warm before the first hotkey.
        if self.marker_still_describes(entry).await {
            tracing::debug!(model = entry.id, "verification restored from marker");
            self.record_verdict(&id, true);
            return Ok(true);
        }

        let digest = hash_file(path).await?;
        let matches = digest.eq_ignore_ascii_case(entry.sha256);
        self.record_verdict(&id, matches);
        if matches {
            self.write_marker(entry).await;
        }
        Ok(matches)
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: verified_marker_file, VerifiedMarker
     * WHAT:  Where the record of a successful hash is kept, beside the model
     *        it describes.
     * WHY:   Beside the artifact rather than in the settings table, because it
     *        is a fact ABOUT THIS FILE and not a user preference. That siting
     *        is what makes it self-maintaining: `delete` removes it with the
     *        weights, and a model that is replaced by hand takes its marker's
     *        validity with it, because the marker records size and mtime.
     * WHERE: read by is_installed_and_verified, written by record_verdict,
     *        removed by delete and by the explicit re-verify.
     */
    fn verified_marker_file(&self, entry: &CatalogEntry) -> PathBuf {
        self.model_file(entry).with_extension("verified")
    }

    /**
     * WHAT:  Reads the marker and reports whether it still describes the file
     *        on disk.
     * WHY:   Size AND mtime, both, against the catalog's own digest. Any of the
     *        three disagreeing means the marker is about a different file than
     *        the one there now, and the answer is to hash again rather than to
     *        trust it. A marker can therefore be stale-invalid, which is safe,
     *        and never stale-valid, which would not be.
     * WHERE: is_installed_and_verified, before it considers hashing.
     */
    async fn marker_still_describes(&self, entry: &CatalogEntry) -> bool {
        let Ok(raw) = tokio::fs::read_to_string(self.verified_marker_file(entry)).await else {
            return false;
        };
        let Ok(marker) = serde_json::from_str::<VerifiedMarker>(&raw) else {
            return false;
        };
        let Ok(meta) = tokio::fs::metadata(self.model_file(entry)).await else {
            return false;
        };

        marker_still_describes(&marker, entry.sha256, meta.len(), modified_ms(&meta))
    }

    async fn write_marker(&self, entry: &CatalogEntry) {
        let Ok(meta) = tokio::fs::metadata(self.model_file(entry)).await else {
            return;
        };
        let Some(modified_ms) = modified_ms(&meta) else {
            return;
        };
        let marker = VerifiedMarker {
            sha256: entry.sha256.to_string(),
            size_bytes: meta.len(),
            modified_ms,
        };
        // A marker we cannot write costs a re-hash next launch and nothing
        // else, so a failure here is logged rather than surfaced.
        match serde_json::to_string(&marker) {
            Ok(encoded) => {
                if let Err(err) =
                    tokio::fs::write(self.verified_marker_file(entry), encoded).await
                {
                    tracing::warn!(model = entry.id, error = %err, "could not record verification");
                }
            }
            Err(err) => tracing::warn!(model = entry.id, error = %err, "could not encode marker"),
        }
    }

    /// The verdict of a hash that has already run, if one has. `None` means
    /// nobody has checked this file yet in this process — never "it is bad".
    fn cached_verdict(&self, entry: &CatalogEntry) -> Option<bool> {
        self.verified
            .lock()
            .get(&ModelId(entry.id.to_string()))
            .copied()
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: state_of, cheap_state, Verifying
     * WHAT:  The state of one catalog entry, from local evidence only, WITHOUT
     *        hashing.
     * WHY:   This is the read path behind `list` and `status`, and it used to
     *        call the gate — so the first `list_models` of a launch hashed
     *        574MB inline and the command took 4472ms. That lands during
     *        onboarding, freezing the setup screen of an app whose entire claim
     *        is that it is instant.
     *
     *        So it reports from evidence it can gather in microseconds, and is
     *        careful about what that evidence proves. A size MISMATCH is
     *        definitive: the file cannot be the catalog's, so `Failed`. A size
     *        match proves nothing — a truncation that lands on the exact byte
     *        count is unlikely, not impossible — so it is `Verifying`, with no
     *        path attached, and the state resolves when something that actually
     *        needs the guarantee runs the hash. That is `ensure`, which the
     *        engine's background prepare goes through.
     *
     *        Offline-safe by construction: nothing here touches the network,
     *        which is what lets the model manager render on a plane.
     * WHERE: list and status. The hashing gate is is_installed_and_verified.
     */
    async fn state_of(&self, entry: &CatalogEntry) -> AppResult<ModelStatus> {
        let descriptor = entry.descriptor();
        let path = self.model_file(entry);

        let installed = tokio::fs::metadata(&path)
            .await
            .ok()
            .filter(|meta| meta.is_file())
            .map(|meta| meta.len());
        let received = resume_offset(&self.part_file(entry)).await;

        let state = state_from_evidence(
            self.cached_verdict(entry),
            installed,
            entry.size_bytes,
            received,
        );

        // The port's contract: a path is handed out only once the hash has
        // verified it. Ready is the one state that means that.
        let path = matches!(state, ModelState::Ready).then_some(path);
        Ok(ModelStatus {
            descriptor,
            state,
            path,
        })
    }

    fn emit(&self, descriptor: &ModelDescriptor, received: u64, total: u64, rate: u64) {
        self.events.download_progress(DownloadProgress {
            model_id: descriptor.id.clone(),
            received_bytes: received,
            total_bytes: total,
            bytes_per_second: rate,
        });
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: record_verdict
     * WHAT:  Caches the result of a hash that has just run, and announces the
     *        state it implies.
     * WHY:   One function so the cache write and the event can never disagree,
     *        and so no future caller can record a verdict silently. The event
     *        matters most in the case nobody is watching for: the flip from
     *        Verifying to Ready happens on the background prepare thread, long
     *        after `list_models` has already answered.
     * WHERE: Every place a hash produces an answer — the gate, install, and the
     *        explicit re-verify.
     */
    fn record_verdict(&self, id: &ModelId, matches: bool) {
        self.verified.lock().insert(id.clone(), matches);
        self.events.model_state_changed(
            id.clone(),
            if matches {
                ModelState::Ready
            } else {
                ModelState::Failed {
                    message: "This model file is damaged or incomplete. Download it again."
                        .to_string(),
                }
            },
        );
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: VerifiedMarker, modified_ms
 * WHAT:  The record that a hash ran and matched, and the file it described.
 * WHY:   Persisted so the verdict survives a restart. Verifying at INSTALL is
 *        the guarantee that matters — a file only reaches the model path by
 *        hashing correctly first — and re-deriving it on every launch cost 6.5
 *        seconds of an engine that is supposed to be warm before the first
 *        hotkey. Size and mtime are stored so the marker can be invalidated by
 *        the file changing, which is the only event that can make the recorded
 *        digest wrong.
 *
 *        This is deliberately NOT protection against a hostile edit that
 *        preserves both: anyone able to write into the application support
 *        directory has better attacks available, and the model manager's
 *        explicit re-verify exists for the case where someone genuinely
 *        suspects the file.
 * WHERE: Written beside the weights as `ggml-<id>.verified`.
 */
#[derive(serde::Serialize, serde::Deserialize)]
struct VerifiedMarker {
    sha256: String,
    size_bytes: u64,
    modified_ms: u64,
}

/**
 * SOURCE OF TRUTH KEYWORDS: marker_still_describes
 * WHAT:  Whether a recorded verdict still applies to the file that is there now.
 * WHY:   Pure, so the predicate that decides whether 574MB gets re-read — and
 *        therefore whether a stale verdict could be trusted — is testable
 *        without a 574MB file. All three facts must agree: the digest is the
 *        catalog's (so a marker written for a different model release is
 *        rejected), and the size and mtime are the file's (so any edit
 *        invalidates it). An unreadable mtime is `None` and fails the check,
 *        which re-hashes — the safe direction.
 * WHERE: HttpModelStore::marker_still_describes supplies the filesystem facts.
 */
fn marker_still_describes(
    marker: &VerifiedMarker,
    expected_sha256: &str,
    actual_size: u64,
    actual_modified_ms: Option<u64>,
) -> bool {
    marker.sha256.eq_ignore_ascii_case(expected_sha256)
        && marker.size_bytes == actual_size
        && Some(marker.modified_ms) == actual_modified_ms
}

/// Milliseconds since the epoch, or None on a filesystem that cannot say —
/// which correctly invalidates the marker rather than silently trusting it.
fn modified_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_millis() as u64)
}

/**
 * SOURCE OF TRUTH KEYWORDS: state_from_evidence, ModelState, Verifying
 * WHAT:  Turns the cheap evidence — a cached hash verdict, the installed
 *        file's length, the catalog's expected length, and how many bytes of a
 *        `.part` exist — into the state the UI is told.
 * WHY:   A pure function so the one decision that can misreport a guarantee is
 *        testable without a 574MB fixture on disk. The order of the arms is the
 *        argument: a hash that has actually run outranks everything, because it
 *        is the only evidence that proves anything; a size mismatch is next,
 *        because it DISPROVES the file outright; and a size match is last and
 *        weakest, because it proves only that the file is not obviously wrong.
 *        That is why a size match reports Verifying and never Ready — the whole
 *        point of hashing is that "looks right" is not a guarantee.
 * WHERE: The only caller is HttpModelStore::state_of, which supplies the
 *        evidence. Tested directly below.
 */
fn state_from_evidence(
    verdict: Option<bool>,
    installed_len: Option<u64>,
    expected_len: u64,
    part_received: u64,
) -> ModelState {
    let damaged = || ModelState::Failed {
        message: "This model file is damaged or incomplete. Download it again.".to_string(),
    };

    // A hash that ran is the only thing that settles it, either way.
    match verdict {
        Some(true) => return ModelState::Ready,
        Some(false) => return damaged(),
        None => {}
    }

    // Installed but not yet hashed. Reported rather than silently
    // re-downloaded, so a genuinely corrupt install is visible instead of
    // looking like an app that re-downloads a model every launch.
    if let Some(len) = installed_len {
        return if len == expected_len {
            ModelState::Verifying
        } else {
            damaged()
        };
    }

    if part_received > 0 {
        return ModelState::Downloading {
            received_bytes: part_received,
            total_bytes: expected_len,
        };
    }

    ModelState::NotDownloaded
}

#[async_trait]
impl ModelStore for HttpModelStore {
    async fn list(&self) -> AppResult<Vec<ModelStatus>> {
        let mut out = Vec::with_capacity(MODEL_CATALOG.len());
        for entry in MODEL_CATALOG {
            out.push(self.state_of(entry).await?);
        }
        Ok(out)
    }

    async fn status(&self, id: &ModelId) -> AppResult<ModelStatus> {
        let entry = self.entry(id)?;
        self.state_of(entry).await
    }

    /**
     * WHAT:  Downloads if needed, verifies by hash, installs atomically, and
     *        returns the path the engine may load.
     * WHY:   The hash is checked before the rename, never after — that ordering
     *        is the whole guarantee. A file at the model path is, by
     *        construction, a file that hashed correctly, so nothing downstream
     *        has to defend against a half-written one.
     *        The `.part` file is deleted on a mismatch: keeping it would make
     *        the next resume continue appending to bytes already known to be
     *        wrong, and the user would download 574MB twice to reach the same
     *        failure.
     * WHERE: Onboarding, the model manager, and any model switch.
     */
    async fn ensure(&self, id: &ModelId) -> AppResult<PathBuf> {
        // Claim this model before looking at the disk. Taking the lock AFTER
        // the installed-and-verified check would leave the exact race open:
        // both callers would see "not installed" and both would download.
        let gate = self.gate_for(id);
        let _claimed = gate.lock().await;

        let entry = self.entry(id)?;
        let descriptor = entry.descriptor();
        let target = self.model_file(entry);

        if self.is_installed_and_verified(entry).await? {
            self.emit(&descriptor, entry.size_bytes, entry.size_bytes, 0);
            return Ok(target);
        }

        let part = self.part_file(entry);
        let emit = |received: u64, total: u64, rate: u64| {
            self.emit(&descriptor, received, total, rate);
        };

        let received = download_resumable(
            &self.client,
            &descriptor.url,
            &part,
            entry.size_bytes,
            &emit,
        )
        .await?;

        if received != entry.size_bytes {
            // Not fatal on its own — the hash below is the real gate — but a
            // length mismatch names the failure precisely in the log.
            tracing::warn!(
                model = entry.id,
                received,
                expected = entry.size_bytes,
                "download length differs from the catalog"
            );
        }

        let digest = hash_file(part.clone()).await?;
        if !digest.eq_ignore_ascii_case(entry.sha256) {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(AppError::new(
                ErrorCode::ModelChecksumMismatch,
                "The downloaded model was damaged in transit. Downloading it again should fix it.",
            )
            .recoverable()
            .with_action(ErrorAction::DownloadModel {
                model_id: entry.id.to_string(),
            })
            .with_detail(format!("expected {} got {digest}", entry.sha256)));
        }

        // Atomic within the models directory. See the module WHY.
        tokio::fs::rename(&part, &target).await.map_err(|err| {
            AppError::from(err).with_detail(format!(
                "installing {} to {}",
                part.display(),
                target.display()
            ))
        })?;

        self.record_verdict(&descriptor.id, true);
        self.write_marker(entry).await;
        self.emit(&descriptor, entry.size_bytes, entry.size_bytes, 0);
        tracing::info!(model = entry.id, "model installed and verified");

        Ok(target)
    }

    /**
     * WHAT:  Re-hashes what is on disk, ignoring anything cached.
     * WHY:   This is the repair path — a caller reaches for it precisely
     *        because it suspects the cached verdict. Honouring the cache here
     *        would make the button that exists to re-check the file not
     *        re-check the file.
     * WHERE: The model manager's "verify" action.
     */
    async fn verify(&self, id: &ModelId) -> AppResult<bool> {
        let entry = self.entry(id)?;
        self.verified.lock().remove(id);
        // The marker is a cached verdict, and this is the button that exists to
        // distrust cached verdicts. Removing it first means a re-verify cannot
        // be answered by the very record it is meant to re-derive.
        let _ = tokio::fs::remove_file(self.verified_marker_file(entry)).await;

        let path = self.model_file(entry);
        if !tokio::fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(false);
        }

        let digest = hash_file(path).await?;
        let matches = digest.eq_ignore_ascii_case(entry.sha256);
        self.record_verdict(id, matches);
        if matches {
            self.write_marker(entry).await;
        }
        Ok(matches)
    }

    /**
     * WHAT:  Removes the weights, any partial download and the Core ML encoder.
     * WHY:   All of them, because leaving the `.mlmodelc` behind means a later
     *        re-download silently pairs new weights with a stale compiled
     *        encoder — and that encoder is 1.2GB, more than twice the model, so
     *        an orphan is the largest file the app leaves behind.
     *        TWO encoder paths are removed, deliberately. `AppPaths::model_coreml_dir`
     *        builds `ggml-<id>-encoder.mlmodelc`, but whisper.cpp strips a
     *        trailing `-qX_X` before looking, so for every quantised model in
     *        our catalog it actually opens `ggml-large-v3-turbo-encoder.mlmodelc`
     *        — a different directory. Until those agree, deleting only one of
     *        them leaves the real one on disk forever.
     *        Missing files are not an error: delete is idempotent so a
     *        half-cleaned state can always be cleaned again.
     * WHERE: The model manager's "remove" action.
     */
    async fn delete(&self, id: &ModelId) -> AppResult<()> {
        // Same gate as `ensure`. Deleting the weights out from under an
        // in-flight download would leave a `.part` file with no owner and a
        // hash that fails for a reason nobody could reconstruct from a log.
        let gate = self.gate_for(id);
        let _claimed = gate.lock().await;

        let entry = self.entry(id)?;
        self.verified.lock().remove(id);

        let _ = tokio::fs::remove_file(self.model_file(entry)).await;
        let _ = tokio::fs::remove_file(self.part_file(entry)).await;
        let _ = tokio::fs::remove_file(self.verified_marker_file(entry)).await;
        let _ = tokio::fs::remove_dir_all(self.paths.model_coreml_dir(entry.id)).await;
        // The name whisper.cpp actually opens. See the WHY above.
        let unquantised = strip_quantisation_suffix(entry.id);
        if unquantised != entry.id {
            let _ = tokio::fs::remove_dir_all(self.paths.model_coreml_dir(unquantised)).await;
        }

        self.events
            .model_state_changed(id.clone(), ModelState::NotDownloaded);
        tracing::info!(model = entry.id, "model removed");
        Ok(())
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: strip_quantisation_suffix
 * WHAT:  Drops a trailing `-qX_X` from a model id, mirroring whisper.cpp.
 * WHY:   whisper.cpp derives the Core ML encoder directory from the weights
 *        path with this exact rule, so a model id must be reduced the same way
 *        before the encoder it pairs with can be named. Kept here rather than
 *        imported from the whisper adapter because two adapters must not depend
 *        on each other — the shared fact belongs to AppPaths, and until it
 *        moves there this is the store's own copy with a pointer to why.
 * WHERE: adapters/http_models/store.rs::delete.
 */
fn strip_quantisation_suffix(model_id: &str) -> &str {
    match model_id.rfind('-') {
        Some(pos) => {
            let suffix = &model_id[pos..];
            let bytes = suffix.as_bytes();
            if suffix.len() == 5 && bytes[1] == b'q' && bytes[3] == b'_' {
                &model_id[..pos]
            } else {
                model_id
            }
        }
        None => model_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /**
     * WHAT:  The state a present-but-unhashed file is reported as.
     * WHY:   This is the assertion that stops the 4.5s fix from becoming a
     *        correctness bug. Making `list` cheap is only safe while it refuses
     *        to say Ready on evidence that cannot support it, and "the file is
     *        exactly the right size" cannot. If this ever reads Ready, the app
     *        is claiming a hash it never ran.
     * WHERE: state_from_evidence, behind list and status.
     */
    #[test]
    fn a_file_that_has_not_been_hashed_is_never_reported_ready() {
        const SIZE: u64 = 574_041_195;

        // Exactly the right size, and still not a guarantee.
        assert!(matches!(
            state_from_evidence(None, Some(SIZE), SIZE, 0),
            ModelState::Verifying
        ));

        // A size mismatch DISPROVES it, so that one is definitive.
        assert!(matches!(
            state_from_evidence(None, Some(SIZE - 1), SIZE, 0),
            ModelState::Failed { .. }
        ));
    }

    #[test]
    fn a_hash_that_ran_outranks_every_other_signal() {
        const SIZE: u64 = 574_041_195;

        // Verified: Ready even though nothing else changed.
        assert!(matches!(
            state_from_evidence(Some(true), Some(SIZE), SIZE, 0),
            ModelState::Ready
        ));

        // Hashed and WRONG, at exactly the right size. Without the cached
        // negative this would report Verifying forever, with nothing left to
        // verify it — see the module WHY.
        assert!(matches!(
            state_from_evidence(Some(false), Some(SIZE), SIZE, 0),
            ModelState::Failed { .. }
        ));
    }

    #[test]
    fn a_partial_download_is_reported_only_when_nothing_is_installed() {
        const SIZE: u64 = 574_041_195;

        assert!(matches!(
            state_from_evidence(None, None, SIZE, 1_000),
            ModelState::Downloading {
                received_bytes: 1_000,
                total_bytes: SIZE,
            }
        ));
        assert!(matches!(
            state_from_evidence(None, None, SIZE, 0),
            ModelState::NotDownloaded
        ));

        // An installed file wins over a leftover `.part`: resuming on top of a
        // model that is already there would download 574MB for nothing.
        assert!(matches!(
            state_from_evidence(None, Some(SIZE), SIZE, 1_000),
            ModelState::Verifying
        ));
    }

    /**
     * WHAT:  Proves `list` does no hashing, by checking it leaves no verdict
     *        behind.
     * WHY:   The regression this guards cost 4472ms on the first `list_models`
     *        of every launch, in the middle of onboarding. The obvious test is
     *        a stopwatch, and a stopwatch is the wrong instrument: it asserts
     *        how fast the machine is, goes red on a busy laptop, and passes on
     *        a fast one even if the hash came back. The cache is the honest
     *        witness — it is written if and only if a hash actually ran, so an
     *        empty cache after a full listing is proof no bytes were read.
     * WHERE: state_of and is_installed_and_verified.
     */
    #[tokio::test]
    async fn listing_models_never_hashes() {
        let Ok(paths) = AppPaths::resolve() else {
            eprintln!("skipped: no application support directory on this host");
            return;
        };
        let Ok(store) = HttpModelStore::new(paths, Arc::new(crate::ports::NullEventSink)) else {
            eprintln!("skipped: could not build an http client");
            return;
        };

        let listed = store.list().await.expect("listing is offline-safe");
        assert!(!listed.is_empty(), "the catalog is not empty");
        assert!(
            store.verified.lock().is_empty(),
            "list() hashed a model: {} verdict(s) recorded",
            store.verified.lock().len()
        );

        // And it still refuses to hand out a path it has not verified.
        for status in listed {
            if !matches!(status.state, ModelState::Ready) {
                assert!(
                    status.path.is_none(),
                    "a path was offered for a model in state {:?}",
                    status.state
                );
            }
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: gate_serialises_the_same_model
     * WHAT:  Two callers asking for the same model share one lock; two callers
     *        asking for different models do not.
     * WHY:   This is the mechanism that stops bootstrap's engine warm-up thread
     *        and onboarding's Download button downloading the same 574MB file
     *        into the same `.part` at the same time — which interleaves the
     *        bytes, fails the SHA-256, and tells the user their download was
     *        "damaged in transit" when nothing was damaged.
     *
     *        Asserted on the lock itself rather than by racing two real
     *        downloads: a test that needs the network to reproduce a race is a
     *        test that passes for the wrong reason on a bad day. The per-model
     *        half matters just as much — one global lock would make a small
     *        model queue behind a large one, and someone would remove it.
     */
    #[tokio::test]
    async fn the_download_gate_serialises_one_model_without_blocking_the_others() {
        let Ok(paths) = AppPaths::resolve() else {
            eprintln!("skipped: no application support directory on this host");
            return;
        };
        let Ok(store) = HttpModelStore::new(paths, Arc::new(crate::ports::NullEventSink)) else {
            eprintln!("skipped: could not build an http client");
            return;
        };

        let one = ModelId("large-v3-turbo-q5_0".to_string());
        let other = ModelId("base.en".to_string());

        // Same id, same lock — the whole point.
        let first = store.gate_for(&one);
        let second = store.gate_for(&one);
        assert!(
            Arc::ptr_eq(&first, &second),
            "two callers for the same model got different locks; the race is still open"
        );
        assert!(
            !Arc::ptr_eq(&first, &store.gate_for(&other)),
            "a per-model lock must not serialise unrelated models"
        );

        // And it actually excludes.
        let held = first.lock().await;
        assert!(
            second.try_lock().is_err(),
            "the gate let a second caller in while the first held it"
        );
        drop(held);
        assert!(
            second.try_lock().is_ok(),
            "the gate stayed locked after the holder finished"
        );
    }

    /**
     * WHAT:  Every way a stored verdict can stop applying.
     * WHY:   This predicate is the only thing standing between "we hashed this
     *        file" and "we hashed A file". It is allowed to be wrong in one
     *        direction only — re-hashing something that was fine costs 6.5
     *        seconds, while trusting a verdict about different bytes is the
     *        corrupt-model failure the hash exists to prevent. So every field
     *        is asserted to invalidate on its own.
     * WHERE: marker_still_describes, consulted before any launch-time hash.
     */
    #[test]
    fn a_stored_verdict_stops_applying_the_moment_the_file_differs() {
        const SHA: &str = "e1d2c3b4a5";
        const SIZE: u64 = 574_041_195;
        const MTIME: u64 = 1_755_000_000_000;

        let marker = VerifiedMarker {
            sha256: SHA.to_string(),
            size_bytes: SIZE,
            modified_ms: MTIME,
        };

        assert!(
            marker_still_describes(&marker, SHA, SIZE, Some(MTIME)),
            "an untouched file must not be re-hashed"
        );
        // Case is not meaningful in a hex digest, and rejecting on it would
        // re-hash 574MB every launch for nothing.
        assert!(marker_still_describes(&marker, "E1D2C3B4A5", SIZE, Some(MTIME)));

        // Each of the three, alone, must invalidate.
        assert!(
            !marker_still_describes(&marker, "ffffffffff", SIZE, Some(MTIME)),
            "a marker written for a different model release must not be trusted"
        );
        assert!(
            !marker_still_describes(&marker, SHA, SIZE - 1, Some(MTIME)),
            "a file that changed size must be re-hashed"
        );
        assert!(
            !marker_still_describes(&marker, SHA, SIZE, Some(MTIME + 1)),
            "a file that was rewritten must be re-hashed"
        );
        assert!(
            !marker_still_describes(&marker, SHA, SIZE, None),
            "an unreadable mtime must re-hash rather than trust the marker"
        );
    }

    #[test]
    fn quantisation_suffixes_are_stripped_the_way_whisper_cpp_strips_them() {
        assert_eq!(strip_quantisation_suffix("large-v3-turbo-q5_0"), "large-v3-turbo");
        assert_eq!(strip_quantisation_suffix("small-q5_1"), "small");
        assert_eq!(strip_quantisation_suffix("large-v3-turbo"), "large-v3-turbo");
        assert_eq!(strip_quantisation_suffix("base-english"), "base-english");
        assert_eq!(strip_quantisation_suffix("tiny"), "tiny");
    }
}
