//! Shared API response types for Otelite
//!
//! This module defines the canonical API response structures used across
//! otelite-server, otelite-cli, and otelite-tui. All types derive both Serialize
//! and Deserialize to support both server-side serialization and client-side
//! deserialization.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Standard error response for all API endpoints
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorResponse {
    /// Human-readable error message
    pub error: String,
    /// Machine-readable error code
    pub code: String,
    /// Optional additional details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(code: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
            details: None,
        }
    }

    /// Create an error response with details
    pub fn with_details(
        code: impl Into<String>,
        error: impl Into<String>,
        details: impl Into<String>,
    ) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
            details: Some(details.into()),
        }
    }

    /// Create a bad request error
    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new("BAD_REQUEST", message)
    }

    /// Create a not found error
    pub fn not_found(resource: impl Into<String>) -> Self {
        Self::new("NOT_FOUND", format!("{} not found", resource.into()))
    }

    /// Create an internal server error
    pub fn internal_error(message: impl Into<String>) -> Self {
        Self::new("INTERNAL_ERROR", message)
    }

    /// Create a storage error
    pub fn storage_error(operation: impl Into<String>) -> Self {
        Self::with_details(
            "STORAGE_ERROR",
            format!("Storage operation failed: {}", operation.into()),
            "Check storage configuration and disk space",
        )
    }
}

/// Response for log listing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LogsResponse {
    pub logs: Vec<LogEntry>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// Individual log entry for API response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LogEntry {
    pub timestamp: i64,
    pub severity: String,
    pub severity_text: Option<String>,
    pub body: String,
    /// Total byte length of the original body (populated when body may be truncated in list view).
    #[serde(default)]
    pub body_length: usize,
    /// True when the body was truncated to fit list-view limits. Full body available via GET /api/logs/:id.
    #[serde(default)]
    pub body_truncated: bool,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    pub resource: Option<Resource>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

impl LogEntry {
    /// Truncate body to `max_bytes` and set `body_truncated`/`body_length` accordingly.
    pub fn truncate_body(mut self, max_bytes: usize) -> Self {
        let original_len = self.body.len();
        self.body_length = original_len;
        if original_len > max_bytes {
            // Truncate at a char boundary.
            let cut = self
                .body
                .char_indices()
                .map(|(i, _)| i)
                .take_while(|&i| i < max_bytes)
                .last()
                .unwrap_or(0);
            self.body.truncate(cut);
            self.body_truncated = true;
        }
        self
    }
}

/// Resource information
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct Resource {
    pub attributes: HashMap<String, String>,
}

/// Response for trace listing
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TracesResponse {
    pub traces: Vec<TraceEntry>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// Individual trace entry (aggregated from spans)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TraceEntry {
    pub trace_id: String,
    pub root_span_name: String,
    pub start_time: i64,
    pub duration: i64,
    pub span_count: usize,
    pub service_names: Vec<String>,
    pub has_errors: bool,
}

/// Detailed trace with all spans
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TraceDetail {
    pub trace_id: String,
    pub spans: Vec<SpanEntry>,
    pub start_time: i64,
    pub end_time: i64,
    pub duration: i64,
    pub span_count: usize,
    pub service_names: Vec<String>,
}

/// Individual span entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SpanEntry {
    pub span_id: String,
    pub trace_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub kind: String,
    pub start_time: i64,
    pub end_time: i64,
    pub duration: i64,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    pub resource: Option<Resource>,
    pub status: SpanStatus,
    pub events: Vec<SpanEvent>,
}

/// Span status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SpanStatus {
    pub code: String,
    pub message: Option<String>,
}

/// Span event
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: i64,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

/// Metric response
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MetricResponse {
    pub name: String,
    pub description: Option<String>,
    pub unit: Option<String>,
    pub metric_type: String,
    pub value: MetricValue,
    pub timestamp: i64,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    pub resource: Option<Resource>,
}

/// Metric value (can be different types)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetricValue {
    Gauge(f64),
    Counter(i64),
    Histogram(HistogramValue),
    Summary(SummaryValue),
}

/// Histogram value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramValue {
    pub sum: f64,
    pub count: u64,
    pub buckets: Vec<HistogramBucket>,
}

/// Histogram bucket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramBucket {
    pub upper_bound: f64,
    pub count: u64,
}

/// Summary value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SummaryValue {
    pub sum: f64,
    pub count: u64,
    pub quantiles: Vec<Quantile>,
}

/// Quantile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quantile {
    pub quantile: f64,
    pub value: f64,
}

// Conversion implementations from telemetry types

impl From<crate::telemetry::LogRecord> for LogEntry {
    fn from(log: crate::telemetry::LogRecord) -> Self {
        let body_length = log.body.len();
        Self {
            timestamp: log.timestamp,
            severity: log.severity.as_str().to_string(),
            severity_text: log
                .severity_text
                .or_else(|| Some(log.severity.as_str().to_string())),
            body: log.body,
            body_length,
            body_truncated: false,
            attributes: log.attributes,
            resource: log.resource.map(Resource::from),
            trace_id: log.trace_id,
            span_id: log.span_id,
        }
    }
}

impl From<crate::telemetry::Resource> for Resource {
    fn from(resource: crate::telemetry::Resource) -> Self {
        Self {
            attributes: resource.attributes,
        }
    }
}

impl From<crate::telemetry::Span> for SpanEntry {
    fn from(span: crate::telemetry::Span) -> Self {
        use crate::telemetry::trace::{SpanKind, StatusCode};

        let kind_str = match span.kind {
            SpanKind::Internal => "Internal",
            SpanKind::Server => "Server",
            SpanKind::Client => "Client",
            SpanKind::Producer => "Producer",
            SpanKind::Consumer => "Consumer",
        };

        let status_code_str = match span.status.code {
            StatusCode::Unset => "Unset",
            StatusCode::Ok => "Ok",
            StatusCode::Error => "Error",
        };

        Self {
            span_id: span.span_id,
            trace_id: span.trace_id,
            parent_span_id: span.parent_span_id,
            name: span.name,
            kind: kind_str.to_string(),
            start_time: span.start_time,
            end_time: span.end_time,
            duration: span.end_time - span.start_time,
            attributes: span.attributes,
            resource: span.resource.map(Resource::from),
            status: SpanStatus {
                code: status_code_str.to_string(),
                message: span.status.message,
            },
            events: span
                .events
                .into_iter()
                .map(|e| SpanEvent {
                    name: e.name,
                    timestamp: e.timestamp,
                    attributes: e.attributes,
                })
                .collect(),
        }
    }
}

impl From<crate::telemetry::Metric> for MetricResponse {
    fn from(metric: crate::telemetry::Metric) -> Self {
        use crate::telemetry::metric::MetricType;

        let (metric_type_str, value) = match metric.metric_type {
            MetricType::Gauge(v) => ("gauge", MetricValue::Gauge(v)),
            MetricType::Counter(v) => ("counter", MetricValue::Counter(v as i64)),
            MetricType::Histogram {
                count,
                sum,
                buckets,
            } => (
                "histogram",
                MetricValue::Histogram(HistogramValue {
                    sum,
                    count,
                    buckets: buckets
                        .into_iter()
                        .map(|b| HistogramBucket {
                            upper_bound: b.upper_bound,
                            count: b.count,
                        })
                        .collect(),
                }),
            ),
            MetricType::Summary {
                count,
                sum,
                quantiles,
            } => (
                "summary",
                MetricValue::Summary(SummaryValue {
                    sum,
                    count,
                    quantiles: quantiles
                        .into_iter()
                        .map(|q| Quantile {
                            quantile: q.quantile,
                            value: q.value,
                        })
                        .collect(),
                }),
            ),
        };

        Self {
            name: metric.name,
            description: metric.description,
            unit: metric.unit,
            metric_type: metric_type_str.to_string(),
            value,
            timestamp: metric.timestamp,
            attributes: metric.attributes,
            resource: metric.resource.map(Resource::from),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_response_new() {
        let err = ErrorResponse::new("TEST_ERROR", "Test error message");
        assert_eq!(err.code, "TEST_ERROR");
        assert_eq!(err.error, "Test error message");
        assert!(err.details.is_none());
    }

    #[test]
    fn test_error_response_with_details() {
        let err =
            ErrorResponse::with_details("TEST_ERROR", "Test error message", "Additional details");
        assert_eq!(err.code, "TEST_ERROR");
        assert_eq!(err.error, "Test error message");
        assert_eq!(err.details, Some("Additional details".to_string()));
    }

    #[test]
    fn test_error_response_bad_request() {
        let err = ErrorResponse::bad_request("Invalid parameter");
        assert_eq!(err.code, "BAD_REQUEST");
        assert_eq!(err.error, "Invalid parameter");
    }

    #[test]
    fn test_error_response_not_found() {
        let err = ErrorResponse::not_found("Log entry");
        assert_eq!(err.code, "NOT_FOUND");
        assert_eq!(err.error, "Log entry not found");
    }

    #[test]
    fn test_error_response_internal_error() {
        let err = ErrorResponse::internal_error("Database connection failed");
        assert_eq!(err.code, "INTERNAL_ERROR");
        assert_eq!(err.error, "Database connection failed");
    }

    #[test]
    fn test_error_response_storage_error() {
        let err = ErrorResponse::storage_error("write");
        assert_eq!(err.code, "STORAGE_ERROR");
        assert!(err.error.contains("write"));
        assert!(err.details.is_some());
    }

    #[test]
    fn test_error_response_serialization() {
        let err = ErrorResponse::with_details("TEST", "message", "details");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\":\"TEST\""));
        assert!(json.contains("\"error\":\"message\""));
        assert!(json.contains("\"details\":\"details\""));
    }

