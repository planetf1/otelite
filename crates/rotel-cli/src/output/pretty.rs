//! Pretty-print table formatting for CLI output

use crate::api::models::{LogEntry, MetricResponse, SpanNode, TraceDetail, TraceEntry};
use comfy_table::{presets::UTF8_FULL, Cell, Color, ContentArrangement, Table};
use rotel_core::telemetry::{format_attribute_value, GenAiSpanInfo};

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

        use chrono::{DateTime, Utc};
        let dt = DateTime::<Utc>::from_timestamp_nanos(log.timestamp);
        let timestamp_str = dt.format("%Y-%m-%d %H:%M:%S").to_string();

        table.add_row(vec![
            Cell::new(log.timestamp.to_string()),
            Cell::new(timestamp_str),
            severity_cell,
            Cell::new(&log.body),
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

    use chrono::{DateTime, Utc};
    let dt = DateTime::<Utc>::from_timestamp_nanos(log.timestamp);
    let timestamp_str = dt.format("%Y-%m-%d %H:%M:%S").to_string();

    println!("Timestamp: {}", timestamp_str);
    println!("Severity:  {}{}{}", severity_color, log.severity, reset);
    println!("Body:      {}", log.body);

    if let Some(trace_id) = &log.trace_id {
        println!("Trace ID:  {}", trace_id);
    }
    if let Some(span_id) = &log.span_id {
        println!("Span ID:   {}", span_id);
    }

    if !log.attributes.is_empty() {
        println!("\nAttributes:");
        for (key, value) in &log.attributes {
            let formatted = format_attribute_value(value);
            print_key_value_block(key, &formatted, 2);
        }
    }
}

/// Print traces in a pretty table format
pub fn print_traces_table(traces: &[TraceEntry], no_color: bool, no_header: bool) {
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
        table.set_header(vec!["Trace ID", "Root Span", "Duration", "Status", "Spans"]);
    }

    // Add rows
    for trace in traces {
        let status = if trace.has_errors { "ERROR" } else { "OK" };
        let status_cell = if no_color {
            Cell::new(status)
        } else {
            let color = if trace.has_errors {
                Color::Red
            } else {
                Color::Green
            };
            Cell::new(status).fg(color)
        };

        let duration_ms = trace.duration / 1_000_000;

        table.add_row(vec![
            Cell::new(&trace.trace_id),
            Cell::new(&trace.root_span_name),
            Cell::new(format!("{}ms", duration_ms)),
            status_cell,
            Cell::new(trace.span_count.to_string()),
        ]);
    }

    println!("{}", table);
}

/// Print a trace with span tree
pub fn print_trace_tree(trace: &TraceDetail, _no_color: bool) {
    println!("Trace ID: {}", trace.trace_id);
    let duration_ms = trace.duration / 1_000_000;
    println!("Duration: {}ms", duration_ms);
    let status = if trace.spans.iter().any(|s| {
        s.status
            .as_ref()
            .map(|st| st.code == "Error")
            .unwrap_or(false)
    }) {
        "ERROR"
    } else {
        "OK"
    };
    println!("Status:   {}", status);
    println!("\nSpans:");

    use crate::api::models::SpanEntry;
    let tree = SpanEntry::build_span_tree(&trace.spans);
    for node in &tree {
        print_span_node(node, 0);
    }
}

