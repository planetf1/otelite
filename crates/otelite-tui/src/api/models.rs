//! API response models for Otelite TUI
//!
//! Re-exports shared types from otelite-core with TUI-specific aliases and helpers.

// Re-export shared API types from otelite-core
pub use otelite_core::api::{
    LogEntry, LogsResponse, MetricResponse, MetricValue, SpanEntry, TraceDetail, TraceEntry,
    TracesResponse,
};

// Re-export types used in tests
#[cfg(test)]
pub use otelite_core::api::{
    HistogramBucket, HistogramValue, Quantile, Resource, SpanStatus, SummaryValue,
};

// Type aliases for backward compatibility with TUI code
pub type TraceSummary = TraceEntry;
pub type Trace = TraceDetail;
pub type Span = SpanEntry;
pub type Metric = MetricResponse;
pub type MetricsResponse = Vec<MetricResponse>;

/// Query parameters for logs
#[derive(Debug, Clone, serde::Serialize)]
pub struct LogsQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

impl Default for LogsQuery {
    fn default() -> Self {
        Self {
            severity: None,
            search: None,
            start_time: None,
            end_time: None,
            limit: Some(100),
            offset: Some(0),
        }
    }
}

/// Query parameters for traces
#[derive(Debug, Clone, serde::Serialize)]
pub struct TracesQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_duration: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_duration: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub has_errors: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub end_time: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub offset: Option<usize>,
}

impl Default for TracesQuery {
    fn default() -> Self {
        Self {
            min_duration: None,
            max_duration: None,
            has_errors: None,
            start_time: None,
            end_time: None,
            limit: Some(100),
            offset: Some(0),
        }
    }
}
