//! Trace-specific models

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use utoipa::{IntoParams, ToSchema};
use validator::Validate;

use super::response::ResourceAttributes;

/// Trace span representation
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct TraceSpan {
    /// Unique span identifier
    pub span_id: String,

    /// Trace identifier this span belongs to
    pub trace_id: String,

    /// Parent span identifier (None for root spans)
    pub parent_span_id: Option<String>,

    /// Span name/operation
    pub name: String,

    /// Span kind (INTERNAL, SERVER, CLIENT, PRODUCER, CONSUMER)
    pub kind: String,

    /// Start time (Unix timestamp in nanoseconds)
    pub start_time: i64,

    /// End time (Unix timestamp in nanoseconds)
    pub end_time: i64,

    /// Duration in nanoseconds (computed from end_time - start_time)
    pub duration_ns: i64,

    /// Span status (OK, ERROR, UNSET)
    pub status: SpanStatus,

    /// Resource attributes (service info)
    pub resource: ResourceAttributes,

    /// Span attributes (key-value pairs)
    pub attributes: HashMap<String, String>,

    /// Span events
    pub events: Vec<SpanEvent>,

    /// Links to other spans
    pub links: Vec<SpanLink>,
}

/// Span status
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SpanStatus {
    /// Status code (OK, ERROR, UNSET)
    pub code: String,

    /// Optional status message
    pub message: Option<String>,
}

/// Span event
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SpanEvent {
    /// Event name
    pub name: String,

    /// Event timestamp (Unix timestamp in nanoseconds)
    pub timestamp: i64,

    /// Event attributes
    pub attributes: HashMap<String, String>,
}

/// Span link to another span
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SpanLink {
    /// Linked trace ID
    pub trace_id: String,

    /// Linked span ID
    pub span_id: String,

    /// Link attributes
    pub attributes: HashMap<String, String>,
}

/// Trace query parameters
#[derive(Debug, Clone, Deserialize, Serialize, Validate, IntoParams, ToSchema)]
pub struct TraceQueryParams {
    /// Maximum number of traces to return (default: 100, max: 1000)
    #[validate(range(min = 1, max = 1000))]
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Offset for pagination (default: 0)
    #[serde(default)]
    pub offset: usize,

    /// Filter by service name
    pub service_name: Option<String>,

    /// Filter by span name
    pub span_name: Option<String>,

    /// Filter by minimum duration (nanoseconds)
    pub min_duration_ns: Option<i64>,

    /// Filter by maximum duration (nanoseconds)
    pub max_duration_ns: Option<i64>,

    /// Filter by status (OK, ERROR, UNSET)
    pub status: Option<String>,

    /// Start time filter (Unix timestamp in milliseconds)
    pub start_time: Option<i64>,

    /// End time filter (Unix timestamp in milliseconds)
    pub end_time: Option<i64>,

    /// Relative time range (e.g., "1h", "30m", "7d")
    pub since: Option<String>,
}

fn default_limit() -> usize {
    100
}

impl Default for TraceQueryParams {
    fn default() -> Self {
        Self {
            limit: default_limit(),
            offset: 0,
            service_name: None,
            span_name: None,
            min_duration_ns: None,
            max_duration_ns: None,
            status: None,
            start_time: None,
            end_time: None,
            since: None,
        }
    }
}

/// Complete trace with all spans
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct Trace {
    /// Trace identifier
    pub trace_id: String,

    /// Root span (entry point)
    pub root_span: TraceSpan,

    /// All spans in the trace
    pub spans: Vec<TraceSpan>,

    /// Total duration (from root span)
    pub duration_ns: i64,

    /// Number of spans in trace
    pub span_count: usize,

    /// Trace start time (from earliest span)
    pub start_time: i64,

    /// Trace end time (from latest span)
    pub end_time: i64,
}

impl Trace {
    /// Build span hierarchy from flat list of spans
    pub fn build_hierarchy(spans: Vec<TraceSpan>) -> Option<Self> {
        if spans.is_empty() {
            return None;
        }

        // Find root span (no parent)
        let root_span = spans.iter().find(|s| s.parent_span_id.is_none()).cloned()?;

        let trace_id = root_span.trace_id.clone();
        let span_count = spans.len();

        // Calculate trace bounds
        let start_time = spans.iter().map(|s| s.start_time).min().unwrap_or(0);
        let end_time = spans.iter().map(|s| s.end_time).max().unwrap_or(0);
        let duration_ns = end_time - start_time;

        Some(Trace {
            trace_id,
            root_span,
            spans,
            duration_ns,
            span_count,
            start_time,
            end_time,
        })
    }
}

// Made with Bob
