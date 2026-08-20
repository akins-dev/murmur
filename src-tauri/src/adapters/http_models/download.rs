/*!
 * SOURCE OF TRUTH KEYWORDS: hash_file, download_resumable, ProgressReporter,
 *   HASH_BUFFER_BYTES, PROGRESS_INTERVAL_MS, resume_offset
 * WHAT:  Hashes a file on disk, and fetches a remote file into a `.part` path
 *        resuming from whatever is already there.
 * WHY:   Separated from the store so the two hard parts are each one testable
 *        thing. Resume is not a nicety: a 574MB download over a hotel network
 *        that restarts from zero on every drop never completes, and the user's
 *        only visible symptom is an app that will not finish setting up. The
 *        hash runs on a blocking thread because it reads the whole file — doing
 *        that on the async runtime would stall every other task for the
 *        duration, which on this machine is a fraction of a second and on a
 *        spinning disk is not.
 * WHERE: Used by adapters/http_models/store.rs; paths come from AppPaths.
 */

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;

use crate::error::{AppError, AppResult, ErrorCode};

/// Read size for hashing. Large enough to keep the SHA-256 core fed, small
/// enough that a 574MB file never lands in memory at once.
const HASH_BUFFER_BYTES: usize = 1024 * 1024;

/// Progress is reported no more often than this. The UI cannot use 4,000 events
/// a second and the channel behind it is bounded.
const PROGRESS_INTERVAL_MS: u128 = 150;

/**
 * SOURCE OF TRUTH KEYWORDS: ProgressReporter
 * WHAT:  Callback plumbing: received bytes in, throttled progress out.
 * WHY:   The adapter takes a plain closure rather than a Tauri `AppHandle`
 *        because an adapter that emits Tauri events directly cannot be tested
 *        and is an upward import into the layer that owns the event system.
 *        Throttling lives here rather than in the caller so every call site
 *        gets it, including the ones written later.
 * WHERE: Constructed by store.rs around the emitter it was built with.
 */
pub struct ProgressReporter<'a> {
    emit: &'a (dyn Fn(u64, u64, u64) + Send + Sync),
    total_bytes: u64,
    started: std::time::Instant,
    /// Bytes already on disk when this run began; excluded from the rate so a
    /// resumed download does not open by claiming several GB per second.
    baseline_bytes: u64,
    last_emit: std::time::Instant,
}

impl<'a> ProgressReporter<'a> {
    pub fn new(
        emit: &'a (dyn Fn(u64, u64, u64) + Send + Sync),
        total_bytes: u64,
        baseline_bytes: u64,
    ) -> Self {
        let now = std::time::Instant::now();
        Self {
            emit,
            total_bytes,
            started: now,
            baseline_bytes,
            last_emit: now,
        }
    }

