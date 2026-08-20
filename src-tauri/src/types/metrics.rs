/*!
 * SOURCE OF TRUTH KEYWORDS: LatencyStage, MetricSample, LatencySummary,
 *   StatsSummary, ActivityDay, LanguageCount
 * WHAT:  The per-stage latency vocabulary and the aggregate shapes the
 *        dashboard renders.
 * WHY:   Every stage timestamps into the same enum so a regression is
 *        attributable to a stage rather than to "it got slower". The speed
 *        claim is the product; measuring it is how it survives feature #40.
 * WHERE: Written by telemetry/latency.rs into session_metrics; read back by
 *        services/stats.rs for the dashboard.
 */

use serde::{Deserialize, Serialize};
use specta::Type;

use super::numeric::TsNumber;

/**
 * SOURCE OF TRUTH KEYWORDS: LatencyStage
 * WHAT:  The stages of the stop-keypress-to-pasted-text path.
 * WHY:   Named separately from the background chunk decode, because only these
 *        are on the critical path — averaging the two together hides the number
 *        that actually matters.
 * WHERE: The `stage` column of session_metrics.
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Type)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LatencyStage {
    /// Hotkey event to FSM transition.
    HotkeyDispatch,
    /// CoreAudio device open — only paid in OnDemand capture mode.
    DeviceOpen,
    /// A background chunk decode. Off the critical path; recorded for context.
    ChunkDecode,
    /// The trailing fragment decode. The dominant term in the budget.
    TailDecode,
    /// Segment join and seam de-duplication.
    Assemble,
    /// The deterministic enhancement rules.
    Enhance,
    ClipboardWrite,
    /// Synthetic paste dispatch.
    Inject,
    /// Stop keypress to text on screen. The number the product promises.
    TotalFinalize,
}

impl LatencyStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            LatencyStage::HotkeyDispatch => "hotkey_dispatch",
            LatencyStage::DeviceOpen => "device_open",
            LatencyStage::ChunkDecode => "chunk_decode",
            LatencyStage::TailDecode => "tail_decode",
            LatencyStage::Assemble => "assemble",
            LatencyStage::Enhance => "enhance",
            LatencyStage::ClipboardWrite => "clipboard_write",
            LatencyStage::Inject => "inject",
            LatencyStage::TotalFinalize => "total_finalize",
        }
    }

    /// True for the stages that make up the promised latency budget.
    pub fn is_critical_path(&self) -> bool {
        !matches!(self, LatencyStage::ChunkDecode | LatencyStage::DeviceOpen)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct MetricSample {
    pub stage: LatencyStage,
    pub duration_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LatencySummary {
    pub stage: LatencyStage,
    pub p50_ms: f64,
    pub p95_ms: f64,
    #[specta(type = TsNumber)]
    pub sample_count: i64,
}

/**
 * SOURCE OF TRUTH KEYWORDS: StatsSummary, ActivityDay, LanguageCount
 * WHAT:  Everything the Stats view shows, in one round trip.
 * WHY:   One shape rather than six commands, because the view renders all of it
 *        at once and six round trips would be six chances to render half a page.
 * WHERE: Produced by services/stats.rs; consumed by the Stats view.
 */
#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct StatsSummary {
    #[specta(type = TsNumber)]
    pub total_sessions: i64,
    #[specta(type = TsNumber)]
    pub total_words: i64,
    #[specta(type = TsNumber)]
    pub total_speaking_ms: i64,
    #[specta(type = TsNumber)]
    pub sessions_this_week: i64,
    #[specta(type = TsNumber)]
    pub words_this_week: i64,
    /// Words per minute actually spoken, measured.
    pub speaking_wpm: f64,
    /// The user's typing baseline, from settings. The comparison, not a fact.
    pub baseline_typing_wpm: f64,
    /// Time that would have been spent typing the same words, minus time spoken.
    #[specta(type = TsNumber)]
    pub time_saved_ms: i64,
    #[specta(type = TsNumber)]
    pub current_streak_days: i64,
    pub activity: Vec<ActivityDay>,
    pub languages: Vec<LanguageCount>,
    pub latency: Vec<LatencySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct ActivityDay {
    /// Local date as YYYY-MM-DD.
    pub date: String,
    #[specta(type = TsNumber)]
    pub session_count: i64,
    #[specta(type = TsNumber)]
    pub word_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Type)]
pub struct LanguageCount {
    pub language: String,
    #[specta(type = TsNumber)]
    pub session_count: i64,
}