    #[test]
    fn test_error_response_deserialization() {
        let json = r#"{"error":"test message","code":"TEST_CODE","details":"test details"}"#;
        let err: ErrorResponse = serde_json::from_str(json).unwrap();
        assert_eq!(err.code, "TEST_CODE");
        assert_eq!(err.error, "test message");
        assert_eq!(err.details, Some("test details".to_string()));
    }
}

/// Token usage summary response for GenAI/LLM spans
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenUsageResponse {
    /// Overall token usage summary
    pub summary: TokenUsageSummary,
    /// Token usage grouped by model
    pub by_model: Vec<ModelUsage>,
    /// Token usage grouped by system (provider)
    pub by_system: Vec<SystemUsage>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Overall token usage summary
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TokenUsageSummary {
    /// Total input tokens across all requests
    pub total_input_tokens: u64,
    /// Total output tokens across all requests
    pub total_output_tokens: u64,
    /// Total number of GenAI requests
    pub total_requests: usize,
    /// Total cache creation input tokens (Anthropic prompt caching)
    #[serde(default)]
    pub total_cache_creation_tokens: u64,
    /// Total cache read input tokens (Anthropic prompt caching)
    #[serde(default)]
    pub total_cache_read_tokens: u64,
}

/// Token usage for a specific model
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelUsage {
    /// Model identity: `provider/model` when a provider is recorded, bare
    /// model otherwise. Never built from a response model when a request
    /// model exists (#143).
    pub model: String,
    /// Input tokens for this model
    pub input_tokens: u64,
    /// Output tokens for this model
    pub output_tokens: u64,
    /// Number of requests for this model
    pub requests: usize,
    /// Dominant response model within this identity when it differs from the
    /// request model (silent provider rerouting), else `null`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_model: Option<String>,
    /// Calls where the recorded response model differs from the request
    /// model (both known).
    #[serde(default)]
    pub rerouted_count: usize,
}

/// Token usage for a specific system (provider)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SystemUsage {
    /// System name (e.g., "openai", "anthropic")
    pub system: String,
    /// Input tokens for this system
    pub input_tokens: u64,
    /// Output tokens for this system
    pub output_tokens: u64,
    /// Number of requests for this system
    pub requests: usize,
}

/// A single time-bucketed cost/usage data point
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CostSeriesPoint {
    /// Bucket start timestamp in nanoseconds since Unix epoch
    pub timestamp: i64,
    /// Model name (nullable — spans without a model attribute are grouped under null)
    pub model: Option<String>,
    /// Input tokens in this bucket
    pub input_tokens: u64,
    /// Output tokens in this bucket
    pub output_tokens: u64,
    /// Cache creation input tokens (Anthropic prompt caching)
    pub cache_creation_tokens: u64,
    /// Cache read input tokens (Anthropic prompt caching)
    pub cache_read_tokens: u64,
    /// Number of requests in this bucket
    pub requests: usize,
    /// Estimated cost in USD for this bucket, computed server-side. `None` when
    /// no pricing data matched the bucket's model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// Origin of the cost figure: "litellm", "fallback", or "none".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
}

/// Sort dimension for top-N span queries.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "snake_case")]
pub enum TopSpanSort {
    /// By total token count (default — same as before).
    #[default]
    TotalTokens,
    /// By span duration (slowest first).
    Duration,
    /// By output/context token ratio (most verbose first). The context includes
    /// uncached input plus cache reads and cache creation.
    OutputInputRatio,
    /// By cache efficiency: worst cache-read rate (ascending) first.
    CacheEfficiency,
}

/// A single top-N expensive LLM span
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TopSpan {
    pub trace_id: String,
    pub span_id: String,
    /// Span start time (nanoseconds since Unix epoch)
    pub start_time: i64,
    /// Span duration in nanoseconds
    pub duration: i64,
    pub model: Option<String>,
    pub system: Option<String>,
    pub session_id: Option<String>,
    pub prompt_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_creation_tokens: u64,
    pub cache_read_tokens: u64,
    pub total_tokens: u64,
    /// First finish/stop reason for this span (e.g. "max_tokens", "end_turn").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
    /// `gen_ai.conversation.id` attribute if present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    /// Estimated cost in USD, computed server-side from the pricing database.
    /// `None` when no pricing data matched this row's (model, system).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    /// Origin of the cost figure: "litellm", "fallback", or "none".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
    /// Human-readable tooltip explaining why cost is None (e.g.
    /// "no pricing data for claude-foo on bedrock").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_reason: Option<String>,
    /// Derived output token throughput (output_tokens / span_duration_sec).
    /// Span duration includes network + queue time — not pure generation time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_output_tokens_per_sec: Option<f64>,
}

/// Aggregated cost/token row for a single session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionCostRow {
    pub session_id: String,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
}

/// Aggregated cost/token row for a single conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationCostRow {
    pub conversation_id: String,
    pub request_count: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
}

/// Distribution entry for a single finish reason
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct FinishReasonCount {
    pub reason: String,
    pub count: usize,
}

/// Latency / TTFT percentile statistics for LLM spans, grouped by model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LatencyStats {
    pub model: Option<String>,
    pub count: usize,
    pub avg_ms: f64,
    pub p50_ms: i64,
    pub p95_ms: i64,
    pub p99_ms: i64,
    /// Number of valid TTFT values used for percentile calculations.
    pub ttft_count: usize,
    /// Number of emitted TTFT values that could not represent first-token latency.
    #[serde(default)]
    pub ttft_invalid_count: usize,
    /// Number of valid values that were at least 90% of complete request duration.
    #[serde(default)]
    pub ttft_degenerate_count: usize,
    /// True when at least 10 valid values exist and 90% are near complete duration.
    #[serde(default)]
    pub ttft_degenerate: bool,
    pub ttft_p50_ms: Option<i64>,
    pub ttft_p95_ms: Option<i64>,
    pub ttft_p99_ms: Option<i64>,
    /// Derived end-to-end output throughput (output_tokens / span_duration),
    /// computed per call from raw nanosecond durations. Span duration
    /// includes provider, queue and network time — NOT pure generation
    /// throughput. Lower-tail / median / upper-reference triple (#119);
    /// p10 is a weak estimate when `throughput_sample_count < 10`.
    /// Only set for spans where both output_tokens > 0 and duration > 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_tokens_per_sec_p10: Option<f64>,
    pub derived_tokens_per_sec_p50: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_tokens_per_sec_p90: Option<f64>,
    /// p95/p99 retained during a documented compatibility period; the
    /// primary presentation is p10/p50/p90 (#119).
    pub derived_tokens_per_sec_p95: Option<f64>,
    pub derived_tokens_per_sec_p99: Option<f64>,
    /// Number of calls eligible for throughput (output_tokens > 0 and
    /// duration > 0). Separate from `count` (all request spans in the
    /// group) — confidence warnings must use this, not `count`.
    #[serde(default)]
    pub throughput_sample_count: usize,
    /// Distribution of input token counts (context / prompt size).
    pub input_tokens_p50: Option<i64>,
    pub input_tokens_p95: Option<i64>,
    pub input_tokens_p99: Option<i64>,
    /// Distribution of output/context token ratio (generation verbosity).
    ///
    /// The legacy field name is retained for API compatibility. Context includes
    /// uncached input plus cache reads and cache creation.
    pub output_input_ratio_p50: Option<f64>,
    pub output_input_ratio_p95: Option<f64>,
    pub output_input_ratio_p99: Option<f64>,
}

