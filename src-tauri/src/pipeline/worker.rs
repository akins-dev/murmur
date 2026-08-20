/*!
 * SOURCE OF TRUTH KEYWORDS: AsrWorker, WorkItem, DecodeResult, spawn,
 *   submit, shutdown, QUEUE_DEPTH
 * WHAT:  The dedicated inference thread. Chunks go in over a bounded queue,
 *        decoded segments come out over a channel.
 * WHY:   Two rules, and both are absolute:
 *
 *        **Never on tokio.** Whisper inference blocks a core for hundreds of
 *        milliseconds. On a tokio worker that stalls every IPC command in the
 *        app — the pill stops updating, the dashboard stops responding, and it
 *        presents as "the app freezes while transcribing". So this is a plain
 *        `std::thread` and the engine's `transcribe` is deliberately a blocking
 *        signature to make the wrong thing hard to write.
 *
 *        **Bounded queue.** An unbounded queue in front of an inference worker
 *        turns a transient slowdown into unbounded memory growth: audio arrives
 *        in realtime and never stops, so if decode falls behind, the backlog
 *        grows without limit until the process dies. Bounded means a slow
 *        machine drops a chunk and says so, which is recoverable and visible.
 * WHERE: Spawned once per app by session/actor.rs; fed by the chunker.
 */

use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::time::{Duration, Instant};
use std::sync::Arc;
use std::thread::JoinHandle;

use crate::ports::engine::{TranscribeRequest, TranscriptionEngine};
use crate::types::{AudioChunk, ChunkKind, LatencyStage, TranscriptSegment, SessionId};

/**
 * How many chunks may wait to be decoded. Small on purpose: chunks are 8-15
 * seconds each, so even two queued means we are already 20 seconds behind
 * realtime and something is badly wrong. A deeper queue would only let the
 * problem grow quietly.
 */
const QUEUE_DEPTH: usize = 3;

enum WorkItem {
    Decode {
        chunk: AudioChunk,
        request: TranscribeRequest,
        /// Echoed back on the result so the caller can route it. See
        /// DecodeResult::session_id.
        session_id: SessionId,
    },
    Shutdown,
}

/**
 * SOURCE OF TRUTH KEYWORDS: DecodeResult
 * WHAT:  One completed decode.
 * WHY:   Carries its own elapsed time and chunk kind so the caller can record
 *        the right latency stage without timing the channel round trip — the
 *        tail decode is the number the product promises, and measuring it
 *        anywhere but here would include queue wait that the user never sees.
 * WHERE: Sent to the session actor over a tokio channel.
 */
#[derive(Debug)]
pub struct DecodeResult {
    /**
     * Which recording this decode belongs to.
     * WHY: results for several recordings share one channel once deliveries
     * run in the background, and "the current transcript" stops being a safe
     * destination the moment two can be in flight. See actor::handle_decode.
     */
    pub session_id: SessionId,
    pub segments: Vec<TranscriptSegment>,
    pub kind: ChunkKind,
    pub elapsed_ms: f64,
    /// Which latency stage this decode belongs to.
    pub stage: LatencyStage,
    /// Set when the decode failed. The session continues on whatever succeeded.
    pub error: Option<String>,
}

/**
 * SOURCE OF TRUTH KEYWORDS: AsrWorker
 * WHAT:  Handle to the inference thread.
 * WHERE: One per app, held by the session actor.
 */
pub struct AsrWorker {
    sender: SyncSender<WorkItem>,
    handle: Option<JoinHandle<()>>,
}