fn print_span_node(node: &SpanNode, depth: usize) {
    let indent = "  ".repeat(depth);
    let prefix = if depth > 0 { "├─ " } else { "" };

    // Check for GenAI information
    let genai_info = GenAiSpanInfo::from_attributes(&node.span.attributes);
    let genai_suffix = if genai_info.is_genai {
        let mut parts = Vec::new();
        if let Some(system) = genai_info.system_display_name() {
            parts.push(format!("[{}]", system));
        }
        if let Some(model) = &genai_info.model {
            parts.push(model.clone());
        }
        if let Some(token_summary) = genai_info.format_token_summary() {
            parts.push(token_summary);
        }
        if !parts.is_empty() {
            format!(" {}", parts.join(" "))
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    let duration_ms = node.span.duration / 1_000_000;

    println!(
        "{}{}{} ({}ms){}",
        indent, prefix, node.span.name, duration_ms, genai_suffix
    );

    if !node.span.attributes.is_empty() {
        for (key, value) in &node.span.attributes {
            let formatted = format_attribute_value(value);
            print_key_value_block(key, &formatted, depth + 1);
        }
    }

    for child in &node.children {
        print_span_node(child, depth + 1);
    }
}

fn print_key_value_block(key: &str, value: &str, indent_level: usize) {
    let indent = "  ".repeat(indent_level);
    let continuation_indent = format!("{indent}    ");
    let mut lines = value.lines();

    if let Some(first_line) = lines.next() {
        println!("{indent}{key}: {first_line}");
    } else {
        println!("{indent}{key}:");
    }

    for line in lines {
        println!("{continuation_indent}{line}");
    }
}

/// Print metrics in a pretty table format
pub fn print_metrics_table(metrics: &[MetricResponse], no_color: bool, no_header: bool) {
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
            Cell::new(&metric.metric_type)
        } else {
            let color = match metric.metric_type.as_str() {
                "counter" => Color::Green,
                "gauge" => Color::Blue,
                "histogram" => Color::Yellow,
                "summary" => Color::Cyan,
                _ => Color::Reset,
            };
            Cell::new(&metric.metric_type).fg(color)
        };

        use crate::api::models::MetricValue;
        use chrono::{DateTime, Utc};

        let value_str = match &metric.value {
            MetricValue::Gauge(v) => format!("{:.2}", v),
            MetricValue::Counter(v) => format!("{}", v),
            MetricValue::Histogram { count, sum, .. } => format!("count={}, sum={:.2}", count, sum),
            MetricValue::Summary { count, sum, .. } => format!("count={}, sum={:.2}", count, sum),
        };

        let dt = DateTime::<Utc>::from_timestamp_nanos(metric.timestamp);
        let timestamp_str = dt.format("%Y-%m-%d %H:%M:%S").to_string();

        table.add_row(vec![
            Cell::new(&metric.name),
            type_cell,
            Cell::new(value_str),
            Cell::new(timestamp_str),
        ]);
    }

    println!("{}", table);
}

