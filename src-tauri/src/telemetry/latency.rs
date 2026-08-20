/*!
 * SOURCE OF TRUTH KEYWORDS: LatencyRecorder, now_ms, stage_timer, StageTimer,
 *   take_samples
 * WHAT:  Collects per-stage timings for one session and hands them over as a
 *        batch when the session ends.
 * WHY:   Timings accumulate in memory and are written once, rather than a
 *        database round trip per stage. A session emits seven or eight of these
 *        inside the finalize budget, and spending even a millisecond each on
 *        SQLite would mean the act of measuring latency became a measurable
 *        part of it.
 *
 *        Timings are recorded against the stage enum, never a free string, so
 *        the dashboard cannot end up querying a stage nothing writes.
 * WHERE: Held by the session actor for the life of a session; drained by
 *        pipeline/deliver.rs into services/metrics.rs.
 */

use std::time::Instant;

use parking_lot::Mutex;

use crate::types::{LatencyStage, MetricSample};

/// Wall-clock milliseconds since the Unix epoch. Used for stored timestamps —
/// never for measuring a duration, which uses Instant so a clock adjustment
/// cannot produce a negative elapsed time.
pub fn now_ms() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or_default()
}

/**
 * SOURCE OF TRUTH KEYWORDS: LatencyRecorder
 * WHAT:  A thread-safe bag of stage timings for one session.
 * WHY:   Shared across the tokio side and the ASR worker thread, so it is
 *        behind a lock — but a parking_lot Mutex held for a push, never across
 *        an await, which keeps it uncontended in practice.
 * WHERE: One per session, created by the session actor.
 */
#[derive(Debug, Default)]
pub struct LatencyRecorder {
    samples: Mutex<Vec<MetricSample>>,
}

impl LatencyRecorder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, stage: LatencyStage, duration_ms: f64) {
        self.samples.lock().push(MetricSample { stage, duration_ms });
    }

    /// Starts a timer that records itself when dropped.
    pub fn stage_timer(&self, stage: LatencyStage) -> StageTimer<'_> {
        StageTimer {
            recorder: self,
            stage,
            started: Instant::now(),
        }
    }

    /// Empties the recorder and returns what it held.
    pub fn take_samples(&self) -> Vec<MetricSample> {
        std::mem::take(&mut *self.samples.lock())
    }

    pub fn is_empty(&self) -> bool {
        self.samples.lock().is_empty()
    }
}

/**
 * SOURCE OF TRUTH KEYWORDS: StageTimer
 * WHAT:  RAII timer — records its stage's elapsed time when it goes out of scope.
 * WHY:   An early return inside a stage still records the timing. A manual
 *        stop() call does not, and the paths that return early are exactly the
 *        slow ones worth knowing about.
 * WHERE: Created by LatencyRecorder::stage_timer at the top of each stage.
 */
pub struct StageTimer<'a> {
    recorder: &'a LatencyRecorder,
    stage: LatencyStage,
    started: Instant,
}

impl Drop for StageTimer<'_> {
    fn drop(&mut self) {
        let elapsed = self.started.elapsed().as_secs_f64() * 1000.0;
        self.recorder.record(self.stage, elapsed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timer_records_itself_on_drop() {
        let recorder = LatencyRecorder::new();
        {
            let _timer = recorder.stage_timer(LatencyStage::Enhance);
        }
        let samples = recorder.take_samples();
        assert_eq!(samples.len(), 1);
        assert_eq!(samples[0].stage, LatencyStage::Enhance);
    }

    #[test]
    fn a_timer_records_even_when_the_scope_exits_early() {
        let recorder = LatencyRecorder::new();

        fn bail_out(recorder: &LatencyRecorder) -> Option<()> {
            let _timer = recorder.stage_timer(LatencyStage::TailDecode);
            None?;
            Some(())
        }

        assert!(bail_out(&recorder).is_none());
        assert_eq!(recorder.take_samples().len(), 1);
    }

    #[test]
    fn taking_samples_empties_the_recorder() {
        let recorder = LatencyRecorder::new();
        recorder.record(LatencyStage::Inject, 12.0);
        assert_eq!(recorder.take_samples().len(), 1);
        assert!(recorder.is_empty());
    }
}
