/*!
 * SOURCE OF TRUTH KEYWORDS: record_metric, record_metrics, metrics_for_session,
 *   latency_summary, percentile, delete_metrics_for_session
 * WHAT:  Pure SQLite access to session_metrics, plus the percentile rollup the
 *        dashboard reads.
 * WHY:   Percentiles are computed in SQL over a bounded recent window rather
 *        than in Rust over every row ever written, because this table grows by
 *        one row per stage per session forever and the dashboard must stay
 *        instant at fifty thousand sessions.
 *
 *        p95 specifically is what a latency regression shows up in — a mean
 *        hides a retry ladder firing on one call in twenty, which is exactly
 *        the failure mode the whisper parameters are chosen to avoid.
 * WHERE: Written by telemetry/latency.rs; read by services/stats.rs.
 */

use rusqlite::params;

use crate::db::Database;
use crate::error::AppResult;
use crate::types::{LatencyStage, LatencySummary, MetricSample, SessionId};

pub fn record_metric(
    db: &Database,
    session_id: &SessionId,
    sample: &MetricSample,
    recorded_at: i64,
) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute(
            "INSERT INTO session_metrics (session_id, stage, duration_ms, recorded_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                session_id.as_str(),
                sample.stage.as_str(),
                sample.duration_ms,
                recorded_at
            ],
        )?;
        Ok(())
    })
}

/// One transaction for a whole session's timings — a session emits seven or
/// eight of these and they all land at the same moment.
pub fn record_metrics(
    db: &Database,
    session_id: &SessionId,
    samples: &[MetricSample],
    recorded_at: i64,
) -> AppResult<()> {
    db.with_connection_mut(|conn| {
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(
                "INSERT INTO session_metrics (session_id, stage, duration_ms, recorded_at)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            for sample in samples {
                stmt.execute(params![
                    session_id.as_str(),
                    sample.stage.as_str(),
                    sample.duration_ms,
                    recorded_at
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    })
}

pub fn metrics_for_session(
    db: &Database,
    session_id: &SessionId,
) -> AppResult<Vec<MetricSample>> {
    db.with_connection(|conn| {
        let mut stmt = conn.prepare(
            "SELECT stage, duration_ms FROM session_metrics WHERE session_id = ?1",
        )?;
        let rows = stmt.query_map(params![session_id.as_str()], |row| {
            let stage: String = row.get(0)?;
            let duration_ms: f64 = row.get(1)?;
            Ok((stage, duration_ms))
        })?;

        let mut out = Vec::new();
        for row in rows {
            let (stage, duration_ms) = row?;
            if let Some(stage) = stage_from_str(&stage) {
                out.push(MetricSample { stage, duration_ms });
            }
        }
        Ok(out)
    })
}

/**
 * SOURCE OF TRUTH KEYWORDS: latency_summary
 * WHAT:  p50 and p95 per stage over the most recent `window` samples.
 * WHY:   Bounded to a recent window so the number reflects how the app behaves
 *        NOW. A lifetime percentile would let six good months hide a regression
 *        that landed yesterday, which defeats the purpose of measuring at all.
 * WHERE: Called by services/stats.rs for the dashboard latency panel.
 */
pub fn latency_summary(db: &Database, window: i64) -> AppResult<Vec<LatencySummary>> {
    let mut summaries = Vec::new();

    for stage in ALL_STAGES {
        let samples: Vec<f64> = db.with_connection(|conn| {
            let mut stmt = conn.prepare(
                "SELECT duration_ms FROM session_metrics
                  WHERE stage = ?1
                  ORDER BY recorded_at DESC
                  LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![stage.as_str(), window], |row| row.get::<_, f64>(0))?;
            rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
        })?;

        if samples.is_empty() {
            continue;
        }

        let mut sorted = samples;
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        summaries.push(LatencySummary {
            stage: *stage,
            p50_ms: percentile(&sorted, 0.50),
            p95_ms: percentile(&sorted, 0.95),
            sample_count: sorted.len() as i64,
        });
    }

    Ok(summaries)
}

pub fn delete_metrics_for_session(db: &Database, session_id: &SessionId) -> AppResult<()> {
    db.with_connection(|conn| {
        conn.execute(
            "DELETE FROM session_metrics WHERE session_id = ?1",
            params![session_id.as_str()],
        )?;
        Ok(())
    })
}

/**
 * WHAT:  Nearest-rank percentile over an already-sorted slice.
 * WHY:   Nearest-rank rather than interpolated: with a handful of samples an
 *        interpolated p95 reports a number no request ever took, which is the
 *        wrong thing to show beside a promise about real latency.
 * WHERE: latency_summary only.
 */
fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (fraction * sorted.len() as f64).ceil() as usize;
    let index = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[index]
}

const ALL_STAGES: &[LatencyStage] = &[
    LatencyStage::HotkeyDispatch,
    LatencyStage::DeviceOpen,
    LatencyStage::ChunkDecode,
    LatencyStage::TailDecode,
    LatencyStage::Assemble,
    LatencyStage::Enhance,
    LatencyStage::ClipboardWrite,
    LatencyStage::Inject,
    LatencyStage::TotalFinalize,
];

fn stage_from_str(value: &str) -> Option<LatencyStage> {
    ALL_STAGES.iter().copied().find(|s| s.as_str() == value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_are_nearest_rank() {
        let samples = [10.0, 20.0, 30.0, 40.0, 50.0];
        assert_eq!(percentile(&samples, 0.50), 30.0);
        assert_eq!(percentile(&samples, 0.95), 50.0);
        // Never interpolates into a value nothing measured.
        assert!(samples.contains(&percentile(&samples, 0.80)));
    }

    #[test]
    fn every_stage_round_trips_through_its_string_form() {
        for stage in ALL_STAGES {
            assert_eq!(stage_from_str(stage.as_str()), Some(*stage));
        }
    }

    #[test]
    fn summary_reports_only_stages_with_samples() -> AppResult<()> {
        let db = Database::open_in_memory()?;
        let id = SessionId("s1".into());
        db.with_connection(|conn| {
            conn.execute(
                "INSERT INTO sessions (id, started_at, outcome, engine_id, model_id)
                 VALUES ('s1', 0, 'delivered', 'whisper', 'turbo')",
                [],
            )?;
            Ok(())
        })?;

        record_metrics(
            &db,
            &id,
            &[
                MetricSample { stage: LatencyStage::TailDecode, duration_ms: 120.0 },
                MetricSample { stage: LatencyStage::TotalFinalize, duration_ms: 240.0 },
            ],
            1000,
        )?;

        let summary = latency_summary(&db, 100)?;
        assert_eq!(summary.len(), 2);
        assert!(summary.iter().all(|s| s.sample_count == 1));
        Ok(())
    }
}
