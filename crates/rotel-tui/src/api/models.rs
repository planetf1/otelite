use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Log entry from the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub id: String,
    pub timestamp: i64,
    pub severity: String,
    pub body: String,
    pub attributes: HashMap<String, String>,
    pub resource: Resource,
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
    pub root_span_name: String,
    pub start_time: i64,
    pub duration: i64,
    pub span_count: usize,
    pub has_errors: bool,
    pub service_names: Vec<String>,
    pub spans: Vec<Span>,
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
    pub status: String,
    pub attributes: HashMap<String, String>,
    pub events: Vec<SpanEvent>,
    pub links: Vec<SpanLink>,
}

/// Span event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: i64,
    pub attributes: HashMap<String, String>,
}

/// Span link
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanLink {
    pub trace_id: String,
    pub span_id: String,
    pub attributes: HashMap<String, String>,
}

/// Metric from the API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    pub name: String,
    pub description: Option<String>,
    pub unit: Option<String>,
    pub metric_type: String,
    pub data_points: Vec<DataPoint>,
}

/// Metric data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataPoint {
    pub timestamp: i64,
    pub value: f64,
    pub attributes: HashMap<String, String>,
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
}

/// API response for traces list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracesResponse {
    pub traces: Vec<TraceSummary>,
    pub total: usize,
}

/// API response for metrics list
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsResponse {
    pub metrics: Vec<Metric>,
    pub total: usize,
}

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
