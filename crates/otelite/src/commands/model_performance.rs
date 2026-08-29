//! Model-performance diagnosis command (#121/#153): the canonical
//! comparison of a selected current interval against its preceding
//! interval and an optional rolling baseline, with the deterministic
//! classification and confidence rendered verbatim.
//!
//! `--format json-compact` output deep-equals the API response
//! (`GET /api/genai/model-performance`) — both are assembled by the same
//! `build_diagnosis` core function. The terminal view shows the exact
//! selected intervals, timezone, and per-identity assessments; it never
//! reclassifies or recomputes.

use crate::commands::usage::{now_ns, parse_cli_time};
use crate::error::{Error, Result};
use clap::Args;
use otelite_core::api::{ModelPerformanceQuery, ModelPerformanceWindow};
use otelite_core::filters::GenAiFilters;
use otelite_core::model_performance::build_diagnosis;
use otelite_storage::StorageBackend;
use std::sync::Arc;

/// Diagnose model performance: current interval vs preceding and rolling
/// baselines, with deterministic classification and confidence
#[derive(Debug, Args)]
pub struct ModelPerformanceCommand {
    /// Exact start of the current interval (RFC 3339, epoch, or date)
    #[arg(long)]
    pub start: String,

    /// Exact end of the current interval (RFC 3339, epoch, or date;
    /// defaults to now)
    #[arg(long)]
    pub end: Option<String>,

    /// Rolling baseline length (e.g. "7d", "24h", "30m"); the baseline sits
    /// immediately before the derived preceding window
    #[arg(long)]
    pub rolling: Option<String>,

    /// Model name filter
    #[arg(long)]
    pub model: Option<String>,

    /// Provider filter
    #[arg(long)]
    pub provider: Option<String>,

    /// IANA timezone echoed for calendar alignment (e.g. Europe/London)
    #[arg(long)]
    pub timezone: Option<String>,

    /// Data directory override (DB at <dir>/otelite.db; default: $OTELITE_DATA_DIR)
    #[arg(long)]
    pub data_dir: Option<std::path::PathBuf>,
}

/// Parse a duration like "7d", "24h", "30m" to nanoseconds.
fn parse_duration_ns(value: &str) -> Result<i64> {
    let bad = || {
        Error::ApiError(format!(
            "Invalid duration '{value}': use a number with an h, d, m, or s suffix (e.g. 7d, 24h, 30m)"
        ))
    };
    if let Some(stripped) = value.strip_suffix('d') {
        return stripped
            .parse::<i64>()
            .ok()
            .filter(|v| *v > 0)
            .map(|v| v * 24 * 3600 * 1_000_000_000)
            .ok_or_else(bad);
    }
    if let Some(stripped) = value.strip_suffix('h') {
        return stripped
            .parse::<i64>()
            .ok()
            .filter(|v| *v > 0)
            .map(|v| v * 3600 * 1_000_000_000)
            .ok_or_else(bad);
    }
    if let Some(stripped) = value.strip_suffix('m') {
        return stripped
            .parse::<i64>()
            .ok()
            .filter(|v| *v > 0)
            .map(|v| v * 60 * 1_000_000_000)
            .ok_or_else(bad);
    }
    if let Some(stripped) = value.strip_suffix('s') {
        return stripped
            .parse::<i64>()
            .ok()
            .filter(|v| *v > 0)
            .map(|v| v * 1_000_000_000)
            .ok_or_else(bad);
    }
    Err(bad())
}

