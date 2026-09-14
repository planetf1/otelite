//! Session-duration distribution (#183). Pure computation: the caller
//! supplies one (tool, session) duration row per session; this module
//! buckets the durations and computes the scalar stats.
//!
//! Durations come from two sources the storage layer merges: opencode
//! reports its own `opencode.session.duration` metric (the max of its
//! per-sample averages per session is the session's final duration);
//! every other tool is approximated from the span time range
//! (max end - min start per session.id), which the issue prescribes as
//! the fallback.

use crate::api::{
    SessionDurationBucket, SessionDurationResponse, SessionDurationRow, SessionDurationStats,
};
use crate::distribution::percentile_f64;

/// Fixed bucket labels, in ascending order.
pub const BUCKETS: [(&str, f64, f64); 5] = [
    ("<5m", 0.0, 300.0),
    ("5-15m", 300.0, 900.0),
    ("15-30m", 900.0, 1800.0),
    ("30-60m", 1800.0, 3600.0),
    (">60m", 3600.0, f64::INFINITY),
];

/// Bucket label for a duration in seconds (`None` is impossible — the
/// top bucket is open-ended; kept as Option for symmetry with
/// session-depth's `bucket_label`).
pub fn bucket_label(duration_secs: f64) -> Option<&'static str> {
    BUCKETS
        .iter()
        .find(|(_, lo, hi)| *lo <= duration_secs && duration_secs < *hi)
        .map(|b| b.0)
}

/// Build the per-(tool, bucket) histogram and scalar stats (#183).
///
/// `pct` is each cell's share of ALL sessions in the window, so the
/// full table sums to 100. Stats (median = p50, p95 — the codebase's
/// `percentile_f64` convention, mean) cover every session.
pub fn distribution(rows: &[SessionDurationRow]) -> SessionDurationResponse {
    let total = rows.len() as u64;
    // (bucket index, tool) -> count
    let mut cells: std::collections::BTreeMap<(usize, String), u64> =
        std::collections::BTreeMap::new();
    let mut durations: Vec<f64> = Vec::with_capacity(rows.len());
    for r in rows {
        let Some(bi) = bucket_index(r.duration_secs) else {
            continue;
        };
        *cells.entry((bi, r.tool.clone())).or_insert(0) += 1;
        durations.push(r.duration_secs);
    }

    let buckets: Vec<SessionDurationBucket> = cells
        .into_iter()
        .map(|((bi, tool), count)| SessionDurationBucket {
            pct: if total > 0 {
                count as f64 / total as f64 * 100.0
            } else {
                0.0
            },
            bucket: BUCKETS[bi].0.to_string(),
            tool,
            count,
        })
        .collect();

    durations.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let stats = if durations.is_empty() {
        SessionDurationStats::default()
    } else {
        let mean_secs: f64 = durations.iter().sum::<f64>() / durations.len() as f64;
        SessionDurationStats {
            sessions: total,
            median_minutes: percentile_f64(&durations, 0.5) / 60.0,
            p95_minutes: percentile_f64(&durations, 0.95) / 60.0,
            mean_minutes: mean_secs / 60.0,
        }
    };

    SessionDurationResponse {
        buckets,
        stats,
        filters_applied: Vec::new(),
    }
}

/// Bucket index for a duration in seconds (always `Some` — the top
/// bucket is open-ended).
fn bucket_index(duration_secs: f64) -> Option<usize> {
    BUCKETS
        .iter()
        .position(|(_, lo, hi)| *lo <= duration_secs && duration_secs < *hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(tool: &str, secs: f64) -> SessionDurationRow {
        SessionDurationRow {
            session_id: "s".to_string(),
            tool: tool.to_string(),
            duration_secs: secs,
        }
    }

    #[test]
    fn buckets_cover_the_boundaries() {
        // Boundary seconds map to the documented labels: the lower
        // bound is inclusive, the upper exclusive.
        assert_eq!(bucket_label(0.0), Some("<5m"));
        assert_eq!(bucket_label(299.9), Some("<5m"));
        assert_eq!(bucket_label(300.0), Some("5-15m"));
        assert_eq!(bucket_label(899.9), Some("5-15m"));
        assert_eq!(bucket_label(900.0), Some("15-30m"));
        assert_eq!(bucket_label(1800.0), Some("30-60m"));
        assert_eq!(bucket_label(3599.9), Some("30-60m"));
        assert_eq!(bucket_label(3600.0), Some(">60m"));
        assert_eq!(bucket_label(7200.0), Some(">60m"));
    }

    #[test]
    fn cells_carry_tool_and_overall_pct() {
        // 2 opencode sessions (<5m), 1 claude_code (5-15m), 1 opencode
        // (5-15m): pct is the share of all 4 sessions.
        let rows = vec![
            row("opencode", 100.0),
            row("opencode", 200.0),
            row("claude_code", 400.0),
            row("opencode", 500.0),
        ];
        let r = distribution(&rows);
        assert_eq!(r.buckets.len(), 3, "{r:?}");
        // (bucket asc, tool asc): <5m/opencode, 5-15m/claude_code,
        // 5-15m/opencode.
        assert_eq!(r.buckets[0].bucket, "<5m");
        assert_eq!(r.buckets[0].tool, "opencode");
        assert_eq!(r.buckets[0].count, 2);
        assert!((r.buckets[0].pct - 50.0).abs() < 1e-9, "{r:?}");
        assert_eq!(r.buckets[1].tool, "claude_code");
        assert_eq!(r.buckets[1].count, 1);
        assert!((r.buckets[1].pct - 25.0).abs() < 1e-9, "{r:?}");
        assert_eq!(r.buckets[2].tool, "opencode");
        assert_eq!(r.buckets[2].count, 1);
        // The full table sums to 100.
        let sum: f64 = r.buckets.iter().map(|b| b.pct).sum();
        assert!((sum - 100.0).abs() < 1e-9, "{r:?}");
    }

    #[test]
    fn stats_cover_median_p95_and_mean() {
        // Durations 1, 2, 3, 4, 5 min (60..300 s): median 3 min,
        // nearest-rank p95 = idx round(4*0.95)=4 -> 5 min, mean 3 min.
        let rows: Vec<SessionDurationRow> =
            (1..=5).map(|m| row("opencode", m as f64 * 60.0)).collect();
        let r = distribution(&rows);
        assert_eq!(r.stats.sessions, 5);
        assert!((r.stats.median_minutes - 3.0).abs() < 1e-9, "{r:?}");
        assert!((r.stats.p95_minutes - 5.0).abs() < 1e-9, "{r:?}");
        assert!((r.stats.mean_minutes - 3.0).abs() < 1e-9, "{r:?}");
    }

    #[test]
    fn empty_input_has_no_buckets_and_zero_stats() {
        let r = distribution(&[]);
        assert!(r.buckets.is_empty());
        assert_eq!(r.stats.sessions, 0);
        assert_eq!(r.stats.median_minutes, 0.0);
        assert_eq!(r.stats.p95_minutes, 0.0);
        assert_eq!(r.stats.mean_minutes, 0.0);
    }
}
