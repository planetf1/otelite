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
use otelite_core::api::{
    GenAiCapabilityResponse, GenAiCorrelationProvenance, GenAiMetricCapability,
};
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
    print!("{}", render_capabilities(response));
}

/// Render the full pretty report. The JSON output stays the canonical
/// evidence; this is a compact projection that keeps the same vocabulary.
fn render_capabilities(response: &GenAiCapabilityResponse) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "\nTelemetry Capabilities");
    let _ = writeln!(
        out,
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
        let _ = writeln!(
            out,
            "\nNo LLM request spans in this range — no capability report."
        );
        return out;
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
        "Correlation",
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
            correlation_cell(&r.correlation),
        ]);
    }
    let _ = writeln!(out, "{table}");
    let _ = writeln!(
        out,
        "\nCells: availability/quality[/derivation] (valid/observed or 0/eligible)."
    );
    let _ = writeln!(
        out,
        "availability: available | sparse | absent — quality: reliable | invalid | degenerate | not_assessed"
    );
    let _ = writeln!(
        out,
        "derivation (shown when not native): correlated | unavailable"
    );
    let _ = writeln!(
        out,
        "Correlation: matched/unmatched/rejected/ambiguous candidates under the group's rule (see JSON for the rule name)."
    );
    if !response.unidentified.is_empty() {
        let _ = writeln!(
            out,
            "\nUnidentified emitters — LLM-ish spans no verified signature matched:"
        );
        for u in &response.unidentified {
            let _ = writeln!(
                out,
                "  {} span(s) need: {}",
                u.span_count,
                u.required_attributes.join(" + ")
            );
        }
        let _ = writeln!(
            out,
            "Attribute names only — no values or identifiers are exposed."
        );
    }
    out
}

/// Compact per-group correlation provenance: candidate counts under the
/// group's rule. Groups without a correlation rule show a dash.
fn correlation_cell(c: &GenAiCorrelationProvenance) -> String {
    if c.rule == "none" {
        return "—".to_string();
    }
    format!(
        "{}/{}/{}/{}",
        c.matched_count, c.unmatched_count, c.rejected_count, c.ambiguous_count
    )
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
            unidentified: vec![],
        };
        let out = render_capabilities(&response);
        assert!(out.contains("No LLM request spans in this range"));
    }

    #[test]
    fn display_reports_identity_and_counts() {
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
            correlation: GenAiCorrelationProvenance {
                rule: "none".into(),
                matched_count: 0,
                unmatched_count: 0,
                rejected_count: 0,
                ambiguous_count: 0,
            },
        };
        let response = GenAiCapabilityResponse {
            reports: vec![report],
            canonical_span_count: 12,
            duplicate_span_count: 2,
            truncated: false,
            filters_applied: vec![],
            unidentified: vec![],
        };
        let out = render_capabilities(&response);
        assert!(out.contains("openai/gpt-4o"));
        assert!(out.contains("12"));
        assert!(out.contains("2 duplicate deliveries collapsed"));
        assert!(out.contains("available/degenerate"));
        assert!(out.contains("absent/not_assessed/unavailable"));
        // No correlation rule: the column is an explicit dash, not a zero.
        assert!(out.contains("—"));
    }

    #[test]
    fn display_renders_correlation_provenance_counts() {
        let report = otelite_core::api::GenAiCapabilityReport {
            provider: None,
            model: Some("m1".into()),
            emitter_fingerprint: "fp-2".into(),
            emitter: "codex".into(),
            adapter_rule: "codex-request-v1".into(),
            request_count: 10,
            input_tokens: cap((10, 5, 5, "sparse", "reliable", "correlated")),
            output_tokens: cap((10, 5, 5, "sparse", "reliable", "correlated")),
            cache_creation_tokens: cap((10, 0, 0, "absent", "not_assessed", "unavailable")),
            cache_read_tokens: cap((10, 0, 0, "absent", "not_assessed", "unavailable")),
            ttft: cap((10, 0, 0, "absent", "not_assessed", "unavailable")),
            correlation: GenAiCorrelationProvenance {
                rule: "codex-one-to-one-v1".into(),
                matched_count: 5,
                unmatched_count: 3,
                rejected_count: 1,
                ambiguous_count: 2,
            },
        };
        let response = GenAiCapabilityResponse {
            reports: vec![report],
            canonical_span_count: 10,
            duplicate_span_count: 0,
            truncated: false,
            filters_applied: vec![],
            unidentified: vec![],
        };
        let out = render_capabilities(&response);
        assert!(out.contains("5/3/1/2"));
        assert!(out.contains("sparse/reliable/correlated"));
        assert!(out.contains("matched/unmatched/rejected/ambiguous"));
    }

    #[test]
    fn display_renders_unidentified_emitter_diagnostics() {
        let report = otelite_core::api::GenAiCapabilityReport {
            provider: Some("openai".into()),
            model: Some("gpt-4o".into()),
            emitter_fingerprint: "fp-3".into(),
            emitter: "generic-otel".into(),
            adapter_rule: "conventions".into(),
            request_count: 2,
            input_tokens: cap((2, 2, 2, "available", "reliable", "native")),
            output_tokens: cap((2, 2, 2, "available", "reliable", "native")),
            cache_creation_tokens: cap((2, 0, 0, "absent", "not_assessed", "unavailable")),
            cache_read_tokens: cap((2, 0, 0, "absent", "not_assessed", "unavailable")),
            ttft: cap((2, 2, 2, "available", "reliable", "native")),
            correlation: GenAiCorrelationProvenance {
                rule: "none".into(),
                matched_count: 0,
                unmatched_count: 0,
                rejected_count: 0,
                ambiguous_count: 0,
            },
        };
        let mut response = GenAiCapabilityResponse {
            reports: vec![report],
            canonical_span_count: 2,
            duplicate_span_count: 0,
            truncated: false,
            filters_applied: vec![],
            unidentified: vec![],
        };
        let plain = render_capabilities(&response);
        assert!(!plain.contains("Unidentified emitters"));

        response.unidentified = vec![otelite_core::api::GenAiUnidentifiedSignature {
            required_attributes: vec![
                "gen_ai.provider.name".to_string(),
                "gen_ai.usage.input_tokens".to_string(),
            ],
            span_count: 4,
        }];
        let out = render_capabilities(&response);
        assert!(out.contains("Unidentified emitters"));
        assert!(out.contains("4 span(s) need"));
        assert!(out.contains("gen_ai.provider.name + gen_ai.usage.input_tokens"));
        assert!(out.contains("Attribute names only"));
    }
}