impl ModelPerformanceCommand {
    pub async fn execute(
        &self,
        _storage: Arc<dyn StorageBackend>,
        format: crate::config::OutputFormat,
    ) -> Result<()> {
        let start = parse_cli_time(&self.start)?;
        let end = match self.end.as_deref() {
            Some(end) => parse_cli_time(end)?,
            None => now_ns()?,
        };
        if start >= end {
            return Err(Error::ApiError("--start must be before --end".to_string()));
        }
        let rolling_ns = match self.rolling.as_deref() {
            Some(duration) => {
                let len = parse_duration_ns(duration)?;
                if len <= 0 {
                    return Err(Error::ApiError(format!(
                        "--rolling must be a positive duration (got {duration})"
                    )));
                }
                Some(len)
            },
            None => None,
        };
        if let Some(tz) = self.timezone.as_deref() {
            let tz = tz.trim();
            let _parsed: chrono_tz::Tz = std::str::FromStr::from_str(tz)
                .map_err(|e| Error::ApiError(format!("unknown IANA timezone '{tz}': {e}")))?;
        }

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

        // Same window derivation as the API endpoint: the rolling baseline
        // sits immediately before the derived preceding window.
        let current_len = end - start;
        let preceding_start = start - current_len;
        let query = ModelPerformanceQuery {
            current: ModelPerformanceWindow {
                start_time: start,
                end_time: end,
            },
            rolling: rolling_ns.map(|len| ModelPerformanceWindow {
                start_time: preceding_start - len,
                end_time: preceding_start,
            }),
            model: self.model.clone(),
            provider: self.provider.clone(),
        };

        let response = storage
            .query_model_performance(&query)
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query model performance: {e}")))?;
        let capability = storage
            .query_genai_capabilities(
                Some(start),
                Some(end),
                &GenAiFilters {
                    model: self.model.clone(),
                    provider: self.provider.clone(),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| Error::ApiError(format!("Failed to query capabilities: {e}")))?;

        let diagnosis = build_diagnosis(&response, &capability, self.timezone.clone());

        use crate::config::OutputFormat;
        match format {
            OutputFormat::Json | OutputFormat::JsonCompact => {
                let json = if matches!(format, OutputFormat::JsonCompact) {
                    serde_json::to_string(&diagnosis)
                } else {
                    serde_json::to_string_pretty(&diagnosis)
                };
                println!(
                    "{}",
                    json.map_err(|e| Error::ApiError(format!("JSON serialization failed: {e}")))?
                );
            },
            OutputFormat::Pretty => {
                print!("{}", render_diagnosis(&diagnosis));
            },
        }
        Ok(())
    }
}

fn format_ns(ns: i64) -> String {
    chrono::DateTime::from_timestamp_nanos(ns).to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

fn format_window(w: &ModelPerformanceWindow) -> String {
    format!("[{} → {})", format_ns(w.start_time), format_ns(w.end_time))
}

/// Render the terminal view: exact intervals, timezone, and per-identity
/// assessments. The classification wording comes verbatim from the
/// deterministic assessment layer — nothing here reclassifies.
pub fn render_diagnosis(diagnosis: &otelite_core::api::ModelPerformanceDiagnosis) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "\nModel Performance Diagnosis");
    let _ = writeln!(
        out,
        "Current:   {}",
        format_window(&diagnosis.current_window)
    );
    let _ = writeln!(
        out,
        "Preceding: {}",
        format_window(&diagnosis.preceding_window)
    );
    match &diagnosis.rolling_window {
        Some(w) => {
            let _ = writeln!(out, "Rolling:   {}", format_window(w));
        },
        None => {
            let _ = writeln!(out, "Rolling:   disabled");
        },
    }
    let _ = writeln!(
        out,
        "Timezone:  {}",
        diagnosis
            .timezone
            .clone()
            .unwrap_or_else(|| "(none supplied)".to_string())
    );
    if diagnosis.truncated {
        let _ = writeln!(out, "Sample:    bounded — older spans excluded");
    }

    if diagnosis.assessments.is_empty() {
        let _ = writeln!(
            out,
            "\nNo LLM request spans in the current interval — nothing to assess."
        );
        return out;
    }

    for assessment in &diagnosis.assessments {
        let identity = match (&assessment.provider, &assessment.model) {
            (Some(p), Some(m)) => format!("{p}/{m}"),
            (Some(p), None) => p.clone(),
            (None, Some(m)) => m.clone(),
            (None, None) => "(unknown)".to_string(),
        };
        let _ = writeln!(out, "\n{identity}");
        let _ = writeln!(out, "  fingerprint: {}", assessment.emitter_fingerprint);
        if !assessment.response_models.is_empty() {
            let _ = writeln!(
                out,
                "  response models: {}",
                assessment.response_models.join(", ")
            );
        }
        let counts = &assessment.request_counts;
        let _ = writeln!(
            out,
            "  requests (current/preceding/rolling): {}/{}/{}",
            counts.current, counts.preceding, counts.rolling
        );
        let _ = writeln!(
            out,
            "  overall: {} (confidence: {})",
            assessment.overall_class, assessment.overall_confidence
        );
        for metric in &assessment.metrics {
            let label = match metric.metric {
                otelite_core::model_performance::PerformanceMetricKind::Duration => "duration",
                otelite_core::model_performance::PerformanceMetricKind::Throughput => "throughput",
                otelite_core::model_performance::PerformanceMetricKind::Ttft => "ttft",
                otelite_core::model_performance::PerformanceMetricKind::ErrorRate => "error rate",
            };
            let _ = writeln!(
                out,
                "  {label}: {} (n={})",
                metric.class, metric.eligible_current
            );
            if let (Some(cur), Some(pre)) = (metric.current_median, metric.preceding_median) {
                let delta = metric.median_delta_vs_preceding.as_ref();
                let _ = writeln!(
                    out,
                    "    median {cur} (preceding {pre}, delta {})",
                    format_delta(delta)
                );
            }
            if let Some(tail) = metric.current_tail {
                let _ = writeln!(out, "    tail (p95) {tail}");
            }
            for note in &metric.notes {
                let _ = writeln!(out, "    note: {note}");
            }
        }
        for note in &assessment.notes {
            let _ = writeln!(out, "  note: {note}");
        }
    }
    let _ = writeln!(
        out,
        "\nClasses: typical_regression | tail_regression | workload_shift_correlated | error_associated | no_material_change | insufficient_telemetry | mixed_evidence."
    );
    let _ = writeln!(
        out,
        "Workload and error relationships are correlation, not causation. TTFT conclusions are suppressed unless TTFT is reliable."
    );
    out
}

