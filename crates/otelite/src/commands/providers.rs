//! Provider × model mix command: tokens, sessions and estimated cost per
//! provider and model, across opencode, codex and claude_code.

use crate::commands::usage::{fetch_pricing, parse_time_range};
use crate::error::{Error, Result};
use crate::output::pretty::fit_to_terminal;
use clap::Args;
use comfy_table::{presets::UTF8_FULL, Table};
use otelite_core::api::ProviderMixResponse;
use otelite_core::pricing::TokenUsage;
use otelite_storage::StorageBackend;
use std::sync::Arc;

/// Show the provider × model mix (tokens, sessions, estimated cost share)
#[derive(Debug, Args)]
pub struct ProvidersCommand {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(long, default_value = "24h", value_parser = crate::commands::usage::validate_since)]
    pub since: String,
}

impl ProvidersCommand {
    pub async fn execute(
        &self,
        storage: Arc<dyn StorageBackend>,
        format: crate::config::OutputFormat,
    ) -> Result<()> {
        let (start_time, end_time) = parse_time_range(&self.since)?;

        let mut response: ProviderMixResponse = storage
            .query_provider_mix(Some(start_time), Some(end_time))
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query provider mix: {}", e)))?;

        // Enrich per-model cost from the pricing table. opencode's own cost
        // counter is zero-valued in the wire data, so tokens x price is the
        // source; reasoning tokens are not priced. Provider cost covers its
        // priced models; None when none has known pricing.
        let pricing_db = fetch_pricing().await;
        let pricing_source = if pricing_db.is_litellm() {
            "LiteLLM".to_string()
        } else {
            "fallback (hardcoded Claude rates)".to_string()
        };
        for provider in &mut response.providers {
            let mut total: f64 = 0.0;
            let mut any_priced = false;
            for m in &mut provider.models {
                let usage = TokenUsage {
                    input: m.tokens.input,
                    output: m.tokens.output,
                    cache_creation: m.tokens.cache_write,
                    cache_read: m.tokens.cache_read,
                };
                let cr = pricing_db.compute_cost(Some(&m.model), usage, None);
                m.cost_usd = cr.cost;
                m.cost_source = Some(cr.source.as_str().to_string());
                if cr.cost.is_some() {
                    total += cr.cost.unwrap_or(0.0);
                    any_priced = true;
                }
            }
            provider.cost_usd = if any_priced { Some(total) } else { None };
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
                display_provider_mix(&response, &pricing_source);
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

fn display_provider_mix(response: &ProviderMixResponse, pricing_source: &str) {
    println!(
        "\nProvider × model mix ({} tokens, method: {})",
        fmt_tokens(response.total_tokens),
        response.method
    );
    println!("cost source: {}\n", pricing_source);

    if response.providers.is_empty() {
        println!("  No provider × model usage in the window.\n");
        return;
    }

    let mut table = Table::new();
    fit_to_terminal(&mut table);
    table.load_preset(UTF8_FULL).set_header(vec![
        "provider", "model", "tokens", "in", "out", "cache rd", "cache wr", "reason", "sessions",
        "cost", "share",
    ]);

    for provider in &response.providers {
        for (i, m) in provider.models.iter().enumerate() {
            let share = if response.total_tokens > 0 {
                m.tokens.total() as f64 / response.total_tokens as f64 * 100.0
            } else {
                0.0
            };
            table.add_row(vec![
                if i == 0 {
                    provider.provider.clone()
                } else {
                    String::new()
                },
                m.model.clone(),
                fmt_tokens(m.tokens.total()),
                fmt_tokens(m.tokens.input),
                fmt_tokens(m.tokens.output),
                fmt_tokens(m.tokens.cache_read),
                fmt_tokens(m.tokens.cache_write),
                fmt_tokens(m.tokens.reasoning),
                m.sessions.to_string(),
                fmt_cost(m.cost_usd),
                format!("{:.1}%", share),
            ]);
        }
        // provider summary row
        let p_tokens: u64 = provider.models.iter().map(|m| m.tokens.total()).sum();
        table.add_row(vec![
            format!("  {} total", provider.provider),
            String::new(),
            fmt_tokens(p_tokens),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            fmt_cost(provider.cost_usd),
            provider
                .share_pct
                .map(|s| format!("{:.1}%", s))
                .unwrap_or_else(|| "—".to_string()),
        ]);
    }
    println!("{}", table);
    println!();
}
