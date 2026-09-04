//! Pure helpers for per-session cost analysis: anomaly threshold and
//! log-spaced cost buckets. Used by the `/api/sessions/costs` and
//! `/api/sessions/cost-distribution` handlers and by the `otelite sessions`
//! CLI; the math is exercised here in isolation.

/// Outlier rule for per-session cost: a session is anomalous when
/// `cost > 3 x median` (across all sessions in the window that have a
/// cost). Returns `(median, threshold)`; `None` when fewer than two
/// sessions have a cost — a median over zero or one value produces no
/// meaningful outlier signal, so nothing is flagged.
pub fn cost_median_threshold(costs: &[f64]) -> Option<(f64, f64)> {
    if costs.len() < 2 {
        return None;
    }
    let mut sorted = costs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    Some((median, median * 3.0))
}

/// Log-spaced bucket boundaries over `[0, max_cost]` with `n` buckets:
/// bucket 0 is `[0, b1)` and catches zero-cost sessions, buckets 1..n span
/// equal decades (geometric progression) from `b1` up to `max_cost`, and
/// the last bucket is inclusive of `max_cost`.
///
/// `max_cost <= 0` or `n < 2` yields a single `[0, 0]` bucket.
pub fn log_cost_buckets(max_cost: f64, n: usize) -> Vec<(f64, f64)> {
    if max_cost <= 0.0 || n < 2 {
        return vec![(0.0, 0.0)];
    }
    let step = 1.0 / (n - 1) as f64;
    (0..n)
        .map(|i| {
            let lo = if i == 0 {
                0.0
            } else {
                max_cost * 10f64.powf(-((n - i) as f64) * step)
            };
            let hi = if i + 1 == n {
                max_cost
            } else {
                max_cost * 10f64.powf(-((n - i - 1) as f64) * step)
            };
            (lo, hi)
        })
        .collect()
}

/// Index of the bucket containing `cost`. The last bucket is inclusive of
/// its upper bound so a session at exactly `max_cost` lands in it.
pub fn cost_bucket_index(buckets: &[(f64, f64)], cost: f64) -> usize {
    for (i, (_, hi)) in buckets.iter().enumerate() {
        if i + 1 == buckets.len() || cost < *hi {
            return i;
        }
    }
    buckets.len() - 1
}

/// Outlier formula stated in the API response.
pub const ANOMALY_RULE: &str = "cost_usd > 3 x median_cost_usd";

/// Price storage rows into wire [`SessionCost`]s, sorted by cost
/// descending (uncosted sessions last, ties by session id). opencode's cost
/// is its own spend counter ("actual"); claude is estimated from tokens x
/// pricing (its `cost.usage` counter under-reports on the live data).
/// Reasoning is billed at the output rate, same convention as the
/// agent-rollup endpoint.
pub fn build_session_costs(
    rows: Vec<crate::api::SessionCostStorage>,
    pricing_db: &crate::pricing::PricingDatabase,
) -> Vec<crate::api::SessionCost> {
    build_session_costs_with_quality(rows, pricing_db, &std::collections::HashMap::new())
}

