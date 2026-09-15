//! Rare-tool session summary (#176). Pure computation: the caller
//! supplies the (session, tool, model) storage rows and the top-level
//! span-name frequencies; this module classifies the tools, folds the
//! rows into sessions, and builds the per-session summary.
//!
//! "Rare" means the tool has no dedicated analytics view (it is
//! outside the main-tool set) AND it has few sessions in the window —
//! pi, deepseek, and any future experimental harness.

use crate::api::{
    RareToolSession, RareToolSessionModel, RareToolSessionSpanName, RareToolSessionStorageRow,
};
use crate::pricing::TokenUsage;
use std::collections::BTreeMap;

/// The "main" tools with dedicated analytics views — their sessions
/// are never rare, whatever the threshold. `com.ibm.bob` is the raw
/// scope label bob currently produces under the daily tool mix
/// convention (no normalising arm yet); the bare `bob` is kept so a
/// future arm stays excluded.
pub const MAIN_TOOLS: [&str; 5] = ["claude_code", "opencode", "codex", "bob", "com.ibm.bob"];

/// Default rarity threshold: a tool with fewer sessions in the window
/// than this is rare.
pub const DEFAULT_RARE_SESSION_THRESHOLD: u64 = 10;

/// Whether the tool label is a main tool (never rare).
pub fn is_main_tool(tool: &str) -> bool {
    MAIN_TOOLS.contains(&tool)
}

