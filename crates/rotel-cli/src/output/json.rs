//! JSON output formatting for CLI

use crate::api::models::{LogEntry, MetricResponse, TraceDetail, TraceEntry};
use crate::error::Result;
use rotel_core::telemetry::GenAiSpanInfo;
use serde_json::{self, json};

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
pub fn print_traces_json(traces: &[TraceEntry]) -> Result<()> {
    let json = serde_json::to_string_pretty(traces)?;
    println!("{}", json);
    Ok(())
}

/// Print a single trace as JSON object
pub fn print_trace_json(trace: &TraceDetail) -> Result<()> {
    // Enrich trace with GenAI information
    let mut trace_json = serde_json::to_value(trace)?;

    if let Some(spans) = trace_json.get_mut("spans").and_then(|s| s.as_array_mut()) {
        for span in spans {
            if let Some(attributes) = span.get("attributes").and_then(|a| a.as_object()) {
                // Convert attributes to HashMap for GenAI parsing
                let attrs_map: std::collections::HashMap<String, String> = attributes
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                    .collect();

                let genai_info = GenAiSpanInfo::from_attributes(&attrs_map);

                if genai_info.is_genai {
                    let mut genai_obj = json!({});

                    if let Some(system) = &genai_info.system {
                        genai_obj["system"] = json!(system);
                    }
                    if let Some(model) = &genai_info.model {
                        genai_obj["model"] = json!(model);
                    }
                    if let Some(operation) = &genai_info.operation {
                        genai_obj["operation"] = json!(operation);
                    }
                    if let Some(input_tokens) = genai_info.input_tokens {
                        genai_obj["input_tokens"] = json!(input_tokens);
                    }
                    if let Some(output_tokens) = genai_info.output_tokens {
                        genai_obj["output_tokens"] = json!(output_tokens);
                    }
                    if let Some(total_tokens) = genai_info.total_tokens {
                        genai_obj["total_tokens"] = json!(total_tokens);
                    }
                    if let Some(temperature) = genai_info.temperature {
                        genai_obj["temperature"] = json!(temperature);
                    }
                    if let Some(max_tokens) = genai_info.max_tokens {
                        genai_obj["max_tokens"] = json!(max_tokens);
                    }
                    if !genai_info.finish_reasons.is_empty() {
                        genai_obj["finish_reasons"] = json!(genai_info.finish_reasons);
                    }

                    span.as_object_mut()
                        .unwrap()
                        .insert("genai".to_string(), genai_obj);
                }
            }
        }
    }

    let json = serde_json::to_string_pretty(&trace_json)?;
    println!("{}", json);
    Ok(())
}

/// Print metrics as JSON array
pub fn print_metrics_json(metrics: &[MetricResponse]) -> Result<()> {
    let json = serde_json::to_string_pretty(metrics)?;
    println!("{}", json);
    Ok(())
}

