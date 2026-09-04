//! Cache economics command: read/write split, hit rate, read:write ratio and
//! estimated savings per model, across opencode, codex and claude_code.

use crate::commands::usage::{fetch_pricing, parse_time_range};
use crate::error::{Error, Result};
use crate::output::pretty::fit_to_terminal;
use clap::Args;
use comfy_table::{presets::UTF8_FULL, Table};
use otelite_core::api::CacheEconomicsResponse;
use otelite_storage::StorageBackend;
use std::sync::Arc;

/// Show cache economics (per-model read/write split, hit rate, savings)
#[derive(Debug, Args)]
pub struct CacheCommand {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(long, default_value = "24h", value_parser = crate::commands::usage::validate_since)]
    pub since: String,

    /// Also show the time-bucketed read/write series
    #[arg(long)]
    pub series: bool,

    /// Bucket size in seconds for the series view (default 3600 = 1 hour)
    #[arg(long, default_value = "3600")]
    pub bucket_secs: u64,
}

impl CacheCommand {
    pub async fn execute(
        &self,
        storage: Arc<dyn StorageBackend>,
        format: crate::config::OutputFormat,
    ) -> Result<()> {
        if self.bucket_secs == 0 {
            return Err(Error::InvalidArgument(
                "bucket_secs must be a positive number of seconds".to_string(),
            ));
        }
        let (start_time, end_time) = parse_time_range(&self.since)?;

        let mut response: CacheEconomicsResponse = storage
            .query_cache_economics(
                Some(start_time),
                Some(end_time),
                self.bucket_secs.saturating_mul(1_000_000_000) as i64,
            )
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query cache economics: {}", e)))?;

        // Enrich per-model estimated savings. savings_known stays false when
        // the cache-read price is unknown (no pricing entry or no cache-read
        // rate) — we never fabricate a rate.
        let pricing_db = fetch_pricing().await;
        for m in &mut response.models {
            let r = pricing_db.compute_cache_savings(Some(&m.model), m.cache_read_tokens, None);
            m.est_savings_usd = r.cost;
            m.savings_known = r.cost.is_some();
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
            OutputFormat::Pretty => {
                display_cache_economics(&response, self.series, self.bucket_secs)
            },
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

fn fmt_ratio(r: Option<f64>) -> String {
    match r {
        Some(v) if v >= 100.0 => format!("{:.0}:1", v),
        Some(v) => format!("{:.1}:1", v),
        None => "—".to_string(),
    }
}

fn fmt_ts(ns: i64) -> String {
    let secs = ns.div_euclid(1_000_000_000);
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|| ns.to_string())
}

fn display_cache_economics(response: &CacheEconomicsResponse, show_series: bool, bucket_secs: u64) {
    let total_read: u64 = response.models.iter().map(|m| m.cache_read_tokens).sum();
    let total_write: u64 = response.models.iter().map(|m| m.cache_write_tokens).sum();
    let total_savings: f64 = response
        .models
        .iter()
        .filter(|m| m.savings_known)
        .map(|m| m.est_savings_usd.unwrap_or(0.0))
        .sum();
    let savings_complete = response.models.iter().all(|m| m.savings_known);

    println!(
        "\nCache economics ({}/{} tokens served from cache)",
        fmt_tokens(total_read),
        fmt_tokens(total_read + total_write)
    );
    println!(
        "estimated savings: {}{}\n",
        fmt_cost(Some(total_savings)),
        if savings_complete {
            ""
        } else {
            " (partial — some models have no known cache-read price)"
        }
    );

    if response.models.is_empty() {
        println!("  No cache activity in the window.\n");
        return;
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL).set_header(vec![
        "model",
        "cache rd",
        "cache wr",
        "read:write",
        "hit rate",
        "est. savings",
    ]);
    fit_to_terminal(&mut table);
    for m in &response.models {
        table.add_row(vec![
            m.model.clone(),
            fmt_tokens(m.cache_read_tokens),
            fmt_tokens(m.cache_write_tokens),
            fmt_ratio(m.read_write_ratio),
            m.hit_rate
                .map(|h| format!("{:.1}%", h * 100.0))
                .unwrap_or_else(|| "—".to_string()),
            fmt_cost(m.est_savings_usd),
        ]);
    }
    println!("{}", table);
    println!();

    if show_series && !response.series.is_empty() {
        println!(
            "Series ({} buckets, {}s each):\n",
            response.series.len(),
            bucket_secs
        );
        let mut st = Table::new();
        st.load_preset(UTF8_FULL)
            .set_header(vec!["bucket", "input", "cache rd", "cache wr", "hit rate"]);
        fit_to_terminal(&mut st);
        for p in &response.series {
            st.add_row(vec![
                fmt_ts(p.timestamp),
                fmt_tokens(p.input),
                fmt_tokens(p.cache_read),
                fmt_tokens(p.cache_write),
                p.hit_rate
                    .map(|h| format!("{:.1}%", h * 100.0))
                    .unwrap_or_else(|| "—".to_string()),
            ]);
        }
        println!("{}", st);
        println!();
    }
}
