//! JSON output formatting for CLI

use crate::api::models::{LogEntry, Metric, Trace};
use crate::error::Result;
use serde_json;

/// Print logs as JSON array
pub fn print_logs_json(logs: &[LogEntry]) -> Result<()> {
    let json = serde_json::to_string_pretty(logs)?;
    println!("{}", json);
    Ok(())
}

/// Print a single log as JSON object
pub fn print_log_json(log: &LogEntry) -> Result<()> {
    let json = serde_json::to_string_pretty(log)?;
    println!("{}", json);
    Ok(())
}

/// Print traces as JSON array
pub fn print_traces_json(traces: &[Trace]) -> Result<()> {
    let json = serde_json::to_string_pretty(traces)?;
    println!("{}", json);
    Ok(())
}

/// Print a single trace as JSON object
pub fn print_trace_json(trace: &Trace) -> Result<()> {
    let json = serde_json::to_string_pretty(trace)?;
    println!("{}", json);
    Ok(())
}

/// Print metrics as JSON array
pub fn print_metrics_json(metrics: &[Metric]) -> Result<()> {
    let json = serde_json::to_string_pretty(metrics)?;
    println!("{}", json);
    Ok(())
}

/// Print a single metric as JSON object
pub fn print_metric_json(metric: &Metric) -> Result<()> {
    let json = serde_json::to_string_pretty(metric)?;
    println!("{}", json);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn test_print_logs_json() {
        let logs = vec![LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "ERROR".to_string(),
            message: "Test error".to_string(),
            attributes: HashMap::new(),
        }];
        let result = print_logs_json(&logs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_log_json() {
        let log = LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "ERROR".to_string(),
            message: "Test error".to_string(),
            attributes: HashMap::new(),
        };
        let result = print_log_json(&log);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_traces_json() {
        let traces = vec![Trace {
            id: "trace-001".to_string(),
            root_span: "http-request".to_string(),
            duration_ms: 1500,
            status: "OK".to_string(),
            spans: vec![],
        }];
        let result = print_traces_json(&traces);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_metrics_json() {
        let metrics = vec![Metric {
            name: "http_requests_total".to_string(),
            type_: "counter".to_string(),
            value: 1234.0,
            timestamp: Utc::now(),
            labels: HashMap::new(),
            percentiles: None,
        }];
        let result = print_metrics_json(&metrics);
        assert!(result.is_ok());
    }

    // T018: Unit test for logs JSON formatter
    #[test]
    fn test_json_is_valid() {
        let log = LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "ERROR".to_string(),
            message: "Test error".to_string(),
            attributes: HashMap::new(),
        };
        let json = serde_json::to_string(&log).unwrap();
        // Verify it can be parsed back
        let parsed: LogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, log.id);
    }

    #[test]
    fn test_print_logs_json_multiple() {
        let logs = vec![
            LogEntry {
                id: "log-001".to_string(),
                timestamp: Utc::now(),
                severity: "ERROR".to_string(),
                message: "Error message".to_string(),
                attributes: HashMap::new(),
            },
            LogEntry {
                id: "log-002".to_string(),
                timestamp: Utc::now(),
                severity: "INFO".to_string(),
                message: "Info message".to_string(),
                attributes: HashMap::new(),
            },
        ];
        let result = print_logs_json(&logs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_log_json_with_attributes() {
        let mut attributes = HashMap::new();
        attributes.insert("user_id".to_string(), "12345".to_string());
        attributes.insert("request_id".to_string(), "abc-def".to_string());

        let log = LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "ERROR".to_string(),
            message: "Error with attributes".to_string(),
            attributes,
        };
        let result = print_log_json(&log);
        assert!(result.is_ok());
    }

    #[test]
    fn test_json_output_is_parseable() {
        let logs = vec![LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "ERROR".to_string(),
            message: "Test error".to_string(),
            attributes: HashMap::new(),
        }];

        // Serialize to JSON string
        let json_str = serde_json::to_string(&logs).unwrap();

        // Verify it can be parsed back
        let parsed: Vec<LogEntry> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].id, "log-001");
        assert_eq!(parsed[0].severity, "ERROR");
    }

    #[test]
    fn test_json_empty_logs() {
        let logs: Vec<LogEntry> = vec![];
        let result = print_logs_json(&logs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_json_special_characters() {
        let log = LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "ERROR".to_string(),
            message: r#"Message with "quotes" and \backslashes\ and newlines\n"#.to_string(),
            attributes: HashMap::new(),
        };
        let result = print_log_json(&log);
        assert!(result.is_ok());

        // Verify JSON is valid and can be parsed
        let json_str = serde_json::to_string(&log).unwrap();
        let parsed: LogEntry = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.message, log.message);
    }

    // T042: Unit test for traces JSON formatter
    #[test]
    fn test_print_traces_json_with_spans() {
        use crate::api::models::Span;

        let traces = vec![Trace {
            id: "trace-001".to_string(),
            root_span: "http-request".to_string(),
            duration_ms: 1500,
            status: "OK".to_string(),
            spans: vec![Span {
                id: "span-001".to_string(),
                name: "http-request".to_string(),
                parent_id: None,
                start_time: Utc::now(),
                duration_ms: 1500,
                attributes: HashMap::new(),
            }],
        }];
        let result = print_traces_json(&traces);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_trace_json() {
        use crate::api::models::Span;

        let trace = Trace {
            id: "trace-001".to_string(),
            root_span: "http-request".to_string(),
            duration_ms: 1500,
            status: "OK".to_string(),
            spans: vec![Span {
                id: "span-001".to_string(),
                name: "http-request".to_string(),
                parent_id: None,
                start_time: Utc::now(),
                duration_ms: 1500,
                attributes: HashMap::new(),
            }],
        };
        let result = print_trace_json(&trace);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_trace_json_with_hierarchy() {
        use crate::api::models::Span;

        let now = Utc::now();
        let trace = Trace {
            id: "trace-001".to_string(),
            root_span: "http-request".to_string(),
            duration_ms: 1500,
            status: "OK".to_string(),
            spans: vec![
                Span {
                    id: "span-001".to_string(),
                    name: "http-request".to_string(),
                    parent_id: None,
                    start_time: now,
                    duration_ms: 1500,
                    attributes: HashMap::new(),
                },
                Span {
                    id: "span-002".to_string(),
                    name: "database-query".to_string(),
                    parent_id: Some("span-001".to_string()),
                    start_time: now,
                    duration_ms: 250,
                    attributes: HashMap::new(),
                },
            ],
        };
        let result = print_trace_json(&trace);
        assert!(result.is_ok());

        // Verify JSON is valid and can be parsed
        let json_str = serde_json::to_string(&trace).unwrap();
        let parsed: Trace = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.id, trace.id);
        assert_eq!(parsed.spans.len(), 2);
    }

    #[test]
    fn test_print_traces_json_empty() {
        let traces: Vec<Trace> = vec![];
        let result = print_traces_json(&traces);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_trace_json_with_attributes() {
        use crate::api::models::Span;

        let mut attributes = HashMap::new();
        attributes.insert("http.method".to_string(), "GET".to_string());
        attributes.insert("http.url".to_string(), "/api/users".to_string());

        let trace = Trace {
            id: "trace-001".to_string(),
            root_span: "http-request".to_string(),
            duration_ms: 1500,
            status: "OK".to_string(),
            spans: vec![Span {
                id: "span-001".to_string(),
                name: "http-request".to_string(),
                parent_id: None,
                start_time: Utc::now(),
                duration_ms: 1500,
                attributes,
            }],
        };
        let result = print_trace_json(&trace);
        assert!(result.is_ok());

        // Verify attributes are preserved in JSON
        let json_str = serde_json::to_string(&trace).unwrap();
        let parsed: Trace = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.spans[0].attributes.len(), 2);
    }
}

// Made with Bob