/// Native telemetry capability for one metric within an emitter/model group.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GenAiMetricCapability {
    /// Request spans eligible to provide this metric.
    pub eligible_count: usize,
    /// Spans carrying an attribute for this metric, whether valid or invalid.
    pub observed_count: usize,
    /// Parsed and usable observations.
    pub valid_count: usize,
    /// Present observations rejected as invalid.
    pub invalid_count: usize,
    /// `available`, `sparse`, or `absent`.
    pub availability: String,
    /// `reliable`, `invalid`, `degenerate`, or `not_assessed`.
    pub quality: String,
    /// `native`, `correlated`, or `unavailable`.
    pub derivation: String,
    /// Attribute keys that supplied observed values, with occurrence counts.
    #[serde(default)]
    pub source_attributes: HashMap<String, usize>,
}

/// Correlation outcome counts for one capability group.
///
/// `rule` is `none` when no cross-span correlation rule applies to the group;
/// Codex groups report `codex-one-to-one-v1`.
///
/// Candidate-centred counts for the rule applied to this group:
/// - `matched_count`: request spans with exactly one verified usage candidate
///   (one per candidate used);
/// - `unmatched_count`: request spans in the group with no usage candidate
///   (a request-level gap, also visible as `absent`/`sparse` availability);
/// - `rejected_count`: request spans whose single candidate failed a join
///   invariant (error status, conflicting model);
/// - `ambiguous_count`: usage candidates sitting under a request span with
///   two or more candidates (retries, concurrent sampling, reused
///   identifiers, turn-level counters) — none of them is attributed.
///
/// Only counts and the rule name are exposed — never span or trace
/// identifiers.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GenAiCorrelationProvenance {
    /// Correlation rule applied: `none` or a versioned rule name.
    pub rule: String,
    /// Request spans joined to exactly one verified usage candidate.
    pub matched_count: usize,
    /// Request spans in the group with no usage candidate.
    pub unmatched_count: usize,
    /// Request spans whose candidate failed a join invariant.
    pub rejected_count: usize,
    /// Usage candidates excluded because their request had multiple candidates.
    pub ambiguous_count: usize,
}

/// GenAI telemetry capabilities grouped by provider, model and emitter fingerprint.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GenAiCapabilityReport {
    pub provider: Option<String>,
    pub model: Option<String>,
    /// Stable non-content fingerprint; it does not expose correlation identifiers.
    pub emitter_fingerprint: String,
    pub emitter: String,
    pub adapter_rule: String,
    /// Canonical request spans after duplicate OTLP deliveries are removed.
    pub request_count: usize,
    pub input_tokens: GenAiMetricCapability,
    pub output_tokens: GenAiMetricCapability,
    pub cache_creation_tokens: GenAiMetricCapability,
    pub cache_read_tokens: GenAiMetricCapability,
    pub ttft: GenAiMetricCapability,
    pub correlation: GenAiCorrelationProvenance,
}

/// One class of LLM-ish spans that no verified emitter signature matched,
/// with the attribute names a verified signature would still require
/// (#149). Attribute *names* and counts only — never values, span or trace
/// identifiers, or service names (the #120 privacy invariant).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GenAiUnidentifiedSignature {
    /// Sorted attribute names still required for a verified signature.
    pub required_attributes: Vec<String>,
    /// Spans in the bounded sample with exactly this missing set.
    pub span_count: usize,
}

/// Result metadata for a bounded GenAI capability query.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct GenAiCapabilityResponse {
    pub reports: Vec<GenAiCapabilityReport>,
    pub canonical_span_count: usize,
    pub duplicate_span_count: usize,
    /// Older physical spans were excluded by the bounded most-recent sample.
    pub truncated: bool,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
    /// LLM-ish spans with no verified emitter signature, and the attribute
    /// names that would identify them (#149). Empty when every LLM-ish span
    /// in the sample matched a verified signature.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unidentified: Vec<GenAiUnidentifiedSignature>,
}

/// One half-open time window `[start_time, end_time)` in nanoseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceWindow {
    pub start_time: i64,
    pub end_time: i64,
}

/// Selection for the model-performance comparison (#121/#151). The
/// preceding window is derived as the equal-length interval immediately
/// before `current`; the rolling baseline is caller-supplied and must
/// exclude both the current and the derived preceding windows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceQuery {
    /// The interval under diagnosis.
    pub current: ModelPerformanceWindow,
    /// Rolling historical baseline; `None` disables the rolling baseline.
    pub rolling: Option<ModelPerformanceWindow>,
    /// Optional exact request-model filter.
    pub model: Option<String>,
    /// Optional exact provider (system) filter.
    pub provider: Option<String>,
}

/// One reported percentile value of a metric for one window.
///
/// `delta_vs_preceding` / `delta_vs_rolling` are only populated on the
/// *current* window's values — baselines are compared against, not
/// compared. `relative` is `None` (JSON `null`) for the documented
/// "percentage unavailable" state: the baseline value is zero or the
/// baseline window had no eligible samples.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformancePercentile {
    /// Percentile rank (e.g. 50, 95).
    pub percentile: u8,
    /// Measured value in the metric's native unit (ms for duration/TTFT,
    /// tokens/s for throughput, counts for token metrics, fraction for
    /// error rate).
    pub value: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_vs_preceding: Option<ModelPerformanceDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_vs_rolling: Option<ModelPerformanceDelta>,
}

/// Absolute and relative change of one percentile vs a baseline.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceDelta {
    /// `current - baseline` in the metric's native unit.
    pub absolute: f64,
    /// Relative change as a fraction (`(current - baseline) / baseline`),
    /// or `null` when the baseline is zero/absent (percentage unavailable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relative: Option<f64>,
}

/// One metric's statistics across the three windows for one identity.
///
/// `None` sample = the window had no eligible requests for this metric
/// (never a measured zero — the capability vocabulary applies).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceMetric {
    pub current: Option<ModelPerformanceSample>,
    pub preceding: Option<ModelPerformanceSample>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolling: Option<ModelPerformanceSample>,
}

/// Eligible-sample count and percentiles for one metric in one window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceSample {
    /// Request spans eligible for this metric in the window.
    pub eligible_count: usize,
    pub percentiles: Vec<ModelPerformancePercentile>,
}

/// Canonical request counts per window for one identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceCounts {
    pub current: usize,
    pub preceding: usize,
    pub rolling: usize,
}

/// Model-performance diagnosis for one
/// `(provider, request model, emitter fingerprint)` identity (#121/#151).
///
/// The response model is a separate observation (`response_models`) so
/// routing changes are never silently merged into one identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceIdentity {
    pub provider: Option<String>,
    /// Request model (the selection dimension; response model is separate).
    pub model: Option<String>,
    pub emitter_fingerprint: String,
    /// Distinct response models observed on current-window requests, sorted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub response_models: Vec<String>,
    /// Canonical request population per window (one population per window;
    /// per-metric eligibility is reported inside each metric).
    pub request_counts: ModelPerformanceCounts,
    /// Total request duration, ms; percentiles p50 and p95.
    pub duration: ModelPerformanceMetric,
    /// Derived end-to-end throughput, tokens/s (#119 raw per-call series);
    /// percentiles p10 and p50. Eligible = output tokens present and
    /// duration > 0 — duration-only emitters are simply ineligible here.
    pub throughput: ModelPerformanceMetric,
    /// Time to first token, ms; percentile p50. Eligible = valid
    /// normalised TTFT (canonical normaliser + classifier).
    pub ttft: ModelPerformanceMetric,
    /// Input (context) tokens; percentile p50.
    pub input_tokens: ModelPerformanceMetric,
    /// Cache-creation tokens; percentile p50.
    pub cache_creation_tokens: ModelPerformanceMetric,
    /// Cache-read tokens; percentile p50.
    pub cache_read_tokens: ModelPerformanceMetric,
    /// Output tokens; percentile p50.
    pub output_tokens: ModelPerformanceMetric,
    /// Error rate per window: request and error counts, the rate, and
    /// deltas of the rate vs each baseline (absolute points + relative
    /// where the baseline rate is non-zero).
    pub error_rate: ModelPerformanceErrorRate,
}

/// Error-rate evidence for one window: counts plus the rate with its
/// deltas. `None` = no requests in the window (not a zero rate).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceErrorValue {
    pub requests: usize,
    pub errors: usize,
    /// `errors / requests` in the range 0.0..=1.0.
    pub rate: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_vs_preceding: Option<ModelPerformanceDelta>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_vs_rolling: Option<ModelPerformanceDelta>,
}

/// Error-rate statistics across the three windows.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceErrorRate {
    pub current: Option<ModelPerformanceErrorValue>,
    pub preceding: Option<ModelPerformanceErrorValue>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolling: Option<ModelPerformanceErrorValue>,
}

/// Deterministic comparison of a current interval against an equal-length
/// preceding interval and an optional rolling historical baseline
/// (#121/#151). Classification and confidence wording are added by #152;
/// this object is the evidence base and must stay byte-stable across
/// API and CLI (parity oracle, #155).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceResponse {
    pub current_window: ModelPerformanceWindow,
    /// Equal-length interval immediately before `current_window`.
    pub preceding_window: ModelPerformanceWindow,
    /// Rolling baseline as selected; `None` when the caller disabled it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolling_window: Option<ModelPerformanceWindow>,
    pub identities: Vec<ModelPerformanceIdentity>,
    /// The bounded most-recent sample excluded older spans.
    pub truncated: bool,
}

