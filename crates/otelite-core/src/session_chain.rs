//! Session-chain continuity (#165). Pure computation: the caller
//! supplies per-session, start-ordered LLM span rows plus the priced
//! per-session costs; this module splits each session's activity into
//! time bursts (segments) and assembles the chain records.
//!
//! A resumed session keeps its stable session ID across the
//! resumption (e.g. Claude Code's `--continue`), so a *chain* is one
//! session ID and a *segment* is a contiguous run of its spans whose
//! consecutive gaps stay within the window. A gap strictly greater
//! than the window starts a new segment — the work was resumed, not
//! continued.

use std::collections::HashMap;

use crate::api::{SessionChain, SessionChainSegment, SessionChainSpan, SessionChainsResponse};

/// Build session chains from per-span rows (#165).
///
/// `spans` must be ordered by (session_id asc, start_time asc) — the
/// storage query guarantees that. `costs` maps session_id to the
/// priced total (`None` when the session is unpriced). Chains are
/// returned in (total_tokens desc, chain_id asc) order.
pub fn build_chains(
    spans: &[SessionChainSpan],
    costs: &HashMap<String, Option<f64>>,
    window_secs: u64,
) -> SessionChainsResponse {
    let window_ns = i64::try_from(window_secs)
        .map(|s| s * 1_000_000_000)
        .unwrap_or(i64::MAX);

    let mut chains: Vec<SessionChain> = Vec::new();
    for (session, idxs) in session_runs(spans) {
        let mut segments: Vec<SessionChainSegment> = Vec::new();
        let mut seg_idx: usize = 0;
        let mut seg_turns: u64 = 0;
        let mut prev_start: Option<i64> = None;
        let mut total_tokens: u64 = 0;
        for &i in idxs.iter() {
            let s = &spans[i];
            let new_segment = match prev_start {
                None => true,
                Some(p) => s.start_time - p > window_ns,
            };
            if new_segment {
                segments.push(SessionChainSegment {
                    index: 0, // renumbered after the loop
                    first_seen: s.start_time,
                    last_seen: s.start_time,
                    turns: 1,
                });
                seg_idx = segments.len() - 1;
                seg_turns = 1;
            } else {
                seg_turns += 1;
                let seg = &mut segments[seg_idx];
                seg.turns = seg_turns;
                seg.last_seen = s.start_time;
            }
            total_tokens +=
                s.input_tokens + s.output_tokens + s.cache_creation_tokens + s.cache_read_tokens;
            prev_start = Some(s.start_time);
        }
        for (n, seg) in segments.iter_mut().enumerate() {
            seg.index = (n + 1) as u64;
        }
        let first = &spans[idxs[0]];
        let last = &spans[*idxs.last().unwrap()];
        chains.push(SessionChain {
            chain_id: session.to_string(),
            tool: first.tool.clone(),
            segments,
            total_turns: idxs.len() as u64,
            total_tokens,
            total_cost_usd: costs.get(session).copied().flatten(),
            first_seen: first.start_time,
            last_seen: last.start_time,
        });
    }

    chains.sort_by(|a, b| {
        b.total_tokens
            .cmp(&a.total_tokens)
            .then_with(|| a.chain_id.cmp(&b.chain_id))
    });

    SessionChainsResponse {
        chains,
        filters_applied: Vec::new(),
    }
}

