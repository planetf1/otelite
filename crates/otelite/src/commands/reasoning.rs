//! Reasoning share command: how much of each model's output was thinking,
//! and what that thinking cost.

use crate::commands::usage::{fetch_pricing, parse_time_range};
use crate::error::{Error, Result};
use crate::output::pretty::fit_to_terminal;
use clap::Args;
use comfy_table::{presets::UTF8_FULL, Table};
use otelite_core::api::ReasoningShareResponse;
use otelite_storage::StorageBackend;
use std::sync::Arc;

/// Show reasoning token share by model and effort
#[derive(Debug, Args)]
pub struct ReasoningCommand {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(long, default_value = "24h", value_parser = crate::commands::usage::validate_since)]
    pub since: String,
}

impl ReasoningCommand {
    pub async fn execute(
        &self,
        storage: Arc<dyn StorageBackend>,
        format: crate::config::OutputFormat,
    ) -> Result<()> {
        let (start_time, end_time) = parse_time_range(&self.since)?;

        let mut response: ReasoningShareResponse = storage
            .query_reasoning_share(Some(start_time), Some(end_time))
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query reasoning share: {}", e)))?;

        // Reasoning tokens are billed at the model's output rate — that is
        // what thinking costs. Unpriced models keep cost None.
        let pricing_db = fetch_pricing().await;
        for m in &mut response.models {
            let usage = otelite_core::pricing::TokenUsage {
                input: 0,
                output: m.reasoning_tokens,
                cache_creation: 0,
                cache_read: 0,
            };
            m.cost_usd = pricing_db
                .compute_cost(Some(m.model.as_str()), usage, None)
                .cost;
        }

        use crate::config::OutputFormat;
        match format {
            OutputFormat::Json | OutputFormat::JsonCompact => {
                let json = if matches!(format, OutputFormat::JsonCompact) {
                    serde_json::to_string(&response)
                } else {
                    serde_json::to_string_pretty(&response)
                };
                println!(
                    "{}",
                    json.map_err(|e| Error::ApiError(format!("JSON serialization failed: {}", e)))?
                );
            },
            OutputFormat::Pretty => display_reasoning_share(&response),
        }
        Ok(())
    }
}

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.2}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn fmt_cost(c: Option<f64>) -> String {
    match c {
        Some(v) if v > 0.0 => format!("${:.2}", v),
        Some(_) => "$0.00".to_string(),
        None => "—".to_string(),
    }
}

fn display_reasoning_share(response: &ReasoningShareResponse) {
    let total_reasoning: u64 = response.models.iter().map(|m| m.reasoning_tokens).sum();
    let total_output: u64 = response.models.iter().map(|m| m.output_tokens).sum();
    let total_cost: f64 = response.models.iter().filter_map(|m| m.cost_usd).sum();

    println!(
        "\nReasoning share ({} thinking / {} output tokens)",
        fmt_tokens(total_reasoning),
        fmt_tokens(total_output)
    );
    println!(
        "estimated thinking cost: {}\n",
        if total_cost > 0.0 {
            format!("${:.2}", total_cost)
        } else {
            "(no priced reasoning tokens)".to_string()
        }
    );

    if response.models.is_empty() {
        println!("  No token activity in the window.\n");
    } else {
        let mut table = Table::new();
        fit_to_terminal(&mut table);
        table.load_preset(UTF8_FULL).set_header(vec![
            "model",
            "reasoning",
            "output",
            "share",
            "thinking cost",
        ]);
        for m in &response.models {
            table.add_row(vec![
                m.model.clone(),
                fmt_tokens(m.reasoning_tokens),
                fmt_tokens(m.output_tokens),
                m.share_pct
                    .map(|s| format!("{:.1}%", s))
                    .unwrap_or_else(|| "—".to_string()),
                fmt_cost(m.cost_usd),
            ]);
        }
        println!("{}", table);
        println!();
    }

    if !response.effort.is_empty() {
        println!("By reasoning effort (codex):\n");
        let mut st = Table::new();
        fit_to_terminal(&mut st);
        st.load_preset(UTF8_FULL)
            .set_header(vec!["effort", "calls", "reasoning tokens"]);
        for e in &response.effort {
            st.add_row(vec![
                e.effort.clone(),
                fmt_tokens(e.calls),
                fmt_tokens(e.reasoning_tokens),
            ]);
        }
        println!("{}", st);
        println!();
    }
}