/// Envelope for the model-performance diagnosis endpoint and CLI
/// (#121/#153). The raw #151 comparison (`identities`) is the canonical
/// evidence; the #152 `assessments` classify it deterministically and are
/// the only source of causal-sounding wording (they are never recomputed by
/// surfaces). `timezone` is echoed for calendar alignment.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelPerformanceDiagnosis {
    pub current_window: ModelPerformanceWindow,
    /// Equal-length interval immediately before `current_window`.
    pub preceding_window: ModelPerformanceWindow,
    /// Rolling baseline as selected; `None` when the caller disabled it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolling_window: Option<ModelPerformanceWindow>,
    /// IANA timezone echoed back for calendar alignment (when supplied).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    /// The bounded most-recent sample excluded older spans.
    pub truncated: bool,
    /// Canonical per-identity raw comparison evidence (#151).
    pub identities: Vec<ModelPerformanceIdentity>,
    /// Deterministic per-identity assessments (#152), in the same order as
    /// `identities`.
    pub assessments: Vec<crate::model_performance::ModelPerformanceAssessment>,
}

/// Minimum eligible current-window requests for any model-performance
/// assessment; every assessment reports its sample count and fewer samples
/// classify as `InsufficientTelemetry` (#152).
pub const MODEL_PERFORMANCE_MIN_ELIGIBLE_SAMPLES: usize = 10;
/// A relative change of at least this fraction is material for ms/token
/// metrics (direction-neutral; the regression classes apply the direction).
pub const MODEL_PERFORMANCE_MATERIAL_RELATIVE_CHANGE: f64 = 0.20;
/// A change of at least this many percentage points is material for the
/// error rate.
pub const MODEL_PERFORMANCE_MATERIAL_ERROR_RATE_POINTS: f64 = 0.05;

/// Error-rate summary for LLM spans grouped by model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorRateByModel {
    pub model: Option<String>,
    pub total: usize,
    pub errors: usize,
    /// Fraction in the range 0.0..1.0.
    pub error_rate: f64,
}

/// Aggregated per-tool usage for tool-execution spans.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolUsage {
    pub tool_name: String,
    pub count: usize,
    pub success_count: usize,
    pub error_count: usize,
    pub avg_duration_ms: f64,
    pub total_duration_ms: i64,
}

/// Retry statistics across LLM spans.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RetryStats {
    pub total_llm_calls: usize,
    /// Calls with attempt > 1 (Claude Code) or comparable retry markers.
    pub retried_calls: usize,
    /// Sum of (attempt - 1) across all calls — total extra attempts.
    pub extra_attempts: usize,
    /// Fraction in the range 0.0..1.0.
    pub retry_rate: f64,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Retrieval / RAG statistics aggregated across retriever spans.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RetrievalStats {
    pub total_retrievals: usize,
    pub avg_documents_per_query: f64,
    /// None when no retrieval span emitted a document score.
    pub avg_top_document_score: Option<f64>,
    pub top_queries: Vec<TopRetrievalQuery>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// A single grouped retrieval query with aggregate stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TopRetrievalQuery {
    pub query: String,
    pub count: usize,
    pub avg_documents: f64,
    pub avg_top_score: Option<f64>,
}

/// Truncation rate (finish_reason = max_tokens/length) per model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TruncationRateByModel {
    pub model: Option<String>,
    pub total: usize,
    pub truncated: usize,
    /// Fraction in the range 0.0..1.0.
    pub rate: f64,
}

/// Cache token efficiency per model.
/// `hit_rate` = cache_read_tokens / (cache_read_tokens + input_tokens).
/// Only set when at least one of the token counts is non-zero.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CacheHitRateByModel {
    pub model: Option<String>,
    pub total_input_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cache_creation_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_rate: Option<f64>,
}

/// Cache economics for one time bucket (all models combined).
///
/// `hit_rate` uses the same definition as the model table:
/// `cache_read / (cache_read + input)`, `None` when the denominator is 0.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CacheEconSeriesPoint {
    /// Bucket start timestamp in nanoseconds since Unix epoch
    pub timestamp: i64,
    pub input: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_rate: Option<f64>,
}

/// Cache economics for one model over the whole window.
///
/// `hit_rate` = `cache_read / (cache_read + input)` (same definition as
/// [`CacheHitRateByModel`]); `read_write_ratio` = `cache_read / cache_write`,
/// `None` when there were no cache writes. `est_savings_usd` /
/// `savings_known` are enriched by the API layer from the pricing table —
/// `savings_known` is false (and savings null) when the model's cache-read
/// price is unknown.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CacheEconModelEntry {
    pub model: String,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hit_rate: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read_write_ratio: Option<f64>,
    /// Estimated savings from cache reads: `cache_read × (input_rate −
    /// cache_read_rate)`. `None` when the cache-read price is unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub est_savings_usd: Option<f64>,
    pub savings_known: bool,
}

/// Response for `GET /api/genai/cache_hit_rate?by_model=1` — per-model cache
/// economics plus a time-bucketed read/write series.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CacheEconomicsResponse {
    pub series: Vec<CacheEconSeriesPoint>,
    pub models: Vec<CacheEconModelEntry>,
}

/// Reasoning ("thinking") token share for one model.
///
/// `share_pct` is `reasoning_tokens / output_tokens × 100` — `None` when the
/// model produced no output tokens in the window. Reasoning tokens are a
/// separate token category from output in both opencode and codex telemetry,
/// so the share is "how much of what the model answered was thinking".
/// `cost_usd` (enriched by the API layer) prices the reasoning tokens at the
/// model's output rate — that is what thinking actually costs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReasoningShareByModel {
    pub model: String,
    pub reasoning_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_pct: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// Reasoning tokens per codex reasoning-effort level.
///
/// Codex's `handle_responses` spans carry the effort but no model, so this
/// breakdown is global, not per model (see `ReasoningShareResponse`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReasoningEffortEntry {
    pub effort: String,
    pub reasoning_tokens: u64,
    pub calls: u64,
}

/// Response for `GET /api/genai/reasoning_share` — "how much am I paying for
/// thinking", by model plus a global per-effort breakdown.
///
/// Sources: opencode `token.usage` counters (types `reasoning`/`output`) and
/// codex `turn.token_usage` histograms (`reasoning_output`/`output`). Claude
/// Code is deliberately absent: its `llm_request` spans carry no
/// thinking-token attributes (verified on the live DB), so nothing would be
/// real to report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ReasoningShareResponse {
    pub models: Vec<ReasoningShareByModel>,
    pub effort: Vec<ReasoningEffortEntry>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Token usage for one agent harness, all five categories.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentTokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

impl AgentTokenUsage {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write + self.reasoning
    }

    /// Add another usage into this one (same categories, all five).
    pub fn fold_tokens(&mut self, other: AgentTokenUsage) {
        self.input += other.input;
        self.output += other.output;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.reasoning += other.reasoning;
    }
}

/// One time bucket of an agent's `series` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentSeriesPoint {
    /// Bucket start (ns), aligned to `bucket_secs`.
    pub ts: i64,
    /// Total tokens (all categories) in the bucket.
    pub tokens: u64,
    /// Cost in the bucket; `None` when no model in the bucket has known
    /// pricing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

/// Per-harness rollup: which agent, how many sessions, what it spent, what
/// it did. `agent` is the harness name ("opencode", "codex", "claude"), not
/// a sub-agent role.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentRollup {
    pub agent: String,
    /// Sessions started in the window.
    pub sessions: u64,
    /// Spend in USD. `None` when no model has known pricing (estimated
    /// agents) — never a fabricated zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// "actual" (the harness's own cost counter) or "estimated"
    /// (tokens x pricing table).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
    pub tokens: AgentTokenUsage,
    pub tool_calls: u64,
    /// Failed/retried API requests where the harness reports them; `None`
    /// when the harness emits no retry telemetry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retries: Option<u64>,
    /// Per-bucket cost/tokens for the chart, ascending by `ts`.
    pub series: Vec<AgentSeriesPoint>,
}