impl AsrWorker {
    /**
     * WHAT:  Starts the inference thread.
     * WHY:   Takes the engine as an Arc rather than constructing one, because
     *        the engine holds the model weights and exactly one must exist for
     *        the process lifetime — creating a second context would cost ~1.5s
     *        and a second copy of the model in memory.
     * WHERE: Called once during app setup, after the engine has been prepared.
     */
    pub fn spawn(
        engine: Arc<dyn TranscriptionEngine>,
        results: tokio::sync::mpsc::Sender<DecodeResult>,
    ) -> std::io::Result<Self> {
        let (sender, receiver) = sync_channel::<WorkItem>(QUEUE_DEPTH);

        let handle = std::thread::Builder::new()
            .name("murmur-asr".into())
            .spawn(move || {
                tracing::debug!("ASR worker started");

                while let Ok(item) = receiver.recv() {
                    let WorkItem::Decode {
                        chunk,
                        request,
                        session_id,
                    } = item
                    else {
                        break;
                    };

                    let kind = chunk.kind;
                    // Only the tail is on the critical path; background chunks
                    // are recorded separately so one cannot mask the other.
                    let stage = match kind {
                        ChunkKind::Tail => LatencyStage::TailDecode,
                        ChunkKind::Interior => LatencyStage::ChunkDecode,
                    };

                    let started = Instant::now();
                    let outcome = engine.transcribe(&chunk, &request);
                    let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;

                    let result = match outcome {
                        Ok(segments) => DecodeResult {
                            session_id,
                            segments,
                            kind,
                            elapsed_ms,
                            stage,
                            error: None,
                        },
                        Err(err) => {
                            tracing::warn!(error = %err, ?kind, "decode failed");
                            DecodeResult {
                                session_id,
                                segments: Vec::new(),
                                kind,
                                elapsed_ms,
                                stage,
                                error: Some(err.message),
                            }
                        }
                    };

                    // A closed receiver means the session ended while this was
                    // decoding. That is normal on cancel — stop rather than log
                    // an error for it.
                    if results.blocking_send(result).is_err() {
                        tracing::debug!("result channel closed; ASR worker stopping");
                        break;
                    }
                }

                tracing::debug!("ASR worker stopped");
            })?;

        Ok(Self {
            sender,
            handle: Some(handle),
        })
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: submit
     * WHAT:  Queues a chunk for decoding. Never blocks.
     * WHY:   `try_send` rather than `send`, because this is called from the
     *        capture path. Blocking here would stall audio drainage and turn a
     *        slow decode into dropped input — trading a recoverable problem for
     *        an unrecoverable one. A full queue drops the chunk and says so.
     * WHERE: Called by the session actor whenever the chunker closes a chunk.
     */
    pub fn submit(
        &self,
        chunk: AudioChunk,
        request: TranscribeRequest,
        session_id: SessionId,
    ) -> bool {
        match self.sender.try_send(WorkItem::Decode {
            chunk,
            request,
            session_id,
        }) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) => {
                tracing::warn!(
                    queue_depth = QUEUE_DEPTH,
                    "ASR queue is full — dropping a chunk. Decode is behind realtime."
                );
                false
            }
            Err(TrySendError::Disconnected(_)) => {
                tracing::error!("ASR worker is gone");
                false
            }
        }
    }

    /**
     * SOURCE OF TRUTH KEYWORDS: submit_tail, bounded_wait
     * WHAT:  Queues the tail chunk, waiting a BOUNDED time for space.
     * WHY:   Used only for the TAIL, where dropping is not acceptable — that
     *        chunk is the words the user is waiting for. So it waits, unlike
     *        `submit`, which drops.
     *
     *        It waits with a deadline, and that word is doing real work. This
     *        used to be a plain blocking `send` whose comment claimed "the wait
     *        is bounded by the finalize timeout the FSM enforces above" — a
     *        guarantee that had never been built. Nothing bounded it. Called
     *        from an async fn on the actor's own task, an indefinite block
     *        froze the actor's run loop, which meant it could not process the
     *        very timeout that was supposed to rescue it. The FSM sat in
     *        Finalizing and every later hotkey press was rejected as an illegal
     *        transition: dictation dead until the app restarted.
     *
     *        Returning `false` on expiry rather than waiting forever is the
     *        whole point — a lost tail is bad, and a permanently wedged app is
     *        worse, and the caller can say which happened.
     * WHERE: Called on the transition into Finalizing, with the user's
     *        `transcription.finalize_timeout_ms`.
     */
    pub fn submit_tail(
        &self,
        chunk: AudioChunk,
        request: TranscribeRequest,
        within: Duration,
        session_id: SessionId,
    ) -> bool {
        // Poll rather than block: `SyncSender` has no timed send in std, and
        // reaching for a channel crate to get one would be more dependency than
        // the four lines it replaces. The interval only decides how promptly we
        // notice space, so it is deliberately short relative to any real
        // timeout.
        const POLL: Duration = Duration::from_millis(10);
        let deadline = Instant::now() + within;
        let mut item = WorkItem::Decode {
            chunk,
            request,
            session_id,
        };

        loop {
            match self.sender.try_send(item) {
                Ok(()) => return true,
                Err(TrySendError::Disconnected(_)) => {
                    tracing::error!("ASR worker is gone");
                    return false;
                }
                Err(TrySendError::Full(returned)) => {
                    if Instant::now() >= deadline {
                        tracing::error!(
                            timeout_ms = within.as_millis(),
                            "the ASR queue never drained; giving up on the tail"
                        );
                        return false;
                    }
                    // try_send hands the item back on failure, so nothing is
                    // cloned and nothing is lost across retries.
                    item = returned;
                    std::thread::sleep(POLL);
                }
            }
        }
    }
}