/// Fold the storage rows into rare-tool sessions (#176).
///
/// A tool is rare when it is not in [`MAIN_TOOLS`] and has fewer than
/// `threshold` sessions in the window. `price` maps a (bare) model
/// name and its usage to an optional cost; a session's cost is the
/// partial sum of its priced (session, model) groups — `None` when
/// nothing is priced (never a fabricated zero).
pub fn build_sessions(
    rows: &[RareToolSessionStorageRow],
    span_names: &[RareToolSessionSpanName],
    threshold: u64,
    price: impl Fn(&str, TokenUsage) -> Option<f64>,
) -> Vec<RareToolSession> {
    // (tool, session) -> fold
    #[derive(Default)]
    struct Fold {
        first_seen: i64,
        last_seen: i64,
        input: u64,
        output: u64,
        cache_creation: u64,
        cache_read: u64,
        models: BTreeMap<String, TokenUsage>,
    }
    let mut sessions: BTreeMap<(String, String), Fold> = BTreeMap::new();
    for r in rows {
        let f = sessions
            .entry((r.tool.clone(), r.session_id.clone()))
            .or_insert_with(|| Fold {
                first_seen: r.first_seen,
                ..Default::default()
            });
        f.first_seen = f.first_seen.min(r.first_seen);
        f.last_seen = f.last_seen.max(r.last_seen);
        f.input += r.input_tokens;
        f.output += r.output_tokens;
        f.cache_creation += r.cache_creation_tokens;
        f.cache_read += r.cache_read_tokens;
        let m = f.models.entry(r.model.clone()).or_insert(TokenUsage {
            input: 0,
            output: 0,
            cache_creation: 0,
            cache_read: 0,
        });
        m.input += r.input_tokens;
        m.output += r.output_tokens;
        m.cache_creation += r.cache_creation_tokens;
        m.cache_read += r.cache_read_tokens;
    }

    // Sessions per tool -> the rarity decision.
    let mut tool_session_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (tool, _) in sessions.keys() {
        *tool_session_counts.entry(tool.clone()).or_default() += 1;
    }
    let is_rare = |tool: &str| {
        !is_main_tool(tool)
            && (tool_session_counts.get(tool).copied().unwrap_or(0) as u64) < threshold
    };

    // Task hint: the most common top-level span name per (tool,
    // session); ties break by name ascending.
    let mut name_counts: BTreeMap<(String, String), BTreeMap<String, u64>> = BTreeMap::new();
    for s in span_names {
        *name_counts
            .entry((s.tool.clone(), s.session_id.clone()))
            .or_default()
            .entry(s.span_name.clone())
            .or_insert(0) += s.count;
    }

    let mut out: Vec<RareToolSession> = Vec::new();
    for ((tool, session_id), f) in &sessions {
        if !is_rare(tool) {
            continue;
        }
        // The "which model" summary: the model with the most total
        // tokens; ties break by name ascending.
        let model = f
            .models
            .iter()
            .max_by(|a, b| {
                let ta: u64 = a.1.input + a.1.output + a.1.cache_creation + a.1.cache_read;
                let tb: u64 = b.1.input + b.1.output + b.1.cache_creation + b.1.cache_read;
                // Most tokens; ties break by the FIRST name (hence the
                // reversed name comparison inside max_by).
                ta.cmp(&tb).then_with(|| b.0.cmp(a.0))
            })
            .map(|(m, _)| m.clone())
            .unwrap_or_else(|| "(unknown)".to_string());

        // Per-model breakdown, total tokens desc (name asc on ties).
        let mut models: Vec<RareToolSessionModel> = f
            .models
            .iter()
            .map(|(m, u)| RareToolSessionModel {
                model: m.clone(),
                input_tokens: u.input,
                output_tokens: u.output,
                cache_creation_tokens: u.cache_creation,
                cache_read_tokens: u.cache_read,
            })
            .collect();
        models.sort_by(|a, b| {
            let ta =
                a.input_tokens + a.output_tokens + a.cache_creation_tokens + a.cache_read_tokens;
            let tb =
                b.input_tokens + b.output_tokens + b.cache_creation_tokens + b.cache_read_tokens;
            tb.cmp(&ta).then_with(|| a.model.cmp(&b.model))
        });

        let key = (tool.clone(), session_id.clone());
        let top_span_name = name_counts
            .get(&key)
            .and_then(|names| {
                names
                    .iter()
                    .max_by(|a, b| {
                        // Most frequent; ties break by the FIRST name
                        // (reversed name comparison inside max_by).
                        a.1.cmp(b.1).then_with(|| b.0.cmp(a.0))
                    })
                    .map(|(n, _)| n.clone())
            })
            .unwrap_or_default();

        let cost = {
            let mut sum: Option<f64> = None;
            for (m, u) in &f.models {
                if let Some(c) = price(m, *u) {
                    sum = Some(sum.unwrap_or(0.0) + c);
                }
            }
            sum
        };

        let duration_ns = f.last_seen.saturating_sub(f.first_seen);
        let duration_ms = u64::try_from(duration_ns).unwrap_or(u64::MAX) / 1_000_000;
        out.push(RareToolSession {
            tool: tool.clone(),
            session_id: session_id.clone(),
            start_time: f.first_seen,
            duration_ms,
            model,
            input_tokens: f.input,
            output_tokens: f.output,
            cache_creation_tokens: f.cache_creation,
            cache_read_tokens: f.cache_read,
            cost_usd: cost,
            top_span_name,
            models,
        });
    }

    // Newest first; ties break by session id asc.
    out.sort_by(|a, b| {
        b.start_time
            .cmp(&a.start_time)
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Six positional fields: a compact fixture builder (the clippy
    // argument-count lint does not understand test helpers).
    #[allow(clippy::too_many_arguments)]
    fn row(
        tool: &str,
        session: &str,
        model: &str,
        first: i64,
        last: i64,
        tokens: u64,
    ) -> RareToolSessionStorageRow {
        RareToolSessionStorageRow {
            session_id: session.to_string(),
            tool: tool.to_string(),
            model: model.to_string(),
            first_seen: first,
            last_seen: last,
            input_tokens: tokens,
            output_tokens: 0,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        }
    }

    fn name(tool: &str, session: &str, span_name: &str, count: u64) -> RareToolSessionSpanName {
        RareToolSessionSpanName {
            session_id: session.to_string(),
            tool: tool.to_string(),
            span_name: span_name.to_string(),
            count,
        }
    }

    const NONE: fn(&str, TokenUsage) -> Option<f64> = |_, _| None;

    #[test]
    fn main_tools_are_never_rare() {
        let rows: Vec<RareToolSessionStorageRow> = MAIN_TOOLS
            .iter()
            .enumerate()
            .map(|(i, t)| row(t, &format!("s{i}"), "m", 0, 1, 1))
            .collect();
        // Even a threshold that would make everything rare.
        let out = build_sessions(&rows, &[], u64::MAX, NONE);
        assert!(out.is_empty(), "{out:?}");
        assert!(is_main_tool("codex"));
        assert!(is_main_tool("com.ibm.bob"));
        assert!(!is_main_tool("pi"));
        assert!(!is_main_tool("deepseek"));
    }

    #[test]
    fn threshold_excludes_tools_at_and_above_it() {
        // pi: 9 sessions (rare at the default 10); deepseek: 10
        // sessions (not rare).
        let mut rows = Vec::new();
        for i in 0..9 {
            rows.push(row("pi", &format!("pi-{i}"), "m", i, i + 1, 1));
        }
        for i in 0..10 {
            rows.push(row("deepseek", &format!("ds-{i}"), "m", i, i + 1, 1));
        }
        let out = build_sessions(&rows, &[], 10, NONE);
        assert_eq!(out.len(), 9, "{out:?}");
        assert!(out.iter().all(|s| s.tool == "pi"));
        // A custom threshold of 3 keeps only tools with < 3 sessions.
        let out = build_sessions(&rows, &[], 3, NONE);
        assert!(out.is_empty(), "{out:?}");
    }

    #[test]
    fn session_summary_picks_heaviest_model_and_task_hint() {
        let rows = vec![
            // Two models in one session: b has more tokens.
            row("pi", "s1", "a", 0, 1000, 10),
            row("pi", "s1", "b", 0, 1000, 90),
            // A second session where the lighter model wins.
            row("pi", "s2", "a", 2000, 3000, 50),
            row("pi", "s2", "b", 2000, 3000, 20),
        ];
        let names = vec![
            name("pi", "s1", "pi.interaction", 4),
            name("pi", "s1", "pi.subagent", 2),
            // Tie on s2 -> lexicographic.
            name("pi", "s2", "zz.task", 3),
            name("pi", "s2", "aa.task", 3),
        ];
        let out = build_sessions(&rows, &names, 10, NONE);
        assert_eq!(out.len(), 2, "{out:?}");
        let s1 = out.iter().find(|s| s.session_id == "s1").unwrap();
        assert_eq!(s1.model, "b");
        assert_eq!(s1.top_span_name, "pi.interaction");
        assert_eq!(s1.input_tokens, 100);
        // Per-model breakdown: tokens desc.
        assert_eq!(s1.models[0].model, "b");
        assert_eq!(s1.models[1].model, "a");
        let s2 = out.iter().find(|s| s.session_id == "s2").unwrap();
        assert_eq!(s2.model, "a");
        assert_eq!(s2.top_span_name, "aa.task");
        // No top-level spans for a session -> empty hint.
        let no_names = build_sessions(&rows, &[], 10, NONE);
        assert!(no_names.iter().all(|s| s.top_span_name.is_empty()));
    }

    #[test]
    fn duration_and_partial_cost_fold() {
        let rows = vec![
            // 10 s session: 0..10_000_000_000 ns.
            row("pi", "s1", "priced", 0, 10_000_000_000, 1_000_000),
            row("pi", "s1", "unpriced", 0, 10_000_000_000, 1_000_000),
        ];
        let price = |model: &str, usage: TokenUsage| -> Option<f64> {
            (model == "priced").then(|| usage.input as f64 / 1_000_000.0 * 3.0)
        };
        let out = build_sessions(&rows, &[], 10, price);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].duration_ms, 10_000, "{out:?}");
        // Partial sum: only the priced model contributes.
        assert!((out[0].cost_usd.unwrap() - 3.0).abs() < 1e-9, "{out:?}");
        // Nothing priced -> None, not zero.
        let out = build_sessions(&rows, &[], 10, NONE);
        assert_eq!(out[0].cost_usd, None, "{out:?}");
    }

    #[test]
    fn newest_first_ordering() {
        let rows = vec![
            row("pi", "old", "m", 0, 1, 1),
            row("pi", "new", "m", 5_000_000_000, 6_000_000_000, 1),
            row("pi", "mid", "m", 2_000_000_000, 3_000_000_000, 1),
        ];
        let out = build_sessions(&rows, &[], 10, NONE);
        let ids: Vec<&str> = out.iter().map(|s| s.session_id.as_str()).collect();
        assert_eq!(ids, vec!["new", "mid", "old"]);
    }

    #[test]
    fn empty_rows_yield_no_sessions() {
        assert!(build_sessions(&[], &[], 10, NONE).is_empty());
    }
}
