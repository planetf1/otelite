//! Tool-switch overhead analysis (#166). Pure computation: the caller
//! supplies per-session, start-ordered LLM span rows (session id, tool,
//! start time, reconciled TTFT); this module detects the tool
//! boundaries and measures their cost.
//!
//! A *switch* is a consecutive span pair within one session whose tools
//! differ. The later span is the *cold* span (first request after the
//! boundary); a span whose predecessor shares its tool is *warm* (the
//! same-tool baseline). The switch gap is the start-time difference
//! between the two spans.

use std::collections::BTreeMap;

use crate::api::{ToolSwitchSpan, ToolSwitchTransition};

/// Per-(tool) warm TTFT baseline, in ms.
type WarmBaseline = BTreeMap<String, Vec<f64>>;

/// Detect tool switches and measure their overhead (#166).
///
/// `spans` must be ordered by (session asc, start_time asc) — the
/// storage query guarantees that. Returns the full stats bundle the
/// API response carries.
pub fn analyze(spans: &[ToolSwitchSpan]) -> crate::api::ToolSwitchOverheadResponse {
    // ── Pass 1: per-tool warm baselines ────────────────────────────────
    let mut warm: WarmBaseline = BTreeMap::new();
    for w in warm_indices(spans).iter() {
        if let Some(ttft) = spans[*w].ttft_ms {
            warm.entry(spans[*w].tool.clone()).or_default().push(ttft);
        }
    }
    let warm_avg: BTreeMap<String, f64> = warm
        .iter()
        .map(|(t, v)| (t.clone(), v.iter().sum::<f64>() / v.len() as f64))
        .collect();
    let warm_all: Vec<f64> = warm.into_values().flatten().collect();

    // ── Pass 2: the switches themselves ────────────────────────────────
    let mut switches: Vec<(String, String, i64, Option<f64>)> = Vec::new();
    // (from, to) -> (count, total gap ms, delta sum, delta count)
    let mut transitions: BTreeMap<(String, String), (u64, f64, f64, u64)> = BTreeMap::new();
    let mut gap_sum = 0.0_f64;
    let mut cold_ttfts: Vec<f64> = Vec::new();

    for (session, idxs) in session_runs(spans) {
        let _ = session; // grouping is implicit in the runs
        for w in 1..idxs.len() {
            let prev = &spans[idxs[w - 1]];
            let cur = &spans[idxs[w]];
            if cur.tool == prev.tool {
                continue;
            }
            let gap_ms = (cur.start_time - prev.start_time) as f64 / 1_000_000.0;
            gap_sum += gap_ms;
            let delta = cur
                .ttft_ms
                .zip(warm_avg.get(&cur.tool).copied())
                .map(|(cold, base)| cold - base);
            if let Some(cold) = cur.ttft_ms {
                cold_ttfts.push(cold);
            }
            let t = transitions
                .entry((prev.tool.clone(), cur.tool.clone()))
                .or_insert((0, 0.0, 0.0, 0));
            t.0 += 1;
            t.1 += gap_ms;
            if let Some(d) = delta {
                t.2 += d;
                t.3 += 1;
            }
            switches.push((
                prev.tool.clone(),
                cur.tool.clone(),
                cur.start_time,
                cur.ttft_ms,
            ));
        }
    }

    let by_transition: Vec<ToolSwitchTransition> = transitions
        .into_iter()
        .map(
            |((from, to), (count, total_gap, delta_sum, delta_count))| ToolSwitchTransition {
                from,
                to,
                count,
                avg_gap_ms: total_gap / count as f64,
                avg_ttft_delta_ms: (delta_count > 0).then(|| delta_sum / delta_count as f64),
            },
        )
        .collect();
    let by_transition = sort_transitions(by_transition);

    let switches_n = switches.len() as u64;
    let avg_ttft_cold_ms = mean(&cold_ttfts);
    let avg_ttft_warm_ms = mean(&warm_all);
    crate::api::ToolSwitchOverheadResponse {
        switches: switches_n,
        avg_gap_ms: if switches_n > 0 {
            gap_sum / switches_n as f64
        } else {
            0.0
        },
        avg_ttft_cold_ms,
        avg_ttft_warm_ms,
        overhead_ratio: avg_ttft_cold_ms
            .zip(avg_ttft_warm_ms)
            .filter(|(_, warm_v)| *warm_v > 0.0)
            .map(|(cold, warm_v)| cold / warm_v),
        by_transition,
        filters_applied: Vec::new(),
    }
}

