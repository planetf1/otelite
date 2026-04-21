//! OTLP to internal type conversion functions
//!
//! This module provides functions to convert OpenTelemetry Protocol (OTLP)
//! protobuf types into rotel-core internal types.

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};
use rotel_core::telemetry::{
    log::{LogRecord, SeverityLevel},
    metric::{HistogramBucket, Metric, MetricType, Quantile},
    resource::Resource,
    trace::{Span, SpanEvent, SpanKind, SpanStatus, StatusCode, Trace},
};
use std::collections::HashMap;

/// Convert OTLP logs request to internal log records
pub fn convert_logs(request: ExportLogsServiceRequest) -> Vec<LogRecord> {
    let mut logs = Vec::new();

    for resource_logs in request.resource_logs {
        let resource = convert_resource(resource_logs.resource);

        for scope_logs in resource_logs.scope_logs {
            let mut scope_attrs = HashMap::new();
            if let Some(scope) = &scope_logs.scope {
                if !scope.name.is_empty() {
                    scope_attrs.insert("otel.scope.name".to_string(), scope.name.clone());
                }
                if !scope.version.is_empty() {
                    scope_attrs.insert("otel.scope.version".to_string(), scope.version.clone());
                }
            }

            for log_record in scope_logs.log_records {
                let mut attributes = convert_attributes(&log_record.attributes);
                attributes.extend(scope_attrs.clone());

                let body = log_record
                    .body
                    .as_ref()
                    .map(any_value_to_string)
                    .unwrap_or_default();

                let severity = convert_severity(log_record.severity_number);

                let severity_text = if log_record.severity_text.is_empty() {
                    None
                } else {
                    Some(log_record.severity_text)
                };

                let trace_id = if log_record.trace_id.is_empty() {
                    None
                } else {
                    Some(bytes_to_hex(&log_record.trace_id))
                };

                let span_id = if log_record.span_id.is_empty() {
                    None
                } else {
                    Some(bytes_to_hex(&log_record.span_id))
                };

                logs.push(LogRecord {
                    timestamp: log_record.time_unix_nano as i64,
                    observed_timestamp: Some(log_record.observed_time_unix_nano as i64),
                    severity,
                    severity_text,
                    body,
                    attributes,
                    trace_id,
                    span_id,
                    resource: resource.clone(),
                });
            }
        }
    }

    logs
}

/// Convert OTLP traces request to internal traces
pub fn convert_traces(request: ExportTraceServiceRequest) -> Vec<Trace> {
    let mut traces: HashMap<String, Trace> = HashMap::new();

    for resource_spans in request.resource_spans {
        let resource = convert_resource(resource_spans.resource);

        for scope_spans in resource_spans.scope_spans {
            let mut scope_attrs = HashMap::new();
            if let Some(scope) = &scope_spans.scope {
                if !scope.name.is_empty() {
                    scope_attrs.insert("otel.scope.name".to_string(), scope.name.clone());
                }
                if !scope.version.is_empty() {
                    scope_attrs.insert("otel.scope.version".to_string(), scope.version.clone());
                }
            }

            for span in scope_spans.spans {
                let trace_id = bytes_to_hex(&span.trace_id);
                let span_id = bytes_to_hex(&span.span_id);

                let parent_span_id = if span.parent_span_id.is_empty() {
                    None
                } else {
                    Some(bytes_to_hex(&span.parent_span_id))
                };

                let kind = SpanKind::from_i32(span.kind).unwrap_or(SpanKind::Internal);

                let mut attributes = convert_attributes(&span.attributes);
                attributes.extend(scope_attrs.clone());

                let events: Vec<SpanEvent> = span
                    .events
                    .into_iter()
                    .map(|event| SpanEvent {
                        name: event.name,
                        timestamp: event.time_unix_nano as i64,
                        attributes: convert_attributes(&event.attributes),
                    })
                    .collect();

                let status = span.status.map_or(
                    SpanStatus {
                        code: StatusCode::Unset,
                        message: None,
                    },
                    |s| SpanStatus {
                        code: StatusCode::from_i32(s.code).unwrap_or(StatusCode::Unset),
                        message: if s.message.is_empty() {
                            None
                        } else {
                            Some(s.message)
                        },
                    },
                );

                let internal_span = Span {
                    trace_id: trace_id.clone(),
                    span_id,
                    parent_span_id,
                    name: span.name,
                    kind,
                    start_time: span.start_time_unix_nano as i64,
                    end_time: span.end_time_unix_nano as i64,
                    attributes,
                    events,
                    status,
                    resource: resource.clone(),
                };

                traces
                    .entry(trace_id.clone())
                    .or_insert_with(|| Trace {
                        trace_id: trace_id.clone(),
                        spans: Vec::new(),
                        resource: resource.clone(),
                    })
                    .spans
                    .push(internal_span);
            }
        }
    }

    traces.into_values().collect()
}

