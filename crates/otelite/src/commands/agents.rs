//! Agent rollup command: per-harness (opencode/codex/claude) sessions, cost,
//! tokens, tool calls and retries over a time window.

use crate::commands::usage::{fetch_pricing, parse_time_range};
use crate::error::{Error, Result};
use crate::output::pretty::fit_to_terminal;
use clap::Args;
use comfy_table::{presets::UTF8_FULL, Table};
use otelite_core::api::AgentRollupResponse;
use otelite_storage::StorageBackend;
use std::sync::Arc;

/// Show a per-harness rollup: sessions, cost, tokens, tool calls, retries
#[derive(Debug, Args)]
pub struct AgentsCommand {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(long, default_value = "24h", value_parser = crate::commands::usage::validate_since)]
    pub since: String,
}

impl AgentsCommand {
    pub async fn execute(
        &self,
        storage: Arc<dyn StorageBackend>,
        format: crate::config::OutputFormat,
    ) -> Result<()> {
        let (start_time, end_time) = parse_time_range(&self.since)?;

        // Series buckets: one hour (the web endpoint accepts its own).
        let rollups = storage
            .query_agent_rollup(Some(start_time), Some(end_time), 3600)
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query agent rollup: {}", e)))?;

        let pricing_db = fetch_pricing().await;
        let mut agents: Vec<_> = rollups.into_iter().map(|r| r.enrich(&pricing_db)).collect();
        agents.sort_by(|a, b| {
            b.cost_usd
                .unwrap_or(0.0)
                .partial_cmp(&a.cost_usd.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.agent.cmp(&b.agent))
        });
        let response = AgentRollupResponse {
            agents,
            filters_applied: Vec::new(),
        };

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
            OutputFormat::Pretty => display_agents(&response),
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

fn fmt_cost(c: Option<f64>, source: Option<&str>) -> String {
    match c {
        Some(v) if v > 0.0 => match source {
            Some("actual") => format!("${:.2} (actual)", v),
            _ => format!("${:.2}", v),
        },
        Some(_) => "$0.00".to_string(),
        None => "—".to_string(),
    }
}

fn display_agents(response: &AgentRollupResponse) {
    if response.agents.is_empty() {
        println!("\nNo agent activity in the window.\n");
        return;
    }

    println!("\nAgents:\n");
    let mut table = Table::new();
    table.load_preset(UTF8_FULL).set_header(vec![
        "agent",
        "sessions",
        "cost",
        "tokens",
        "tool calls",
        "retries",
    ]);
    fit_to_terminal(&mut table);
    for a in &response.agents {
        table.add_row(vec![
            a.agent.clone(),
            a.sessions.to_string(),
            fmt_cost(a.cost_usd, a.cost_source.as_deref()),
            fmt_tokens(a.tokens.total()),
            fmt_tokens(a.tool_calls),
            a.retries.map(fmt_tokens).unwrap_or_else(|| "—".to_string()),
        ]);
    }
    println!("{}", table);
    println!();
}
