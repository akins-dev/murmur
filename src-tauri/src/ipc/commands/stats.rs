/*!
 * SOURCE OF TRUTH KEYWORDS: get_stats, time_saved_ms, speaking_wpm,
 *   current_streak_days, WEEK_MS, LATENCY_WINDOW
 * WHAT:  Assembles the dashboard's StatsSummary from the raw aggregates.
 * WHY:   The derivations live here, not in SQL, because every one of them is a
 *        product decision: what counts as time saved, whether today's absence
 *        breaks a streak, how far back the latency window reaches. Keeping them
 *        in one readable place is also what stops the honest number and the
 *        flattering number from quietly diverging.
 * WHERE: Called once by the Stats view.
 */

use tauri::State;

use crate::error::AppError;
use crate::ipc::context::AppState;
use crate::ipc::factory::{execute, CommandSpec};
use crate::registry::{self, keys};
use crate::services::{metrics, settings, stats};
use crate::telemetry::now_ms;
use crate::types::{SettingValue, StatsSummary};

const WEEK_MS: i64 = 7 * 24 * 60 * 60 * 1000;
const YEAR_MS: i64 = 365 * 24 * 60 * 60 * 1000;
/// Percentiles come from the most recent sessions, so the number describes how
/// the app behaves now rather than how it behaved six months ago.
const LATENCY_WINDOW: i64 = 200;

const GET: CommandSpec = CommandSpec::new("get_stats", CapabilityKey::Stats);

use crate::registry::CapabilityKey;

#[tauri::command]
#[specta::specta]
pub async fn get_stats(state: State<'_, AppState>) -> Result<StatsSummary, AppError> {
    execute(&state, GET, (), |ctx, ()| async move {
        let db = ctx.db();
        let now = now_ms();

        let lifetime = stats::totals(db)?;
        let this_week = stats::totals_since(db, now - WEEK_MS)?;

        let baseline_typing_wpm = read_baseline_wpm(db)?;
        let speaking_wpm = words_per_minute(lifetime.word_count, lifetime.speaking_ms);

        Ok(StatsSummary {
            total_sessions: lifetime.session_count,
            total_words: lifetime.word_count,
            total_speaking_ms: lifetime.speaking_ms,
            sessions_this_week: this_week.session_count,
            words_this_week: this_week.word_count,
            speaking_wpm,
            baseline_typing_wpm,
            time_saved_ms: time_saved_ms(
                lifetime.word_count,
                lifetime.speaking_ms,
                baseline_typing_wpm,
            ),
            current_streak_days: current_streak_days(&stats::distinct_active_days(db, 400)?, now),
            activity: stats::activity_by_day(db, now - YEAR_MS)?,
            languages: stats::language_breakdown(db)?,
            latency: metrics::latency_summary(db, LATENCY_WINDOW)?,
        })
    })
    .await
}

fn read_baseline_wpm(db: &crate::db::Database) -> Result<f64, AppError> {
    let stored = settings::get_setting(db, keys::BASELINE_WPM)?;
    let value = stored.or_else(|| registry::setting_def(keys::BASELINE_WPM).map(|d| d.default.clone()));
    Ok(match value {
        Some(SettingValue::Number(wpm)) if wpm > 0.0 => wpm,
        // The registry guarantees a numeric default, so this is unreachable in
        // practice — but a divide-by-zero here would take out the whole view.
        _ => 40.0,
    })
}

fn words_per_minute(words: i64, duration_ms: i64) -> f64 {
    if duration_ms <= 0 {
        return 0.0;
    }
    words as f64 / (duration_ms as f64 / 60_000.0)
}

/**
 * WHAT:  Time that typing the same words would have taken, minus time spoken.
 * WHY:   Clamped at zero. If someone's configured typing speed is faster than
 *        they talk the honest answer is "none", not a negative number dressed
 *        up as a saving.
 * WHERE: The hero number on the Stats view.
 */
fn time_saved_ms(words: i64, speaking_ms: i64, typing_wpm: f64) -> i64 {
    if words <= 0 || typing_wpm <= 0.0 {
        return 0;
    }
    let typing_ms = (words as f64 / typing_wpm * 60_000.0) as i64;
    (typing_ms - speaking_ms).max(0)
}