    fn report(&mut self, received_bytes: u64, force: bool) {
        let now = std::time::Instant::now();
        if !force && now.duration_since(self.last_emit).as_millis() < PROGRESS_INTERVAL_MS {
            return;
        }
        self.last_emit = now;

        let elapsed = now.duration_since(self.started).as_secs_f64();
        let fresh = received_bytes.saturating_sub(self.baseline_bytes);
        let rate = if elapsed > 0.0 {
            (fresh as f64 / elapsed) as u64
        } else {
            0
        };
        (self.emit)(received_bytes, self.total_bytes, rate);
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: hash_file
 * WHAT:  The lowercase hex SHA-256 of a file, streamed.
 * WHY:   This is the only definition of "the model is fine". Existence is not
 *        one: a truncated file opens, loads, and then fails somewhere inside
 *        whisper.cpp with an error nobody can act on. Runs on a blocking
 *        thread; see the module WHY.
 * WHERE: Called by store.rs on every ensure/verify/status.
 */
pub async fn hash_file(path: PathBuf) -> AppResult<String> {
    tokio::task::spawn_blocking(move || {
        use sha2::Digest;
        use std::io::Read;

        let mut file = std::fs::File::open(&path)
            .map_err(|err| AppError::from(err).with_detail(format!("hashing {}", path.display())))?;
        let mut hasher = sha2::Sha256::new();
        let mut buffer = vec![0_u8; HASH_BUFFER_BYTES];

        loop {
            let read = file.read(&mut buffer)?;
            if read == 0 {
                break;
            }
            hasher.update(&buffer[..read]);
        }

        Ok(hex::encode(hasher.finalize()))
    })
    .await
    .map_err(|err| AppError::internal(format!("hashing task failed: {err}")))?
}

/// Bytes already downloaded into `part`, or zero if there is nothing there.
pub async fn resume_offset(part: &Path) -> u64 {
    match tokio::fs::metadata(part).await {
        Ok(meta) => meta.len(),
        Err(_) => 0,
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: download_resumable
 * WHAT:  Streams `url` into `part`, continuing from any bytes already there,
 *        and returns the final byte count.
 * WHY:   A `Range` request is sent whenever the part file is non-empty, but the
 *        response status decides what actually happens: only `206 Partial
 *        Content` means the server honoured it. A server that ignores `Range`
 *        answers `200` with the whole body, and appending that to the existing
 *        bytes produces a file of the right length made of the wrong bytes —
 *        which the hash then rejects, sending the user round the whole download
 *        again. So a `200` truncates first, deliberately.
 *        Progress is emitted as bytes land rather than per response chunk, so
 *        the reported figure is what is actually on disk.
 * WHERE: adapters/http_models/store.rs::ensure.
 */
pub async fn download_resumable(
    client: &reqwest::Client,
    url: &str,
    part: &Path,
    expected_total: u64,
    emit: &(dyn Fn(u64, u64, u64) + Send + Sync),
) -> AppResult<u64> {
    let mut offset = resume_offset(part).await;

    // The bytes are all there already — the process died between the last byte
    // landing and the hash running. Re-fetching 574MB to reach the same hash is
    // the wrong answer; hand it back and let the caller verify it.
    if offset == expected_total {
        let mut reporter = ProgressReporter::new(emit, expected_total, offset);
        reporter.report(offset, true);
        return Ok(offset);
    }

    // More on disk than the file can possibly be: a stale part from a changed
    // descriptor. Start again rather than resume into bytes that cannot match.
    if offset > expected_total {
        offset = 0;
    }

    let mut request = client.get(url);
    if offset > 0 {
        request = request.header(reqwest::header::RANGE, format!("bytes={offset}-"));
    }

    let response = request.send().await?;
    let status = response.status();

    let appending = if status == reqwest::StatusCode::PARTIAL_CONTENT {
        true
    } else if status.is_success() {
        // The server ignored Range. See the WHY.
        offset = 0;
        false
    } else {
        return Err(AppError::new(
            ErrorCode::ModelDownloadFailed,
            "The model could not be downloaded. Try again in a moment.",
        )
        .recoverable()
        .with_action(crate::error::ErrorAction::Retry)
        .with_detail(format!("{status} from {url}")));
    };

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .append(appending)
        .truncate(!appending)
        .open(part)
        .await
        .map_err(|err| AppError::from(err).with_detail(format!("opening {}", part.display())))?;

    let mut received = offset;
    let mut reporter = ProgressReporter::new(emit, expected_total, offset);
    reporter.report(received, true);

    let mut response = response;
    while let Some(bytes) = response.chunk().await? {
        file.write_all(&bytes).await?;
        received = received.saturating_add(bytes.len() as u64);
        reporter.report(received, false);
    }

    // Flush before the caller hashes: an unflushed tail is a hash mismatch that
    // looks exactly like a corrupt download.
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    reporter.report(received, true);
    Ok(received)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn hashing_matches_the_known_sha256_of_the_empty_input() {
        let dir = std::env::temp_dir().join("murmur-hash-test-empty");
        tokio::fs::create_dir_all(&dir).await.expect("temp dir");
        let path = dir.join("empty.bin");
        tokio::fs::write(&path, b"").await.expect("write");

        assert_eq!(
            hash_file(path.clone()).await.expect("hashes"),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn hashing_matches_the_known_sha256_of_abc() {
        let dir = std::env::temp_dir().join("murmur-hash-test-abc");
        tokio::fs::create_dir_all(&dir).await.expect("temp dir");
        let path = dir.join("abc.bin");
        tokio::fs::write(&path, b"abc").await.expect("write");

        assert_eq!(
            hash_file(path.clone()).await.expect("hashes"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[tokio::test]
    async fn hashing_a_file_that_is_not_there_is_an_error_not_a_panic() {
        let err = hash_file(PathBuf::from("/nonexistent/murmur/model.bin"))
            .await
            .expect_err("no such file");
        assert_eq!(err.code, ErrorCode::Io);
    }

    #[tokio::test]
    async fn a_missing_part_file_resumes_from_zero() {
        assert_eq!(
            resume_offset(Path::new("/nonexistent/murmur/model.bin.part")).await,
            0
        );
    }

    #[tokio::test]
    async fn an_existing_part_file_reports_its_length() {
        let dir = std::env::temp_dir().join("murmur-resume-test");
        tokio::fs::create_dir_all(&dir).await.expect("temp dir");
        let path = dir.join("model.bin.part");
        tokio::fs::write(&path, vec![7_u8; 4096]).await.expect("write");

        assert_eq!(resume_offset(&path).await, 4096);
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn a_resumed_download_does_not_report_the_bytes_it_did_not_fetch_as_speed() {
        let seen = std::sync::Mutex::new(Vec::new());
        let emit = |received: u64, total: u64, rate: u64| {
            if let Ok(mut seen) = seen.lock() {
                seen.push((received, total, rate));
            }
        };
        let mut reporter = ProgressReporter::new(&emit, 1000, 900);
        reporter.report(900, true);

        let captured = seen.lock().expect("not poisoned").clone();
        assert_eq!(captured.len(), 1);
        assert_eq!(captured[0].0, 900);
        assert_eq!(captured[0].1, 1000);
        // No fresh bytes yet, so no rate to claim.
        assert_eq!(captured[0].2, 0);
    }

    #[test]
    fn progress_is_throttled_so_the_event_channel_is_not_flooded() {
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let emit = |_: u64, _: u64, _: u64| {
            calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        };
        let mut reporter = ProgressReporter::new(&emit, 1000, 0);
        for byte in 0..500 {
            reporter.report(byte, false);
        }
        assert!(
            calls.load(std::sync::atomic::Ordering::Relaxed) <= 1,
            "500 immediate updates must collapse to at most one"
        );
    }
}
