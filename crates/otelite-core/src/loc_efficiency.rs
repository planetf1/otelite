//! Lines-of-code efficiency (#178). Pure computation: the caller
//! supplies one priced (tool, model) row; this module computes the
//! per-100-lines cost and the response ordering.
//!
//! Cost-per-line is one of the most legible productivity signals the
//! data supports, but it is only meaningful where both sides exist —
//! unpriced models and zero-line windows must read as `None`, never
//! as a fabricated zero.

use crate::api::{LocEfficiencyRow, LocEfficiencyStorageRow};
use crate::pricing::TokenUsage;
use std::cmp::Ordering;

/// `cost / lines * 100` — the cost of producing 100 added lines.
///
/// `None` when the cost is unpriced or no lines were added: a zero
/// here would read as "free", which is a claim the data cannot make.
pub fn cost_per_100_lines(total_lines: u64, total_cost_usd: Option<f64>) -> Option<f64> {
    let cost = total_cost_usd?;
    if total_lines == 0 {
        return None;
    }
    Some(cost / total_lines as f64 * 100.0)
}

/// Sort efficiency rows cheapest-first: `cost_per_100_lines`
/// ascending, unpriced (`None`) rows last, ties broken by (tool,
/// model) ascending for a stable display.
pub fn sort_cheapest_first(rows: &mut [LocEfficiencyRow]) {
    rows.sort_by(|a, b| {
        match (a.cost_per_100_lines, b.cost_per_100_lines) {
            (Some(x), Some(y)) => x.partial_cmp(&y).unwrap_or(Ordering::Equal),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => Ordering::Equal,
        }
        .then_with(|| a.tool.cmp(&b.tool))
        .then_with(|| a.model.cmp(&b.model))
    });
}

