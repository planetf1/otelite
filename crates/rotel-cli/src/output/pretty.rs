//! Pretty-print table formatting for CLI output

use crate::api::models::{LogEntry, MetricResponse, SpanEntry, TraceDetail, TraceEntry};
use crate::config::Config;
use crate::output::{colors, pager};
use comfy_table::{presets::UTF8_FULL, Cell, ContentArrangement, Table};
use rotel_core::telemetry::{format_attribute_value, GenAiSpanInfo};
use std::collections::HashMap;
use std::io;

/// A span node in the tree for display
#[derive(Debug, Clone)]
struct SpanNode {
    span: SpanEntry,
    children: Vec<SpanNode>,
}

/// Build a simple span tree from flat list
fn build_span_tree(spans: &[SpanEntry]) -> Vec<SpanNode> {
    let mut span_map: HashMap<String, Vec<SpanEntry>> = HashMap::new();

    // Group spans by parent
    for span in spans {
        if let Some(parent_id) = &span.parent_span_id {
            span_map
                .entry(parent_id.clone())
                .or_default()
                .push(span.clone());
        }
    }

    // Find root spans and build tree
    let mut nodes = Vec::new();
    for span in spans {
        if span.parent_span_id.is_none() {
            nodes.push(build_node(span.clone(), &span_map));
        }
    }

    nodes
}

fn build_node(span: SpanEntry, span_map: &HashMap<String, Vec<SpanEntry>>) -> SpanNode {
    let children = span_map
        .get(&span.span_id)
        .map(|children| {
            children
                .iter()
                .map(|child| build_node(child.clone(), span_map))
                .collect()
        })
        .unwrap_or_default();

    SpanNode { span, children }
}