impl Drop for AsrWorker {
    fn drop(&mut self) {
        let _ = self.sender.try_send(WorkItem::Shutdown);
        if let Some(handle) = self.handle.take() {
            if handle.join().is_err() {
                tracing::error!("the ASR worker thread panicked");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::AppResult;
    use crate::types::{EngineCapabilities, EngineId, LanguageSupport};

    /// An engine that returns a fixed transcript without loading anything.
    struct FakeEngine {
        delay: std::time::Duration,
    }

    impl TranscriptionEngine for FakeEngine {
        fn capabilities(&self) -> EngineCapabilities {
            EngineCapabilities {
                id: EngineId("fake".into()),
                display_name: "Fake".into(),
                languages: LanguageSupport::All,
                features: vec![],
                realtime_factor: 1.0,
                requires_download: false,
                runs_offline: true,
            }
        }
        fn prepare(&self) -> AppResult<()> {
            Ok(())
        }
        fn is_ready(&self) -> bool {
            true
        }
        fn transcribe(
            &self,
            chunk: &AudioChunk,
            _request: &TranscribeRequest,
        ) -> AppResult<Vec<TranscriptSegment>> {
            std::thread::sleep(self.delay);
            Ok(vec![TranscriptSegment {
                text: format!("chunk at {}", chunk.start_ms),
                start_ms: chunk.start_ms,
                end_ms: chunk.end_ms,
                language: None,
            }])
        }
    }

    fn chunk(start_ms: u64, kind: ChunkKind) -> AudioChunk {
        AudioChunk {
            samples: vec![0.1; 16_000],
            start_ms,
            end_ms: start_ms + 1000,
            kind,
        }
    }

    #[tokio::test]
    async fn a_submitted_chunk_comes_back_decoded() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let worker = AsrWorker::spawn(
            Arc::new(FakeEngine {
                delay: std::time::Duration::ZERO,
            }),
            tx,
        )
        .expect("spawn");

        assert!(worker.submit(chunk(0, ChunkKind::Interior), TranscribeRequest::default(), SessionId::new()));

        let result = rx.recv().await.expect("a result");
        assert_eq!(result.segments.len(), 1);
        assert_eq!(result.kind, ChunkKind::Interior);
        assert!(result.error.is_none());
    }

    #[tokio::test]
    async fn the_tail_is_recorded_against_the_critical_path_stage() {
        // Background decodes and the tail must never be averaged together —
        // the tail is the number the product promises.
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let worker = AsrWorker::spawn(
            Arc::new(FakeEngine {
                delay: std::time::Duration::ZERO,
            }),
            tx,
        )
        .expect("spawn");

        worker.submit(chunk(0, ChunkKind::Tail), TranscribeRequest::default(), SessionId::new());
        let result = rx.recv().await.expect("a result");

        assert_eq!(result.stage, LatencyStage::TailDecode);
        assert!(result.stage.is_critical_path());
    }

    #[tokio::test]
    async fn an_interior_chunk_is_recorded_off_the_critical_path() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let worker = AsrWorker::spawn(
            Arc::new(FakeEngine {
                delay: std::time::Duration::ZERO,
            }),
            tx,
        )
        .expect("spawn");

        worker.submit(chunk(0, ChunkKind::Interior), TranscribeRequest::default(), SessionId::new());
        let result = rx.recv().await.expect("a result");

        assert_eq!(result.stage, LatencyStage::ChunkDecode);
        assert!(!result.stage.is_critical_path());
    }

    #[tokio::test]
    async fn a_full_queue_drops_rather_than_blocking_the_caller() {
        // The property that protects the audio path: submit never blocks, so a
        // slow decode cannot become dropped microphone input.
        //
        // The receiver is DROPPED rather than merely unused. An unread but open
        // channel makes the worker's `blocking_send` wait forever, which is
        // correct in production — the actor always reads — but in a test it
        // deadlocks the thread that Drop then joins. Dropping the receiver
        // models the real end-of-session case and lets the worker exit.
        let (tx, rx) = tokio::sync::mpsc::channel(1);
        drop(rx);

        let worker = AsrWorker::spawn(
            Arc::new(FakeEngine {
                delay: std::time::Duration::from_millis(200),
            }),
            tx,
        )
        .expect("spawn");

        let started = Instant::now();
        for index in 0..20 {
            worker.submit(
                chunk(index * 1000, ChunkKind::Interior),
                TranscribeRequest::default(),
                SessionId::new(),
            );
        }
        let elapsed = started.elapsed();

        assert!(
            elapsed < std::time::Duration::from_millis(500),
            "submitting must never block the capture path; took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn a_failing_engine_yields_an_error_result_rather_than_killing_the_worker() {
        struct BrokenEngine;
        impl TranscriptionEngine for BrokenEngine {
            fn capabilities(&self) -> EngineCapabilities {
                EngineCapabilities {
                    id: EngineId("broken".into()),
                    display_name: "Broken".into(),
                    languages: LanguageSupport::All,
                    features: vec![],
                    realtime_factor: 1.0,
                    requires_download: false,
                    runs_offline: true,
                }
            }
            fn prepare(&self) -> AppResult<()> {
                Ok(())
            }
            fn is_ready(&self) -> bool {
                true
            }
            fn transcribe(
                &self,
                _chunk: &AudioChunk,
                _request: &TranscribeRequest,
            ) -> AppResult<Vec<TranscriptSegment>> {
                Err(crate::error::AppError::new(
                    crate::error::ErrorCode::TranscriptionFailed,
                    "no",
                ))
            }
        }

        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let worker = AsrWorker::spawn(Arc::new(BrokenEngine), tx).expect("spawn");

        worker.submit(chunk(0, ChunkKind::Interior), TranscribeRequest::default(), SessionId::new());
        let first = rx.recv().await.expect("a result");
        assert!(first.error.is_some());

        // And the worker is still alive for the next chunk.
        worker.submit(chunk(1000, ChunkKind::Interior), TranscribeRequest::default(), SessionId::new());
        let second = rx.recv().await.expect("a second result");
        assert!(second.error.is_some());
    }
}
