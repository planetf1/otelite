//! Monthly cost projection from a priced per-day cost series (#170).
//!
//! Pure computation: the storage query (`query_cost_series` over the
//! trailing 30 days, daily buckets) and the pricing enrichment happen in
//! the caller; this module turns the priced points into the projection.
//! All calendar math is UTC.

use std::collections::BTreeMap;

use crate::api::{CostProjectionModel, CostProjectionResponse, CostSeriesPoint};
use chrono::{DateTime, Datelike, Utc};

const DAY_NS: i64 = 86_400 * 1_000_000_000;

/// Build the monthly projection from priced cost-series points covering
/// the trailing 30 calendar days (points outside that window are
/// ignored).
///
/// - `avg_daily_7d` / `avg_daily_30d`: total priced spend over the
///   trailing 7 / 30 calendar-day buckets (today included) divided by
///   the window length.
/// - `days_remaining`: full days after today in the current UTC
///   calendar month (0 on the last day).
/// - `projected_month_total`: month-to-date spend + `avg_daily_7d` ×
///   `days_remaining` — where the month lands if the recent rate holds.
/// - `by_model`: the same per model (`avg_daily` = 7-day rate,
///   `projected` = model MTD + rate × days remaining), sorted by
///   projected cost descending; models with no projected spend are
///   omitted.
pub fn compute(points: &[CostSeriesPoint], as_of_ns: i64) -> CostProjectionResponse {
    let as_of = DateTime::<Utc>::from_timestamp_nanos(as_of_ns);
    let date = as_of.date_naive();
    let days_remaining = date.num_days_in_month() as u32 - date.day();

    let today_bucket = as_of_ns.div_euclid(DAY_NS) * DAY_NS;
    let month_start = date
        .with_day(1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|t| t.and_utc().timestamp_nanos_opt())
        .unwrap_or(today_bucket);
    let d7_start = today_bucket - 7 * DAY_NS;
    let d30_start = today_bucket - 30 * DAY_NS;

    let mut total_7d = 0.0_f64;
    let mut total_30d = 0.0_f64;
    let mut total_mtd = 0.0_f64;
    // model -> (7-day spend, month-to-date spend)
    let mut models: BTreeMap<String, (f64, f64)> = BTreeMap::new();

    for p in points {
        let cost = match p.cost {
            Some(c) if c > 0.0 => c,
            _ => continue, // unpriced models carry no cost signal
        };
        let bucket = p.timestamp.div_euclid(DAY_NS) * DAY_NS;
        if bucket > d30_start && bucket <= today_bucket {
            total_30d += cost;
        }
        if bucket > d7_start && bucket <= today_bucket {
            total_7d += cost;
            let key = p.model.clone().unwrap_or_else(|| "(unknown)".to_string());
            let entry = models.entry(key).or_default();
            entry.0 += cost;
        }
        if bucket >= month_start {
            total_mtd += cost;
            let key = p.model.clone().unwrap_or_else(|| "(unknown)".to_string());
            let entry = models.entry(key).or_default();
            entry.1 += cost;
        }
    }

    let avg_daily_7d = total_7d / 7.0;
    let avg_daily_30d = total_30d / 30.0;
    let projected_month_total = total_mtd + avg_daily_7d * days_remaining as f64;

    let mut by_model: Vec<CostProjectionModel> = models
        .into_iter()
        .map(|(model, (spend_7d, mtd))| {
            let avg_daily = spend_7d / 7.0;
            CostProjectionModel {
                model,
                avg_daily,
                projected: mtd + avg_daily * days_remaining as f64,
            }
        })
        .filter(|m| m.projected > 0.0)
        .collect();
    by_model.sort_by(|a, b| {
        b.projected
            .partial_cmp(&a.projected)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.model.cmp(&b.model))
    });

    CostProjectionResponse {
        avg_daily_7d,
        avg_daily_30d,
        days_remaining,
        projected_month_total,
        by_model,
        filters_applied: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A priced point on the UTC day `offset` days before `as_of`'s day
    /// (0 = today), for `model`, at `cost`.
    fn point(as_of_ns: i64, offset: i64, model: &str, cost: f64) -> CostSeriesPoint {
        let today_bucket = as_of_ns.div_euclid(DAY_NS) * DAY_NS;
        CostSeriesPoint {
            timestamp: today_bucket - offset * DAY_NS,
            model: Some(model.to_string()),
            input_tokens: 0,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            requests: 0,
            cost: Some(cost),
            cost_source: Some("test".to_string()),
        }
    }

    fn unpriced(as_of_ns: i64, offset: i64, model: &str) -> CostSeriesPoint {
        CostSeriesPoint {
            timestamp: as_of_ns.div_euclid(DAY_NS) * DAY_NS - offset * DAY_NS,
            model: Some(model.to_string()),
            input_tokens: 100,
            output_tokens: 50,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
            requests: 1,
            cost: None,
            cost_source: None,
        }
    }

    /// 2026-09-14T12:00:00Z — mid-month (September has 30 days, so 16
    /// days remain after today).
    const AS_OF: i64 = 1_789_387_200_000_000_000;

    #[test]
    fn empty_series_projects_zero() {
        let r = compute(&[], AS_OF);
        assert_eq!(r.avg_daily_7d, 0.0);
        assert_eq!(r.avg_daily_30d, 0.0);
        assert_eq!(r.projected_month_total, 0.0);
        assert_eq!(r.days_remaining, 16);
        assert!(r.by_model.is_empty());
    }

    #[test]
    fn mid_month_projection_uses_7d_rate_and_mtd() {
        // September 2026: 14th -> MTD covers days 1-14 (offsets 0..13),
        // 16 days remain.
        // Model A: $1/day every day of the month so far (14 days).
        let mut pts: Vec<CostSeriesPoint> = (0..14)
            .map(|off| point(AS_OF, off, "model-a", 1.0))
            .collect();
        // Model B: $2/day only in the last 7 days (offsets 0..6).
        pts.extend((0..7).map(|off| point(AS_OF, off, "model-b", 2.0)));
        // Unpriced model C: tokens but no cost -> never appears.
        pts.push(unpriced(AS_OF, 0, "model-c"));

        let r = compute(&pts, AS_OF);

        // 7-day totals: A 7.0, B 14.0 -> avg_daily_7d = (7 + 14) / 7 = 3.0
        assert!((r.avg_daily_7d - 3.0).abs() < 1e-9);
        // 30-day totals: A 14.0 (all within 30d), B 14.0 -> 28 / 30
        assert!((r.avg_daily_30d - 28.0 / 30.0).abs() < 1e-9);
        // MTD: A 14.0 + B 14.0 = 28.0; projected = 28 + 3.0 * 16 = 76.0
        assert!((r.projected_month_total - 76.0).abs() < 1e-9);
        assert_eq!(r.days_remaining, 16);

        // By model, projected desc: B (14 + 2*16 = 46) ahead of A
        // (14 + 1*16 = 30).
        assert_eq!(r.by_model.len(), 2);
        assert_eq!(r.by_model[0].model, "model-b");
        assert!((r.by_model[0].avg_daily - 2.0).abs() < 1e-9);
        assert!((r.by_model[0].projected - 46.0).abs() < 1e-9);
        assert_eq!(r.by_model[1].model, "model-a");
        assert!((r.by_model[1].avg_daily - 1.0).abs() < 1e-9);
        assert!((r.by_model[1].projected - 30.0).abs() < 1e-9);
        assert!(
            !r.by_model.iter().any(|m| m.model == "model-c"),
            "unpriced models must not appear"
        );
    }

    #[test]
    fn last_day_of_month_projects_only_mtd() {
        // 2026-09-30T12:00:00Z — the final day: nothing remains. All 14
        // spend days (offsets 0..13 = Sept 17-30) sit inside MTD.
        let last = AS_OF + 16 * DAY_NS;
        let pts: Vec<CostSeriesPoint> = (0..14)
            .map(|off| point(last, off, "model-a", 1.0))
            .collect();
        let r = compute(&pts, last);
        assert_eq!(r.days_remaining, 0);
        assert!(
            (r.projected_month_total - 14.0).abs() < 1e-9,
            "projected = MTD on the last day"
        );
        // 7-day rate still reported from the trailing window.
        assert!((r.avg_daily_7d - 1.0).abs() < 1e-9);
        assert_eq!(r.by_model.len(), 1);
        assert!((r.by_model[0].projected - 14.0).abs() < 1e-9);
    }

    #[test]
    fn points_outside_the_trailing_30_days_are_ignored() {
        // Day 40 back: inside neither the 7-day, 30-day, nor MTD windows.
        let mut pts = vec![point(AS_OF, 40, "old-model", 100.0)];
        // Day 20 back (Aug 25): in the 30-day window, before month start.
        pts.push(point(AS_OF, 20, "mtd-only", 5.0));
        let r = compute(&pts, AS_OF);
        // "old-model": no 7d, no mtd -> omitted entirely.
        assert!(!r.by_model.iter().any(|m| m.model == "old-model"));
        // "mtd-only" (Aug 25): in the 30-day window (avg 30d = 5/30) but
        // before month start (Sept 1) -> no MTD, no 7d -> projected 0 ->
        // omitted from by_model, yet it still feeds avg_daily_30d.
        assert!(!r.by_model.iter().any(|m| m.model == "mtd-only"));
        assert!((r.avg_daily_30d - 5.0 / 30.0).abs() < 1e-9);
        assert_eq!(r.avg_daily_7d, 0.0);
        assert_eq!(r.projected_month_total, 0.0);
    }
}
