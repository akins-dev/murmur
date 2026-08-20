/*!
 * SOURCE OF TRUTH KEYWORDS: StatePool, StateLease, acquire, release
 * WHAT:  A reusable set of WhisperState scratch buffers, handed out one at a
 *        time and returned automatically.
 * WHY:   WhisperState is per-inference working memory and creating one is not
 *        free, so it is reused rather than rebuilt per decode. A pool rather
 *        than a `thread_local!` because a thread_local cannot be scoped to an
 *        engine instance: two engines — a test, or a model switch that has not
 *        finished tearing the old one down — would share one slot and hand a
 *        state built from one model's weights to the other. There is exactly
 *        one ASR worker, so the lock never contends; it is here for
 *        correctness, not throughput.
 *        The lease returns on Drop because the paths that would otherwise leak
 *        a state are the error paths, and an engine that leaks its scratch
 *        space on every failed decode allocates a fresh one next time — a
 *        transient error quietly becoming a permanent slowdown.
 * WHERE: Owned by adapters/whisper/engine.rs::LoadedModel; leased for the
 *        duration of one transcribe call.
 */

use parking_lot::Mutex;
use whisper_rs::{WhisperContext, WhisperState};

use crate::error::{AppError, AppResult, ErrorCode};

pub struct StatePool {
    states: Mutex<Vec<WhisperState>>,
}

impl StatePool {
    /// Seeded with the state `prepare` already built and warmed, so the first
    /// real transcription does not pay for one.
    pub fn seeded(state: WhisperState) -> Self {
        Self {
            states: Mutex::new(vec![state]),
        }
    }

    /**
     * WHAT:  Borrows a state, building one only if the pool is empty.
     * WHERE: adapters/whisper/engine.rs::transcribe.
     */
    pub fn acquire<'a>(&'a self, context: &WhisperContext) -> AppResult<StateLease<'a>> {
        let pooled = self.states.lock().pop();
        let state = match pooled {
            Some(state) => state,
            None => context.create_state().map_err(|err| {
                AppError::new(
                    ErrorCode::EngineNotReady,
                    "Murmur could not prepare the transcription model for this recording.",
                )
                .with_detail(format!("{err:?}"))
            })?,
        };

        Ok(StateLease {
            pool: &self.states,
            state: Some(state),
        })
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: StateLease
 * WHAT:  One borrowed WhisperState. Returns to the pool when dropped.
 * WHERE: Created by StatePool::acquire.
 */
pub struct StateLease<'a> {
    pool: &'a Mutex<Vec<WhisperState>>,
    state: Option<WhisperState>,
}

impl StateLease<'_> {
    pub fn get(&mut self) -> AppResult<&mut WhisperState> {
        self.state
            .as_mut()
            .ok_or_else(|| AppError::internal("whisper state lease was already released"))
    }
}

impl Drop for StateLease<'_> {
    fn drop(&mut self) {
        if let Some(state) = self.state.take() {
            self.pool.lock().push(state);
        }
    }
}
