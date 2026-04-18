//! API response models for Rotel backend

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Response for log listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsResponse {
    pub logs: Vec<LogEntry>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// Individual log entry from the backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Timestamp in nanoseconds since Unix epoch
    pub timestamp: i64,
    /// Severity level (TRACE, DEBUG, INFO, WARN, ERROR, FATAL)
    pub severity: String,
    /// Optional severity text
    pub severity_text: Option<String>,
    /// Log message body
    pub body: String,
    /// Additional attributes/metadata
    #[serde(default)]
    pub attributes: HashMap<String, String>,
    /// Resource information
    pub resource: Option<Resource>,
    /// Associated trace ID
    pub trace_id: Option<String>,
    /// Associated span ID
    pub span_id: Option<String>,
}

/// Resource information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Resource {
    pub attributes: HashMap<String, String>,
}

/// Response for trace listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TracesResponse {
    pub traces: Vec<TraceEntry>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// Individual trace entry (aggregated from spans)
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub status: Option<SpanStatus>,
    pub events: Vec<SpanEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanStatus {
    pub code: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: i64,
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

/// Response structure for a single metric
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricResponse {
    pub name: String,
    pub description: Option<String>,
    pub unit: Option<String>,
    pub metric_type: String,
    pub value: MetricValue,
    pub timestamp: i64,
    #[serde(default)]
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

impl LogEntry {
    /// Get a short display string for the log
    pub fn short_display(&self) -> String {
        use chrono::{DateTime, Utc};
        let dt = DateTime::<Utc>::from_timestamp_nanos(self.timestamp);
        format!(
            "{} [{}] {}",
            dt.format("%Y-%m-%d %H:%M:%S"),
            self.severity,
            self.body
        )
    }
}

impl TraceEntry {
    /// Get a short display string for the trace
    pub fn short_display(&self) -> String {
        let duration_ms = self.duration / 1_000_000;
        let status = if self.has_errors { "ERROR" } else { "OK" };
        format!("{} ({}ms) [{}]", self.root_span_name, duration_ms, status)
    }
}

impl SpanEntry {
    /// Build a tree structure of spans from a flat list
    pub fn build_span_tree(spans: &[SpanEntry]) -> Vec<SpanNode> {
        let mut nodes: HashMap<String, SpanNode> = HashMap::new();
        let mut root_nodes = Vec::new();

        // Create nodes for all spans
        for span in spans {
            nodes.insert(
                span.span_id.clone(),
                SpanNode {
                    span: span.clone(),
                    children: Vec::new(),
                },
            );
        }

        // Build tree structure - collect parent-child relationships first
        let mut parent_child_map: HashMap<String, Vec<String>> = HashMap::new();
        for span in spans {
            if let Some(parent_id) = &span.parent_span_id {
                parent_child_map
                    .entry(parent_id.clone())
                    .or_default()
                    .push(span.span_id.clone());
            }
        }

        // Now build the tree by adding children to parents
        for (parent_id, child_ids) in parent_child_map {
            // First collect all child nodes
            let children: Vec<SpanNode> = child_ids
                .iter()
                .filter_map(|child_id| nodes.get(child_id).cloned())
                .collect();

            // Then add them to the parent
            if let Some(parent_node) = nodes.get_mut(&parent_id) {
                parent_node.children.extend(children);
            }
        }

        // Collect root nodes (spans without parents)
        for span in spans {
            if span.parent_span_id.is_none() {
                if let Some(node) = nodes.get(&span.span_id).cloned() {
                    root_nodes.push(node);
                }
            }
        }

        root_nodes
    }
}

/// Span node in a tree structure
#[derive(Debug, Clone)]
pub struct SpanNode {
    pub span: SpanEntry,
    pub children: Vec<SpanNode>,
}

impl MetricResponse {
    /// Get a short display string for the metric
    pub fn short_display(&self) -> String {
        let value_str = match &self.value {
            MetricValue::Gauge(v) => format!("{}", v),
            MetricValue::Counter(v) => format!("{}", v),
            MetricValue::Histogram { count, sum, .. } => format!("count={}, sum={}", count, sum),
            MetricValue::Summary { count, sum, .. } => format!("count={}, sum={}", count, sum),
        };
        format!("{} = {} ({})", self.name, value_str, self.metric_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_short_display() {
        let log = LogEntry {
            timestamp: 1_000_000_000_000_000_000, // 2001-09-09 01:46:40 UTC
            severity: "ERROR".to_string(),
            severity_text: None,
            body: "Test error".to_string(),
            attributes: HashMap::new(),
            resource: None,
            trace_id: None,
            span_id: None,
        };
        let display = log.short_display();
        assert!(display.contains("ERROR"));
        assert!(display.contains("Test error"));
    }

    #[test]
    fn test_trace_entry_short_display() {
        let trace = TraceEntry {
            trace_id: "trace-001".to_string(),
            root_span_name: "http-request".to_string(),
            start_time: 1_000_000_000_000_000_000,
            duration: 1_500_000_000, // 1.5 seconds in nanoseconds
            span_count: 5,
            service_names: vec!["api".to_string()],
            has_errors: false,
        };
        let display = trace.short_display();
        assert!(display.contains("http-request"));
        assert!(display.contains("1500ms"));
        assert!(display.contains("OK"));
    }

    #[test]
    fn test_metric_response_short_display() {
        let metric = MetricResponse {
            name: "http_requests_total".to_string(),
            description: None,
            unit: None,
            metric_type: "counter".to_string(),
            value: MetricValue::Counter(1234),
            timestamp: 1_000_000_000_000_000_000,
            attributes: HashMap::new(),
            resource: None,
        };
        let display = metric.short_display();
        assert!(display.contains("http_requests_total"));
        assert!(display.contains("1234"));
        assert!(display.contains("counter"));
    }
}

// Made with Bob
