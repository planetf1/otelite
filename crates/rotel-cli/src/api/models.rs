//! API response models for Rotel backend

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Log entry from the backend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    /// Unique log identifier
    pub id: String,
    /// Timestamp when the log was created
    pub timestamp: DateTime<Utc>,
    /// Severity level (DEBUG, INFO, WARN, ERROR)
    pub severity: String,
    /// Log message content
    pub message: String,
    /// Additional attributes/metadata
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

/// Trace with spans
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Trace {
    /// Unique trace identifier
    pub id: String,
    /// Root span name
    pub root_span: String,
    /// Total duration in milliseconds
    pub duration_ms: u64,
    /// Trace status (OK, ERROR)
    pub status: String,
    /// All spans in the trace
    pub spans: Vec<Span>,
}

/// Individual span within a trace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Span {
    /// Unique span identifier
    pub id: String,
    /// Span name/operation
    pub name: String,
    /// Parent span ID (None for root span)
    pub parent_id: Option<String>,
    /// Span start time
    pub start_time: DateTime<Utc>,
    /// Span duration in milliseconds
    pub duration_ms: u64,
    /// Span attributes/metadata
    #[serde(default)]
    pub attributes: HashMap<String, String>,
}

/// Metric data point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Metric {
    /// Metric name
    pub name: String,
    /// Metric type (counter, gauge, histogram, summary)
    #[serde(rename = "type")]
    pub type_: String,
    /// Metric value
    pub value: f64,
    /// Timestamp when the metric was recorded
    pub timestamp: DateTime<Utc>,
    /// Metric labels/dimensions
    #[serde(default)]
    pub labels: HashMap<String, String>,
    /// Histogram percentiles (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub percentiles: Option<HashMap<String, f64>>,
}

impl LogEntry {
    /// Get a short display string for the log
    pub fn short_display(&self) -> String {
        format!(
            "{} [{}] {}",
            self.timestamp.format("%Y-%m-%d %H:%M:%S"),
            self.severity,
            self.message
        )
    }
}

impl Trace {
    /// Get a short display string for the trace
    pub fn short_display(&self) -> String {
        format!(
            "{} ({}ms) [{}]",
            self.root_span, self.duration_ms, self.status
        )
    }

    /// Build a tree structure of spans
    pub fn build_span_tree(&self) -> Vec<SpanNode> {
        let mut nodes: HashMap<String, SpanNode> = HashMap::new();
        let mut root_nodes = Vec::new();

        // Create nodes for all spans
        for span in &self.spans {
            nodes.insert(
                span.id.clone(),
                SpanNode {
                    span: span.clone(),
                    children: Vec::new(),
                },
            );
        }

        // Build tree structure - collect parent-child relationships first
        let mut parent_child_map: HashMap<String, Vec<String>> = HashMap::new();
        for span in &self.spans {
            if let Some(parent_id) = &span.parent_id {
                parent_child_map
                    .entry(parent_id.clone())
                    .or_default()
                    .push(span.id.clone());
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
        for span in &self.spans {
            if span.parent_id.is_none() {
                if let Some(node) = nodes.get(&span.id).cloned() {
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
    pub span: Span,
    pub children: Vec<SpanNode>,
}

impl Metric {
    /// Get a short display string for the metric
    pub fn short_display(&self) -> String {
        format!("{} = {} ({})", self.name, self.value, self.type_)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_entry_short_display() {
        let log = LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "ERROR".to_string(),
            message: "Test error".to_string(),
            attributes: HashMap::new(),
        };
        let display = log.short_display();
        assert!(display.contains("ERROR"));
        assert!(display.contains("Test error"));
    }

    #[test]
    fn test_trace_short_display() {
        let trace = Trace {
            id: "trace-001".to_string(),
            root_span: "http-request".to_string(),
            duration_ms: 1500,
            status: "OK".to_string(),
            spans: Vec::new(),
        };
        let display = trace.short_display();
        assert!(display.contains("http-request"));
        assert!(display.contains("1500ms"));
        assert!(display.contains("OK"));
    }

    #[test]
    fn test_metric_short_display() {
        let metric = Metric {
            name: "http_requests_total".to_string(),
            type_: "counter".to_string(),
            value: 1234.0,
            timestamp: Utc::now(),
            labels: HashMap::new(),
            percentiles: None,
        };
        let display = metric.short_display();
        assert!(display.contains("http_requests_total"));
        assert!(display.contains("1234"));
        assert!(display.contains("counter"));
    }
}

// Made with Bob
