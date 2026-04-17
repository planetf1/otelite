//! Pretty-print table formatting for CLI output

use crate::api::models::{LogEntry, Metric, SpanNode, Trace};
use comfy_table::{presets::UTF8_FULL, Cell, Color, ContentArrangement, Table};

/// Print logs in a pretty table format
pub fn print_logs_table(logs: &[LogEntry], no_color: bool, no_header: bool) {
    if logs.is_empty() {
        println!("No logs found");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Add header (unless disabled)
    if !no_header {
        table.set_header(vec!["ID", "Timestamp", "Severity", "Message"]);
    }

    // Add rows
    for log in logs {
        let severity_cell = if no_color {
            Cell::new(&log.severity)
        } else {
            let color = match log.severity.as_str() {
                "ERROR" => Color::Red,
                "WARN" => Color::Yellow,
                "INFO" => Color::Blue,
                "DEBUG" => Color::DarkGrey,
                _ => Color::Reset,
            };
            Cell::new(&log.severity).fg(color)
        };

        table.add_row(vec![
            Cell::new(&log.id),
            Cell::new(log.timestamp.format("%Y-%m-%d %H:%M:%S").to_string()),
            severity_cell,
            Cell::new(&log.message),
        ]);
    }

    println!("{}", table);
}

/// Print a single log entry with full details
pub fn print_log_details(log: &LogEntry, no_color: bool) {
    let severity_color = if no_color {
        ""
    } else {
        match log.severity.as_str() {
            "ERROR" => "\x1b[31m",
            "WARN" => "\x1b[33m",
            "INFO" => "\x1b[34m",
            "DEBUG" => "\x1b[90m",
            _ => "",
        }
    };
    let reset = if no_color { "" } else { "\x1b[0m" };

    println!("ID:        {}", log.id);
    println!("Timestamp: {}", log.timestamp.format("%Y-%m-%d %H:%M:%S"));
    println!("Severity:  {}{}{}", severity_color, log.severity, reset);
    println!("Message:   {}", log.message);

    if !log.attributes.is_empty() {
        println!("\nAttributes:");
        for (key, value) in &log.attributes {
            println!("  {}: {}", key, value);
        }
    }
}

/// Print traces in a pretty table format
pub fn print_traces_table(traces: &[Trace], no_color: bool, no_header: bool) {
    if traces.is_empty() {
        println!("No traces found");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Add header (unless disabled)
    if !no_header {
        table.set_header(vec!["ID", "Root Span", "Duration", "Status", "Spans"]);
    }

    // Add rows
    for trace in traces {
        let status_cell = if no_color {
            Cell::new(&trace.status)
        } else {
            let color = match trace.status.as_str() {
                "ERROR" => Color::Red,
                "OK" => Color::Green,
                _ => Color::Reset,
            };
            Cell::new(&trace.status).fg(color)
        };

        table.add_row(vec![
            Cell::new(&trace.id),
            Cell::new(&trace.root_span),
            Cell::new(format!("{}ms", trace.duration_ms)),
            status_cell,
            Cell::new(trace.spans.len().to_string()),
        ]);
    }

    println!("{}", table);
}

/// Print a trace with span tree
pub fn print_trace_tree(trace: &Trace, _no_color: bool) {
    println!("Trace ID: {}", trace.id);
    println!("Duration: {}ms", trace.duration_ms);
    println!("Status:   {}", trace.status);
    println!("\nSpans:");

    let tree = trace.build_span_tree();
    for node in &tree {
        print_span_node(node, 0);
    }
}

fn print_span_node(node: &SpanNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let prefix = if depth > 0 { "├─ " } else { "" };

    println!(
        "{}{}{} ({}ms)",
        indent, prefix, node.span.name, node.span.duration_ms
    );

    for child in &node.children {
        print_span_node(child, depth + 1);
    }
}

/// Print metrics in a pretty table format
pub fn print_metrics_table(metrics: &[Metric], no_color: bool, no_header: bool) {
    if metrics.is_empty() {
        println!("No metrics found");
        return;
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Add header (unless disabled)
    if !no_header {
        table.set_header(vec!["Name", "Type", "Value", "Timestamp"]);
    }

    // Add rows
    for metric in metrics {
        let type_cell = if no_color {
            Cell::new(&metric.type_)
        } else {
            let color = match metric.type_.as_str() {
                "counter" => Color::Green,
                "gauge" => Color::Blue,
                "histogram" => Color::Yellow,
                "summary" => Color::Cyan,
                _ => Color::Reset,
            };
            Cell::new(&metric.type_).fg(color)
        };

        table.add_row(vec![
            Cell::new(&metric.name),
            type_cell,
            Cell::new(format!("{:.2}", metric.value)),
            Cell::new(metric.timestamp.format("%Y-%m-%d %H:%M:%S").to_string()),
        ]);
    }

    println!("{}", table);
}

/// Print a single metric with full details including percentiles
pub fn print_metric_details(metric: &Metric, _no_color: bool) {
    println!("Name:      {}", metric.name);
    println!("Type:      {}", metric.type_);
    println!("Value:     {:.2}", metric.value);
    println!(
        "Timestamp: {}",
        metric.timestamp.format("%Y-%m-%d %H:%M:%S")
    );

    if !metric.labels.is_empty() {
        println!("\nLabels:");
        for (key, value) in &metric.labels {
            println!("  {}: {}", key, value);
        }
    }

    if let Some(percentiles) = &metric.percentiles {
        println!("\nPercentiles:");
        for (key, value) in percentiles {
            println!("  {}: {:.4}", key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::collections::HashMap;

    #[test]
    fn test_print_logs_table_empty() {
        let logs: Vec<LogEntry> = vec![];
        // Should not panic
        print_logs_table(&logs, true, false);
    }

    #[test]
    fn test_print_traces_table_empty() {
        let traces: Vec<Trace> = vec![];
        // Should not panic
        print_traces_table(&traces, true, false);
    }

    #[test]
    fn test_print_metrics_table_empty() {
        let metrics: Vec<Metric> = vec![];
        // Should not panic
        print_metrics_table(&metrics, true, false);
    }

    // T017: Unit test for logs pretty-print formatter
    #[test]
    fn test_print_log_details() {
        let log = LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "ERROR".to_string(),
            message: "Test error".to_string(),
            attributes: HashMap::new(),
        };
        // Should not panic
        print_log_details(&log, true);
    }

    #[test]
    fn test_print_logs_table_with_data() {
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
            LogEntry {
                id: "log-003".to_string(),
                timestamp: Utc::now(),
                severity: "WARN".to_string(),
                message: "Warning message".to_string(),
                attributes: HashMap::new(),
            },
        ];
        // Should not panic and should handle different severity levels
        print_logs_table(&logs, true, false);
        print_logs_table(&logs, false, false); // Test without colors
    }

    #[test]
    fn test_print_logs_table_severity_colors() {
        // Test that different severities are handled
        let severities = vec!["ERROR", "WARN", "INFO", "DEBUG", "TRACE"];
        for severity in severities {
            let logs = vec![LogEntry {
                id: format!("log-{}", severity),
                timestamp: Utc::now(),
                severity: severity.to_string(),
                message: format!("{} message", severity),
                attributes: HashMap::new(),
            }];
            // Should not panic for any severity level
            print_logs_table(&logs, true, false);
        }
    }

    #[test]
    fn test_print_log_details_with_attributes() {
        let mut attributes = HashMap::new();
        attributes.insert("user_id".to_string(), "12345".to_string());
        attributes.insert("request_id".to_string(), "abc-def-ghi".to_string());

        let log = LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "ERROR".to_string(),
            message: "Test error with attributes".to_string(),
            attributes,
        };
        // Should not panic and should display attributes
        print_log_details(&log, true);
    }

    #[test]
    fn test_print_logs_table_long_messages() {
        let logs = vec![LogEntry {
            id: "log-001".to_string(),
            timestamp: Utc::now(),
            severity: "INFO".to_string(),
            message: "This is a very long message that should be truncated in the table view to ensure the table remains readable and doesn't overflow the terminal width".to_string(),
            attributes: HashMap::new(),
        }];
        // Should not panic and should truncate long messages
        print_logs_table(&logs, true, false);
    }

    // T040: Unit test for traces pretty-print formatter
    #[test]
    fn test_print_traces_table_with_data() {
        let traces = vec![
            Trace {
                id: "trace-001".to_string(),
                root_span: "http-request".to_string(),
                duration_ms: 1500,
                status: "OK".to_string(),
                spans: vec![],
            },
            Trace {
                id: "trace-002".to_string(),
                root_span: "database-query".to_string(),
                duration_ms: 250,
                status: "ERROR".to_string(),
                spans: vec![],
            },
        ];
        // Should not panic
        print_traces_table(&traces, true, false);
    }

    #[test]
    fn test_print_traces_table_with_color() {
        let traces = vec![Trace {
            id: "trace-001".to_string(),
            root_span: "http-request".to_string(),
            duration_ms: 1500,
            status: "OK".to_string(),
            spans: vec![],
        }];
        // Should not panic with color enabled
        print_traces_table(&traces, false, false);
    }

    // T041: Unit test for span tree formatter
    #[test]
    fn test_print_trace_tree_simple() {
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
        // Should not panic
        print_trace_tree(&trace, true);
    }

    #[test]
    fn test_print_trace_tree_with_hierarchy() {
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
                Span {
                    id: "span-003".to_string(),
                    name: "cache-lookup".to_string(),
                    parent_id: Some("span-001".to_string()),
                    start_time: now,
                    duration_ms: 50,
                    attributes: HashMap::new(),
                },
            ],
        };
        // Should not panic and should show hierarchy
        print_trace_tree(&trace, true);
    }

    #[test]
    fn test_print_trace_tree_deep_hierarchy() {
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
                    name: "middleware".to_string(),
                    parent_id: Some("span-001".to_string()),
                    start_time: now,
                    duration_ms: 1000,
                    attributes: HashMap::new(),
                },
                Span {
                    id: "span-003".to_string(),
                    name: "handler".to_string(),
                    parent_id: Some("span-002".to_string()),
                    start_time: now,
                    duration_ms: 800,
                    attributes: HashMap::new(),
                },
                Span {
                    id: "span-004".to_string(),
                    name: "database-query".to_string(),
                    parent_id: Some("span-003".to_string()),
                    start_time: now,
                    duration_ms: 250,
                    attributes: HashMap::new(),
                },
            ],
        };
        // Should not panic and should show deep hierarchy
        print_trace_tree(&trace, true);
    }

    // T063: Unit test for metrics pretty-print formatter
    #[test]
    fn test_print_metrics_table_with_data() {
        let metrics = vec![
            Metric {
                name: "http_requests_total".to_string(),
                type_: "counter".to_string(),
                value: 1234.0,
                timestamp: Utc::now(),
                labels: HashMap::from([
                    ("method".to_string(), "GET".to_string()),
                    ("status".to_string(), "200".to_string()),
                ]),
                percentiles: None,
            },
            Metric {
                name: "response_time_ms".to_string(),
                type_: "histogram".to_string(),
                value: 150.5,
                timestamp: Utc::now(),
                labels: HashMap::new(),
                percentiles: Some(HashMap::from([
                    ("p50".to_string(), 100.0),
                    ("p95".to_string(), 200.0),
                    ("p99".to_string(), 300.0),
                ])),
            },
            Metric {
                name: "memory_usage_bytes".to_string(),
                type_: "gauge".to_string(),
                value: 1048576.0,
                timestamp: Utc::now(),
                labels: HashMap::from([("host".to_string(), "server1".to_string())]),
                percentiles: None,
            },
        ];
        // Should not panic and should handle different metric types
        print_metrics_table(&metrics, true, false);
        print_metrics_table(&metrics, false, false); // Test with colors
    }

    #[test]
    fn test_print_metric_details_with_percentiles() {
        let metric = Metric {
            name: "response_time_ms".to_string(),
            type_: "histogram".to_string(),
            value: 150.5,
            timestamp: Utc::now(),
            labels: HashMap::from([
                ("endpoint".to_string(), "/api/users".to_string()),
                ("method".to_string(), "GET".to_string()),
            ]),
            percentiles: Some(HashMap::from([
                ("p50".to_string(), 100.0),
                ("p95".to_string(), 200.0),
                ("p99".to_string(), 300.0),
                ("p99.9".to_string(), 500.0),
            ])),
        };
        // Should not panic and should display percentiles
        print_metric_details(&metric, true);
    }

    #[test]
    fn test_print_metric_details_without_percentiles() {
        let metric = Metric {
            name: "http_requests_total".to_string(),
            type_: "counter".to_string(),
            value: 1234.0,
            timestamp: Utc::now(),
            labels: HashMap::from([("status".to_string(), "200".to_string())]),
            percentiles: None,
        };
        // Should not panic even without percentiles
        print_metric_details(&metric, true);
    }
}

// Made with Bob
