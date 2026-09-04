//! Session cost commands: top-cost sessions (with outlier flags) and the
//! log-spaced cost distribution over a time window.

use crate::commands::usage::{fetch_pricing, parse_time_range, validate_since};
use crate::error::{Error, Result};
use crate::output::pretty::fit_to_terminal;
use clap::{Args, Subcommand};
use comfy_table::{presets::UTF8_FULL, Table};
use otelite_core::session_cost;
use otelite_storage::StorageBackend;
use std::sync::Arc;

/// Session cost analysis: top-cost sessions and the cost distribution
#[derive(Debug, Subcommand)]
pub enum SessionsCommand {
    /// Top sessions by cost, with the > 3× median anomaly flag
    Costs(CostsArgs),
    /// Log-spaced histogram of per-session costs (ASCII)
    CostHist(CostHistArgs),
    /// Everything observed for one session: spans, logs and metric
    /// aggregates on one timeline (issue #134)
    Context {
        /// Session id (from `otelite sessions costs` or the web Sessions tab)
        session: String,
        /// Window start (ns since epoch; default: no lower bound)
        #[arg(long)]
        start: Option<i64>,
        /// Window end (ns since epoch; default: no upper bound)
        #[arg(long)]
        end: Option<i64>,
        /// Max spans and logs to show (default 500, cap 5000)
        #[arg(long, default_value_t = 500)]
        limit: usize,
    },
}

#[derive(Debug, Args)]
pub struct CostsArgs {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(long, default_value = "24h", value_parser = validate_since)]
    pub since: String,
    /// Number of sessions to show
    #[arg(long, default_value_t = 50)]
    pub top: usize,
}

#[derive(Debug, Args)]
pub struct CostHistArgs {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(long, default_value = "24h", value_parser = validate_since)]
    pub since: String,
    /// Number of log-spaced buckets
    #[arg(long, default_value_t = 20)]
    pub buckets: usize,
}

impl SessionsCommand {
    pub async fn execute(
        &self,
        storage: Arc<dyn StorageBackend>,
        format: crate::config::OutputFormat,
    ) -> Result<()> {
        match self {
            SessionsCommand::Costs(args) => {
                let (start_time, end_time) = parse_time_range(&args.since)?;
                let rows = storage
                    .query_session_costs(Some(start_time), Some(end_time))
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query session costs: {e}")))?;
                let pricing_db = fetch_pricing().await;
                let mut sessions = session_cost::build_session_costs(rows, &pricing_db);
                let median = session_cost::apply_anomaly_flags(&mut sessions).map(|(m, _)| m);
                sessions.truncate(args.top);
                let response = otelite_core::api::SessionCostResponse {
                    sessions,
                    median_cost_usd: median,
                    anomaly_rule: session_cost::ANOMALY_RULE.to_string(),
                    filters_applied: Vec::new(),
                };
                match format {
                    crate::config::OutputFormat::Json
                    | crate::config::OutputFormat::JsonCompact => {
                        print_json(&response, format)?;
                    },
                    crate::config::OutputFormat::Pretty => display_costs(&response),
                }
                Ok(())
            },
            SessionsCommand::CostHist(args) => {
                let (start_time, end_time) = parse_time_range(&args.since)?;
                let rows = storage
                    .query_session_costs(Some(start_time), Some(end_time))
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query session costs: {e}")))?;
                let pricing_db = fetch_pricing().await;
                let sessions = session_cost::build_session_costs(rows, &pricing_db);
                let response = session_cost::build_cost_distribution(&sessions, args.buckets);
                match format {
                    crate::config::OutputFormat::Json
                    | crate::config::OutputFormat::JsonCompact => {
                        print_json(&response, format)?;
                    },
                    crate::config::OutputFormat::Pretty => display_cost_hist(&response),
                }
                Ok(())
            },
            SessionsCommand::Context {
                session,
                start,
                end,
                limit,
            } => {
                let resp = storage
                    .query_session_context(session, *start, *end, *limit as u64)
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to query session context: {e}")))?
                    .ok_or_else(|| {
                        Error::ApiError(format!(
                            "No spans, logs or metrics found for session {session}"
                        ))
                    })?;
                match format {
                    crate::config::OutputFormat::Json
                    | crate::config::OutputFormat::JsonCompact => {
                        print_json(&resp, format)?;
                    },
                    crate::config::OutputFormat::Pretty => display_context(&resp),
                }
                Ok(())
            },
        }
    }
}

fn fmt_ns(ts: i64) -> String {
    chrono::DateTime::from_timestamp_nanos(ts)
        .with_timezone(&chrono::Local)
        .format("%H:%M:%S%.3f")
        .to_string()
}

fn fmt_ns_duration(ns: i64) -> String {
    let ms = ns as f64 / 1_000_000.0;
    if ms < 1.0 {
        format!("{ms:.0}µs")
    } else if ms < 1000.0 {
        format!("{ms:.0}ms")
    } else {
        format!("{:.2}s", ms / 1000.0)
    }
}