/// Per-harness rollup response. `agents` is sorted by total cost (estimated
/// or actual) descending, ties by agent name.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentRollupResponse {
    pub agents: Vec<AgentRollup>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Storage-layer per-agent rollup: token detail per model (the API layer
/// prices it) plus the harness cost counter where one exists. Not a wire
/// type — see [`AgentRollupResponse`].
#[derive(Debug, Clone)]
pub struct AgentRollupStorage {
    pub agent: String,
    pub sessions: u64,
    pub tool_calls: u64,
    pub retries: Option<u64>,
    /// The harness's own cost counter delta in the window (opencode only;
    /// `None` for harnesses without a cost metric — [`enrich`] falls back
    /// to tokens x pricing).
    pub counter_cost_usd: Option<f64>,
    /// Per-model token totals, for API-layer pricing.
    pub models: Vec<(String, AgentTokenUsage)>,
    /// Per-bucket per-model tokens; `ts` is the aligned bucket start.
    pub series: Vec<(i64, Vec<(String, AgentTokenUsage)>)>,
}

impl AgentRollupStorage {
    /// Price this rollup into a wire [`AgentRollup`]. opencode's cost is its
    /// own spend counter ("actual"); other harnesses are estimated from
    /// tokens x pricing (their cost counters under-report on the live data).
    /// Reasoning is billed at the output rate, same convention as the
    /// reasoning-share endpoint.
    pub fn enrich(self, pricing_db: &crate::pricing::PricingDatabase) -> AgentRollup {
        use crate::pricing::TokenUsage;

        let pricing_usage = |tokens: &AgentTokenUsage| TokenUsage {
            input: tokens.input,
            output: tokens.output + tokens.reasoning,
            cache_creation: tokens.cache_write,
            cache_read: tokens.cache_read,
        };

        let mut tokens = AgentTokenUsage::default();
        let mut estimated: Option<f64> = None;
        for (model, model_tokens) in &self.models {
            tokens.input += model_tokens.input;
            tokens.output += model_tokens.output;
            tokens.cache_read += model_tokens.cache_read;
            tokens.cache_write += model_tokens.cache_write;
            tokens.reasoning += model_tokens.reasoning;
            if let Some(cost) = pricing_db
                .compute_cost(Some(model.as_str()), pricing_usage(model_tokens), None)
                .cost
            {
                estimated = Some(estimated.unwrap_or(0.0) + cost);
            }
        }

        let mut series: Vec<AgentSeriesPoint> = self
            .series
            .into_iter()
            .map(|(ts, per_model)| {
                let mut bucket_tokens = 0u64;
                let mut bucket_cost: Option<f64> = None;
                for (model, model_tokens) in per_model {
                    bucket_tokens += model_tokens.total();
                    if let Some(cost) = pricing_db
                        .compute_cost(Some(&model), pricing_usage(&model_tokens), None)
                        .cost
                    {
                        bucket_cost = Some(bucket_cost.unwrap_or(0.0) + cost);
                    }
                }
                AgentSeriesPoint {
                    ts,
                    tokens: bucket_tokens,
                    cost_usd: bucket_cost,
                }
            })
            .collect();
        series.sort_by_key(|p| p.ts);

        let (cost_usd, cost_source) = match self.counter_cost_usd {
            Some(actual) => (Some(actual), Some("actual".to_string())),
            None => (estimated, Some("estimated".to_string())),
        };

        AgentRollup {
            agent: self.agent,
            sessions: self.sessions,
            cost_usd,
            cost_source,
            tokens,
            tool_calls: self.tool_calls,
            retries: self.retries,
            series,
        }
    }
}

/// One entry of a project's `top_models`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProjectTopModel {
    pub model: String,
    /// Tokens × pricing; `None` when the model has no known pricing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    pub tokens: AgentTokenUsage,
}

/// Per-project rollup: which project, how many sessions, what it spent.
/// `project_id` is the opencode `project.id` label; codex and claude emit
/// no project label today, so their activity is grouped under the
/// sentinel `"unattributed"` (the limitation, not a gap in the query).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProjectRollup {
    pub project_id: String,
    pub sessions: u64,
    /// Spend in USD; `None` when no cost can be established — never a
    /// fabricated zero.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// "actual" (the opencode cost counter covers the project),
    /// "estimated" (tokens × pricing), or "mixed" (the unattributed row:
    /// a counter for label-less opencode sessions plus priced codex/claude
    /// tokens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
    pub tokens: AgentTokenUsage,
    /// Up to 5 models, cost desc (unpriced models last, tokens desc).
    pub top_models: Vec<ProjectTopModel>,
}

/// Per-project rollup response, sorted by cost desc (unpriced last),
/// ties by `project_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProjectRollupResponse {
    pub projects: Vec<ProjectRollup>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Storage-layer per-project rollup. Not a wire type — see
/// [`ProjectRollupResponse`].
#[derive(Debug, Clone)]
pub struct ProjectRollupStorage {
    pub project_id: String,
    pub sessions: u64,
    /// The opencode cost-counter delta for this project's sessions
    /// (`None` when the project has no counter cost).
    pub counter_cost_usd: Option<f64>,
    /// Per-model token totals, for API-layer pricing.
    pub models: Vec<(String, AgentTokenUsage)>,
    /// False (the common case): `counter_cost_usd` prices the same tokens
    /// as `models` — it is the authoritative cost. True (only the
    /// unattributed row): the counter covers label-less opencode sessions
    /// while `models` holds codex/claude tokens, so the two costs are
    /// disjoint and add up.
    pub counter_disjoint_from_tokens: bool,
}

impl ProjectRollupStorage {
    /// Price this rollup into a wire [`ProjectRollup`]. Same convention as
    /// [`AgentRollupStorage::enrich`]: reasoning is billed at the output
    /// rate.
    pub fn enrich(self, pricing_db: &crate::pricing::PricingDatabase) -> ProjectRollup {
        use crate::pricing::TokenUsage;

        let pricing_usage = |tokens: &AgentTokenUsage| TokenUsage {
            input: tokens.input,
            output: tokens.output + tokens.reasoning,
            cache_creation: tokens.cache_write,
            cache_read: tokens.cache_read,
        };

        let mut tokens = AgentTokenUsage::default();
        let mut priced: Vec<(String, Option<f64>, AgentTokenUsage)> = Vec::new();
        for (model, model_tokens) in &self.models {
            tokens.input += model_tokens.input;
            tokens.output += model_tokens.output;
            tokens.cache_read += model_tokens.cache_read;
            tokens.cache_write += model_tokens.cache_write;
            tokens.reasoning += model_tokens.reasoning;
            let cost = pricing_db
                .compute_cost(Some(model.as_str()), pricing_usage(model_tokens), None)
                .cost;
            priced.push((model.clone(), cost, *model_tokens));
        }
        // Cost desc, unpriced last, then tokens desc.
        priced.sort_by(|a, b| {
            b.1.unwrap_or(0.0)
                .partial_cmp(&a.1.unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| b.2.total().cmp(&a.2.total()))
        });
        let estimated: Option<f64> = priced
            .iter()
            .filter_map(|(_, c, _)| *c)
            .reduce(|s, c| s + c);
        let top_models = priced
            .into_iter()
            .take(5)
            .map(|(model, cost_usd, tokens)| ProjectTopModel {
                model,
                cost_usd,
                tokens,
            })
            .collect();

        let (cost_usd, cost_source) = if self.counter_disjoint_from_tokens {
            match (self.counter_cost_usd, estimated) {
                // A $0 counter is not a cost source of its own.
                (Some(c), Some(e)) => (
                    Some(c + e),
                    Some(if c > 0.0 { "mixed" } else { "estimated" }.to_string()),
                ),
                (Some(c), None) => (Some(c), Some("actual".to_string())),
                (None, Some(e)) => (Some(e), Some("estimated".to_string())),
                (None, None) => (None, Some("estimated".to_string())),
            }
        } else {
            match self.counter_cost_usd {
                Some(actual) => (Some(actual), Some("actual".to_string())),
                None => (estimated, Some("estimated".to_string())),
            }
        };

        ProjectRollup {
            project_id: self.project_id,
            sessions: self.sessions,
            cost_usd,
            cost_source,
            tokens,
            top_models,
        }
    }
}

/// Per-session cost record (wire). `agent` is the harness that emitted the
/// session ("opencode" or "claude" — codex emits no per-session identifiers,
/// so it never appears). `cost_usd` is `None` when the cost cannot be
/// established (no counter and no priced models) — never a fabricated zero.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionCost {
    pub session_id: String,
    pub agent: String,
    /// Project the session ran in, where the harness reports one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// "actual" (the harness's own cost counter) or "estimated"
    /// (tokens x pricing table).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
    pub tokens: u64,
    /// Wall-clock duration of the session in seconds, where measured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_secs: Option<f64>,
    /// `true` when `cost_usd > 3 x median_cost_usd` (see
    /// [`SessionCostResponse::anomaly_rule`]).
    pub anomaly: bool,
}

