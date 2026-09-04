//! `otelite llm` — recent LLM request health view.
//!
//! Shows a table of recent LLM requests: timestamp, model, tokens (in/cached/out),
//! TTFT (derived), status, and trace-id for drill-down. Optionally drills into one
//! trace to show its spans inline.

use crate::commands::usage::parse_time_range;
use crate::config::{Config, OutputFormat};
use crate::error::{Error, Result};
use crate::output::pretty::fit_to_terminal;
use chrono::{DateTime, Local};
use clap::Args;
use comfy_table::{modifiers::UTF8_ROUND_CORNERS, presets::UTF8_FULL, Cell, Color, Table};
use otelite_client::ApiClient;
use otelite_core::api::TopSpan;

#[derive(Args, Debug)]
#[command(
    about = "Recent LLM request health: status, model, tokens, TTFT, trace-id",
    after_help = "Examples:\n  otelite llm\n  otelite llm --since 1h --limit 50\n  otelite llm --model claude-sonnet-4-5 --status error\n  otelite llm --trace <trace-id>"
)]
pub struct LlmCommand {
    /// Time window, e.g. 1h, 24h, 7d (default: 1h)
    #[arg(long, default_value = "1h")]
    pub since: String,

    /// Maximum number of requests to show (default: 30)
    #[arg(long, default_value = "30")]
    pub limit: u64,

    /// Filter by model name (substring match)
    #[arg(long)]
    pub model: Option<String>,

    /// Filter by session ID
    #[arg(long)]
    pub session: Option<String>,

    /// Filter by status: "ok" (finish_reason=end_turn) or "error" (max_tokens/tool_use/other)
    #[arg(long)]
    pub status: Option<String>,

    /// Drill into a specific trace: print all spans for that trace
    #[arg(long)]
    pub trace: Option<String>,
}

impl LlmCommand {
    pub async fn execute(self, config: Config) -> Result<()> {
        let client = ApiClient::new(config.endpoint.clone(), config.timeout)
            .map_err(|e| Error::ApiError(format!("Failed to create API client: {}", e)))?;

        // If --trace is given, show that trace's spans and return
        if let Some(ref trace_id) = self.trace {
            return drill_trace(&client, &config, trace_id).await;
        }

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

        let resp = client
            .fetch_top_spans(params)
            .await
            .map_err(|e| Error::ApiError(format!("Failed to fetch LLM requests: {}", e)))?;

        // Apply client-side status filter (top_spans has no server-side status filter)
        let spans: Vec<&TopSpan> = resp
            .iter()
            .filter(|s| match self.status.as_deref() {
                Some("ok") => s
                    .finish_reason
                    .as_deref()
                    .map(|r| r == "end_turn" || r == "stop")
                    .unwrap_or(false),
                Some("error") => s
                    .finish_reason
                    .as_deref()
                    .map(|r| r != "end_turn" && r != "stop")
                    .unwrap_or(true),
                _ => true,
            })
            .collect();

        match config.format {
            OutputFormat::Json | OutputFormat::JsonCompact => {
                let json = if matches!(config.format, OutputFormat::JsonCompact) {
                    serde_json::to_string(&spans)
                } else {
                    serde_json::to_string_pretty(&spans)
                };
                println!(
                    "{}",
                    json.map_err(|e| Error::ApiError(format!("JSON error: {}", e)))?
                );
            },
            OutputFormat::Pretty => {
                display_llm_table(&spans, &self.since);
            },
        }

        Ok(())
    }
}