/// Format a delta: absolute plus the relative state — "pct unavailable"
/// when the percentage is undefined (zero baseline), never a fabricated 0%.
fn format_delta(delta: Option<&otelite_core::api::ModelPerformanceDelta>) -> String {
    match delta {
        None => "n/a".to_string(),
        Some(d) => match d.relative {
            Some(rel) => format!("{:+.4} ({:.2}%)", d.absolute, rel * 100.0),
            None => format!("{:+.4} (pct unavailable)", d.absolute),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_core::api::{
        ModelPerformanceCounts, ModelPerformanceDelta, ModelPerformanceDiagnosis,
        ModelPerformanceErrorRate, ModelPerformanceIdentity, ModelPerformanceMetric,
        ModelPerformancePercentile, ModelPerformanceSample,
    };
    use otelite_core::model_performance::{
        ModelPerformanceAssessment, ModelPerformanceMetricAssessment, PerformanceChangeClass,
        PerformanceConfidence, PerformanceMetricKind, TtftTrust,
    };

    fn p50(value: f64) -> ModelPerformanceSample {
        ModelPerformanceSample {
            eligible_count: 12,
            percentiles: vec![ModelPerformancePercentile {
                percentile: 50,
                value,
                delta_vs_preceding: None,
                delta_vs_rolling: None,
            }],
        }
    }

    fn flat_metric() -> ModelPerformanceMetric {
        ModelPerformanceMetric {
            current: Some(p50(100.0)),
            preceding: Some(p50(100.0)),
            rolling: None,
        }
    }

    fn metric_assessment(
        kind: PerformanceMetricKind,
        class: PerformanceChangeClass,
        current: Option<f64>,
        preceding: Option<f64>,
    ) -> ModelPerformanceMetricAssessment {
        let delta = match (current, preceding) {
            (Some(c), Some(p)) => Some(ModelPerformanceDelta {
                absolute: c - p,
                relative: if p != 0.0 { Some((c - p) / p) } else { None },
            }),
            _ => None,
        };
        ModelPerformanceMetricAssessment {
            metric: kind,
            class,
            confidence: PerformanceConfidence::Low,
            eligible_current: 12,
            eligible_preceding: 12,
            eligible_rolling: None,
            current_median: current,
            current_tail: None,
            preceding_median: preceding,
            preceding_tail: None,
            rolling_median: None,
            rolling_tail: None,
            median_delta_vs_preceding: delta,
            median_delta_vs_rolling: None,
            tail_delta_vs_preceding: None,
            tail_delta_vs_rolling: None,
            workload_shift: None,
            error_association: None,
            notes: vec![],
        }
    }

    fn identity() -> ModelPerformanceIdentity {
        ModelPerformanceIdentity {
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            emitter_fingerprint: "genai-v1-abc123".to_string(),
            response_models: vec!["gpt-4o-2024".to_string()],
            request_counts: ModelPerformanceCounts {
                current: 12,
                preceding: 12,
                rolling: 0,
            },
            duration: flat_metric(),
            throughput: flat_metric(),
            ttft: flat_metric(),
            input_tokens: flat_metric(),
            cache_creation_tokens: flat_metric(),
            cache_read_tokens: flat_metric(),
            output_tokens: flat_metric(),
            error_rate: ModelPerformanceErrorRate {
                current: None,
                preceding: None,
                rolling: None,
            },
        }
    }

    fn diagnosis(
        assessments: Vec<ModelPerformanceAssessment>,
        identities: Vec<ModelPerformanceIdentity>,
    ) -> ModelPerformanceDiagnosis {
        ModelPerformanceDiagnosis {
            current_window: ModelPerformanceWindow {
                start_time: 100_000_000_000,
                end_time: 200_000_000_000,
            },
            preceding_window: ModelPerformanceWindow {
                start_time: 0,
                end_time: 100_000_000_000,
            },
            rolling_window: Some(ModelPerformanceWindow {
                start_time: -100_000_000_000,
                end_time: 0,
            }),
            timezone: Some("Europe/London".to_string()),
            truncated: false,
            identities,
            assessments,
        }
    }

    fn assessment(class: PerformanceChangeClass) -> ModelPerformanceAssessment {
        ModelPerformanceAssessment {
            provider: Some("openai".to_string()),
            model: Some("gpt-4o".to_string()),
            emitter_fingerprint: "genai-v1-abc123".to_string(),
            response_models: vec!["gpt-4o-2024".to_string()],
            request_counts: ModelPerformanceCounts {
                current: 12,
                preceding: 12,
                rolling: 0,
            },
            metrics: vec![
                metric_assessment(
                    PerformanceMetricKind::Duration,
                    class,
                    Some(130.0),
                    Some(100.0),
                ),
                metric_assessment(
                    PerformanceMetricKind::Throughput,
                    PerformanceChangeClass::NoMaterialChange,
                    Some(500.0),
                    Some(500.0),
                ),
                metric_assessment(
                    PerformanceMetricKind::Ttft,
                    PerformanceChangeClass::NoMaterialChange,
                    Some(50.0),
                    Some(50.0),
                ),
                metric_assessment(
                    PerformanceMetricKind::ErrorRate,
                    PerformanceChangeClass::NoMaterialChange,
                    Some(0.0),
                    Some(0.0),
                ),
            ],
            overall_class: class,
            overall_confidence: PerformanceConfidence::Low,
            truncated: false,
            notes: vec![],
        }
    }

    #[test]
    fn render_shows_exact_intervals_timezone_and_identity() {
        let out = render_diagnosis(&diagnosis(
            vec![assessment(PerformanceChangeClass::NoMaterialChange)],
            vec![identity()],
        ));
        assert!(out.contains("Model Performance Diagnosis"));
        assert!(out.contains("Current:   [1970-01-01T00:01:40Z → 1970-01-01T00:03:20Z)"));
        assert!(out.contains("Preceding: [1970-01-01T00:00:00Z → 1970-01-01T00:01:40Z)"));
        assert!(out.contains("Rolling:   [1969-12-31T23:58:20Z → 1970-01-01T00:00:00Z)"));
        assert!(out.contains("Timezone:  Europe/London"));
        assert!(out.contains("openai/gpt-4o"));
        assert!(out.contains("genai-v1-abc123"));
        assert!(out.contains("gpt-4o-2024"));
        assert!(out.contains("requests (current/preceding/rolling): 12/12/0"));
    }

    #[test]
    fn render_keeps_first_class_states_verbatim() {
        let mut a = assessment(PerformanceChangeClass::InsufficientTelemetry);
        a.metrics[0].notes.push(
            "duration: 9 of the 9 eligible current-window requests are below the 10 minimum — no assessment"
                .to_string(),
        );
        let mut b = assessment(PerformanceChangeClass::MixedEvidence);
        b.metrics[0].notes.push(
            "duration: mixed evidence — the change is material against the preceding baseline so but not so against the rolling baseline; both are reported"
                .to_string(),
        );
        let out = render_diagnosis(&diagnosis(vec![a, b], vec![identity(), identity()]));
        assert!(out.contains("insufficient_telemetry"));
        assert!(out.contains("below the 10 minimum"));
        assert!(out.contains("mixed_evidence"));
        assert!(out.contains("mixed evidence"));
        assert!(out.contains("correlation, not causation"));
    }

    #[test]
    fn render_empty_diagnosis_is_diagnosable() {
        let out = render_diagnosis(&diagnosis(Vec::new(), Vec::new()));
        assert!(out.contains("No LLM request spans in the current interval"));
    }

    #[test]
    fn delta_format_keeps_percentage_unavailable_distinct() {
        assert_eq!(
            format_delta(Some(&ModelPerformanceDelta {
                absolute: 30.0,
                relative: Some(0.3)
            })),
            "+30.0000 (30.00%)"
        );
        assert_eq!(
            format_delta(Some(&ModelPerformanceDelta {
                absolute: 30.0,
                relative: None
            })),
            "+30.0000 (pct unavailable)"
        );
        assert_eq!(format_delta(None), "n/a");
    }

    #[test]
    fn parse_duration_ns_accepts_units_and_rejects_garbage() {
        assert_eq!(
            parse_duration_ns("7d").unwrap(),
            7 * 24 * 3600 * 1_000_000_000
        );
        assert_eq!(parse_duration_ns("24h").unwrap(), 24 * 3600 * 1_000_000_000);
        assert_eq!(parse_duration_ns("30m").unwrap(), 30 * 60 * 1_000_000_000);
        assert_eq!(parse_duration_ns("90s").unwrap(), 90 * 1_000_000_000);
        assert!(parse_duration_ns("7w").is_err());
        assert!(parse_duration_ns("d").is_err());
        assert!(parse_duration_ns("-3d").is_err());
    }

    // Keep the TtftTrust import honest in the test surface too.
    #[test]
    fn _trust_variant_roundtrip() {
        assert_ne!(TtftTrust::Reliable, TtftTrust::Unreliable);
    }
}