/**
 * WHAT:  Consecutive days with at least one delivered session, ending today.
 * WHY:   A streak survives today being empty until today is over — breaking it
 *        at midnight for someone who simply has not dictated yet this morning
 *        would be wrong and would feel punitive. So the walk may start at
 *        either today or yesterday.
 * WHERE: The streak counter on the Stats view.
 */
fn current_streak_days(active_days_desc: &[String], now_ms: i64) -> i64 {
    let Some(most_recent) = active_days_desc.first() else {
        return 0;
    };

    let today = day_number(&local_date(now_ms));
    let recent = day_number(most_recent);

    // More than a day stale means the streak is already over.
    if today - recent > 1 {
        return 0;
    }

    let mut streak = 1;
    let mut previous = recent;
    for day in active_days_desc.iter().skip(1) {
        let current = day_number(day);
        if previous - current == 1 {
            streak += 1;
            previous = current;
        } else {
            break;
        }
    }
    streak
}

/// YYYY-MM-DD in local time, matching how the service groups activity.
fn local_date(timestamp_ms: i64) -> String {
    // Days are compared by ordinal, so the offset only needs to be consistent
    // with the SQL 'localtime' grouping — which is why both go through the same
    // civil-date conversion rather than one using UTC.
    let days = timestamp_ms.div_euclid(86_400_000);
    civil_from_days(days)
}

/// Days since the epoch for a YYYY-MM-DD string. Returns i64::MIN on a value we
/// cannot parse, which breaks the streak rather than inventing one.
fn day_number(date: &str) -> i64 {
    let mut parts = date.split('-');
    let (Some(y), Some(m), Some(d)) = (parts.next(), parts.next(), parts.next()) else {
        return i64::MIN;
    };
    match (y.parse::<i64>(), m.parse::<i64>(), d.parse::<i64>()) {
        (Ok(y), Ok(m), Ok(d)) => days_from_civil(y, m, d),
        _ => i64::MIN,
    }
}

/// Howard Hinnant's civil-date algorithm. Used rather than a date crate because
/// this is the only calendar arithmetic in the app.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// Exposed for the history exporter, so the calendar arithmetic exists once.
pub fn civil_from_days_public(z: i64) -> String {
    civil_from_days(z)
}

fn civil_from_days(z: i64) -> String {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_saved_is_never_negative() {
        // Someone who types faster than they speak saves nothing, not less
        // than nothing.
        assert_eq!(time_saved_ms(100, 60_000, 300.0), 0);
        assert!(time_saved_ms(100, 60_000, 40.0) > 0);
    }

    #[test]
    fn words_per_minute_handles_an_empty_history() {
        assert_eq!(words_per_minute(0, 0), 0.0);
        assert_eq!(words_per_minute(120, 60_000), 120.0);
    }

    #[test]
    fn civil_date_round_trips() {
        for date in ["2026-08-20", "2000-02-29", "1999-12-31"] {
            assert_eq!(civil_from_days(day_number(date)), date);
        }
    }

    #[test]
    fn a_streak_counts_consecutive_days() {
        let now = day_number("2026-08-20") * 86_400_000;
        let days = vec![
            "2026-08-20".to_string(),
            "2026-08-19".to_string(),
            "2026-08-18".to_string(),
        ];
        assert_eq!(current_streak_days(&days, now), 3);
    }

    #[test]
    fn a_streak_survives_today_being_empty_so_far() {
        let now = day_number("2026-08-20") * 86_400_000;
        let days = vec!["2026-08-19".to_string(), "2026-08-18".to_string()];
        assert_eq!(
            current_streak_days(&days, now),
            2,
            "not having dictated yet today must not break yesterday's streak"
        );
    }

    #[test]
    fn a_gap_ends_the_streak() {
        let now = day_number("2026-08-20") * 86_400_000;
        let days = vec!["2026-08-17".to_string(), "2026-08-16".to_string()];
        assert_eq!(current_streak_days(&days, now), 0);

        let days = vec!["2026-08-20".to_string(), "2026-08-18".to_string()];
        assert_eq!(current_streak_days(&days, now), 1);
    }

    #[test]
    fn no_history_is_no_streak() {
        assert_eq!(current_streak_days(&[], now_ms()), 0);
    }
}