/// Top-cost sessions response. `sessions` is sorted by cost descending
/// (uncosted sessions last) and truncated to the requested limit; the
/// anomaly flag and median are computed over the **full** window before
/// truncation, so `limit` never hides an outlier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionCostResponse {
    pub sessions: Vec<SessionCost>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub median_cost_usd: Option<f64>,
    /// The outlier formula, stated for consumers: a session is anomalous
    /// when its cost exceeds three times the median session cost.
    pub anomaly_rule: String,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// One log-spaced cost bucket of the per-session cost distribution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CostBucket {
    pub min_usd: f64,
    pub max_usd: f64,
    pub count: u64,
}

/// Log-spaced distribution of per-session costs (wire).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CostDistributionResponse {
    pub buckets: Vec<CostBucket>,
}

/// One bin of a generic distribution (issue #133).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DistributionBucket {
    /// Inclusive lower bound.
    pub min: f64,
    /// Exclusive upper bound (inclusive for the last bin).
    pub max: f64,
    pub count: u64,
}

/// Summary statistics of a distribution's values.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DistributionStats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub count: u64,
}

/// `GET /api/genai/distributions` response: one named cohort binned
/// linearly or log-spaced, with summary stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct DistributionResponse {
    /// Metric name: session_cost | tool_duration | llm_duration | ttft | output_tokens.
    pub metric: String,
    /// Unit of the values: usd | ms | tokens.
    pub unit: String,
    /// Binning scale: linear | log.
    pub scale: String,
    #[serde(default)]
    pub buckets: Vec<DistributionBucket>,
    /// None when the window has no values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<DistributionStats>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Session header for the session context endpoint (issue #134).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionContextSession {
    pub id: String,
    /// Detected agent: claude | opencode | codex (None when unrecognised).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// opencode's `project.id` label; absent for claude/codex (they emit no
    /// project label).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// "full" | "partial": how much of this agent's span set carries
    /// session.id. opencode labels only llm/tool spans; codex labels only
    /// `mcp.tools.call` spans. claude labels its whole span set.
    pub span_coverage: String,
}

/// One span in the session context (issue #134).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionContextSpan {
    pub trace_id: String,
    pub span_id: String,
    pub name: String,
    pub start_time: i64,
    pub duration_ns: i64,
    /// `model` attribute when present (timeline labels).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// One log in the session context (issue #134). Body is truncated to
/// 512 chars.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionContextLog {
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    pub body: String,
}

/// Aggregated metric series for one metric name in the session context
/// (issue #134) — counts and scalar stats, not raw points.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionContextMetric {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    /// OTel metric type: 0 gauge, 1 counter, 2 histogram (see MetricType).
    pub metric_type: u8,
    pub count: u64,
    /// Sum/min/max over scalar (gauge/counter) points; None when the
    /// series has no scalar values (e.g. histograms only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sum: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    pub first_ts: i64,
    pub last_ts: i64,
}

/// One merged span/log event on the session timeline (issue #134).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionContextTimelineEvent {
    pub ts: i64,
    /// "span" | "log".
    pub kind: String,
    /// e.g. "llm_request claude-opus-5[1m]" or "api_request WARN".
    pub label: String,
}

/// GET /api/sessions/:id/context — everything observed for one session on
/// one timeline (issue #134). Spans and logs are truncated to `limit` with
/// `*_total` counts (counting the queried scope: the time window, when one
/// is given, else the whole session); metrics are aggregated per name, not
/// raw-dumped.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionContextResponse {
    pub session: SessionContextSession,
    #[serde(default)]
    pub spans: Vec<SessionContextSpan>,
    pub spans_total: u64,
    #[serde(default)]
    pub logs: Vec<SessionContextLog>,
    pub logs_total: u64,
    #[serde(default)]
    pub metrics: Vec<SessionContextMetric>,
    /// Spans and logs merged, ascending by ts, capped at `limit`.
    #[serde(default)]
    pub timeline: Vec<SessionContextTimelineEvent>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Storage-layer per-session cost: token detail per model (the API layer
/// prices it) plus the harness cost counter where one exists. Not a wire
/// type — see [`SessionCostResponse`].
#[derive(Debug, Clone)]
pub struct SessionCostStorage {
    pub agent: String,
    pub session_id: String,
    pub project_id: Option<String>,
    /// The harness's own per-session cost (opencode's cumulative
    /// `session.cost.total` counter, last value in the window); `None` for
    /// harnesses without a cost metric.
    pub counter_cost_usd: Option<f64>,
    /// Total tokens across the session's models.
    pub tokens: u64,
    /// Per-model token detail; priced by the API layer for harnesses
    /// without a cost counter.
    pub models: Vec<(String, AgentTokenUsage)>,
    /// Session duration in seconds, where the harness measures it.
    pub duration_secs: Option<f64>,
}

/// Token usage split for one sub-agent role.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RoleTokenUsage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub reasoning: u64,
}

impl RoleTokenUsage {
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_read + self.cache_write + self.reasoning
    }
}

/// Per-model token breakdown for one sub-agent role. Cost is enriched by the
/// API layer from the pricing table; `cost` is `None` when the model has no
/// known pricing (e.g. local models).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RoleModelBreakdown {
    pub model: String,
    pub tokens: RoleTokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_reason: Option<String>,
}

/// Cost and token attribution for one sub-agent role (the opencode `agent`
/// label). `share_pct` is the role's share of total tokens across all roles
/// (cost-based shares would be misleading while local models carry no price).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentRoleBreakdown {
    /// Role name; missing `agent` labels are reported as "unknown".
    pub role: String,
    pub tokens: RoleTokenUsage,
    /// Distinct sessions that used this role in the window.
    pub sessions: u64,
    /// Sum of per-model costs; `None` when no model in the role has pricing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_pct: Option<f64>,
    /// Top 5 models by total tokens.
    pub top_models: Vec<RoleModelBreakdown>,
}

/// Response for `GET /api/genai/agent_roles`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct AgentRolesResponse {
    /// Roles sorted by total tokens descending.
    pub roles: Vec<AgentRoleBreakdown>,
    /// Share of all tokens attributed to the "unknown" role (`None` when no
    /// rows have an `agent` label at all).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_share_pct: Option<f64>,
    /// Agents whose telemetry this analysis covers. Claude Code and Codex do
    /// not emit a role label today, so this is currently `["opencode"]`.
    pub agents_covered: Vec<String>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// One model row inside a provider mix entry. `cost_usd` is estimated from
/// tokens × the pricing table and enriched by the API layer; `None` for
/// local or unpriced models.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProviderModelEntry {
    pub model: String,
    pub tokens: RoleTokenUsage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<String>,
    /// Distinct sessions observed for this model in the window (0 when the
    /// source does not carry a session id, e.g. codex turn metrics).
    pub sessions: u64,
}

/// One provider row in the provider × model mix. `share_pct` is the
/// provider's share of total tokens (cost-based shares would be misleading
/// while local models carry no price). `cost_usd` covers the priced models
/// only; `None` when none of the provider's models has known pricing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProviderMixEntry {
    /// Provider as carried by the telemetry; "(unknown)" when the harness
    /// does not emit a provider/system attribute (never guessed).
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_pct: Option<f64>,
    /// Models under this provider, sorted by total tokens descending.
    pub models: Vec<ProviderModelEntry>,
}

/// Response for `GET /api/genai/provider_mix`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ProviderMixResponse {
    /// How a model's tokens and cost were attributed to providers:
    /// "direct" (every model maps to exactly one provider in the window) or
    /// "token-share-split" (at least one model is served by several
    /// providers; its totals were split proportionally to that provider's
    /// share of the model's usage rows). Cost values are estimated from
    /// tokens × pricing in all cases — opencode's own cost counter is
    /// zero-valued in the wire data.
    pub method: String,
    /// Providers sorted by total tokens descending.
    pub providers: Vec<ProviderMixEntry>,
    /// Total tokens across all providers (the denominator of share_pct).
    pub total_tokens: u64,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Distribution of `gen_ai.request.temperature` values across LLM calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct TemperatureBucket {
    /// Rounded to 2 decimal places. None = attribute not set.
    pub temperature: Option<f64>,
    pub count: usize,
}

/// Distribution of `gen_ai.request.max_tokens` values across LLM calls.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct MaxTokensBucket {
    /// None = attribute not set.
    pub max_tokens: Option<i64>,
    pub count: usize,
}

