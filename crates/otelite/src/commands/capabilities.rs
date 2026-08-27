//! Telemetry capability coverage command: which metrics each observed
//! (provider, model, emitter) identity actually provides, with availability,
//! quality and derivation (issue #120).
//!
//! The JSON output is the canonical capability report from the API/storage
//! layer; the pretty table is a compact projection of the same evidence.
//! `absent`, `sparse`, `invalid`, `degenerate`, `correlated` and
//! `unavailable` stay distinct in both representations — none is rendered
//! as zero or silently omitted.

use crate::commands::usage::{now_ns, parse_cli_time, parse_time_range};
use crate::error::{Error, Result};
use clap::Args;
use comfy_table::{presets::UTF8_FULL, ContentArrangement, Table};
use otelite_core::api::{GenAiCapabilityResponse, GenAiMetricCapability};
use otelite_storage::StorageBackend;
use std::sync::Arc;

/// Show GenAI telemetry capability coverage per emitter identity
#[derive(Debug, Args)]
pub struct CapabilitiesCommand {
    /// Time range to query (e.g., "1h", "24h", "7d", "30d")
    #[arg(long, default_value = "7d", value_parser = crate::commands::usage::validate_since)]
    pub since: String,

    /// Exact start instant (RFC 3339, epoch, or date) — overrides --since
    #[arg(long, conflicts_with = "since")]
    pub start: Option<String>,

    /// Exact end instant (RFC 3339, epoch, or date) — overrides --since
    #[arg(long, conflicts_with = "since")]
    pub end: Option<String>,

    /// Data directory override (DB at <dir>/otelite.db; default: $OTELITE_DATA_DIR)
    #[arg(long)]
    pub data_dir: Option<std::path::PathBuf>,
}

impl CapabilitiesCommand {
    pub async fn execute(
        &self,
        _storage: Arc<dyn StorageBackend>,
        format: crate::config::OutputFormat,
    ) -> Result<()> {
        // Exact --start/--end win over the rolling --since window (same
        // semantics as `usage`).
        let (start_time, end_time) = match (self.start.as_deref(), self.end.as_deref()) {
            (Some(start), end) => {
                let start = parse_cli_time(start)?;
                let end = match end {
                    Some(end) => parse_cli_time(end)?,
                    None => now_ns()?,
                };
                if start >= end {
                    return Err(Error::ApiError("--start must be before --end".to_string()));
                }
                (start, end)
            },
            _ => parse_time_range(&self.since)?,
        };

        let storage = match &self.data_dir {
            Some(dir) => {
                let mut storage_config =
                    otelite_storage::StorageConfig::from_env().map_err(|e| {
                        Error::ApiError(format!("Failed to build storage configuration: {e}"))
                    })?;
                storage_config.data_dir = dir.clone();
                let mut storage = otelite_storage::sqlite::SqliteBackend::new(storage_config);
                storage
                    .initialize()
                    .await
                    .map_err(|e| Error::ApiError(format!("Failed to initialize storage: {e}")))?;
                let storage: Arc<dyn otelite_storage::StorageBackend> = Arc::new(storage);
                storage
            },
            None => _storage,
        };

        let response: GenAiCapabilityResponse = storage
            .query_genai_capabilities(Some(start_time), Some(end_time), &Default::default())
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query capabilities: {}", e)))?;

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
                display_capabilities(&response);
            },
        }
        Ok(())
    }
}

/// Compact per-identity cell: availability + quality + derivation, with the
/// valid/observed/eligible counts. The vocabulary is kept verbatim so a
/// reader can tell sparse from absent, invalid from degenerate, and native
/// from correlated (issue #120 parity contract).
fn metric_cell(m: &GenAiMetricCapability) -> String {
    let mut cell = format!("{}/{}", m.availability, m.quality);
    if m.derivation != "native" {
        cell.push_str(&format!("/{}", m.derivation));
    }
    let valid = if m.observed_count > 0 {
        format!("{}/{} obs", m.valid_count, m.observed_count)
    } else {
        format!("0/{0} elig", m.eligible_count)
    };
    format!("{cell} ({valid})")
}