/// Group the span indices into per-session runs, preserving input
/// order. The input must already be (session asc, start asc) — this
/// only walks it once, so a mis-ordered input yields mis-grouped runs
/// (the storage query's ORDER BY is the contract).
fn session_runs(spans: &[SessionChainSpan]) -> Vec<(&str, Vec<usize>)> {
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

    const SEC: i64 = 1_000_000_000;

    fn span(session: &str, tool: &str, start_s: i64, tokens: u64) -> SessionChainSpan {
        SessionChainSpan {
            session_id: session.to_string(),
            tool: tool.to_string(),
            model: "test-model".to_string(),
            start_time: start_s * SEC,
            input_tokens: tokens,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        }
    }

    fn cost(map: &[(&str, Option<f64>)]) -> HashMap<String, Option<f64>> {
        map.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    /// The acceptance criterion: two consecutive bursts of the same
    /// session UUID link into one chain with two segments.
    #[test]
    fn two_bursts_of_one_session_form_a_chain() {
        let spans = vec![
            span("s1", "claude_code", 0, 10),
            span("s1", "claude_code", 60, 10),
            span("s1", "claude_code", 120, 10),
            // 3 h gap (> the default 2 h window) — a resumption.
            span("s1", "claude_code", 3 * 3600, 20),
            span("s1", "claude_code", 3 * 3600 + 60, 20),
        ];
        let r = build_chains(&spans, &cost(&[]), 7200);
        assert_eq!(r.chains.len(), 1, "{r:?}");
        let c = &r.chains[0];
        assert_eq!(c.chain_id, "s1");
        assert_eq!(c.segments.len(), 2, "{c:?}");
        assert_eq!(c.segments[0].first_seen, 0);
        assert_eq!(c.segments[0].last_seen, 120 * SEC);
        assert_eq!(c.segments[0].turns, 3);
        assert_eq!(c.segments[1].first_seen, 3 * 3600 * SEC);
        assert_eq!(c.segments[1].turns, 2);
        assert_eq!(c.total_turns, 5);
        assert_eq!(c.total_tokens, 70);
        assert_eq!(c.first_seen, 0);
        assert_eq!(c.last_seen, (3 * 3600 + 60) * SEC);
        assert!(c.total_cost_usd.is_none(), "unpriced -> None");
    }

    #[test]
    fn gaps_within_the_window_stay_one_segment() {
        // 90-minute gaps: below the 2 h window, so one segment.
        let spans = vec![
            span("s1", "codex", 0, 1),
            span("s1", "codex", 90 * 60, 1),
            span("s1", "codex", 180 * 60, 1),
        ];
        let r = build_chains(&spans, &cost(&[]), 7200);
        assert_eq!(r.chains[0].segments.len(), 1, "{r:?}");
        assert_eq!(r.chains[0].segments[0].turns, 3);
    }

    #[test]
    fn a_gap_equal_to_the_window_does_not_split() {
        // Splitting is strictly greater-than: a 2 h gap at a 2 h
        // window is still one segment.
        let spans = vec![span("s1", "pi", 0, 1), span("s1", "pi", 7200, 1)];
        let r = build_chains(&spans, &cost(&[]), 7200);
        assert_eq!(r.chains[0].segments.len(), 1, "{r:?}");
    }

    #[test]
    fn distinct_sessions_never_merge() {
        // Even back-to-back, different session IDs are different
        // chains.
        let spans = vec![
            span("s1", "opencode", 0, 1),
            span("s2", "opencode", 1, 1),
            span("s3", "opencode", 2, 1),
        ];
        let r = build_chains(&spans, &cost(&[]), 7200);
        assert_eq!(r.chains.len(), 3, "{r:?}");
        assert!(r.chains.iter().all(|c| c.segments.len() == 1));
    }

    #[test]
    fn a_tighter_window_splits_more_segments() {
        // 10-minute gaps: one segment at a 2 h window, three at a
        // 5-minute window.
        let spans = vec![
            span("s1", "opencode", 0, 1),
            span("s1", "opencode", 10 * 60, 1),
            span("s1", "opencode", 20 * 60, 1),
        ];
        assert_eq!(
            build_chains(&spans, &cost(&[]), 7200).chains[0]
                .segments
                .len(),
            1
        );
        let tight = build_chains(&spans, &cost(&[]), 300);
        assert_eq!(tight.chains[0].segments.len(), 3, "{tight:?}");
        // Segment indexes are 1-based and in order.
        assert_eq!(
            tight.chains[0]
                .segments
                .iter()
                .map(|s| s.index)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn priced_sessions_carry_their_cost() {
        let spans = vec![
            span("s1", "claude_code", 0, 1),
            span("s2", "opencode", 0, 1),
        ];
        let costs = cost(&[("s1", Some(5.5)), ("s2", None)]);
        let r = build_chains(&spans, &costs, 7200);
        let by_id = |id: &str| r.chains.iter().find(|c| c.chain_id == id).unwrap();
        assert_eq!(by_id("s1").total_cost_usd, Some(5.5));
        assert_eq!(by_id("s2").total_cost_usd, None);
    }

    #[test]
    fn chains_sort_by_total_tokens_desc_then_id() {
        let spans = vec![
            span("big", "opencode", 0, 100),
            span("small", "opencode", 0, 1),
            span("tie-a", "opencode", 0, 10),
            span("tie-b", "opencode", 0, 10),
        ];
        let r = build_chains(&spans, &cost(&[]), 7200);
        let ids: Vec<&str> = r.chains.iter().map(|c| c.chain_id.as_str()).collect();
        assert_eq!(ids, vec!["big", "tie-a", "tie-b", "small"]);
    }

    #[test]
    fn empty_input_has_no_chains() {
        let r = build_chains(&[], &cost(&[]), 7200);
        assert!(r.chains.is_empty());
    }
}
