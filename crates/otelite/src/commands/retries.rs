//! `otelite retries` — recent retried LLM calls.
//!
//! Lists LLM requests whose final attempt marker is > 1 (a failed first
//! attempt, e.g. a dropped stream, followed by a successful retry):
//! timestamp, model, attempt, TTFT, stop reason, and short session/trace
//! ids for drill-down.

use crate::commands::usage::parse_time_range;
use crate::config::{Config, OutputFormat};
use crate::error::{Error, Result};
use crate::output::pretty::fit_to_terminal;
use chrono::{DateTime, Local};
use clap::Args;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, Table};
use otelite_client::ApiClient;
use otelite_core::api::RetryIncident;

#[derive(Args, Debug)]
#[command(
    about = "Recent retried LLM calls (failed first attempt, then a retry)",
    after_help = "Examples:\n  otelite retries\n  otelite retries --since 7d\n  otelite retries --model claude-sonnet --limit 50\n  otelite retries --session <session-id>"
)]
pub struct RetriesCommand {
    /// Time window, e.g. 1h, 24h, 7d (default: 24h)
    #[arg(long, default_value = "24h")]
    pub since: String,

    /// Maximum number of incidents to show (default: 20)
    #[arg(long, default_value = "20")]
    pub limit: u64,

    /// Filter by model name
    #[arg(long)]
    pub model: Option<String>,

    /// Filter by session ID
    #[arg(long)]
    pub session: Option<String>,
}

impl RetriesCommand {
    pub async fn execute(self, config: Config) -> Result<()> {
        let client = ApiClient::new(config.endpoint.clone(), config.timeout)
            .map_err(|e| Error::ApiError(format!("Failed to create API client: {}", e)))?;

        let (start_ns, end_ns) = parse_time_range(&self.since)?;

        let mut params: Vec<(&str, String)> = vec![
            ("start_time", start_ns.to_string()),
            ("end_time", end_ns.to_string()),
            ("limit", self.limit.to_string()),
        ];
        if let Some(ref m) = self.model {
            params.push(("model", m.clone()));
        }
        if let Some(ref s) = self.session {
            params.push(("session", s.clone()));
        }

        let rows: Vec<RetryIncident> = client
            .fetch_recent_retries(params)
            .await
            .map_err(|e| Error::ApiError(format!("Failed to fetch retry incidents: {}", e)))?;

        match config.format {
            OutputFormat::Json | OutputFormat::JsonCompact => {
                let json = if matches!(config.format, OutputFormat::JsonCompact) {
                    serde_json::to_string(&rows)
                } else {
                    serde_json::to_string_pretty(&rows)
                };
                println!(
                    "{}",
                    json.map_err(|e| Error::ApiError(format!("JSON error: {}", e)))?
                );
            },
            OutputFormat::Pretty => display_retries_table(&rows, &self.since),
        }

        Ok(())
    }
}

fn display_retries_table(rows: &[RetryIncident], since: &str) {
    if rows.is_empty() {
        println!("Retry incidents (last {}): none", since);
        return;
    }
    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS);
    fit_to_terminal(&mut table);
    table.set_header(vec![
        Cell::new("Time").fg(Color::Cyan),
        Cell::new("Model").fg(Color::Cyan),
        Cell::new("Attempt").fg(Color::Cyan),
        Cell::new("TTFT").fg(Color::Cyan),
        Cell::new("Stop").fg(Color::Cyan),
        Cell::new("Session").fg(Color::Cyan),
        Cell::new("Trace").fg(Color::Cyan),
    ]);

    for r in rows {
        let time_str = DateTime::from_timestamp(r.start_time / 1_000_000_000, 0)
            .map(|dt| {
                dt.with_timezone(&Local)
                    .format("%m-%d %H:%M:%S")
                    .to_string()
            })
            .unwrap_or_else(|| "?".to_string());

        let model = r
            .model
            .as_deref()
            .unwrap_or("?")
            .rsplit('/')
            .next()
            .unwrap_or("?");
        let model = if model.len() > 28 {
            format!("…{}", &model[model.len() - 27..])
        } else {
            model.to_string()
        };

        let ttft = r
            .ttft_ms
            .map(|ms| {
                if ms < 10_000 {
                    format!("{} ms", ms)
                } else {
                    format!("{:.1} s", ms as f64 / 1000.0)
                }
            })
            .unwrap_or_else(|| "—".to_string());

        let stop = r
            .finish_reason
            .as_deref()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "—".to_string());

        let session_short = r
            .session_id
            .as_deref()
            .map(|s| s.chars().take(8).collect::<String>())
            .unwrap_or_else(|| "—".to_string());

        let trace_short = r.trace_id.chars().take(8).collect::<String>();

        table.add_row(vec![
            Cell::new(time_str),
            Cell::new(model),
            Cell::new(format!("{}", r.attempt)).fg(Color::Yellow),
            Cell::new(ttft),
            Cell::new(stop),
            Cell::new(session_short),
            Cell::new(trace_short),
        ]);
    }

    println!("{}", table);
    println!(
        "{} retry incident{} in the last {} — the retry succeeded in every listed case; use the trace id to inspect the failed first attempt",
        rows.len(),
        if rows.len() == 1 { "" } else { "s" },
        since
    );
}