/// Price the storage rows into the final efficiency rows (#178).
///
/// Shared by the API handler and the CLI so the two agree. The `price`
/// closure maps a (bare) model name and its token usage to an optional
/// cost — `None` for an unpriced model.
///
/// opencode's LOC row carries no model, so its cost is the sum of the
/// tool's per-model zero-line token rows, each priced with its own
/// model (a mixed-model total cannot be priced as one). Those
/// zero-line rows are consumed, not emitted.
pub fn price_rows(
    price: impl Fn(&str, TokenUsage) -> Option<f64>,
    rows: &[LocEfficiencyStorageRow],
) -> Vec<LocEfficiencyRow> {
    let usage_of = |r: &LocEfficiencyStorageRow| TokenUsage {
        input: r.input_tokens,
        output: r.output_tokens,
        cache_creation: r.cache_creation_tokens,
        cache_read: r.cache_read_tokens,
    };

    let opencode_cost: Option<f64> = {
        let mut sum: Option<f64> = None;
        for t in rows.iter().filter(|t| t.tool == "opencode") {
            if let Some(c) = price(&t.model, usage_of(t)) {
                sum = Some(sum.unwrap_or(0.0) + c);
            }
        }
        sum
    };

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        if r.lines_added == 0 {
            continue; // opencode per-model token row — folded into the tool row
        }
        let cost = if r.tool == "opencode" {
            opencode_cost
        } else {
            price(&r.model, usage_of(r))
        };
        out.push(LocEfficiencyRow {
            tool: r.tool.clone(),
            model: r.model.clone(),
            total_lines: r.lines_added,
            total_cost_usd: cost,
            cost_per_100_lines: cost_per_100_lines(r.lines_added, cost),
        });
    }
    sort_cheapest_first(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(tool: &str, model: &str, lines: u64, cost: Option<f64>) -> LocEfficiencyRow {
        LocEfficiencyRow {
            tool: tool.to_string(),
            model: model.to_string(),
            total_lines: lines,
            total_cost_usd: cost,
            cost_per_100_lines: cost_per_100_lines(lines, cost),
        }
    }

    #[test]
    fn cost_per_100_lines_computes_and_refuses_to_fabricate() {
        // $2.00 for 100 lines -> $2.00 per 100 lines.
        assert_eq!(cost_per_100_lines(100, Some(2.0)), Some(2.0));
        // $0.50 for 200 lines -> $0.25 per 100 lines.
        assert_eq!(cost_per_100_lines(200, Some(0.5)), Some(0.25));
        // Zero lines: even a priced cost is a claim the data cannot make.
        assert_eq!(cost_per_100_lines(0, Some(5.0)), None);
        // Unpriced model: never a fabricated zero.
        assert_eq!(cost_per_100_lines(50, None), None);
    }

    #[test]
    fn sort_cheapest_first_puts_unpriced_last() {
        let mut rows = vec![
            row("opencode", "(unknown)", 100, Some(1.0)), // $1.00/100
            row("claude_code", "model-a", 100, None),     // unpriced
            row("claude_code", "model-b", 100, Some(0.2)), // $0.20/100
            row("pi", "model-c", 50, Some(0.2)),          // $0.40/100
        ];
        sort_cheapest_first(&mut rows);
        let order: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r.tool.as_str(), r.model.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![
                ("claude_code", "model-b"),
                ("pi", "model-c"),
                ("opencode", "(unknown)"),
                ("claude_code", "model-a"),
            ]
        );
    }

    #[test]
    fn sort_ties_break_by_tool_then_model() {
        let mut rows = vec![
            row("opencode", "z", 100, Some(1.0)),
            row("opencode", "a", 100, Some(1.0)),
            row("claude_code", "m", 200, Some(2.0)), // same $1.00/100
        ];
        sort_cheapest_first(&mut rows);
        let order: Vec<(&str, &str)> = rows
            .iter()
            .map(|r| (r.tool.as_str(), r.model.as_str()))
            .collect();
        assert_eq!(
            order,
            vec![("claude_code", "m"), ("opencode", "a"), ("opencode", "z")]
        );
    }

    fn storage_row(
        tool: &str,
        model: &str,
        lines: u64,
        input: u64,
        output: u64,
    ) -> LocEfficiencyStorageRow {
        LocEfficiencyStorageRow {
            tool: tool.to_string(),
            model: model.to_string(),
            lines_added: lines,
            input_tokens: input,
            output_tokens: output,
            cache_creation_tokens: 0,
            cache_read_tokens: 0,
        }
    }

    #[test]
    fn price_rows_prices_per_model_and_folds_opencode() {
        // $0.50 per 1000 input tokens, for the two known models only.
        let price = |model: &str, usage: TokenUsage| -> Option<f64> {
            let known = ["a", "b"].contains(&model);
            known.then(|| (usage.input as f64 + usage.output as f64) / 1000.0 * 0.5)
        };
        let storage = vec![
            // claude rows price against their own model.
            storage_row("claude_code", "a", 100, 1000, 0), // $0.50
            storage_row("claude_code", "unpriced", 50, 1000, 0),
            // opencode's model-less row + its per-model token rows.
            storage_row("opencode", "(unknown)", 100, 0, 0),
            storage_row("opencode", "a", 0, 1000, 0), // $0.50
            storage_row("opencode", "b", 0, 2000, 0), // $1.00
            storage_row("opencode", "unpriced", 0, 999, 0),
        ];
        let rows = price_rows(price, &storage);
        assert_eq!(rows.len(), 3, "{rows:?}");
        // claude "a": $0.50 for 100 lines -> $0.50/100. Cheapest first.
        assert_eq!(rows[0].tool, "claude_code");
        assert_eq!(rows[0].model, "a");
        assert!(
            (rows[0].total_cost_usd.unwrap() - 0.5).abs() < 1e-9,
            "{rows:?}"
        );
        assert!(
            (rows[0].cost_per_100_lines.unwrap() - 0.5).abs() < 1e-9,
            "{rows:?}"
        );
        // opencode: the sum of its priced token rows ($0.50 + $1.00),
        // unpriced model skipped. $1.50 for 100 lines -> $1.50/100.
        let oc = rows.iter().find(|r| r.tool == "opencode").unwrap();
        assert!((oc.total_cost_usd.unwrap() - 1.5).abs() < 1e-9, "{rows:?}");
        assert!(
            (oc.cost_per_100_lines.unwrap() - 1.5).abs() < 1e-9,
            "{rows:?}"
        );
        // claude unpriced: null cost, null per-100, sorted last.
        assert_eq!(rows[2].model, "unpriced");
        assert_eq!(rows[2].total_cost_usd, None);
        assert_eq!(rows[2].cost_per_100_lines, None);
    }

    #[test]
    fn price_rows_empty_and_unpriced_opencode() {
        assert!(price_rows(|_, _| None, &[]).is_empty());
        // opencode with lines but no priced tokens -> unpriced, not zero.
        let storage = vec![storage_row("opencode", "(unknown)", 100, 0, 0)];
        let rows = price_rows(|_, _| None, &storage);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].total_cost_usd, None);
        assert_eq!(rows[0].cost_per_100_lines, None);
    }
}