/// Print a single metric as JSON object
pub fn print_metric_json(metric: &MetricResponse) -> Result<()> {
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
            timestamp: 1000000000000000000,
            severity: "ERROR".to_string(),
            severity_text: None,
            body: "Test error".to_string(),
            attributes: HashMap::new(),
            resource: None,
            trace_id: None,
            span_id: None,
            resource: None,
            trace_id: None,
            span_id: None,
        }];
        let result = print_logs_json(&logs);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_log_json() {
        let log = LogEntry {
            timestamp: 1000000000000000000,
            severity: "ERROR".to_string(),
            severity_text: None,
            body: "Test error".to_string(),
            attributes: HashMap::new(),
            resource: None,
            trace_id: None,
            span_id: None,
            resource: None,
            trace_id: None,
            span_id: None,
        };
        let result = print_log_json(&log);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_traces_json() {
        let traces = vec![TraceEntry {
            trace_id: "trace-001".to_string(),
            root_span_name: "http-request".to_string(),
            start_time: 1000000000000000000,
            duration: 1500000000,
            span_count: 0,
            service_names: vec![],
            has_errors: false,
        }];
        let result = print_traces_json(&traces);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_metrics_json() {
        let metrics = vec![Metric {
            name: "http_requests_total".to_string(),
            description: None,
            unit: None,
            metric_type: "counter".to_string(),
            value: MetricValue::Counter(1234.0 as u64),
            timestamp: 1000000000000000000,
            attributes: HashMap::new(),
            resource: None,
        }];
        let result = print_metrics_json(&metrics);
        assert!(result.is_ok());
    }

    // T018: Unit test for logs JSON formatter
    #[test]
    fn test_json_is_valid() {
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
        let json = serde_json::to_string(&log).unwrap();
        // Verify it can be parsed back
        let parsed: LogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.id, log.id);
    }

    #[test]
    fn test_print_logs_json_multiple() {
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
            timestamp: 1000000000000000000,
            severity: "ERROR".to_string(),
            severity_text: None,
            body: "Error with attributes".to_string(),
            attributes,
        };
        let result = print_log_json(&log);
        assert!(result.is_ok());
    }

    #[test]
    fn test_json_output_is_parseable() {
        let logs = vec![LogEntry {
            timestamp: 1000000000000000000,
            severity: "ERROR".to_string(),
            severity_text: None,
            body: "Test error".to_string(),
            attributes: HashMap::new(),
            resource: None,
            trace_id: None,
            span_id: None,
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
            timestamp: 1000000000000000000,
            severity: "ERROR".to_string(),
            message: r#"Message with "quotes" and \backslashes\ and newlines\n"#.to_string(),
            attributes: HashMap::new(),
            resource: None,
            trace_id: None,
            span_id: None,
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
        use crate::api::models::SpanEntry;

        let traces = vec![TraceEntry {
            trace_id: "trace-001".to_string(),
            root_span_name: "http-request".to_string(),
            start_time: 1000000000000000000,
            duration: 1500000000,
            span_count: 1,
            service_names: vec![],
            has_errors: false,
        }];
        let result = print_traces_json(&traces);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_trace_json() {
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
        let result = print_trace_json(&trace);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_trace_json_with_hierarchy() {
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
            ],
            start_time: 1000000000000000000,
            end_time: 1000000001500000000,
            duration: 1500000000,
            span_count: 2,
            service_names: vec![],
        };
        let result = print_trace_json(&trace);
        assert!(result.is_ok());

        // Verify JSON is valid and can be parsed
        let json_str = serde_json::to_string(&trace).unwrap();
        let parsed: TraceDetail = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.trace_id, trace.trace_id);
        assert_eq!(parsed.spans.len(), 2);
    }

    #[test]
    fn test_print_traces_json_empty() {
        let traces: Vec<TraceEntry> = vec![];
        let result = print_traces_json(&traces);
        assert!(result.is_ok());
    }

    #[test]
    fn test_print_trace_json_with_attributes() {
        use crate::api::models::SpanEntry;

        let mut attributes = HashMap::new();
        attributes.insert("http.method".to_string(), "GET".to_string());
        attributes.insert("http.url".to_string(), "/api/users".to_string());

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
                attributes,
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
        let result = print_trace_json(&trace);
        assert!(result.is_ok());

        // Verify attributes are preserved in JSON
        let json_str = serde_json::to_string(&trace).unwrap();
        let parsed: TraceDetail = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.spans[0].attributes.len(), 2);
    }

    // T064: Unit test for metrics JSON formatter
    #[test]
    fn test_print_metrics_json_with_labels() {
        let metrics = vec![
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(1234.0 as u64),
                timestamp: 1000000000000000000,
                labels: HashMap::from([
                    ("method".to_string(), "GET".to_string()),
                    ("status".to_string(), "200".to_string()),
                ]),
                resource: None,
            },
            MetricResponse {
                name: "http_requests_total".to_string(),
                description: None,
                unit: None,
                metric_type: "counter".to_string(),
                value: MetricValue::Counter(567.0 as u64),
                timestamp: 1000000000000000000,
                labels: HashMap::from([
                    ("method".to_string(), "POST".to_string()),
                    ("status".to_string(), "201".to_string()),
                ]),
                resource: None,
            },
        ];
        let result = print_metrics_json(&metrics);
        assert!(result.is_ok());

        // Verify labels are preserved in JSON
        let json_str = serde_json::to_string(&metrics).unwrap();
        let parsed: Vec<Metric> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed[0].labels.len(), 2);
        assert_eq!(parsed[1].labels.len(), 2);
    }

    #[test]
    fn test_print_metric_json_with_percentiles() {
        let metric = Metric {
            name: "response_time_ms".to_string(),
            description: None,
            unit: None,
            metric_type: "histogram".to_string(),
            value: MetricValue::Counter(150.5 as u64),
            timestamp: 1000000000000000000,
            labels: HashMap::from([("endpoint".to_string(), "/api/users".to_string())]),
            percentiles: Some(HashMap::from([
                ("p50".to_string(), 100.0),
                ("p95".to_string(), 200.0),
                ("p99".to_string(), 300.0),
                ("p99.9".to_string(), 500.0),
            ])),
        };
        let result = print_metric_json(&metric);
        assert!(result.is_ok());

        // Verify percentiles are preserved in JSON
        let json_str = serde_json::to_string(&metric).unwrap();
        let parsed: Metric = serde_json::from_str(&json_str).unwrap();
        assert!(parsed.percentiles.is_some());
        assert_eq!(parsed.percentiles.as_ref().unwrap().len(), 4);
    }

    #[test]
    fn test_print_metrics_json_empty() {
        let metrics: Vec<Metric> = vec![];
        let result = print_metrics_json(&metrics);
        assert!(result.is_ok());
    }

    #[test]
    fn test_metrics_json_time_series() {
        // Test time-series data (multiple data points for same metric)
        let now = Utc::now();
        let metrics = vec![
            MetricResponse {
                name: "cpu_usage_percent".to_string(),
                description: None,
                unit: None,
                metric_type: "gauge".to_string(),
                value: MetricValue::Counter(45.2 as u64),
                timestamp: now - chrono::Duration::minutes(2),
                labels: HashMap::from([("host".to_string(), "server1".to_string())]),
                resource: None,
            },
            MetricResponse {
                name: "cpu_usage_percent".to_string(),
                description: None,
                unit: None,
                metric_type: "gauge".to_string(),
                value: MetricValue::Counter(52.8 as u64),
                timestamp: now - chrono::Duration::minutes(1),
                labels: HashMap::from([("host".to_string(), "server1".to_string())]),
                resource: None,
            },
            MetricResponse {
                name: "cpu_usage_percent".to_string(),
                description: None,
                unit: None,
                metric_type: "gauge".to_string(),
                value: MetricValue::Counter(48.5 as u64),
                timestamp: now,
                labels: HashMap::from([("host".to_string(), "server1".to_string())]),
                resource: None,
            },
        ];
        let result = print_metrics_json(&metrics);
        assert!(result.is_ok());

        // Verify time-series structure is preserved
        let json_str = serde_json::to_string(&metrics).unwrap();
        let parsed: Vec<Metric> = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0].name, "cpu_usage_percent");
        assert_eq!(parsed[1].name, "cpu_usage_percent");
        assert_eq!(parsed[2].name, "cpu_usage_percent");
    }
}

// Made with Bob