/// Print logs in a pretty table format
pub fn print_logs_table(logs: &[LogEntry], config: &Config) -> io::Result<()> {
    if logs.is_empty() {
        println!("No logs found");
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Add header (unless disabled)
    if !config.no_header {
        table.set_header(vec!["ID", "Timestamp", "Severity", "Message"]);
    }

    // Add rows
    for log in logs {
        let severity_cell = if config.no_color {
            Cell::new(&log.severity)
        } else {
            let color = colors::severity_color(&log.severity);
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

    let output = format!("{}\n", table);
    pager::write_with_pager(config, &output)
}

/// Print a single log entry with full details
pub fn print_log_details(log: &LogEntry, config: &Config) -> io::Result<()> {
    use std::fmt::Write;
    let mut output = String::new();

    let severity_color = if config.no_color {
        ""
    } else {
        colors::ansi::severity_color(&log.severity)
    };
    let reset = if config.no_color {
        ""
    } else {
        colors::ansi::RESET
    };

    use chrono::{DateTime, Utc};
    let dt = DateTime::<Utc>::from_timestamp_nanos(log.timestamp);
    let timestamp_str = dt.format("%Y-%m-%d %H:%M:%S").to_string();

    writeln!(output, "Timestamp: {}", timestamp_str).unwrap();
    writeln!(
        output,
        "Severity:  {}{}{}",
        severity_color, log.severity, reset
    )
    .unwrap();
    writeln!(output, "Body:      {}", log.body).unwrap();

    if let Some(trace_id) = &log.trace_id {
        writeln!(output, "Trace ID:  {}", trace_id).unwrap();
    }
    if let Some(span_id) = &log.span_id {
        writeln!(output, "Span ID:   {}", span_id).unwrap();
    }

    if !log.attributes.is_empty() {
        writeln!(output, "\nAttributes:").unwrap();
        for (key, value) in &log.attributes {
            let formatted = format_attribute_value(value);
            format_key_value_block(&mut output, key, &formatted, 2);
        }
    }

    pager::write_with_pager(config, &output)
}

/// Print traces in a pretty table format
pub fn print_traces_table(traces: &[TraceEntry], config: &Config) -> io::Result<()> {
    if traces.is_empty() {
        println!("No traces found");
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Add header (unless disabled)
    if !config.no_header {
        table.set_header(vec!["Trace ID", "Root Span", "Duration", "Status", "Spans"]);
    }

    // Add rows
    for trace in traces {
        let status = if trace.has_errors { "ERROR" } else { "OK" };
        let status_cell = if config.no_color {
            Cell::new(status)
        } else {
            let color = colors::trace_status_color(trace.has_errors);
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

    let output = format!("{}\n", table);
    pager::write_with_pager(config, &output)
}

/// Print a trace with span tree
pub fn print_trace_tree(trace: &TraceDetail, config: &Config) -> io::Result<()> {
    use std::fmt::Write;
    let mut output = String::new();

    writeln!(output, "Trace ID: {}", trace.trace_id).unwrap();
    let duration_ms = trace.duration / 1_000_000;
    writeln!(output, "Duration: {}ms", duration_ms).unwrap();
    let status = if trace.spans.iter().any(|s| s.status.code == "Error") {
        "ERROR"
    } else {
        "OK"
    };
    writeln!(output, "Status:   {}", status).unwrap();
    writeln!(output, "\nSpans:").unwrap();

    let tree = build_span_tree(&trace.spans);
    for node in &tree {
        format_span_node(&mut output, node, 0);
    }

    pager::write_with_pager(config, &output)
}

fn format_span_node(output: &mut String, node: &SpanNode, depth: usize) {
    use std::fmt::Write;
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

    writeln!(
        output,
        "{}{}{} ({}ms){}",
        indent, prefix, node.span.name, duration_ms, genai_suffix
    )
    .unwrap();

    if !node.span.attributes.is_empty() {
        for (key, value) in &node.span.attributes {
            let formatted = format_attribute_value(value);
            format_key_value_block(output, key, &formatted, depth + 1);
        }
    }

    for child in &node.children {
        format_span_node(output, child, depth + 1);
    }
}

fn format_key_value_block(output: &mut String, key: &str, value: &str, indent_level: usize) {
    use std::fmt::Write;
    let indent = "  ".repeat(indent_level);
    let continuation_indent = format!("{indent}    ");
    let mut lines = value.lines();

    if let Some(first_line) = lines.next() {
        writeln!(output, "{indent}{key}: {first_line}").unwrap();
    } else {
        writeln!(output, "{indent}{key}:").unwrap();
    }

    for line in lines {
        writeln!(output, "{continuation_indent}{line}").unwrap();
    }
}

/// Print metrics in a pretty table format
pub fn print_metrics_table(metrics: &[MetricResponse], config: &Config) -> io::Result<()> {
    if metrics.is_empty() {
        println!("No metrics found");
        return Ok(());
    }

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .set_content_arrangement(ContentArrangement::Dynamic);

    // Add header (unless disabled)
    if !config.no_header {
        table.set_header(vec!["Name", "Type", "Value", "Timestamp"]);
    }

    // Add rows
    for metric in metrics {
        let type_cell = if config.no_color {
            Cell::new(&metric.metric_type)
        } else {
            let color = colors::metric_type_color(&metric.metric_type);
            Cell::new(&metric.metric_type).fg(color)
        };

        use crate::api::models::MetricValue;
        use chrono::{DateTime, Utc};

        let value_str = match &metric.value {
            MetricValue::Gauge(v) => format!("{:.2}", v),
            MetricValue::Counter(v) => format!("{}", v),
            MetricValue::Histogram(h) => format!("count={}, sum={:.2}", h.count, h.sum),
            MetricValue::Summary(s) => format!("count={}, sum={:.2}", s.count, s.sum),
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

    let output = format!("{}\n", table);
    pager::write_with_pager(config, &output)
}

/// Print a single metric with full details including percentiles
pub fn print_metric_details(metric: &MetricResponse, config: &Config) -> io::Result<()> {
    use crate::api::models::MetricValue;
    use chrono::{DateTime, Utc};
    use std::fmt::Write;
    let mut output = String::new();

    writeln!(output, "Name:      {}", metric.name).unwrap();
    writeln!(output, "Type:      {}", metric.metric_type).unwrap();

    match &metric.value {
        MetricValue::Gauge(v) => writeln!(output, "Value:     {:.2}", v).unwrap(),
        MetricValue::Counter(v) => writeln!(output, "Value:     {}", v).unwrap(),
        MetricValue::Histogram(h) => {
            writeln!(output, "Count:     {}", h.count).unwrap();
            writeln!(output, "Sum:       {:.2}", h.sum).unwrap();
            writeln!(output, "Buckets:").unwrap();
            for bucket in &h.buckets {
                writeln!(output, "  <= {:.2}: {}", bucket.upper_bound, bucket.count).unwrap();
            }
        },
        MetricValue::Summary(s) => {
            writeln!(output, "Count:     {}", s.count).unwrap();
            writeln!(output, "Sum:       {:.2}", s.sum).unwrap();
            writeln!(output, "Quantiles:").unwrap();
            for q in &s.quantiles {
                writeln!(output, "  p{}: {:.4}", (q.quantile * 100.0) as u32, q.value).unwrap();
            }
        },
    }

    let dt = DateTime::<Utc>::from_timestamp_nanos(metric.timestamp);
    writeln!(output, "Timestamp: {}", dt.format("%Y-%m-%d %H:%M:%S")).unwrap();

    if !metric.attributes.is_empty() {
        writeln!(output, "\nAttributes:").unwrap();
        for (key, value) in &metric.attributes {
            writeln!(output, "  {}: {}", key, value).unwrap();
        }
    }

    pager::write_with_pager(config, &output)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::models::{MetricValue, SpanStatus, TraceEntry};
    use std::collections::HashMap;

    fn test_config() -> Config {
        Config {
            endpoint: "http://localhost:3000".to_string(),
            timeout: std::time::Duration::from_secs(30),
            format: crate::config::OutputFormat::Pretty,
            no_color: true,
            no_header: false,
            no_pager: true,
        }
    }

    #[test]
    fn test_print_logs_table_empty() {
        let logs: Vec<LogEntry> = vec![];
        let config = test_config();
        // Should not panic
        let _ = print_logs_table(&logs, &config);
    }

    #[test]
    fn test_print_traces_table_empty() {
        let traces: Vec<TraceEntry> = vec![];
        let config = test_config();
        // Should not panic
        let _ = print_traces_table(&traces, &config);
    }

    #[test]
    fn test_print_metrics_table_empty() {
        let metrics: Vec<MetricResponse> = vec![];
        let config = test_config();
        // Should not panic
        let _ = print_metrics_table(&metrics, &config);
    }

    // T017: Unit test for logs pretty-print formatter
    #[test]
    fn test_print_log_details() {
        let config = test_config();
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
        let _ = print_log_details(&log, &config);
    }

    #[test]
    fn test_print_logs_table_with_data() {
        let config = test_config();
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
        let _ = print_logs_table(&logs, &config);
        let config_color = Config {
            no_color: false,
            ..config
        };
        let _ = print_logs_table(&logs, &config_color);
    }

    #[test]
    fn test_print_logs_table_severity_colors() {
        let config = test_config();
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
            let _ = print_logs_table(&logs, &config);
        }
    }

    #[test]
    fn test_print_log_details_with_attributes() {
        let config = test_config();
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
        let _ = print_log_details(&log, &config);
    }

    #[test]
    fn test_print_logs_table_long_messages() {
        let config = test_config();
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
        let _ = print_logs_table(&logs, &config);
    }

    // T040: Unit test for traces pretty-print formatter
    #[test]
    fn test_print_traces_table_with_data() {
        let config = test_config();
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
        let _ = print_traces_table(&traces, &config);
    }

    #[test]
    fn test_print_traces_table_with_color() {
        let config = Config {
            no_color: false,
            ..test_config()
        };
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
        let _ = print_traces_table(&traces, &config);
    }

    // T041: Unit test for span tree formatter
    #[test]
    fn test_print_trace_tree_simple() {
        use crate::api::models::SpanEntry;
        let config = test_config();

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
                status: SpanStatus {
                    code: "Ok".to_string(),
                    message: None,
                },
                events: vec![],
            }],
            start_time: 1000000000000000000,
            end_time: 1000000001500000000,
            duration: 1500000000,
            span_count: 1,
            service_names: vec![],
        };
        // Should not panic
        let _ = print_trace_tree(&trace, &config);
    }

    #[test]
    fn test_print_trace_tree_with_hierarchy() {
        use crate::api::models::SpanEntry;
        let config = test_config();

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
                    status: SpanStatus {
                        code: "Ok".to_string(),
                        message: None,
                    },
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
                    status: SpanStatus {
                        code: "Ok".to_string(),
                        message: None,
                    },
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
                    status: SpanStatus {
                        code: "Ok".to_string(),
                        message: None,
                    },
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
        let _ = print_trace_tree(&trace, &config);
    }

    #[test]
    fn test_print_trace_tree_deep_hierarchy() {
        use crate::api::models::SpanEntry;
        let config = test_config();

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
                    status: SpanStatus {
                        code: "Ok".to_string(),
                        message: None,
                    },
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
                    status: SpanStatus {
                        code: "Ok".to_string(),
                        message: None,
                    },
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
                    status: SpanStatus {
                        code: "Ok".to_string(),
                        message: None,
                    },
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
                    status: SpanStatus {
                        code: "Ok".to_string(),
                        message: None,
                    },
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
        let _ = print_trace_tree(&trace, &config);
    }

    // T063: Unit test for metrics pretty-print formatter
    #[test]
    fn test_print_metrics_table_with_data() {
        let config = test_config();
        let metrics = vec![
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(1234),
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
                value: MetricValue::Gauge(150.5),
                timestamp: 1000000000000000000,
                attributes: HashMap::new(),
                resource: None,
            },
            MetricResponse {
                name: "memory_usage_bytes".to_string(),
                description: None,
                unit: None,
                metric_type: "gauge".to_string(),
                value: MetricValue::Gauge(1048576.0),
                timestamp: 1000000000000000000,
                attributes: HashMap::from([("host".to_string(), "server1".to_string())]),
                resource: None,
            },
        ];
        // Should not panic and should handle different metric types
        let _ = print_metrics_table(&metrics, &config);
        let config_color = Config {
            no_color: false,
            ..config
        };
        let _ = print_metrics_table(&metrics, &config_color);
    }

    #[test]
    fn test_print_metric_details_with_histogram() {
        use crate::api::models::{HistogramBucket, HistogramValue};
        let config = test_config();

        let metric = MetricResponse {
            name: "response_time_ms".to_string(),
            description: None,
            unit: None,
            metric_type: "histogram".to_string(),
            value: MetricValue::Histogram(HistogramValue {
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
            }),
            timestamp: 1000000000000000000,
            attributes: HashMap::from([
                ("endpoint".to_string(), "/api/users".to_string()),
                ("method".to_string(), "GET".to_string()),
            ]),
            resource: None,
        };
        // Should not panic and should display histogram
        let _ = print_metric_details(&metric, &config);
    }

    #[test]
    fn test_print_metric_details_without_histogram() {
        let config = test_config();
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
        let _ = print_metric_details(&metric, &config);
    }
}