/// Convert OTLP metrics request to internal metrics
pub fn convert_metrics(request: ExportMetricsServiceRequest) -> Vec<Metric> {
    let mut metrics = Vec::new();

    for resource_metrics in request.resource_metrics {
        let resource = convert_resource(resource_metrics.resource);

        for scope_metrics in resource_metrics.scope_metrics {
            let mut scope_attrs = HashMap::new();
            if let Some(scope) = &scope_metrics.scope {
                if !scope.name.is_empty() {
                    scope_attrs.insert("otel.scope.name".to_string(), scope.name.clone());
                }
                if !scope.version.is_empty() {
                    scope_attrs.insert("otel.scope.version".to_string(), scope.version.clone());
                }
            }

            for metric in scope_metrics.metrics {
                let description = if metric.description.is_empty() {
                    None
                } else {
                    Some(metric.description)
                };

                let unit = if metric.unit.is_empty() {
                    None
                } else {
                    Some(metric.unit)
                };

                if let Some(data) = metric.data {
                    use opentelemetry_proto::tonic::metrics::v1::metric::Data;

                    match data {
                        Data::Gauge(gauge) => {
                            for data_point in gauge.data_points {
                                let mut attributes = convert_attributes(&data_point.attributes);
                                attributes.extend(scope_attrs.clone());

                                let value = match data_point.value {
                                    Some(
                                        opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(v),
                                    ) => v,
                                    Some(
                                        opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(v),
                                    ) => v as f64,
                                    None => 0.0,
                                };

                                metrics.push(Metric {
                                    name: metric.name.clone(),
                                    description: description.clone(),
                                    unit: unit.clone(),
                                    metric_type: MetricType::Gauge(value),
                                    timestamp: data_point.time_unix_nano as i64,
                                    attributes,
                                    resource: resource.clone(),
                                });
                            }
                        },
                        Data::Sum(sum) => {
                            for data_point in sum.data_points {
                                let mut attributes = convert_attributes(&data_point.attributes);
                                attributes.extend(scope_attrs.clone());

                                let value = match data_point.value {
                                    Some(
                                        opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(v),
                                    ) => v as u64,
                                    Some(
                                        opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(v),
                                    ) => v as u64,
                                    None => 0,
                                };

                                metrics.push(Metric {
                                    name: metric.name.clone(),
                                    description: description.clone(),
                                    unit: unit.clone(),
                                    metric_type: MetricType::Counter(value),
                                    timestamp: data_point.time_unix_nano as i64,
                                    attributes,
                                    resource: resource.clone(),
                                });
                            }
                        },
                        Data::Histogram(histogram) => {
                            for data_point in histogram.data_points {
                                let mut attributes = convert_attributes(&data_point.attributes);
                                attributes.extend(scope_attrs.clone());

                                let buckets: Vec<HistogramBucket> = data_point
                                    .bucket_counts
                                    .iter()
                                    .zip(data_point.explicit_bounds.iter())
                                    .map(|(count, bound)| HistogramBucket {
                                        upper_bound: *bound,
                                        count: *count,
                                    })
                                    .collect();

                                metrics.push(Metric {
                                    name: metric.name.clone(),
                                    description: description.clone(),
                                    unit: unit.clone(),
                                    metric_type: MetricType::Histogram {
                                        count: data_point.count,
                                        sum: data_point.sum.unwrap_or(0.0),
                                        buckets,
                                    },
                                    timestamp: data_point.time_unix_nano as i64,
                                    attributes,
                                    resource: resource.clone(),
                                });
                            }
                        },
                        Data::Summary(summary) => {
                            for data_point in summary.data_points {
                                let mut attributes = convert_attributes(&data_point.attributes);
                                attributes.extend(scope_attrs.clone());

                                let quantiles: Vec<Quantile> = data_point
                                    .quantile_values
                                    .iter()
                                    .map(|qv| Quantile {
                                        quantile: qv.quantile,
                                        value: qv.value,
                                    })
                                    .collect();

                                metrics.push(Metric {
                                    name: metric.name.clone(),
                                    description: description.clone(),
                                    unit: unit.clone(),
                                    metric_type: MetricType::Summary {
                                        count: data_point.count,
                                        sum: data_point.sum,
                                        quantiles,
                                    },
                                    timestamp: data_point.time_unix_nano as i64,
                                    attributes,
                                    resource: resource.clone(),
                                });
                            }
                        },
                        Data::ExponentialHistogram(_) => {
                            // Not supported in internal types, skip
                        },
                    }
                }
            }
        }
    }

    metrics
}

// Helper functions