fn display_context(resp: &otelite_core::api::SessionContextResponse) {
    let s = &resp.session;
    println!(
        "Session {} — agent: {} — span coverage: {}{}",
        s.id,
        s.agent.as_deref().unwrap_or("unknown"),
        s.span_coverage,
        s.project_id
            .as_ref()
            .map(|p| format!(" — project: {p}"))
            .unwrap_or_default()
    );
    println!(
        "\nSpans ({} shown of {}):",
        resp.spans.len(),
        resp.spans_total
    );
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    fit_to_terminal(&mut table);
    table.set_header(vec!["time", "span", "model", "duration"]);
    for span in &resp.spans {
        table.add_row(vec![
            fmt_ns(span.start_time),
            span.name.clone(),
            span.model.clone().unwrap_or_default(),
            fmt_ns_duration(span.duration_ns),
        ]);
    }
    println!("{table}");
    println!("\nLogs ({} shown of {}):", resp.logs.len(), resp.logs_total);
    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    fit_to_terminal(&mut table);
    table.set_header(vec!["time", "severity", "body"]);
    for log in &resp.logs {
        table.add_row(vec![
            fmt_ns(log.timestamp),
            log.severity.clone().unwrap_or_default(),
            log.body.chars().take(100).collect::<String>(),
        ]);
    }
    println!("{table}");
    if !resp.metrics.is_empty() {
        println!("\nMetrics (aggregated):");
        let mut table = Table::new();
        table.load_preset(UTF8_FULL);
        fit_to_terminal(&mut table);
        table.set_header(vec!["name", "type", "count", "sum", "min", "max"]);
        for m in &resp.metrics {
            table.add_row(vec![
                m.name.clone(),
                match m.metric_type {
                    0 => "gauge".to_string(),
                    1 => "counter".to_string(),
                    _ => "histogram".to_string(),
                },
                m.count.to_string(),
                m.sum.map(|v| format!("{v:.2}")).unwrap_or_default(),
                m.min.map(|v| format!("{v:.2}")).unwrap_or_default(),
                m.max.map(|v| format!("{v:.2}")).unwrap_or_default(),
            ]);
        }
        println!("{table}");
    }
    println!("\nTimeline ({} events):", resp.timeline.len());
    for e in &resp.timeline {
        println!(
            "  {}  {:>4}  {}",
            fmt_ns(e.ts),
            e.kind,
            e.label.chars().take(100).collect::<String>()
        );
    }
}

fn print_json<T: serde::Serialize>(value: &T, format: crate::config::OutputFormat) -> Result<()> {
    let json = if matches!(format, crate::config::OutputFormat::JsonCompact) {
        serde_json::to_string(value)
    } else {
        serde_json::to_string_pretty(value)
    }
    .map_err(|e| Error::ApiError(format!("JSON serialization failed: {e}")))?;
    println!("{json}");
    Ok(())
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

fn fmt_duration(secs: Option<f64>) -> String {
    let Some(s) = secs else {
        return "—".to_string();
    };
    if s < 90.0 {
        format!("{:.0}s", s)
    } else if s < 5400.0 {
        format!("{}m{:02.0}s", (s / 60.0) as u64, (s % 60.0) as u64)
    } else {
        format!(
            "{}h{:02.0}m",
            (s / 3600.0) as u64,
            ((s % 3600.0) / 60.0) as u64
        )
    }
}

fn display_costs(response: &otelite_core::api::SessionCostResponse) {
    if response.sessions.is_empty() {
        println!("\nNo costed sessions in the window.\n");
        return;
    }

    println!("\nSessions by cost ({}):\n", response.anomaly_rule);
    let mut table = Table::new();
    table.load_preset(UTF8_FULL).set_header(vec![
        "session", "agent", "cost", "tokens", "duration", "anomaly",
    ]);
    fit_to_terminal(&mut table);
    for s in &response.sessions {
        table.add_row(vec![
            s.session_id.clone(),
            s.agent.clone(),
            fmt_cost(s.cost_usd, s.cost_source.as_deref()),
            fmt_tokens(s.tokens),
            fmt_duration(s.duration_secs),
            if s.anomaly { "!" } else { "" }.to_string(),
        ]);
    }
    println!("{}", table);
    if let Some(median) = response.median_cost_usd {
        println!(
            "\n{} session(s) shown; median cost ${:.4}.",
            response.sessions.len(),
            median
        );
    }
    println!();
}

fn display_cost_hist(response: &otelite_core::api::CostDistributionResponse) {
    if response.buckets.is_empty() {
        println!("\nNo costed sessions in the window.\n");
        return;
    }
    let max_count = response.buckets.iter().map(|b| b.count).max().unwrap_or(0);
    println!("\nSession cost distribution (log-spaced):\n");
    for b in &response.buckets {
        let bar_len = if max_count == 0 {
            0
        } else {
            ((b.count as f64 / max_count as f64) * 40.0).round() as usize
        };
        let bar = "#".repeat(bar_len);
        println!(
            "{} - {:>8}  {:<40} {}",
            fmt_usd(b.min_usd),
            fmt_usd(b.max_usd),
            bar,
            b.count
        );
    }
    println!();
}

fn fmt_usd(v: f64) -> String {
    if v <= 0.0 {
        "$0".to_string()
    } else if v < 0.01 {
        format!("${:.4}", v)
    } else {
        format!("${:.2}", v)
    }
}
