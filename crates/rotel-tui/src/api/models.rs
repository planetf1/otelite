use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Log entry from the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: i64,
    pub severity: String,
    pub severity_text: Option<String>,
    pub body: String,
    pub attributes: HashMap<String, String>,
    pub resource: Option<Resource>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

/// Trace summary from the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceSummary {
    pub trace_id: String,
    pub root_span_name: String,
    pub start_time: i64,
    pub duration: i64,
    pub span_count: usize,
    pub has_errors: bool,
    pub service_names: Vec<String>,
}

/// Full trace with spans
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    pub trace_id: String,
    pub spans: Vec<Span>,
    pub start_time: i64,
    pub end_time: i64,
    pub duration: i64,
    pub span_count: usize,
    pub service_names: Vec<String>,
}

/// Span within a trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    pub span_id: String,
    pub trace_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub kind: String,
    pub start_time: i64,
    pub end_time: i64,
    pub duration: i64,
    pub attributes: HashMap<String, String>,
    pub resource: Option<Resource>,
    pub status: Option<SpanStatus>,
    pub events: Vec<SpanEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanStatus {
    pub code: String,
    pub message: Option<String>,
}

/// Span event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: i64,
    pub attributes: HashMap<String, String>,
}

/// Metric from the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub description: Option<String>,
    pub unit: Option<String>,
    pub metric_type: String,
    pub value: MetricValue,
    pub timestamp: i64,
    pub attributes: HashMap<String, String>,
    pub resource: Option<HashMap<String, String>>,
}

/// Metric value (varies by type)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MetricValue {
    Gauge(f64),
    Counter(u64),
    Histogram {
        count: u64,
        sum: f64,
        buckets: Vec<HistogramBucket>,
    },
    Summary {
        count: u64,
        sum: f64,
        quantiles: Vec<Quantile>,
    },
}

/// Histogram bucket
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistogramBucket {
    pub upper_bound: f64,
    pub count: u64,
}

/// Summary quantile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Quantile {
    pub quantile: f64,
    pub value: f64,
}

/// Resource information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub attributes: HashMap<String, String>,
}

/// API response for logs list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsResponse {
    pub logs: Vec<LogEntry>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// API response for traces list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracesResponse {
    pub traces: Vec<TraceSummary>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// API response for metrics list (just a Vec, no wrapper in dashboard API)
pub type MetricsResponse = Vec<Metric>;

/// Query parameters for logs
#[derive(Debug, Clone, Serialize)]
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
#[derive(Debug, Clone, Serialize)]
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

// Made with Bob