/// Distribution of request parameter settings (temperature, max_tokens).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct RequestParamProfile {
    pub temperature_buckets: Vec<TemperatureBucket>,
    pub max_tokens_buckets: Vec<MaxTokensBucket>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Turn-count distribution across all observed conversations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ConversationDepthStats {
    pub total_conversations: usize,
    pub avg_turns: f64,
    pub p50_turns: i64,
    pub p95_turns: i64,
    pub p99_turns: i64,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Single time-bucket latency point, grouped by model (LLM mode) or span name (all-spans mode).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LatencySeriesPoint {
    /// Bucket start timestamp in nanoseconds since Unix epoch.
    pub timestamp: i64,
    /// Model name — set when `span_filter=llm` (default); null for `span_filter=all`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Span name — set when `span_filter=all`; null for `span_filter=llm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Number of spans in this bucket.
    pub count: usize,
    /// Number of error spans (status_code = 2) in this bucket.
    pub error_count: usize,
    /// Minimum span duration in milliseconds.
    pub min_ms: i64,
    /// Average span duration in milliseconds.
    pub avg_ms: f64,
    /// 95th-percentile span duration in milliseconds.
    pub p95_ms: i64,
    /// Maximum span duration in milliseconds.
    pub max_ms: i64,
    /// Average time-to-first-token in milliseconds (None when no spans carry TTFT data).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_ttft_ms: Option<f64>,
    /// 95th-percentile time-to-first-token in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p95_ttft_ms: Option<i64>,
    /// Number of valid TTFT values used for this bucket.
    #[serde(default)]
    pub ttft_count: usize,
    /// Number of emitted TTFT values that could not represent first-token latency.
    #[serde(default)]
    pub ttft_invalid_count: usize,
    /// Number of valid TTFT values at least 90% of complete request duration.
    #[serde(default)]
    pub ttft_degenerate_count: usize,
    /// True when at least 10 valid values exist and 90% are near complete duration.
    #[serde(default)]
    pub ttft_degenerate: bool,
    /// End-to-end output throughput p10 tok/s across throughput-eligible
    /// calls in this bucket (output tokens > 0 and duration > 0), computed
    /// per call from the raw nanosecond duration. `None` when no call in
    /// the bucket is eligible.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_p10_tok_s: Option<f64>,
    /// 50th percentile end-to-end output throughput in tokens/s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_p50_tok_s: Option<f64>,
    /// 90th percentile end-to-end output throughput in tokens/s.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_p90_tok_s: Option<f64>,
    /// Number of throughput-eligible calls (distinct from `count`).
    #[serde(default)]
    pub throughput_sample_count: usize,
}

/// One time bucket of the latency percentile series (issue #132).
/// Units are milliseconds, matching the other latency endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LatencyPercentilePoint {
    /// Bucket start timestamp in nanoseconds since Unix epoch.
    pub ts: i64,
    /// Bucket end timestamp in nanoseconds since Unix epoch; the bucket
    /// covers `[ts, end_ts)`. In calendar-day mode a DST day is 23 or 25
    /// hours, so `end_ts - ts` is not a fixed 86400 s. Absent from
    /// pre-#119 server responses.
    #[serde(default)]
    pub end_ts: i64,
    /// 10th percentile in milliseconds (lower tail, #119). Weak estimate
    /// when `count < 10`. `None` only for empty calendar-day buckets
    /// (`count == 0`); rolling buckets always have a value. Absent from
    /// pre-#119 server responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub p10_ms: Option<f64>,
    /// 50th percentile in milliseconds. `None` only for empty
    /// calendar-day buckets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p50_ms: Option<f64>,
    /// 90th percentile in milliseconds. `None` only for empty
    /// calendar-day buckets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p90_ms: Option<f64>,
    /// 95th percentile in milliseconds. `None` only for empty
    /// calendar-day buckets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p95_ms: Option<f64>,
    /// 99th percentile in milliseconds. `None` only for empty
    /// calendar-day buckets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p99_ms: Option<f64>,
    /// Number of values in this bucket. Zero only for empty calendar-day
    /// buckets.
    pub count: u64,
    /// Derived end-to-end output throughput percentiles in tokens/second,
    /// computed per call from raw nanosecond durations (never aggregate
    /// tokens / aggregate duration). `None` when the bucket has no
    /// throughput-eligible calls (output_tokens > 0 and duration > 0).
    /// Absent from pre-#119 server responses.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_p10_tok_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_p50_tok_s: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub throughput_p90_tok_s: Option<f64>,
    /// Calls in this bucket eligible for throughput. Separate from
    /// `count` — confidence warnings must use this.
    #[serde(default)]
    pub throughput_sample_count: u64,
}

/// Percentile series for one metric ("duration" or "ttft"): the "all"
/// cohort plus one series per model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LatencyPercentileSeries {
    /// Percentiles across every model, ascending by `ts`.
    pub all: Vec<LatencyPercentilePoint>,
    /// Per-model percentiles, ascending by `ts`.
    #[serde(default)]
    pub models: std::collections::BTreeMap<String, Vec<LatencyPercentilePoint>>,
}

/// `GET /api/genai/latency_percentiles` response. Keys of `metrics` are the
/// requested metric names ("duration", "ttft").
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LatencyPercentilesResponse {
    #[serde(default)]
    pub metrics: std::collections::BTreeMap<String, LatencyPercentileSeries>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// LLM latency broken down by input-token context size bin.
/// Useful for understanding whether larger prompts are slower.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct LatencyByContextBin {
    /// Human-readable bin label, e.g. "0–1K", "1K–10K", "100K+".
    pub bin: String,
    /// Inclusive lower bound of the bin in tokens.
    pub min_tokens: u64,
    /// Exclusive upper bound (u64::MAX for the open-ended last bin).
    pub max_tokens: u64,
    /// Model (None = spans without a model attribute).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub count: usize,
    pub avg_ms: f64,
    pub p95_ms: i64,
    pub max_ms: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avg_ttft_ms: Option<f64>,
    /// Number of valid TTFT values used for this bin.
    #[serde(default)]
    pub ttft_count: usize,
    /// Number of emitted TTFT values that could not represent first-token latency.
    #[serde(default)]
    pub ttft_invalid_count: usize,
    /// Number of valid TTFT values at least 90% of complete request duration.
    #[serde(default)]
    pub ttft_degenerate_count: usize,
    /// True when at least 10 valid values exist and 90% are near complete duration.
    #[serde(default)]
    pub ttft_degenerate: bool,
}

/// Single time-bucket point for calls-over-time series.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct CallsSeriesPoint {
    /// Bucket start timestamp in nanoseconds since Unix epoch.
    pub timestamp: i64,
    /// Model name — set when `span_filter=llm` (default); null for `span_filter=all`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Span name — set when `span_filter=all`; null for `span_filter=llm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Number of spans in this bucket.
    pub requests: usize,
}

/// Per-(model, error_type) breakdown of error spans, bucketed into actionable categories.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ErrorTypeBreakdown {
    pub model: Option<String>,
    /// Raw `error.type` value as observed (or exception.type / HTTP status code).
    pub error_type: String,
    /// Coarse actionable bucket: "rate_limit" | "timeout" | "context_length" |
    /// "content_filter" | "auth" | "server_error" | "unknown"
    pub bucket: String,
    pub count: usize,
}

/// A (request_model → response_model) pair that providers actually served.
/// `differs == true` means the provider silently rerouted to a different model snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ModelDriftPair {
    pub request_model: Option<String>,
    pub response_model: Option<String>,
    pub count: usize,
    /// True when both fields are non-null and differ from each other.
    pub differs: bool,
}

/// One LLM interaction within a session (derived from a single trace's root GenAI span).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionInteraction {
    pub index: usize,
    /// Wall-clock time of the interaction start (HH:MM:SS, server local time).
    pub time: String,
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    /// Tokens written to the prompt cache on this turn (expensive; indicates context just grew).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_tokens: Option<u64>,
    /// Time-to-first-token in seconds (None when not instrumented or non-streaming).
    pub ttft_secs: Option<f64>,
    pub duration_ms: i64,
    pub is_error: bool,
    /// True when a stream started (TTFT present) but the span ended in error after >30 s.
    pub is_stall: bool,
    pub response_id: Option<String>,
    pub trace_id: String,
    pub start_time_ns: i64,
    /// Body size in bytes from the `api_request_body` log (present on errored Claude Code spans).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_length: Option<u64>,
    /// LiteLLM proxy correlation ID from the `api_request_body` log (`prompt.id` attribute).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt_id: Option<String>,
}

/// Context-growth summary for a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionContextGrowth {
    pub first_tokens: u64,
    pub last_tokens: u64,
    pub peak_tokens: u64,
    pub interaction_count: usize,
}

/// Full session diagnose report returned by GET /api/sessions/:id/diagnose.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionDiagnoseResponse {
    pub session_id: String,
    pub models: Vec<String>,
    /// Formatted start time of the first interaction (RFC 3339).
    pub start_time: String,
    /// Formatted end time of the last interaction (RFC 3339).
    pub end_time: String,
    pub total_interactions: usize,
    pub error_count: usize,
    pub stall_count: usize,
    pub interactions: Vec<SessionInteraction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_growth: Option<SessionContextGrowth>,
}

