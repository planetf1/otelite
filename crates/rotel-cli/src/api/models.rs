//! API response models for Rotel backend
//!
//! Re-exports shared types from rotel-core.

// Re-export shared API types from rotel-core
pub use rotel_core::api::{
    HistogramBucket, LogEntry, LogsResponse, MetricResponse, MetricValue, Quantile, Resource,
    SpanEntry, SpanEvent, SpanNode, SpanStatus, TraceDetail, TraceEntry, TracesResponse,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

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