fn display_llm_table(spans: &[&TopSpan], since: &str) {
    if spans.is_empty() {
        println!("LLM Requests (last {}): none", since);
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
        Cell::new("In").fg(Color::Cyan),
        Cell::new("Cache-R").fg(Color::Cyan),
        Cell::new("Out").fg(Color::Cyan),
        Cell::new("Dur s").fg(Color::Cyan),
        Cell::new("Cost").fg(Color::Cyan),
        Cell::new("Status").fg(Color::Cyan),
        Cell::new("Trace ID").fg(Color::Cyan),
    ]);

    for s in spans {
        let time_str = DateTime::from_timestamp(s.start_time / 1_000_000_000, 0)
            .map(|dt| dt.with_timezone(&Local).format("%H:%M:%S").to_string())
            .unwrap_or_else(|| "?".to_string());

        let model = s
            .model
            .as_deref()
            .unwrap_or("?")
            .rsplit('/')
            .next()
            .unwrap_or("?");
        // Truncate long model names
        let model = if model.len() > 28 {
            format!("…{}", &model[model.len() - 27..])
        } else {
            model.to_string()
        };

        let dur_s = s.duration as f64 / 1_000_000_000.0;

        let reason = s.finish_reason.as_deref().unwrap_or("");
        let (status_cell, is_ok) = if reason == "end_turn" || reason == "stop" {
            (Cell::new("✓ ok").fg(Color::Green), true)
        } else if reason.is_empty() {
            (Cell::new("?"), true) // unknown, not coloured red
        } else {
            (Cell::new(format!("✗ {}", reason)).fg(Color::Red), false)
        };
        let _ = is_ok;

        let cost_str = s
            .cost
            .map(|c| {
                if c < 0.001 {
                    format!("{:.5}", c)
                } else if c < 0.01 {
                    format!("{:.4}", c)
                } else {
                    format!("${:.3}", c)
                }
            })
            .unwrap_or_else(|| "—".to_string());

        // Abbreviate trace ID to first 12 chars
        let trace_short = if s.trace_id.len() > 12 {
            format!("{}…", &s.trace_id[..12])
        } else {
            s.trace_id.clone()
        };

        table.add_row(vec![
            Cell::new(time_str),
            Cell::new(model),
            Cell::new(format_number(s.input_tokens)),
            Cell::new(format_number(s.cache_read_tokens)),
            Cell::new(format_number(s.output_tokens)),
            Cell::new(format!("{:.1}", dur_s)),
            Cell::new(cost_str),
            status_cell,
            Cell::new(trace_short),
        ]);
    }

    println!("LLM Requests (last {}):", since);
    println!("{}", table);
    println!("Tip: otelite llm --trace <trace-id>  to drill into a request");
}

async fn drill_trace(client: &ApiClient, config: &Config, trace_id: &str) -> Result<()> {
    let detail = client
        .fetch_trace_by_id(trace_id)
        .await
        .map_err(|e| Error::ApiError(format!("Failed to fetch trace {}: {}", trace_id, e)))?;

    match config.format {
        OutputFormat::Json | OutputFormat::JsonCompact => {
            let json = if matches!(config.format, OutputFormat::JsonCompact) {
                serde_json::to_string(&detail)
            } else {
                serde_json::to_string_pretty(&detail)
            };
            println!(
                "{}",
                json.map_err(|e| Error::ApiError(format!("JSON error: {}", e)))?
            );
        },
        OutputFormat::Pretty => {
            println!("Trace: {}", trace_id);
            println!("Spans ({}):", detail.spans.len());
            for span in &detail.spans {
                let dur_ms = span.duration / 1_000_000;
                let indent = if span.parent_span_id.is_some() {
                    "  └─ "
                } else {
                    "   "
                };
                let status = if span.status.code == "Error" {
                    "ERROR"
                } else {
                    "ok"
                };
                println!(
                    "{}{} ({}) {}ms [{}]",
                    indent,
                    span.name,
                    &span.span_id[..8],
                    dur_ms,
                    status
                );
                for (k, v) in &span.attributes {
                    if k.starts_with("gen_ai.") || k.starts_with("llm.") {
                        println!("       {}: {}", k, v);
                    }
                }
            }
        },
    }
    Ok(())
}

fn format_number(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_number_thousands() {
        assert_eq!(format_number(1500), "1.5k");
    }

    #[test]
    fn test_format_number_millions() {
        assert_eq!(format_number(2_500_000), "2.5M");
    }

    #[test]
    fn test_format_number_small() {
        assert_eq!(format_number(42), "42");
    }
}