/// Print a single metric with full details including percentiles
pub fn print_metric_details(metric: &MetricResponse, _no_color: bool) {
    use crate::api::models::MetricValue;
    use chrono::{DateTime, Utc};

    println!("Name:      {}", metric.name);
    println!("Type:      {}", metric.metric_type);

    match &metric.value {
        MetricValue::Gauge(v) => println!("Value:     {:.2}", v),
        MetricValue::Counter(v) => println!("Value:     {}", v),
        MetricValue::Histogram {
            count,
            sum,
            buckets,
        } => {
            println!("Count:     {}", count);
            println!("Sum:       {:.2}", sum);
            println!("Buckets:");
            for bucket in buckets {
                println!("  <= {:.2}: {}", bucket.upper_bound, bucket.count);
            }
        },
        MetricValue::Summary {
            count,
            sum,
            quantiles,
        } => {
            println!("Count:     {}", count);
            println!("Sum:       {:.2}", sum);
            println!("Quantiles:");
            for q in quantiles {
                println!("  p{}: {:.4}", (q.quantile * 100.0) as u32, q.value);
            }
        },
    }

    let dt = DateTime::<Utc>::from_timestamp_nanos(metric.timestamp);
    println!("Timestamp: {}", dt.format("%Y-%m-%d %H:%M:%S"));

    if !metric.attributes.is_empty() {
        println!("\nAttributes:");
        for (key, value) in &metric.attributes {
            println!("  {}: {}", key, value);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{MetricValue, TraceEntry};
    use std::collections::HashMap;

    #[test]
    fn test_print_logs_table_empty() {
        let logs: Vec<LogEntry> = vec![];
        // Should not panic
        print_logs_table(&logs, true, false);
    }

    #[test]
    fn test_print_traces_table_empty() {
        let traces: Vec<TraceEntry> = vec![];
        // Should not panic
        print_traces_table(&traces, true, false);
    }

    #[test]
    fn test_print_metrics_table_empty() {
        let metrics: Vec<MetricResponse> = vec![];
        // Should not panic
        print_metrics_table(&metrics, true, false);
    }

    // T017: Unit test for logs pretty-print formatter
    #[test]
    fn test_print_log_details() {
        let log = LogEntry {
            timestamp: 1000000000000000000,
            severity: "ERROR".to_string(),
            severity_text: None,
            body: "Test error".to_string(),
            attributes: HashMap::new(),
            resource: None,
            trace_id: None,
            span_id: None,
        };
        // Should not panic
        print_log_details(&log, true);
    }

    #[test]
    fn test_print_logs_table_with_data() {
        let logs = vec![
            LogEntry {
                timestamp: 1000000000000000000,
                severity: "ERROR".to_string(),
                severity_text: None,
                body: "Error message".to_string(),
                attributes: HashMap::new(),
                resource: None,
                trace_id: None,
                span_id: None,
            },
            LogEntry {
                timestamp: 1000000000000000000,
                severity: "INFO".to_string(),
                severity_text: None,
                body: "Info message".to_string(),
                attributes: HashMap::new(),
                resource: None,
                trace_id: None,
                span_id: None,
            },
            LogEntry {
                timestamp: 1000000000000000000,
                severity: "WARN".to_string(),
                severity_text: None,
                body: "Warning message".to_string(),
                attributes: HashMap::new(),
                resource: None,
                trace_id: None,
                span_id: None,
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
                timestamp: 1000000000000000000,
                severity: severity.to_string(),
                severity_text: Some(severity.to_string()),
                body: format!("{} message", severity),
                attributes: HashMap::new(),
                resource: None,
                trace_id: None,
                span_id: None,
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
            timestamp: 1000000000000000000,
            severity: "ERROR".to_string(),
            severity_text: None,
            body: "Test error with attributes".to_string(),
            attributes,
            resource: None,
            trace_id: None,
            span_id: None,
        };
        // Should not panic and should display attributes
        print_log_details(&log, true);
    }

    #[test]
    fn test_print_logs_table_long_messages() {
        let logs = vec![LogEntry {

            timestamp: 1000000000000000000,
            severity: "INFO".to_string(),
            severity_text: None,
            body: "This is a very long message that should be truncated in the table view to ensure the table remains readable and doesn't overflow the terminal width".to_string(),
            attributes: HashMap::new(),
            resource: None,
            trace_id: None,
            span_id: None,
        }];
        // Should not panic and should truncate long messages
        print_logs_table(&logs, true, false);
    }

    // T040: Unit test for traces pretty-print formatter
    #[test]
    fn test_print_traces_table_with_data() {
        let traces = vec![
            TraceEntry {
                trace_id: "trace-001".to_string(),
                root_span_name: "http-request".to_string(),
                start_time: 1000000000000000000,
                duration: 1_500_000_000,
                span_count: 1,
                service_names: vec![],
                has_errors: false,
            },
            TraceEntry {
                trace_id: "trace-002".to_string(),
                root_span_name: "database-query".to_string(),
                start_time: 1000000000000000000,
                duration: 250_000_000,
                span_count: 1,
                service_names: vec![],
                has_errors: true,
            },
        ];
        // Should not panic
        print_traces_table(&traces, true, false);
    }

    #[test]
    fn test_print_traces_table_with_color() {
        let traces = vec![TraceEntry {
            trace_id: "trace-001".to_string(),
            root_span_name: "http-request".to_string(),
            start_time: 1000000000000000000,
            duration: 1_500_000_000,
            span_count: 1,
            service_names: vec![],
            has_errors: false,
        }];
        // Should not panic with color enabled
        print_traces_table(&traces, false, false);
    }

    // T041: Unit test for span tree formatter
    #[test]
    fn test_print_trace_tree_simple() {
        use crate::api::models::SpanEntry;

        let trace = TraceDetail {
            trace_id: "trace-001".to_string(),
            spans: vec![SpanEntry {
                span_id: "span-001".to_string(),
                trace_id: "trace-001".to_string(),
                parent_span_id: None,
                name: "http-request".to_string(),
                kind: "Internal".to_string(),
                start_time: 1000000000000000000,
                end_time: 1000000001500000000,
                duration: 1500000000,
                attributes: HashMap::new(),
                resource: None,
                status: None,
                events: vec![],
            }],
            start_time: 1000000000000000000,
            end_time: 1000000001500000000,
            duration: 1500000000,
            span_count: 1,
            service_names: vec![],
        };
        // Should not panic
        print_trace_tree(&trace, true);
    }

    #[test]
    fn test_print_trace_tree_with_hierarchy() {
        use crate::api::models::SpanEntry;

        let trace = TraceDetail {
            trace_id: "trace-001".to_string(),
            spans: vec![
                SpanEntry {
                    span_id: "span-001".to_string(),
                    trace_id: "trace-001".to_string(),
                    parent_span_id: None,
                    name: "http-request".to_string(),
                    kind: "Internal".to_string(),
                    start_time: 1000000000000000000,
                    end_time: 1000000001500000000,
                    duration: 1500000000,
                    attributes: HashMap::new(),
                    resource: None,
                    status: None,
                    events: vec![],
                },
                SpanEntry {
                    span_id: "span-002".to_string(),
                    trace_id: "trace-001".to_string(),
                    parent_span_id: Some("span-001".to_string()),
                    name: "database-query".to_string(),
                    kind: "Internal".to_string(),
                    start_time: 1000000000000000000,
                    end_time: 1000000000250000000,
                    duration: 250000000,
                    attributes: HashMap::new(),
                    resource: None,
                    status: None,
                    events: vec![],
                },
                SpanEntry {
                    span_id: "span-003".to_string(),
                    trace_id: "trace-001".to_string(),
                    parent_span_id: Some("span-001".to_string()),
                    name: "cache-lookup".to_string(),
                    kind: "Internal".to_string(),
                    start_time: 1000000000000000000,
                    end_time: 1000000000050000000,
                    duration: 50000000,
                    attributes: HashMap::new(),
                    resource: None,
                    status: None,
                    events: vec![],
                },
            ],
            start_time: 1000000000000000000,
            end_time: 1000000001500000000,
            duration: 1500000000,
            span_count: 3,
            service_names: vec![],
        };
        // Should not panic and should show hierarchy
        print_trace_tree(&trace, true);
    }

    #[test]
    fn test_print_trace_tree_deep_hierarchy() {
        use crate::api::models::SpanEntry;

        let trace = TraceDetail {
            trace_id: "trace-001".to_string(),
            spans: vec![
                SpanEntry {
                    span_id: "span-001".to_string(),
                    trace_id: "trace-001".to_string(),
                    parent_span_id: None,
                    name: "http-request".to_string(),
                    kind: "Internal".to_string(),
                    start_time: 1000000000000000000,
                    end_time: 1000000001500000000,
                    duration: 1500000000,
                    attributes: HashMap::new(),
                    resource: None,
                    status: None,
                    events: vec![],
                },
                SpanEntry {
                    span_id: "span-002".to_string(),
                    trace_id: "trace-001".to_string(),
                    parent_span_id: Some("span-001".to_string()),
                    name: "middleware".to_string(),
                    kind: "Internal".to_string(),
                    start_time: 1000000000000000000,
                    end_time: 1000000001000000000,
                    duration: 1000000000,
                    attributes: HashMap::new(),
                    resource: None,
                    status: None,
                    events: vec![],
                },
                SpanEntry {
                    span_id: "span-003".to_string(),
                    trace_id: "trace-001".to_string(),
                    parent_span_id: Some("span-002".to_string()),
                    name: "handler".to_string(),
                    kind: "Internal".to_string(),
                    start_time: 1000000000000000000,
                    end_time: 1000000000800000000,
                    duration: 800000000,
                    attributes: HashMap::new(),
                    resource: None,
                    status: None,
                    events: vec![],
                },
                SpanEntry {
                    span_id: "span-004".to_string(),
                    trace_id: "trace-001".to_string(),
                    parent_span_id: Some("span-003".to_string()),
                    name: "database-query".to_string(),
                    kind: "Internal".to_string(),
                    start_time: 1000000000000000000,
                    end_time: 1000000000250000000,
                    duration: 250000000,
                    attributes: HashMap::new(),
                    resource: None,
                    status: None,
                    events: vec![],
                },
            ],
            start_time: 1000000000000000000,
            end_time: 1000000001500000000,
            duration: 1500000000,
            span_count: 4,
            service_names: vec![],
        };
        // Should not panic and should show deep hierarchy
        print_trace_tree(&trace, true);
    }

    // T063: Unit test for metrics pretty-print formatter
    #[test]
    fn test_print_metrics_table_with_data() {
        let metrics = vec![
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(1234.0 as u64),
                timestamp: 1000000000000000000,
                attributes: HashMap::from([
                    ("method".to_string(), "GET".to_string()),
                    ("status".to_string(), "200".to_string()),
                ]),
                resource: None,
            },
            MetricResponse {
                name: "response_time_ms".to_string(),
                description: None,
                unit: None,
                metric_type: "histogram".to_string(),
                value: MetricValue::Counter(150.5 as u64),
                timestamp: 1000000000000000000,
                attributes: HashMap::new(),
                resource: None,
            },
            MetricResponse {
                name: "memory_usage_bytes".to_string(),
                description: None,
                unit: None,
                metric_type: "gauge".to_string(),
                value: MetricValue::Counter(1048576.0 as u64),
                timestamp: 1000000000000000000,
                attributes: HashMap::from([("host".to_string(), "server1".to_string())]),
                resource: None,
            },
        ];
        // Should not panic and should handle different metric types
        print_metrics_table(&metrics, true, false);
        print_metrics_table(&metrics, false, false); // Test with colors
    }

    #[test]
    fn test_print_metric_details_with_histogram() {
        use crate::api::models::HistogramBucket;

        let metric = MetricResponse {
            name: "response_time_ms".to_string(),
            description: None,
            unit: None,
            metric_type: "histogram".to_string(),
            value: MetricValue::Histogram {
                count: 150,
                sum: 15000.0,
                buckets: vec![
                    HistogramBucket {
                        upper_bound: 100.0,
                        count: 50,
                    },
                    HistogramBucket {
                        upper_bound: 200.0,
                        count: 75,
                    },
                    HistogramBucket {
                        upper_bound: 300.0,
                        count: 20,
                    },
                    HistogramBucket {
                        upper_bound: 500.0,
                        count: 5,
                    },
                ],
            },
            timestamp: 1000000000000000000,
            attributes: HashMap::from([
                ("endpoint".to_string(), "/api/users".to_string()),
                ("method".to_string(), "GET".to_string()),
            ]),
            resource: None,
        };
        // Should not panic and should display histogram
        print_metric_details(&metric, true);
    }

    #[test]
    fn test_print_metric_details_without_histogram() {
        let metric = MetricResponse {
            name: "http_requests_total".to_string(),
            description: None,
            unit: None,
            metric_type: "counter".to_string(),
            value: MetricValue::Counter(1234),
            timestamp: 1000000000000000000,
            attributes: HashMap::from([("status".to_string(), "200".to_string())]),
            resource: None,
        };
        // Should not panic even without histogram
        print_metric_details(&metric, true);
    }
}

// Made with Bob