/// Convert OTLP Resource to internal Resource
fn convert_resource(
    otlp_resource: Option<opentelemetry_proto::tonic::resource::v1::Resource>,
) -> Option<Resource> {
    otlp_resource.map(|r| Resource {
        attributes: convert_attributes(&r.attributes),
    })
}

/// Convert OTLP KeyValue vec to HashMap<String, String>
fn convert_attributes(kvs: &[KeyValue]) -> HashMap<String, String> {
    kvs.iter()
        .map(|kv| {
            let value = kv
                .value
                .as_ref()
                .map(any_value_to_string)
                .unwrap_or_default();
            (kv.key.clone(), value)
        })
        .collect()
}

/// Convert OTLP AnyValue to string
fn any_value_to_string(value: &AnyValue) -> String {
    match &value.value {
        Some(any_value::Value::StringValue(s)) => s.clone(),
        Some(any_value::Value::BoolValue(b)) => b.to_string(),
        Some(any_value::Value::IntValue(i)) => i.to_string(),
        Some(any_value::Value::DoubleValue(d)) => d.to_string(),
        Some(any_value::Value::BytesValue(b)) => format!("{:?}", b),
        Some(any_value::Value::ArrayValue(arr)) => {
            let values: Vec<String> = arr.values.iter().map(any_value_to_string).collect();
            format!("[{}]", values.join(", "))
        },
        Some(any_value::Value::KvlistValue(kvlist)) => {
            let pairs: Vec<String> = kvlist
                .values
                .iter()
                .map(|kv| {
                    let v = kv
                        .value
                        .as_ref()
                        .map(any_value_to_string)
                        .unwrap_or_default();
                    format!("{}={}", kv.key, v)
                })
                .collect();
            format!("{{{}}}", pairs.join(", "))
        },
        None => String::new(),
    }
}