/// Build session cost rows, annotating each with a quality grade from
/// `quality_map` (keyed by `session_id`). Sessions absent from the map
/// default to `SessionQuality::Clean`.
pub fn build_session_costs_with_quality(
    rows: Vec<crate::api::SessionCostStorage>,
    pricing_db: &crate::pricing::PricingDatabase,
    quality_map: &std::collections::HashMap<String, crate::api::SessionQuality>,
) -> Vec<crate::api::SessionCost> {
    use crate::pricing::TokenUsage;

    let pricing_usage = |tokens: &crate::api::AgentTokenUsage| TokenUsage {
        input: tokens.input,
        output: tokens.output + tokens.reasoning,
        cache_creation: tokens.cache_write,
        cache_read: tokens.cache_read,
    };

    let mut out: Vec<crate::api::SessionCost> = rows
        .into_iter()
        .map(|row| {
            let mut estimated: Option<f64> = None;
            for (model, tokens) in &row.models {
                if let Some(cost) = pricing_db
                    .compute_cost(Some(model.as_str()), pricing_usage(tokens), None)
                    .cost
                {
                    estimated = Some(estimated.unwrap_or(0.0) + cost);
                }
            }
            let (cost_usd, cost_source) = match row.counter_cost_usd {
                Some(actual) => (Some(actual), Some("actual".to_string())),
                None => (estimated, Some("estimated".to_string())),
            };
            let quality = quality_map
                .get(&row.session_id)
                .copied()
                .unwrap_or_default();
            crate::api::SessionCost {
                session_id: row.session_id,
                agent: row.agent,
                project_id: row.project_id,
                cost_usd,
                cost_source,
                tokens: row.tokens,
                duration_secs: row.duration_secs,
                anomaly: false,
                quality,
            }
        })
        .collect();
    out.sort_by(|a, b| {
        b.cost_usd
            .unwrap_or(f64::MIN)
            .partial_cmp(&a.cost_usd.unwrap_or(f64::MIN))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    out
}

/// Flag anomalous sessions in place and return `(median, threshold)` —
/// `None` (nothing flagged) when fewer than two sessions have a
/// *positive* cost. The median is taken over positive costs only: most
/// sessions are free, so a median over all costs is 0 and the rule
/// `cost > 3 x 0` would flag every session that spent anything at all.
pub fn apply_anomaly_flags(sessions: &mut [crate::api::SessionCost]) -> Option<(f64, f64)> {
    let costs: Vec<f64> = sessions
        .iter()
        .filter_map(|s| s.cost_usd)
        .filter(|c| *c > 0.0)
        .collect();
    let rule = cost_median_threshold(&costs)?;
    for s in sessions.iter_mut() {
        s.anomaly = s.cost_usd.is_some_and(|c| c > rule.1);
    }
    Some(rule)
}

/// Log-spaced distribution of per-session costs for the wire.
pub fn build_cost_distribution(
    sessions: &[crate::api::SessionCost],
    bucket_count: usize,
) -> crate::api::CostDistributionResponse {
    use crate::api::CostBucket;

    let costs: Vec<f64> = sessions.iter().filter_map(|s| s.cost_usd).collect();
    let max = costs.iter().cloned().fold(0.0_f64, f64::max);
    let bounds = log_cost_buckets(max, bucket_count);
    let mut counts = vec![0u64; bounds.len()];
    for cost in &costs {
        counts[cost_bucket_index(&bounds, *cost)] += 1;
    }
    let buckets = bounds
        .into_iter()
        .zip(counts)
        .map(|((lo, hi), count)| CostBucket {
            min_usd: lo,
            max_usd: hi,
            count,
        })
        .collect();
    crate::api::CostDistributionResponse { buckets }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_threshold_requires_two_sessions() {
        assert_eq!(cost_median_threshold(&[]), None);
        assert_eq!(cost_median_threshold(&[42.0]), None);
    }

    #[test]
    fn median_threshold_odd_and_even() {
        // odd: median is the middle value
        let (median, threshold) = cost_median_threshold(&[1.0, 2.0, 100.0]).unwrap();
        assert_eq!(median, 2.0);
        assert_eq!(threshold, 6.0);
        // even: median is the mean of the two middle values
        let (median, threshold) = cost_median_threshold(&[1.0, 2.0, 3.0, 4.0]).unwrap();
        assert_eq!(median, 2.5);
        assert_eq!(threshold, 7.5);
    }

    #[test]
    fn anomaly_rule_matches_issue_formula() {
        let (median, threshold) = cost_median_threshold(&[1.0, 2.0, 3.0, 100.0]).unwrap();
        assert_eq!(median, 2.5);
        // 100 > 3 x 2.5 → anomalous; 3 is under the threshold → not
        assert!(100.0 > threshold);
        assert!(threshold > 3.0);

        // strict inequality: a cost exactly at the threshold is not
        // anomalous.
        let mut exact = build_session_costs(
            vec![
                row("a", "opencode", Some(1.0)),
                row("b", "opencode", Some(2.0)),
                row("c", "opencode", Some(3.0)),
                row("d", "opencode", Some(7.5)),
            ],
            &PricingDatabase::empty(),
        );
        apply_anomaly_flags(&mut exact);
        assert!(
            !exact.iter().any(|s| s.anomaly),
            "7.5 == 3 x 2.5 is not strictly greater"
        );
    }

    #[test]
    fn zero_max_yields_single_bucket() {
        assert_eq!(log_cost_buckets(0.0, 20), vec![(0.0, 0.0)]);
        assert_eq!(log_cost_buckets(5.0, 1), vec![(0.0, 0.0)]);
    }

    #[test]
    fn log_buckets_geometric_and_cover_range() {
        let buckets = log_cost_buckets(1000.0, 4);
        assert_eq!(buckets.len(), 4);
        // first bucket catches zero costs
        assert_eq!(buckets[0].0, 0.0);
        // boundaries form a geometric progression (equal log10 steps) up
        // to max: [0,100) [100,215.4) [215.4,464.2) [464.2,1000]
        assert!((buckets[0].1 - 100.0).abs() < 1e-9);
        assert!((buckets[1].0 - 100.0).abs() < 1e-9);
        assert!((buckets[1].1 - 1000.0 * 10f64.powf(-2.0 / 3.0)).abs() < 1e-6);
        assert!((buckets[2].0 - 1000.0 * 10f64.powf(-2.0 / 3.0)).abs() < 1e-6);
        assert!((buckets[2].1 - 1000.0 * 10f64.powf(-1.0 / 3.0)).abs() < 1e-6);
        assert_eq!(buckets[3].1, 1000.0);
        // contiguous: each lo is the previous hi
        for w in buckets.windows(2) {
            assert!((w[0].1 - w[1].0).abs() < 1e-9);
        }
    }

    #[test]
    fn bucket_index_assignment() {
        let buckets = log_cost_buckets(1000.0, 4);
        assert_eq!(cost_bucket_index(&buckets, 0.0), 0);
        assert_eq!(cost_bucket_index(&buckets, 99.999), 0);
        assert_eq!(cost_bucket_index(&buckets, 100.0), 1);
        assert_eq!(cost_bucket_index(&buckets, 999.9), 3);
        // exactly max lands in the last (inclusive) bucket
        assert_eq!(cost_bucket_index(&buckets, 1000.0), 3);
        // single bucket catches everything
        let single = log_cost_buckets(0.0, 20);
        assert_eq!(cost_bucket_index(&single, 0.0), 0);
    }

    use crate::api::SessionCostStorage;
    use crate::pricing::PricingDatabase;

    fn row(sid: &str, agent: &str, counter: Option<f64>) -> SessionCostStorage {
        SessionCostStorage {
            agent: agent.to_string(),
            session_id: sid.to_string(),
            project_id: None,
            counter_cost_usd: counter,
            tokens: 1_000,
            models: vec![],
            duration_secs: None,
        }
    }

    #[test]
    fn build_costs_prefers_counter_and_sorts_desc() {
        let db = PricingDatabase::empty();
        let out = build_session_costs(
            vec![
                row("s-codex", "claude", None),
                row("s-cheap", "opencode", Some(1.0)),
                row("s-expensive", "opencode", Some(9.0)),
            ],
            &db,
        );
        let ids: Vec<_> = out.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, vec!["s-expensive", "s-cheap", "s-codex"]);
        assert_eq!(out[0].cost_source.as_deref(), Some("actual"));
        assert_eq!(out[0].cost_usd, Some(9.0));
        // no counter and no priced models → null cost, "estimated" source
        assert_eq!(out[2].cost_usd, None);
        assert_eq!(out[2].cost_source.as_deref(), Some("estimated"));
    }

    #[test]
    fn anomaly_flags_only_outliers() {
        let db = PricingDatabase::empty();
        let mut out = build_session_costs(
            vec![
                row("a", "opencode", Some(1.0)),
                row("b", "opencode", Some(2.0)),
                row("c", "opencode", Some(3.0)),
                row("d", "opencode", Some(100.0)),
                row("e", "opencode", None),
            ],
            &db,
        );
        let (median, threshold) = apply_anomaly_flags(&mut out).unwrap();
        // priced costs [1, 2, 3, 100] → median (2+3)/2
        assert_eq!(median, 2.5);
        assert_eq!(threshold, 7.5);
        let flags: Vec<_> = out
            .iter()
            .map(|s| (s.session_id.as_str(), s.anomaly))
            .collect();
        assert_eq!(
            flags,
            vec![
                ("d", true),
                ("c", false),
                ("b", false),
                ("a", false),
                ("e", false)
            ]
        );
    }

    #[test]
    fn anomaly_median_ignores_free_sessions() {
        // Zero-cost sessions must not dilute the median: with them
        // included the median is 0 and every positive cost would be
        // flagged.
        let mut out = build_session_costs(
            vec![
                row("f1", "opencode", Some(0.0)),
                row("f2", "opencode", Some(0.0)),
                row("f3", "opencode", Some(0.0)),
                row("low", "opencode", Some(1.0)),
                row("mid", "opencode", Some(2.0)),
                row("out", "opencode", Some(100.0)),
            ],
            &PricingDatabase::empty(),
        );
        let (median, threshold) = apply_anomaly_flags(&mut out).unwrap();
        // median of positive costs [1, 2, 100]
        assert_eq!(median, 2.0);
        assert_eq!(threshold, 6.0);
        let flags: Vec<_> = out
            .iter()
            .map(|s| (s.session_id.as_str(), s.anomaly))
            .collect();
        assert_eq!(
            flags,
            vec![
                ("out", true),
                ("mid", false),
                ("low", false),
                ("f1", false),
                ("f2", false),
                ("f3", false)
            ]
        );
    }

    #[test]
    fn anomaly_needs_two_positive_cost_sessions() {
        let mut out = build_session_costs(
            vec![
                row("f", "opencode", Some(0.0)),
                row("solo", "opencode", Some(10.0)),
            ],
            &PricingDatabase::empty(),
        );
        assert_eq!(apply_anomaly_flags(&mut out), None);
        assert!(!out.iter().any(|s| s.anomaly));
    }

    #[test]
    fn anomaly_needs_two_priced_sessions() {
        let db = PricingDatabase::empty();
        let mut out = build_session_costs(vec![row("a", "opencode", Some(5.0))], &db);
        assert_eq!(apply_anomaly_flags(&mut out), None);
        assert!(!out[0].anomaly);
    }

    #[test]
    fn distribution_buckets_priced_sessions_only() {
        let db = PricingDatabase::empty();
        let out = build_session_costs(
            vec![
                row("a", "opencode", Some(0.0)),
                row("b", "opencode", Some(0.05)),
                // 6.0 sits in the second bucket ([5, 10.46)) with max=50,
                // 4 buckets.
                row("c", "opencode", Some(6.0)),
                row("d", "opencode", Some(50.0)),
                row("e", "claude", None),
            ],
            &db,
        );
        let dist = build_cost_distribution(&out, 4);
        assert_eq!(dist.buckets.len(), 4);
        let total: u64 = dist.buckets.iter().map(|b| b.count).sum();
        // the unpriced session (e) is excluded
        assert_eq!(total, 4);
        // zero-cost sessions sit in the first bucket
        assert_eq!(dist.buckets[0].count, 2);
        assert_eq!(dist.buckets[3].count, 1);
        // bucket bounds ascend and cover [0, max]
        assert_eq!(dist.buckets[0].min_usd, 0.0);
        assert_eq!(dist.buckets[3].max_usd, 50.0);
    }
}
