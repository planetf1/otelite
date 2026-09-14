//! Session depth vs cost bucketing (#180). Pure computation: the caller
//! resolves per-session turn counts and priced costs (storage query +
//! pricing layer); this module buckets sessions by turn count and
//! computes the per-(tool, bucket) statistics.

use std::collections::BTreeMap;

use crate::api::{SessionDepthBucket, SessionDepthSession};
use crate::distribution::percentile_f64;

/// Turn-count buckets, in display order.
const BUCKETS: [(&str, u64, u64); 5] = [
    ("1-5", 1, 5),
    ("6-15", 6, 15),
    ("16-30", 16, 30),
    ("31-50", 31, 50),
    ("51+", 51, u64::MAX),
];

/// Sessions with fewer turns than this are not multi-turn conversations
/// and do not contribute (#180).
const MIN_TURNS: u64 = 2;

/// Bucket label for a turn count (`None` below the minimum).
pub fn bucket_label(turns: u64) -> Option<&'static str> {
    BUCKETS
        .iter()
        .find(|(_, lo, hi)| *lo <= turns && turns <= *hi)
        .map(|b| b.0)
}

/// Per-(tool, bucket) statistics over per-session inputs (#180).
///
/// Sessions with fewer than 2 turns are dropped. Cost stats (median =
/// p50, p95 — the codebase's `percentile_f64` convention) cover only the
/// priced sessions in the bucket; a bucket with none carries `None`
/// rather than a fabricated zero. Rows are (tool asc, bucket asc).
pub fn bucket_stats(sessions: &[SessionDepthSession]) -> Vec<SessionDepthBucket> {
    // (tool, bucket index) -> (sessions, total turns, priced costs)
    let mut groups: BTreeMap<(String, usize), (u64, u64, Vec<f64>)> = BTreeMap::new();
    for s in sessions {
        if s.turn_count < MIN_TURNS {
            continue;
        }
        let Some(bi) = BUCKETS
            .iter()
            .position(|(_, lo, hi)| *lo <= s.turn_count && s.turn_count <= *hi)
        else {
            continue;
        };
        let g = groups.entry((s.tool.clone(), bi)).or_default();
        g.0 += 1;
        g.1 += s.turn_count;
        if let Some(c) = s.cost_usd {
            g.2.push(c);
        }
    }

    groups
        .into_iter()
        .map(|((tool, bi), (sessions, total_turns, mut costs))| {
            let median = if costs.is_empty() {
                None
            } else {
                costs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                Some(percentile_f64(&costs, 0.5))
            };
            let p95 = (!costs.is_empty()).then(|| percentile_f64(&costs, 0.95));
            SessionDepthBucket {
                tool,
                bucket: BUCKETS[bi].0.to_string(),
                sessions,
                avg_turns: total_turns as f64 / sessions as f64,
                median_cost_usd: median,
                p95_cost_usd: p95,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(id: &str, tool: &str, turns: u64, cost: Option<f64>) -> SessionDepthSession {
        SessionDepthSession {
            session_id: id.to_string(),
            tool: tool.to_string(),
            turn_count: turns,
            cost_usd: cost,
        }
    }

    #[test]
    fn buckets_cover_the_ranges() {
        assert_eq!(bucket_label(1), Some("1-5"));
        assert_eq!(bucket_label(5), Some("1-5"));
        assert_eq!(bucket_label(6), Some("6-15"));
        assert_eq!(bucket_label(15), Some("6-15"));
        assert_eq!(bucket_label(16), Some("16-30"));
        assert_eq!(bucket_label(30), Some("16-30"));
        assert_eq!(bucket_label(31), Some("31-50"));
        assert_eq!(bucket_label(50), Some("31-50"));
        assert_eq!(bucket_label(51), Some("51+"));
        assert_eq!(bucket_label(10_000), Some("51+"));
    }

    #[test]
    fn single_turn_sessions_are_dropped() {
        let rows = bucket_stats(&[session("a", "opencode", 1, Some(1.0))]);
        assert!(rows.is_empty());
    }

    #[test]
    fn bucket_stats_group_order_and_values() {
        let sessions = vec![
            // opencode 6-15: three priced sessions (costs 1, 2, 10).
            session("a", "opencode", 6, Some(1.0)),
            session("b", "opencode", 10, Some(2.0)),
            session("c", "opencode", 15, Some(10.0)),
            // opencode 6-15: one unpriced session (counts, no cost stats).
            session("d", "opencode", 7, None),
            // opencode 1-5: two sessions.
            session("e", "opencode", 2, Some(0.5)),
            session("f", "opencode", 5, Some(0.75)),
            // claude_code 51+: one session.
            session("g", "claude_code", 100, Some(50.0)),
            // Dropped: single turn.
            session("h", "opencode", 1, Some(9.0)),
        ];
        let rows = bucket_stats(&sessions);

        // (tool asc, bucket asc): claude_code 51+, opencode 1-5, opencode 6-15.
        assert_eq!(rows.len(), 3);
        let cc = &rows[0];
        assert_eq!(cc.tool, "claude_code");
        assert_eq!(cc.bucket, "51+");
        assert_eq!(cc.sessions, 1);
        assert_eq!(cc.avg_turns, 100.0);
        // Single priced session: median = p95 = its cost.
        assert!((cc.median_cost_usd.unwrap() - 50.0).abs() < 1e-9);
        assert!((cc.p95_cost_usd.unwrap() - 50.0).abs() < 1e-9);

        let oc15 = &rows[1];
        assert_eq!(oc15.tool, "opencode");
        assert_eq!(oc15.bucket, "1-5");
        assert_eq!(oc15.sessions, 2);
        assert!((oc15.avg_turns - 3.5).abs() < 1e-9);

        let oc615 = &rows[2];
        assert_eq!(oc615.bucket, "6-15");
        assert_eq!(oc615.sessions, 4); // unpriced session still counted
                                       // (6 + 10 + 15 + 7) / 4
        assert!((oc615.avg_turns - 9.5).abs() < 1e-9);
        // Costs [1, 2, 10]: p50 = percentile_f64(sorted, 0.5) -> idx
        // round((3-1)*0.5) = 1 -> 2.0. p95 -> idx round(2*0.95)=2 -> 10.0.
        assert!((oc615.median_cost_usd.unwrap() - 2.0).abs() < 1e-9);
        assert!((oc615.p95_cost_usd.unwrap() - 10.0).abs() < 1e-9);
    }

    #[test]
    fn unpriced_bucket_carries_none_costs() {
        let rows = bucket_stats(&[
            session("a", "opencode", 3, None),
            session("b", "opencode", 4, None),
        ]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].sessions, 2);
        assert!(rows[0].median_cost_usd.is_none());
        assert!(rows[0].p95_cost_usd.is_none());
    }

    #[test]
    fn empty_input_has_no_rows() {
        assert!(bucket_stats(&[]).is_empty());
    }
}
