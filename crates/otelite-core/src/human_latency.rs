//! Human response latency (#171). Pure computation: the caller
//! supplies the inter-turn gap observations (already capped); this
//! module computes the per-tool percentiles and the per-hour-of-day
//! buckets.
//!
//! The gap is the wall time between an assistant turn ending and the
//! next turn of the same session and tool starting — how long the
//! human took to read, think, and send the next prompt. Small gaps
//! mean the flow state is intact.

use crate::api::{HumanLatencyGapRow, HumanLatencyHourRow, HumanLatencyToolRow};
use crate::distribution::percentile_f64;
use std::collections::BTreeMap;

/// Summarise the inter-turn gaps (#171) into per-tool percentiles
/// (p50/p90/p95, nearest-rank — the codebase convention) and per
/// (UTC hour, tool) p50 rows.
pub fn summarize(
    gaps: &[HumanLatencyGapRow],
) -> (Vec<HumanLatencyToolRow>, Vec<HumanLatencyHourRow>) {
    // (tool) -> gaps ms ; (hour, tool) -> gaps ms
    let mut by_tool: BTreeMap<String, Vec<f64>> = BTreeMap::new();
    let mut by_hour: BTreeMap<(u8, String), Vec<f64>> = BTreeMap::new();
    for g in gaps {
        let hour = ((g.at_ns.div_euclid(1_000_000_000)).rem_euclid(86_400) / 3600) as u8;
        by_tool.entry(g.tool.clone()).or_default().push(g.gap_ms);
        by_hour
            .entry((hour, g.tool.clone()))
            .or_default()
            .push(g.gap_ms);
    }

    let tools = by_tool
        .into_iter()
        .map(|(tool, mut values)| {
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            HumanLatencyToolRow {
                p50_ms: percentile_f64(&values, 0.5),
                p90_ms: percentile_f64(&values, 0.9),
                p95_ms: percentile_f64(&values, 0.95),
                n: values.len() as u64,
                tool,
            }
        })
        .collect();

    let hours = by_hour
        .into_iter()
        .map(|((hour, tool), mut values)| {
            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            HumanLatencyHourRow {
                hour,
                tool,
                p50_ms: percentile_f64(&values, 0.5),
                n: values.len() as u64,
            }
        })
        .collect();

    (tools, hours)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gap(tool: &str, at_ns: i64, gap_ms: f64) -> HumanLatencyGapRow {
        HumanLatencyGapRow {
            tool: tool.to_string(),
            at_ns,
            gap_ms,
        }
    }

    // 2026-01-01 10:00:00 UTC.
    const H10: i64 = 1_767_261_600_000_000_000;

    #[test]
    fn per_tool_percentiles_and_counts() {
        // pi: gaps 100..500 ms (n=5) -> p50 = idx round(4*0.5)=2 -> 300;
        // p90 = idx round(4*0.9)=4 -> 500; p95 = idx round(4*0.95)=4 -> 500.
        let gaps = (1..=5)
            .map(|i| gap("pi", H10, i as f64 * 100.0))
            .collect::<Vec<_>>();
        let (tools, hours) = summarize(&gaps);
        assert_eq!(tools.len(), 1, "{tools:?}");
        assert_eq!(tools[0].tool, "pi");
        assert_eq!(tools[0].n, 5);
        assert!((tools[0].p50_ms - 300.0).abs() < 1e-9, "{tools:?}");
        assert!((tools[0].p90_ms - 500.0).abs() < 1e-9, "{tools:?}");
        assert!((tools[0].p95_ms - 500.0).abs() < 1e-9, "{tools:?}");
        // All gaps at hour 10 UTC.
        assert_eq!(hours.len(), 1, "{hours:?}");
        assert_eq!(hours[0].hour, 10);
        assert_eq!(hours[0].tool, "pi");
        assert!((hours[0].p50_ms - 300.0).abs() < 1e-9, "{hours:?}");
        assert_eq!(hours[0].n, 5);
    }

    #[test]
    fn hours_and_tools_are_grouped_independently() {
        // opencode: one gap at 03:00 UTC, one at 15:00 UTC.
        let gaps = vec![
            gap("opencode", H10 - 7 * 3600 * 1_000_000_000, 1000.0), // 03:00
            gap("opencode", H10 + 5 * 3600 * 1_000_000_000, 3000.0), // 15:00
            gap("pi", H10, 2000.0),                                  // 10:00
        ];
        let (tools, hours) = summarize(&gaps);
        assert_eq!(tools.len(), 2);
        // (tool asc): opencode then pi.
        assert_eq!(tools[0].tool, "opencode");
        assert_eq!(tools[0].n, 2);
        assert_eq!(tools[1].tool, "pi");
        // (hour asc, tool asc): 03/opencode, 10/pi, 15/opencode.
        let keys: Vec<(u8, &str)> = hours.iter().map(|h| (h.hour, h.tool.as_str())).collect();
        assert_eq!(keys, vec![(3, "opencode"), (10, "pi"), (15, "opencode")]);
    }

    #[test]
    fn empty_gaps_yield_no_rows() {
        let (tools, hours) = summarize(&[]);
        assert!(tools.is_empty() && hours.is_empty());
    }
}