fn mean(values: &[f64]) -> Option<f64> {
    (!values.is_empty()).then(|| values.iter().sum::<f64>() / values.len() as f64)
}

/// count desc, then (from, to) asc for a deterministic table.
fn sort_transitions(mut v: Vec<ToolSwitchTransition>) -> Vec<ToolSwitchTransition> {
    v.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| (&a.from, &a.to).cmp(&(&b.from, &b.to)))
    });
    v
}

/// Indices of the *warm* spans: spans whose predecessor in the session
/// has the same tool. The first span of a session is never warm.
fn warm_indices(spans: &[ToolSwitchSpan]) -> Vec<usize> {
    let mut out = Vec::new();
    for (session, idxs) in session_runs(spans) {
        let _ = session;
        for w in 1..idxs.len() {
            if spans[idxs[w]].tool == spans[idxs[w - 1]].tool {
                out.push(idxs[w]);
            }
        }
    }
    out
}

/// Group the span indices into per-session runs, preserving input order.
/// The input must already be (session asc, start asc) — this only walks
/// it once, so a mis-ordered input yields mis-grouped runs (the storage
/// query's ORDER BY is the contract).
fn session_runs(spans: &[ToolSwitchSpan]) -> Vec<(&str, Vec<usize>)> {
    let mut runs: Vec<(&str, Vec<usize>)> = Vec::new();
    for (i, s) in spans.iter().enumerate() {
        match runs.last_mut() {
            Some((session, idxs)) if *session == s.session_id => idxs.push(i),
            _ => runs.push((&s.session_id, vec![i])),
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One span; ttft in ms when given.
    fn span(session: &str, tool: &str, start_ms: i64, ttft_ms: Option<f64>) -> ToolSwitchSpan {
        ToolSwitchSpan {
            session_id: session.to_string(),
            tool: tool.to_string(),
            start_time: start_ms * 1_000_000,
            ttft_ms,
        }
    }

    #[test]
    fn no_switches_within_single_tool_sessions() {
        let spans = vec![
            span("s1", "opencode", 0, Some(100.0)),
            span("s1", "opencode", 10_000, Some(120.0)),
            span("s2", "codex", 0, Some(90.0)),
            span("s2", "codex", 5_000, Some(95.0)),
        ];
        let r = analyze(&spans);
        assert_eq!(r.switches, 0);
        assert_eq!(r.avg_gap_ms, 0.0);
        assert!(r.by_transition.is_empty());
        // Both same-tool continuations are warm.
        assert!(r.avg_ttft_warm_ms.is_some());
        assert!(r.avg_ttft_cold_ms.is_none());
        assert!(r.overhead_ratio.is_none());
    }

    #[test]
    fn boundary_detected_between_two_tools() {
        // The acceptance criterion: two different-tool spans in close
        // proximity detect at least one boundary.
        let spans = vec![
            span("s1", "opencode", 0, Some(100.0)),
            span("s1", "opencode", 1_000, Some(110.0)), // warm opencode
            span("s1", "codex", 1_020, Some(500.0)),    // cold codex, gap 20 ms
        ];
        let r = analyze(&spans);
        assert_eq!(r.switches, 1, "{r:?}");
        assert!((r.avg_gap_ms - 20.0).abs() < 1e-9, "{r:?}");
        assert_eq!(r.by_transition.len(), 1);
        let t = &r.by_transition[0];
        assert_eq!((t.from.as_str(), t.to.as_str()), ("opencode", "codex"));
        assert_eq!(t.count, 1);
        assert!((t.avg_gap_ms - 20.0).abs() < 1e-9);
        // Cold = the boundary span's TTFT; warm = the same-tool
        // continuation baseline (codex has no warm spans, so the
        // transition delta is unmeasured).
        assert!((r.avg_ttft_cold_ms.unwrap() - 500.0).abs() < 1e-9, "{r:?}");
        assert!((r.avg_ttft_warm_ms.unwrap() - 110.0).abs() < 1e-9, "{r:?}");
        assert!(
            (r.overhead_ratio.unwrap() - 500.0 / 110.0).abs() < 1e-9,
            "{r:?}"
        );
        assert!(t.avg_ttft_delta_ms.is_none(), "{r:?}");
    }

    #[test]
    fn ttft_delta_uses_the_destination_tools_warm_baseline() {
        // codex warm baseline = 110 ms (the second codex span); the cold
        // codex span after the switch carries 300 ms -> delta 190 ms.
        // The switch into opencode has no opencode warm baseline, so its
        // delta stays unmeasured.
        let spans = vec![
            span("s1", "codex", 0, Some(100.0)),
            span("s1", "codex", 1_000, Some(110.0)),
            span("s1", "opencode", 5_000, Some(200.0)),
            span("s1", "codex", 9_000, Some(300.0)),
        ];
        let r = analyze(&spans);
        assert_eq!(r.switches, 2, "two tool changes: {r:?}");
        let to_codex = r
            .by_transition
            .iter()
            .find(|t| t.to == "codex")
            .expect("opencode->codex transition");
        assert_eq!(to_codex.count, 1);
        assert!(
            (to_codex.avg_ttft_delta_ms.unwrap() - 190.0).abs() < 1e-9,
            "{r:?}"
        );
        let to_opencode = r
            .by_transition
            .iter()
            .find(|t| t.to == "opencode")
            .expect("codex->opencode transition");
        assert!(to_opencode.avg_ttft_delta_ms.is_none(), "{r:?}");
    }

    #[test]
    fn transition_counts_and_sorting() {
        // opencode->codex twice, codex->opencode once: the more frequent
        // transition sorts first.
        let mut spans = vec![
            span("s1", "opencode", 0, None),
            span("s1", "codex", 1_000, None),
            span("s1", "opencode", 2_000, None),
            span("s1", "codex", 3_000, None),
        ];
        // A second session with the same pattern keeps counts honest.
        spans.push(span("s2", "opencode", 0, None));
        spans.push(span("s2", "codex", 1_000, None));
        let r = analyze(&spans);
        assert_eq!(r.switches, 4, "{r:?}");
        assert_eq!(r.by_transition[0].from, "opencode");
        assert_eq!(r.by_transition[0].to, "codex");
        assert_eq!(r.by_transition[0].count, 3);
        assert_eq!(r.by_transition[1].count, 1);
        // Gaps are 1000 ms everywhere.
        assert!((r.avg_gap_ms - 1000.0).abs() < 1e-9, "{r:?}");
        // No TTFT anywhere -> all the ttft fields are None.
        assert!(r.avg_ttft_cold_ms.is_none());
        assert!(r.avg_ttft_warm_ms.is_none());
        assert!(r.overhead_ratio.is_none());
        assert!(r
            .by_transition
            .iter()
            .all(|t| t.avg_ttft_delta_ms.is_none()));
    }

    #[test]
    fn empty_input_is_all_zero() {
        let r = analyze(&[]);
        assert_eq!(r.switches, 0);
        assert_eq!(r.avg_gap_ms, 0.0);
        assert!(r.avg_ttft_cold_ms.is_none());
        assert!(r.avg_ttft_warm_ms.is_none());
        assert!(r.overhead_ratio.is_none());
        assert!(r.by_transition.is_empty());
    }
}
