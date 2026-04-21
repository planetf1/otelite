//! Shared API response types for Rotel
//!
//! This module defines the canonical API response structures used across
//! rotel-server, rotel-cli, and rotel-tui. All types derive both Serialize
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
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    pub resource: Option<Resource>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
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
        Self {
            timestamp: log.timestamp,
            severity: log.severity.as_str().to_string(),
            severity_text: Some(log.severity.as_str().to_string()),
            body: log.body,
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
    /// Model name (e.g., "gpt-4", "claude-sonnet-4-20250514")
    pub model: String,
    /// Input tokens for this model
    pub input_tokens: u64,
    /// Output tokens for this model
    pub output_tokens: u64,
    /// Number of requests for this model
    pub requests: usize,
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