/// Convert bytes to lowercase hex string
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Convert OTLP severity number to internal SeverityLevel
fn convert_severity(severity_number: i32) -> SeverityLevel {
    // OTLP severity numbers: 1-4=TRACE, 5-8=DEBUG, 9-12=INFO, 13-16=WARN, 17-20=ERROR, 21-24=FATAL
    match severity_number {
        1..=4 => SeverityLevel::Trace,
        5..=8 => SeverityLevel::Debug,
        9..=12 => SeverityLevel::Info,
        13..=16 => SeverityLevel::Warn,
        17..=20 => SeverityLevel::Error,
        21..=24 => SeverityLevel::Fatal,
        _ => SeverityLevel::Info, // Default to Info for unknown values
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_proto::tonic::common::v1::InstrumentationScope;
    use opentelemetry_proto::tonic::logs::v1::{
        LogRecord as OtlpLogRecord, ResourceLogs, ScopeLogs,
    };
    use opentelemetry_proto::tonic::metrics::v1::{
        metric::Data, number_data_point, summary_data_point::ValueAtQuantile, Gauge, Histogram,
        HistogramDataPoint, Metric as OtlpMetric, NumberDataPoint, ResourceMetrics, ScopeMetrics,
        Sum, Summary, SummaryDataPoint,
    };
    use opentelemetry_proto::tonic::trace::v1::{
        span::Event, ResourceSpans, ScopeSpans, Span as OtlpSpan, Status,
    };

    // Helper tests

    #[test]
    fn test_bytes_to_hex() {
        assert_eq!(bytes_to_hex(&[]), "");
        assert_eq!(bytes_to_hex(&[0x01, 0x02, 0x03]), "010203");
        assert_eq!(
            bytes_to_hex(&[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]),
            "0102030405060708"
        );
        assert_eq!(bytes_to_hex(&[0xff, 0xaa, 0xbb]), "ffaabb");
    }

    #[test]
    fn test_any_value_to_string_all_types() {
        let string_val = AnyValue {
            value: Some(any_value::Value::StringValue("test".to_string())),
        };
        assert_eq!(any_value_to_string(&string_val), "test");

        let bool_val = AnyValue {
            value: Some(any_value::Value::BoolValue(true)),
        };
        assert_eq!(any_value_to_string(&bool_val), "true");

        let int_val = AnyValue {
            value: Some(any_value::Value::IntValue(42)),
        };
        assert_eq!(any_value_to_string(&int_val), "42");

        let double_val = AnyValue {
            value: Some(any_value::Value::DoubleValue(3.15)),
        };
        assert_eq!(any_value_to_string(&double_val), "3.15");

        let empty_val = AnyValue { value: None };
        assert_eq!(any_value_to_string(&empty_val), "");
    }

    #[test]
    fn test_convert_attributes() {
        let kvs = vec![
            KeyValue {
                key: "key1".to_string(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::StringValue("value1".to_string())),
                }),
            },
            KeyValue {
                key: "key2".to_string(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::IntValue(42)),
                }),
            },
        ];

        let attrs = convert_attributes(&kvs);
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs.get("key1"), Some(&"value1".to_string()));
        assert_eq!(attrs.get("key2"), Some(&"42".to_string()));
    }

    #[test]
    fn test_convert_resource() {
        let otlp_resource = opentelemetry_proto::tonic::resource::v1::Resource {
            attributes: vec![KeyValue {
                key: "service.name".to_string(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::StringValue("test-service".to_string())),
                }),
            }],
            dropped_attributes_count: 0,
            entity_refs: vec![],
        };

        let resource = convert_resource(Some(otlp_resource)).unwrap();
        assert_eq!(resource.attributes.len(), 1);
        assert_eq!(
            resource.attributes.get("service.name"),
            Some(&"test-service".to_string())
        );
    }

    #[test]
    fn test_convert_severity() {
        assert_eq!(convert_severity(1), SeverityLevel::Trace);
        assert_eq!(convert_severity(5), SeverityLevel::Debug);
        assert_eq!(convert_severity(9), SeverityLevel::Info);
        assert_eq!(convert_severity(13), SeverityLevel::Warn);
        assert_eq!(convert_severity(17), SeverityLevel::Error);
        assert_eq!(convert_severity(21), SeverityLevel::Fatal);
        assert_eq!(convert_severity(0), SeverityLevel::Info);
        assert_eq!(convert_severity(100), SeverityLevel::Info);
    }

    // Logs tests

    #[test]
    fn test_convert_empty_logs_request() {
        let request = ExportLogsServiceRequest {
            resource_logs: vec![],
        };
        let logs = convert_logs(request);
        assert_eq!(logs.len(), 0);
    }

    #[test]
    fn test_convert_single_log() {
        let request = ExportLogsServiceRequest {
            resource_logs: vec![ResourceLogs {
                resource: Some(opentelemetry_proto::tonic::resource::v1::Resource {
                    attributes: vec![KeyValue {
                        key: "service.name".to_string(),
                        value: Some(AnyValue {
                            value: Some(any_value::Value::StringValue("test-service".to_string())),
                        }),
                    }],
                    dropped_attributes_count: 0,
                    entity_refs: vec![],
                }),
                scope_logs: vec![ScopeLogs {
                    scope: Some(InstrumentationScope {
                        name: "test-scope".to_string(),
                        version: "1.0.0".to_string(),
                        attributes: vec![],
                        dropped_attributes_count: 0,
                    }),
                    log_records: vec![OtlpLogRecord {
                        time_unix_nano: 1234567890,
                        observed_time_unix_nano: 1234567891,
                        severity_number: 9,
                        severity_text: "INFO".to_string(),
                        body: Some(AnyValue {
                            value: Some(any_value::Value::StringValue("Test log".to_string())),
                        }),
                        attributes: vec![KeyValue {
                            key: "log.key".to_string(),
                            value: Some(AnyValue {
                                value: Some(any_value::Value::StringValue("log.value".to_string())),
                            }),
                        }],
                        dropped_attributes_count: 0,
                        flags: 0,
                        event_name: String::new(),
                        trace_id: vec![],
                        span_id: vec![],
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let logs = convert_logs(request);
        assert_eq!(logs.len(), 1);

        let log = &logs[0];
        assert_eq!(log.timestamp, 1234567890);
        assert_eq!(log.observed_timestamp, Some(1234567891));
        assert_eq!(log.severity, SeverityLevel::Info);
        assert_eq!(log.severity_text, Some("INFO".to_string()));
        assert_eq!(log.body, "Test log");
        assert_eq!(
            log.attributes.get("log.key"),
            Some(&"log.value".to_string())
        );
        assert_eq!(
            log.attributes.get("otel.scope.name"),
            Some(&"test-scope".to_string())
        );
        assert_eq!(
            log.attributes.get("otel.scope.version"),
            Some(&"1.0.0".to_string())
        );
        assert!(log.resource.is_some());
    }

    #[test]
    fn test_convert_multiple_resources() {
        let request = ExportLogsServiceRequest {
            resource_logs: vec![
                ResourceLogs {
                    resource: Some(opentelemetry_proto::tonic::resource::v1::Resource {
                        attributes: vec![KeyValue {
                            key: "service.name".to_string(),
                            value: Some(AnyValue {
                                value: Some(any_value::Value::StringValue("service1".to_string())),
                            }),
                        }],
                        dropped_attributes_count: 0,
                        entity_refs: vec![],
                    }),
                    scope_logs: vec![ScopeLogs {
                        scope: None,
                        log_records: vec![OtlpLogRecord {
                            time_unix_nano: 1000,
                            observed_time_unix_nano: 1000,
                            severity_number: 9,
                            severity_text: "".to_string(),
                            body: Some(AnyValue {
                                value: Some(any_value::Value::StringValue("Log 1".to_string())),
                            }),
                            attributes: vec![],
                            dropped_attributes_count: 0,
                            flags: 0,
                            event_name: String::new(),
                            trace_id: vec![],
                            span_id: vec![],
                        }],
                        schema_url: "".to_string(),
                    }],
                    schema_url: "".to_string(),
                },
                ResourceLogs {
                    resource: Some(opentelemetry_proto::tonic::resource::v1::Resource {
                        attributes: vec![KeyValue {
                            key: "service.name".to_string(),
                            value: Some(AnyValue {
                                value: Some(any_value::Value::StringValue("service2".to_string())),
                            }),
                        }],
                        dropped_attributes_count: 0,
                        entity_refs: vec![],
                    }),
                    scope_logs: vec![ScopeLogs {
                        scope: None,
                        log_records: vec![OtlpLogRecord {
                            time_unix_nano: 2000,
                            observed_time_unix_nano: 2000,
                            severity_number: 17,
                            severity_text: "".to_string(),
                            body: Some(AnyValue {
                                value: Some(any_value::Value::StringValue("Log 2".to_string())),
                            }),
                            attributes: vec![],
                            dropped_attributes_count: 0,
                            flags: 0,
                            event_name: String::new(),
                            trace_id: vec![],
                            span_id: vec![],
                        }],
                        schema_url: "".to_string(),
                    }],
                    schema_url: "".to_string(),
                },
            ],
        };

        let logs = convert_logs(request);
        assert_eq!(logs.len(), 2);

        assert_eq!(
            logs[0]
                .resource
                .as_ref()
                .unwrap()
                .attributes
                .get("service.name"),
            Some(&"service1".to_string())
        );
        assert_eq!(
            logs[1]
                .resource
                .as_ref()
                .unwrap()
                .attributes
                .get("service.name"),
            Some(&"service2".to_string())
        );
    }

    #[test]
    fn test_convert_missing_fields() {
        let request = ExportLogsServiceRequest {
            resource_logs: vec![ResourceLogs {
                resource: None,
                scope_logs: vec![ScopeLogs {
                    scope: None,
                    log_records: vec![OtlpLogRecord {
                        time_unix_nano: 1000,
                        observed_time_unix_nano: 1000,
                        severity_number: 0,
                        severity_text: "".to_string(),
                        body: None,
                        attributes: vec![],
                        dropped_attributes_count: 0,
                        flags: 0,
                        event_name: String::new(),
                        trace_id: vec![],
                        span_id: vec![],
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let logs = convert_logs(request);
        assert_eq!(logs.len(), 1);

        let log = &logs[0];
        assert_eq!(log.body, "");
        assert_eq!(log.severity, SeverityLevel::Info);
        assert_eq!(log.severity_text, None);
        assert!(log.resource.is_none());
        assert!(log.trace_id.is_none());
        assert!(log.span_id.is_none());
    }

    #[test]
    fn test_convert_log_with_trace_context() {
        let request = ExportLogsServiceRequest {
            resource_logs: vec![ResourceLogs {
                resource: None,
                scope_logs: vec![ScopeLogs {
                    scope: None,
                    log_records: vec![OtlpLogRecord {
                        time_unix_nano: 1000,
                        observed_time_unix_nano: 1000,
                        severity_number: 9,
                        severity_text: "".to_string(),
                        body: Some(AnyValue {
                            value: Some(any_value::Value::StringValue("Log".to_string())),
                        }),
                        attributes: vec![],
                        dropped_attributes_count: 0,
                        flags: 0,
                        event_name: String::new(),
                        trace_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                        span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let logs = convert_logs(request);
        assert_eq!(logs.len(), 1);

        let log = &logs[0];
        assert_eq!(
            log.trace_id,
            Some("0102030405060708090a0b0c0d0e0f10".to_string())
        );
        assert_eq!(log.span_id, Some("0102030405060708".to_string()));
    }

    // Traces tests

    #[test]
    fn test_convert_empty_traces_request() {
        let request = ExportTraceServiceRequest {
            resource_spans: vec![],
        };
        let traces = convert_traces(request);
        assert_eq!(traces.len(), 0);
    }

    #[test]
    fn test_convert_single_span() {
        let request = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: Some(opentelemetry_proto::tonic::resource::v1::Resource {
                    attributes: vec![KeyValue {
                        key: "service.name".to_string(),
                        value: Some(AnyValue {
                            value: Some(any_value::Value::StringValue("test-service".to_string())),
                        }),
                    }],
                    dropped_attributes_count: 0,
                    entity_refs: vec![],
                }),
                scope_spans: vec![ScopeSpans {
                    scope: Some(InstrumentationScope {
                        name: "test-scope".to_string(),
                        version: "1.0.0".to_string(),
                        attributes: vec![],
                        dropped_attributes_count: 0,
                    }),
                    spans: vec![OtlpSpan {
                        trace_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                        span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                        trace_state: "".to_string(),
                        parent_span_id: vec![],
                        name: "test-span".to_string(),
                        kind: 1,
                        start_time_unix_nano: 1000,
                        end_time_unix_nano: 2000,
                        attributes: vec![],
                        dropped_attributes_count: 0,
                        events: vec![],
                        dropped_events_count: 0,
                        links: vec![],
                        dropped_links_count: 0,
                        status: None,
                        flags: 0,
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let traces = convert_traces(request);
        assert_eq!(traces.len(), 1);

        let trace = &traces[0];
        assert_eq!(trace.spans.len(), 1);

        let span = &trace.spans[0];
        assert_eq!(span.trace_id, "0102030405060708090a0b0c0d0e0f10");
        assert_eq!(span.span_id, "0102030405060708");
        assert_eq!(span.name, "test-span");
        assert!(span.parent_span_id.is_none());
        assert_eq!(
            span.attributes.get("otel.scope.name"),
            Some(&"test-scope".to_string())
        );
    }

    #[test]
    fn test_convert_multiple_spans_same_trace() {
        let trace_id = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];

        let request = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: None,
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans: vec![
                        OtlpSpan {
                            trace_id: trace_id.clone(),
                            span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                            trace_state: "".to_string(),
                            parent_span_id: vec![],
                            name: "span1".to_string(),
                            kind: 1,
                            start_time_unix_nano: 1000,
                            end_time_unix_nano: 2000,
                            attributes: vec![],
                            dropped_attributes_count: 0,
                            events: vec![],
                            dropped_events_count: 0,
                            links: vec![],
                            dropped_links_count: 0,
                            status: None,
                            flags: 0,
                        },
                        OtlpSpan {
                            trace_id: trace_id.clone(),
                            span_id: vec![9, 10, 11, 12, 13, 14, 15, 16],
                            trace_state: "".to_string(),
                            parent_span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                            name: "span2".to_string(),
                            kind: 1,
                            start_time_unix_nano: 1500,
                            end_time_unix_nano: 1800,
                            attributes: vec![],
                            dropped_attributes_count: 0,
                            events: vec![],
                            dropped_events_count: 0,
                            links: vec![],
                            dropped_links_count: 0,
                            status: None,
                            flags: 0,
                        },
                    ],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let traces = convert_traces(request);
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].spans.len(), 2);
    }

    #[test]
    fn test_convert_multiple_traces() {
        let request = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: None,
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans: vec![
                        OtlpSpan {
                            trace_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                            span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                            trace_state: "".to_string(),
                            parent_span_id: vec![],
                            name: "span1".to_string(),
                            kind: 1,
                            start_time_unix_nano: 1000,
                            end_time_unix_nano: 2000,
                            attributes: vec![],
                            dropped_attributes_count: 0,
                            events: vec![],
                            dropped_events_count: 0,
                            links: vec![],
                            dropped_links_count: 0,
                            status: None,
                            flags: 0,
                        },
                        OtlpSpan {
                            trace_id: vec![16, 15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
                            span_id: vec![8, 7, 6, 5, 4, 3, 2, 1],
                            trace_state: "".to_string(),
                            parent_span_id: vec![],
                            name: "span2".to_string(),
                            kind: 1,
                            start_time_unix_nano: 3000,
                            end_time_unix_nano: 4000,
                            attributes: vec![],
                            dropped_attributes_count: 0,
                            events: vec![],
                            dropped_events_count: 0,
                            links: vec![],
                            dropped_links_count: 0,
                            status: None,
                            flags: 0,
                        },
                    ],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let traces = convert_traces(request);
        assert_eq!(traces.len(), 2);
    }

    #[test]
    fn test_convert_span_with_parent() {
        let request = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: None,
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans: vec![OtlpSpan {
                        trace_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                        span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                        trace_state: "".to_string(),
                        parent_span_id: vec![9, 10, 11, 12, 13, 14, 15, 16],
                        name: "child-span".to_string(),
                        kind: 1,
                        start_time_unix_nano: 1000,
                        end_time_unix_nano: 2000,
                        attributes: vec![],
                        dropped_attributes_count: 0,
                        events: vec![],
                        dropped_events_count: 0,
                        links: vec![],
                        dropped_links_count: 0,
                        status: None,
                        flags: 0,
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let traces = convert_traces(request);
        assert_eq!(traces.len(), 1);

        let span = &traces[0].spans[0];
        assert_eq!(span.parent_span_id, Some("090a0b0c0d0e0f10".to_string()));
    }

    #[test]
    fn test_convert_span_with_events() {
        let request = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: None,
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans: vec![OtlpSpan {
                        trace_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                        span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                        trace_state: "".to_string(),
                        parent_span_id: vec![],
                        name: "span-with-events".to_string(),
                        kind: 1,
                        start_time_unix_nano: 1000,
                        end_time_unix_nano: 2000,
                        attributes: vec![],
                        dropped_attributes_count: 0,
                        events: vec![Event {
                            time_unix_nano: 1500,
                            name: "test-event".to_string(),
                            attributes: vec![KeyValue {
                                key: "event.key".to_string(),
                                value: Some(AnyValue {
                                    value: Some(any_value::Value::StringValue(
                                        "event.value".to_string(),
                                    )),
                                }),
                            }],
                            dropped_attributes_count: 0,
                        }],
                        dropped_events_count: 0,
                        links: vec![],
                        dropped_links_count: 0,
                        status: None,
                        flags: 0,
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let traces = convert_traces(request);
        assert_eq!(traces.len(), 1);

        let span = &traces[0].spans[0];
        assert_eq!(span.events.len(), 1);
        assert_eq!(span.events[0].name, "test-event");
        assert_eq!(span.events[0].timestamp, 1500);
    }

    #[test]
    fn test_convert_span_with_status() {
        let request = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: None,
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans: vec![OtlpSpan {
                        trace_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                        span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                        trace_state: "".to_string(),
                        parent_span_id: vec![],
                        name: "span-with-status".to_string(),
                        kind: 1,
                        start_time_unix_nano: 1000,
                        end_time_unix_nano: 2000,
                        attributes: vec![],
                        dropped_attributes_count: 0,
                        events: vec![],
                        dropped_events_count: 0,
                        links: vec![],
                        dropped_links_count: 0,
                        status: Some(Status {
                            message: "Error occurred".to_string(),
                            code: 2,
                        }),
                        flags: 0,
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let traces = convert_traces(request);
        assert_eq!(traces.len(), 1);

        let span = &traces[0].spans[0];
        assert_eq!(span.status.code, StatusCode::Error);
        assert_eq!(span.status.message, Some("Error occurred".to_string()));
    }

    #[test]
    fn test_convert_span_kinds() {
        let kinds = vec![
            (0, SpanKind::Internal),
            (1, SpanKind::Server),
            (2, SpanKind::Client),
            (3, SpanKind::Producer),
            (4, SpanKind::Consumer),
        ];

        for (otlp_kind, expected_kind) in kinds {
            let request = ExportTraceServiceRequest {
                resource_spans: vec![ResourceSpans {
                    resource: None,
                    scope_spans: vec![ScopeSpans {
                        scope: None,
                        spans: vec![OtlpSpan {
                            trace_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                            span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                            trace_state: "".to_string(),
                            parent_span_id: vec![],
                            name: "test-span".to_string(),
                            kind: otlp_kind,
                            start_time_unix_nano: 1000,
                            end_time_unix_nano: 2000,
                            attributes: vec![],
                            dropped_attributes_count: 0,
                            events: vec![],
                            dropped_events_count: 0,
                            links: vec![],
                            dropped_links_count: 0,
                            status: None,
                            flags: 0,
                        }],
                        schema_url: "".to_string(),
                    }],
                    schema_url: "".to_string(),
                }],
            };

            let traces = convert_traces(request);
            assert_eq!(traces[0].spans[0].kind, expected_kind);
        }
    }

    // Metrics tests

    #[test]
    fn test_convert_empty_metrics_request() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![],
        };
        let metrics = convert_metrics(request);
        assert_eq!(metrics.len(), 0);
    }

    #[test]
    fn test_convert_gauge_metric() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.gauge".to_string(),
                        description: "A test gauge".to_string(),
                        unit: "1".to_string(),
                        metadata: vec![],
                        data: Some(Data::Gauge(Gauge {
                            data_points: vec![NumberDataPoint {
                                attributes: vec![],
                                start_time_unix_nano: 1000,
                                time_unix_nano: 2000,
                                value: Some(number_data_point::Value::AsDouble(42.5)),
                                exemplars: vec![],
                                flags: 0,
                            }],
                        })),
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let metrics = convert_metrics(request);
        assert_eq!(metrics.len(), 1);

        let metric = &metrics[0];
        assert_eq!(metric.name, "test.gauge");
        assert_eq!(metric.description, Some("A test gauge".to_string()));
        assert_eq!(metric.unit, Some("1".to_string()));
        assert_eq!(metric.metric_type, MetricType::Gauge(42.5));
        assert_eq!(metric.timestamp, 2000);
    }

    #[test]
    fn test_convert_counter_metric() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.counter".to_string(),
                        description: "".to_string(),
                        unit: "".to_string(),
                        metadata: vec![],
                        data: Some(Data::Sum(Sum {
                            data_points: vec![NumberDataPoint {
                                attributes: vec![],
                                start_time_unix_nano: 1000,
                                time_unix_nano: 2000,
                                value: Some(number_data_point::Value::AsInt(100)),
                                exemplars: vec![],
                                flags: 0,
                            }],
                            aggregation_temporality: 0,
                            is_monotonic: true,
                        })),
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let metrics = convert_metrics(request);
        assert_eq!(metrics.len(), 1);

        let metric = &metrics[0];
        assert_eq!(metric.name, "test.counter");
        assert_eq!(metric.metric_type, MetricType::Counter(100));
    }

    #[test]
    fn test_convert_histogram_metric() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.histogram".to_string(),
                        description: "".to_string(),
                        unit: "ms".to_string(),
                        metadata: vec![],
                        data: Some(Data::Histogram(Histogram {
                            data_points: vec![HistogramDataPoint {
                                attributes: vec![],
                                start_time_unix_nano: 1000,
                                time_unix_nano: 2000,
                                count: 10,
                                sum: Some(100.0),
                                bucket_counts: vec![2, 5, 3],
                                explicit_bounds: vec![10.0, 50.0, 100.0],
                                exemplars: vec![],
                                flags: 0,
                                min: None,
                                max: None,
                            }],
                            aggregation_temporality: 0,
                        })),
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let metrics = convert_metrics(request);
        assert_eq!(metrics.len(), 1);

        let metric = &metrics[0];
        assert_eq!(metric.name, "test.histogram");
        match &metric.metric_type {
            MetricType::Histogram {
                count,
                sum,
                buckets,
            } => {
                assert_eq!(*count, 10);
                assert_eq!(*sum, 100.0);
                assert_eq!(buckets.len(), 3);
                assert_eq!(buckets[0].upper_bound, 10.0);
                assert_eq!(buckets[0].count, 2);
            },
            _ => panic!("Expected Histogram metric type"),
        }
    }

    #[test]
    fn test_convert_summary_metric() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.summary".to_string(),
                        description: "".to_string(),
                        unit: "".to_string(),
                        metadata: vec![],
                        data: Some(Data::Summary(Summary {
                            data_points: vec![SummaryDataPoint {
                                attributes: vec![],
                                start_time_unix_nano: 1000,
                                time_unix_nano: 2000,
                                count: 100,
                                sum: 500.0,
                                quantile_values: vec![
                                    ValueAtQuantile {
                                        quantile: 0.5,
                                        value: 50.0,
                                    },
                                    ValueAtQuantile {
                                        quantile: 0.95,
                                        value: 95.0,
                                    },
                                ],
                                flags: 0,
                            }],
                        })),
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let metrics = convert_metrics(request);
        assert_eq!(metrics.len(), 1);

        let metric = &metrics[0];
        match &metric.metric_type {
            MetricType::Summary {
                count,
                sum,
                quantiles,
            } => {
                assert_eq!(*count, 100);
                assert_eq!(*sum, 500.0);
                assert_eq!(quantiles.len(), 2);
                assert_eq!(quantiles[0].quantile, 0.5);
                assert_eq!(quantiles[0].value, 50.0);
            },
            _ => panic!("Expected Summary metric type"),
        }
    }

    #[test]
    fn test_convert_multiple_data_points() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.gauge".to_string(),
                        description: "".to_string(),
                        unit: "".to_string(),
                        metadata: vec![],
                        data: Some(Data::Gauge(Gauge {
                            data_points: vec![
                                NumberDataPoint {
                                    attributes: vec![],
                                    start_time_unix_nano: 1000,
                                    time_unix_nano: 2000,
                                    value: Some(number_data_point::Value::AsDouble(10.0)),
                                    exemplars: vec![],
                                    flags: 0,
                                },
                                NumberDataPoint {
                                    attributes: vec![],
                                    start_time_unix_nano: 2000,
                                    time_unix_nano: 3000,
                                    value: Some(number_data_point::Value::AsDouble(20.0)),
                                    exemplars: vec![],
                                    flags: 0,
                                },
                            ],
                        })),
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let metrics = convert_metrics(request);
        assert_eq!(metrics.len(), 2);
        assert_eq!(metrics[0].metric_type, MetricType::Gauge(10.0));
        assert_eq!(metrics[1].metric_type, MetricType::Gauge(20.0));
    }

    #[test]
    fn test_convert_missing_metric_value() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.gauge".to_string(),
                        description: "".to_string(),
                        unit: "".to_string(),
                        metadata: vec![],
                        data: Some(Data::Gauge(Gauge {
                            data_points: vec![NumberDataPoint {
                                attributes: vec![],
                                start_time_unix_nano: 1000,
                                time_unix_nano: 2000,
                                value: None,
                                exemplars: vec![],
                                flags: 0,
                            }],
                        })),
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };

        let metrics = convert_metrics(request);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].metric_type, MetricType::Gauge(0.0));
    }
}