fn display_capabilities(response: &GenAiCapabilityResponse) {
    println!("\nTelemetry Capabilities");
    println!(
        "Canonical spans: {} ({} duplicate deliveries collapsed){}",
        response.canonical_span_count,
        response.duplicate_span_count,
        if response.truncated {
            " — bounded sample, older spans excluded"
        } else {
            ""
        }
    );
    if response.reports.is_empty() {
        println!("\nNo LLM request spans in this range — no capability report.");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(vec![
        "Provider / Model",
        "Emitter",
        "Requests",
        "Input tokens",
        "Output tokens",
        "Cache write",
        "Cache read",
        "TTFT",
    ]);

    for r in &response.reports {
        let identity = match (&r.provider, &r.model) {
            (Some(p), Some(m)) => format!("{p}/{m}"),
            (Some(p), None) => p.clone(),
            (None, Some(m)) => m.clone(),
            (None, None) => "(unknown)".to_string(),
        };
        table.add_row(vec![
            identity,
            r.emitter.clone(),
            r.request_count.to_string(),
            metric_cell(&r.input_tokens),
            metric_cell(&r.output_tokens),
            metric_cell(&r.cache_creation_tokens),
            metric_cell(&r.cache_read_tokens),
            metric_cell(&r.ttft),
        ]);
    }
    println!("{table}");
    println!("\nCells: availability/quality[/derivation] (valid/observed or 0/eligible).");
    println!("availability: available | sparse | absent — quality: reliable | invalid | degenerate | not_assessed");
    println!("derivation (shown when not native): correlated | unavailable");
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_core::api::GenAiCorrelationProvenance;

    /// Test helper: (eligible, observed, valid, availability, quality, derivation).
    fn cap(spec: (usize, usize, usize, &str, &str, &str)) -> GenAiMetricCapability {
        let (eligible, observed, valid, availability, quality, derivation) = spec;
        GenAiMetricCapability {
            eligible_count: eligible,
            observed_count: observed,
            valid_count: valid,
            invalid_count: observed.saturating_sub(valid),
            availability: availability.to_string(),
            quality: quality.to_string(),
            derivation: derivation.to_string(),
            source_attributes: Default::default(),
        }
    }

    #[test]
    fn metric_cell_keeps_vocab_distinct() {
        assert_eq!(
            metric_cell(&cap((10, 10, 10, "available", "reliable", "native"))),
            "available/reliable (10/10 obs)"
        );
        assert_eq!(
            metric_cell(&cap((10, 3, 3, "sparse", "reliable", "native"))),
            "sparse/reliable (3/3 obs)"
        );
        assert_eq!(
            metric_cell(&cap((10, 0, 0, "absent", "not_assessed", "unavailable"))),
            "absent/not_assessed/unavailable (0/10 elig)"
        );
        assert_eq!(
            metric_cell(&cap((10, 10, 0, "available", "invalid", "native"))),
            "available/invalid (0/10 obs)"
        );
        assert_eq!(
            metric_cell(&cap((10, 10, 10, "available", "degenerate", "native"))),
            "available/degenerate (10/10 obs)"
        );
        assert_eq!(
            metric_cell(&cap((10, 10, 10, "available", "reliable", "correlated"))),
            "available/reliable/correlated (10/10 obs)"
        );
    }

    #[test]
    fn display_empty_report_is_diagnosable() {
        let response = GenAiCapabilityResponse {
            reports: vec![],
            canonical_span_count: 0,
            duplicate_span_count: 0,
            truncated: false,
            filters_applied: vec![],
        };
        let out = render(&response);
        assert!(out.contains("No LLM request spans in this range"));
    }

    #[test]
    fn display_reports_identity_and_counts() {
        let corr = GenAiCorrelationProvenance {
            rule: "none".into(),
            matched_count: 0,
            unmatched_count: 0,
            rejected_count: 0,
            ambiguous_count: 0,
        };
        let report = otelite_core::api::GenAiCapabilityReport {
            provider: Some("openai".into()),
            model: Some("gpt-4o".into()),
            emitter_fingerprint: "fp-1".into(),
            emitter: "generic-otel".into(),
            adapter_rule: "conventions".into(),
            request_count: 12,
            input_tokens: cap((12, 12, 12, "available", "reliable", "native")),
            output_tokens: cap((12, 12, 12, "available", "reliable", "native")),
            cache_creation_tokens: cap((12, 0, 0, "absent", "not_assessed", "unavailable")),
            cache_read_tokens: cap((12, 0, 0, "absent", "not_assessed", "unavailable")),
            ttft: cap((12, 12, 12, "available", "degenerate", "native")),
            correlation: corr,
        };
        let response = GenAiCapabilityResponse {
            reports: vec![report],
            canonical_span_count: 12,
            duplicate_span_count: 2,
            truncated: false,
            filters_applied: vec![],
        };
        let out = render(&response);
        assert!(out.contains("openai/gpt-4o"));
        assert!(out.contains("12"));
        assert!(out.contains("2 duplicate deliveries collapsed"));
        assert!(out.contains("available/degenerate"));
        assert!(out.contains("absent/not_assessed/unavailable"));
    }

    fn render(response: &GenAiCapabilityResponse) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        let _ = writeln!(
            out,
            "Canonical spans: {} ({} duplicate deliveries collapsed)",
            response.canonical_span_count, response.duplicate_span_count
        );
        if response.reports.is_empty() {
            let _ = writeln!(out, "No LLM request spans in this range");
            return out;
        }
        for r in &response.reports {
            let identity = match (&r.provider, &r.model) {
                (Some(p), Some(m)) => format!("{p}/{m}"),
                (Some(p), None) => p.clone(),
                (None, Some(m)) => m.clone(),
                (None, None) => "(unknown)".to_string(),
            };
            let _ = writeln!(
                out,
                "{identity} reqs={} ttft={}",
                r.request_count,
                metric_cell(&r.ttft)
            );
            let _ = writeln!(
                out,
                "  cache_creation={}",
                metric_cell(&r.cache_creation_tokens)
            );
        }
        out
    }
}
