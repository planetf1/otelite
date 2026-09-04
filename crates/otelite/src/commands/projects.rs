//! Project rollup command: cost, sessions and tokens per project.id,
//! with codex/claude (no project label today) under "unattributed".

use crate::commands::usage::{fetch_pricing, parse_time_range};
use crate::error::{Error, Result};
use crate::output::pretty::fit_to_terminal;
use clap::Args;
use comfy_table::{presets::UTF8_FULL, Table};
use otelite_core::api::ProjectRollupResponse;
use otelite_storage::StorageBackend;
use std::sync::Arc;

/// Show per-project usage: sessions, cost, tokens and top model
#[derive(Debug, Args)]
pub struct ProjectsCommand {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(long, default_value = "24h", value_parser = crate::commands::usage::validate_since)]
    pub since: String,
}

impl ProjectsCommand {
    pub async fn execute(
        &self,
        storage: Arc<dyn StorageBackend>,
        format: crate::config::OutputFormat,
    ) -> Result<()> {
        let (start_time, end_time) = parse_time_range(&self.since)?;

        let rollups = storage
            .query_project_rollup(Some(start_time), Some(end_time))
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query project rollup: {e}")))?;

        let pricing_db = fetch_pricing().await;
        let mut projects: Vec<_> = rollups.into_iter().map(|r| r.enrich(&pricing_db)).collect();
        projects.sort_by(|a, b| {
            b.cost_usd
                .unwrap_or(0.0)
                .partial_cmp(&a.cost_usd.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.project_id.cmp(&b.project_id))
        });
        let response = ProjectRollupResponse {
            projects,
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
                    json.map_err(|e| Error::ApiError(format!("JSON serialization failed: {e}")))?
                );
            },
            OutputFormat::Pretty => display_projects(&response),
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
            Some("mixed") => format!("${:.2} (mixed)", v),
            _ => format!("${:.2}", v),
        },
        Some(_) => "$0.00".to_string(),
        None => "—".to_string(),
    }
}

fn display_projects(response: &ProjectRollupResponse) {
    if response.projects.is_empty() {
        println!("\nNo project activity in the window.\n");
        return;
    }

    println!("\nProjects (codex/claude have no project label → unattributed):\n");
    let mut table = Table::new();
    fit_to_terminal(&mut table);
    table.load_preset(UTF8_FULL).set_header(vec![
        "project",
        "sessions",
        "cost",
        "tokens",
        "top model",
    ]);
    for p in &response.projects {
        let top = p
            .top_models
            .first()
            .map(|m| format!("{} ({})", m.model, fmt_tokens(m.tokens.total())))
            .unwrap_or_else(|| "—".to_string());
        table.add_row(vec![
            p.project_id.clone(),
            p.sessions.to_string(),
            fmt_cost(p.cost_usd, p.cost_source.as_deref()),
            fmt_tokens(p.tokens.total()),
            top,
        ]);
    }
    println!("{table}");
    println!();
}