/// One row in the response from GET /api/sessions — a summary line for the
/// Sessions tab list view.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionSummary {
    pub session_id: String,
    /// Models observed within this session (deduplicated).
    pub models: Vec<String>,
    pub interaction_count: usize,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub error_count: usize,
    /// First interaction start time (epoch ns).
    pub first_seen_ns: i64,
    /// Last interaction start time (epoch ns).
    pub last_seen_ns: i64,
    /// Project ids observed in this session (opencode data only).
    pub projects: Vec<String>,
    /// Provider identifiers observed in this session (gen_ai.system /
    /// gen_ai.provider.name / llm.provider).
    pub providers: Vec<String>,
    /// Agent families observed in this session: claude / opencode / codex.
    pub agent_families: Vec<String>,
}

/// Wrapper for paginated session lists from GET /api/sessions.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct SessionListResponse {
    pub sessions: Vec<SessionSummary>,
    pub total: usize,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// Summary counts for tool approval events (claude_code.tool.blocked_on_user spans).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolApprovalStats {
    /// Approved automatically by config (decision = "accept", source = "config").
    pub auto_accepted: usize,
    /// Approved interactively by the user.
    pub user_accepted: usize,
    /// Rejected by the user.
    pub rejected: usize,
    /// Decision unknown / yolo mode (no approval prompt was shown).
    pub unknown: usize,
    /// Total approval events.
    pub total: usize,
    /// Top tools that were explicitly rejected (tool_name + count).
    pub top_rejected: Vec<ToolApprovalEntry>,
    /// Filter dimensions the endpoint actually applied (global filter bar,
    /// #135). Empty when the endpoint accepts but does not apply filters.
    pub filters_applied: Vec<String>,
}

/// A single tool name with a count, used in approval stats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolApprovalEntry {
    pub tool_name: String,
    pub count: usize,
}

/// Distribution of LLM request stop / finish reasons (claude_code `stop_reason` field).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct StopReasonCount {
    /// The stop_reason value, e.g. "tool_use", "end_turn", "max_tokens".
    pub reason: String,
    pub count: usize,
}

/// LLM token usage broken down by request context type (e.g. "interaction" vs "sub-agent").
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ContextTypeSplit {
    /// Value of the `llm_request.context` attribute, or "(unknown)" when absent.
    pub context: String,
    pub calls: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub avg_ms: f64,
}

/// Aggregated error message for a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct ToolErrorEntry {
    pub tool_name: String,
    pub error_message: String,
    pub count: usize,
}

/// Hour-of-day usage bucket (0–23) with call counts.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
pub struct HourOfDayBucket {
    /// Hour of day in UTC, 0–23.
    pub hour: u8,
    pub llm_calls: usize,
    pub tool_calls: usize,
}

#[cfg(test)]
mod project_rollup_tests {
    use super::*;
    use crate::pricing::PricingDatabase;

    fn priced_db() -> PricingDatabase {
        // $2.5/M input, $10/M output.
        PricingDatabase::from_litellm_json(
            r#"{ "m1": { "input_cost_per_token": 2.5e-6, "output_cost_per_token": 1e-5 } }"#,
        )
        .unwrap()
    }

    fn usage(input: u64, output: u64) -> AgentTokenUsage {
        AgentTokenUsage {
            input,
            output,
            cache_read: 0,
            cache_write: 0,
            reasoning: 0,
        }
    }

    fn storage(
        project_id: &str,
        sessions: u64,
        counter: Option<f64>,
        disjoint: bool,
        models: Vec<(&str, AgentTokenUsage)>,
    ) -> ProjectRollupStorage {
        ProjectRollupStorage {
            project_id: project_id.to_string(),
            sessions,
            counter_cost_usd: counter,
            counter_disjoint_from_tokens: disjoint,
            models: models
                .into_iter()
                .map(|(m, t)| (m.to_string(), t))
                .collect(),
        }
    }

    #[test]
    fn enrich_actual_counter_wins_over_estimate() {
        let r = storage("p1", 4, Some(1.2), false, vec![("m1", usage(1_000, 100))])
            .enrich(&priced_db());
        assert_eq!(r.cost_usd, Some(1.2), "opencode's counter is authoritative");
        assert_eq!(r.cost_source.as_deref(), Some("actual"));
        assert_eq!(r.sessions, 4);
        assert_eq!(r.tokens.input, 1_000);
        // top models still priced individually
        assert_eq!(r.top_models.len(), 1);
        assert!(
            (r.top_models[0].cost_usd.unwrap() - (1000.0 * 2.5e-6 + 100.0 * 1e-5)).abs() < 1e-9
        );
    }

    #[test]
    fn enrich_estimated_when_no_counter() {
        let r = storage(
            "unattributed",
            2,
            None,
            true,
            vec![("m1", usage(1_000, 100))],
        )
        .enrich(&priced_db());
        // 1000 x 2.5e-6 + 100 x 1e-5 = 0.0025 + 0.001 = 0.0035
        assert!((r.cost_usd.unwrap() - 0.0035).abs() < 1e-9);
        assert_eq!(r.cost_source.as_deref(), Some("estimated"));
    }

    #[test]
    fn enrich_mixed_adds_disjoint_costs() {
        // counter covers label-less opencode sessions; models price
        // codex/claude tokens — disjoint, so they add.
        let r = storage(
            "unattributed",
            3,
            Some(0.5),
            true,
            vec![("m1", usage(1_000, 100))],
        )
        .enrich(&priced_db());
        assert!((r.cost_usd.unwrap() - (0.5 + 0.0035)).abs() < 1e-9);
        assert_eq!(r.cost_source.as_deref(), Some("mixed"));
    }

    #[test]
    fn enrich_zero_disjoint_counter_is_not_a_source() {
        let r = storage(
            "unattributed",
            1,
            Some(0.0),
            true,
            vec![("m1", usage(1_000, 0))],
        )
        .enrich(&priced_db());
        assert!((r.cost_usd.unwrap() - 0.0025).abs() < 1e-9);
        assert_eq!(
            r.cost_source.as_deref(),
            Some("estimated"),
            "a $0 counter must not upgrade the source to mixed"
        );
    }

    #[test]
    fn enrich_no_pricing_gives_null_cost() {
        let r = storage(
            "p1",
            1,
            None,
            false,
            vec![("unknown-model", usage(100, 100))],
        )
        .enrich(&PricingDatabase::empty());
        assert_eq!(r.cost_usd, None, "never a fabricated zero");
        assert_eq!(r.cost_source.as_deref(), Some("estimated"));
        assert_eq!(r.top_models[0].cost_usd, None);
    }

    #[test]
    fn enrich_unpriced_models_sort_last() {
        let r = storage(
            "p1",
            1,
            None,
            false,
            vec![("m1", usage(10, 0)), ("unknown-model", usage(9_999, 0))],
        )
        .enrich(&priced_db());
        assert_eq!(
            r.top_models
                .iter()
                .map(|m| m.model.as_str())
                .collect::<Vec<_>>(),
            vec!["m1", "unknown-model"],
            "priced model sorts ahead of a bigger unpriced one"
        );
    }

    #[test]
    fn enrich_caps_top_models_at_five() {
        let names: Vec<String> = (0..7).map(|i| format!("m{i}")).collect();
        let models: Vec<(&str, AgentTokenUsage)> =
            names.iter().map(|n| (n.as_str(), usage(1, 1))).collect();
        let r = storage("p1", 1, None, false, models).enrich(&priced_db());
        assert_eq!(r.top_models.len(), 5);
    }

    #[test]
    fn fold_tokens_sums_all_categories() {
        let mut a = AgentTokenUsage {
            input: 1,
            output: 2,
            cache_read: 3,
            cache_write: 4,
            reasoning: 5,
        };
        a.fold_tokens(usage(10, 20));
        assert_eq!(a.input, 11);
        assert_eq!(a.output, 22);
        assert_eq!(a.cache_read, 3);
        assert_eq!(a.cache_write, 4);
        assert_eq!(a.reasoning, 5);
    }

    #[test]
    fn test_log_entry_preserves_original_severity_text() {
        let mut record = crate::telemetry::LogRecord::new(
            crate::telemetry::log::SeverityLevel::Info,
            "severity-reproduction",
            1,
        );
        record.severity_text = Some("Information".to_string());

        let entry: LogEntry = record.into();

        assert_eq!(entry.severity, "INFO");
        assert_eq!(entry.severity_text.as_deref(), Some("Information"));
    }

    #[test]
    fn test_log_entry_derives_severity_text_when_absent() {
        let record = crate::telemetry::LogRecord::new(
            crate::telemetry::log::SeverityLevel::Warn,
            "no text",
            1,
        );

        let entry: LogEntry = record.into();

        assert_eq!(entry.severity, "WARN");
        assert_eq!(entry.severity_text.as_deref(), Some("WARN"));
    }
}
