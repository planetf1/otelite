//! OTLP to internal type conversion functions
//!
//! This module provides functions to convert OpenTelemetry Protocol (OTLP)
//! protobuf types into otelite-core internal types.

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};
use otelite_core::telemetry::{
    log::{LogRecord, SeverityLevel},
    metric::{HistogramBucket, Metric, MetricType, Quantile},
    resource::Resource,
    trace::{Span, SpanEvent, SpanKind, SpanStatus, StatusCode, Trace},
};
use std::collections::HashMap;

const TRACE_ID_BYTES: usize = 16;
const SPAN_ID_BYTES: usize = 8;

/// Telemetry that OTLP carries but the internal model cannot store
/// (#256). Counted at conversion and reported to the exporter (partial
/// success) and the operator (warn log) — never silently dropped, and
/// never asserted stored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DroppedCounts {
    /// ExponentialHistogram data points (no internal exponential-histogram
    /// type).
    pub exponential_histogram_points: u64,
    /// Gauge/Sum data points with no value set. Storing them would
    /// fabricate a 0 measurement, so they are skipped.
    pub unset_value_data_points: u64,
    /// Classic-histogram data points that carried observations in the
    /// `+Inf` overflow bucket: bucket bounds are finite, so that tail
    /// cannot be represented in `HistogramBucket` and is not stored (the
    /// reader reconstructs tail mass from `count`/`sum`).
    pub histogram_overflow_data_points: u64,
    /// Total observations inside those overflow buckets.
    pub histogram_overflow_observations: u64,
    /// Span links (the internal span model has no links).
    pub span_links: u64,
}

impl DroppedCounts {
    pub fn total(&self) -> u64 {
        self.exponential_histogram_points
            + self.unset_value_data_points
            + self.histogram_overflow_data_points
            + self.histogram_overflow_observations
            + self.span_links
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// Data points that were not fully accepted — the unit of the OTLP
    /// `rejected_data_points` field (a `0`/absent value means the request
    /// was fully accepted, so lossily-stored points count too):
    /// exponential-histogram points and unset-value points were not stored
    /// at all, overflow points lost their `+Inf` bucket.
    pub fn rejected_data_points(&self) -> u64 {
        self.exponential_histogram_points
            .saturating_add(self.unset_value_data_points)
            .saturating_add(self.histogram_overflow_data_points)
    }

    /// One-line breakdown for warn logs and partial-success messages.
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.exponential_histogram_points > 0 {
            parts.push(format!(
                "{} exponential-histogram data point(s) (unsupported type)",
                self.exponential_histogram_points
            ));
        }
        if self.unset_value_data_points > 0 {
            parts.push(format!(
                "{} data point(s) with no value set",
                self.unset_value_data_points
            ));
        }
        if self.histogram_overflow_data_points > 0 {
            parts.push(format!(
                "{} histogram data point(s) with {} observation(s) in the +Inf overflow bucket (bucket bounds are finite; tail not stored)",
                self.histogram_overflow_data_points,
                self.histogram_overflow_observations
            ));
        }
        if self.span_links > 0 {
            parts.push(format!("{} span link(s) (not supported)", self.span_links));
        }
        parts.join(", ")
    }
}

pub struct TraceConversion {
    pub traces: Vec<Trace>,
    pub rejected_spans: usize,
    /// Telemetry dropped from *accepted* spans (reported separately from
    /// `rejected_spans`, which the protocol reserves for whole-span
    /// rejections).
    pub dropped: DroppedCounts,
}

/// Result of converting one OTLP metrics export: the storable metrics plus
/// everything the internal model could not represent (#256).
pub struct MetricConversion {
    pub metrics: Vec<Metric>,
    pub dropped: DroppedCounts,
}

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

                let trace_id = optional_otel_id(
                    &log_record.trace_id,
                    TRACE_ID_BYTES,
                    "trace_id",
                    "log record",
                );
                let span_id =
                    optional_otel_id(&log_record.span_id, SPAN_ID_BYTES, "span_id", "log record");

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
    convert_traces_with_rejections(request).traces
}

pub fn convert_traces_with_rejections(request: ExportTraceServiceRequest) -> TraceConversion {
    let mut traces: HashMap<String, Trace> = HashMap::new();
    let mut rejected_spans = 0;
    let mut dropped = DroppedCounts::default();

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
                let Some(trace_id) =
                    required_otel_id(&span.trace_id, TRACE_ID_BYTES, "trace_id", "span")
                else {
                    rejected_spans += 1;
                    continue;
                };
                let Some(span_id) =
                    required_otel_id(&span.span_id, SPAN_ID_BYTES, "span_id", "span")
                else {
                    rejected_spans += 1;
                    continue;
                };
                let parent_span_id = optional_otel_id(
                    &span.parent_span_id,
                    SPAN_ID_BYTES,
                    "parent_span_id",
                    "span",
                );

                let kind = otlp_span_kind(span.kind);

                // Span links have no home in the internal span model:
                // count them so the drop is reported, not silent (#256).
                dropped.span_links += span.links.len() as u64;

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

    TraceConversion {
        traces: traces.into_values().collect(),
        rejected_spans,
        dropped,
    }
}

/// Convert OTLP metrics request to internal metrics
pub fn convert_metrics(request: ExportMetricsServiceRequest) -> Vec<Metric> {
    convert_metrics_with_drops(request).metrics
}

/// Convert an OTLP metrics request, counting the telemetry the internal
/// model cannot represent (#256). The thin [`convert_metrics`] wrapper
/// keeps existing callers (CLI import, tests) unchanged.
pub fn convert_metrics_with_drops(request: ExportMetricsServiceRequest) -> MetricConversion {
    let mut metrics = Vec::new();
    let mut dropped = DroppedCounts::default();

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
                                // A data point with no value set carries no
                                // measurement: storing 0.0 would fabricate
                                // one, so count the drop instead (#256).
                                let Some(value) = data_point.value else {
                                    dropped.unset_value_data_points += 1;
                                    continue;
                                };
                                let value = match value {
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(v) => v,
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(v) => v as f64,
                                };

                                let mut attributes = convert_attributes(&data_point.attributes);
                                attributes.extend(scope_attrs.clone());

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
                                // Unset value: count the drop, don't store a
                                // fabricated 0 counter (#256).
                                let Some(value) = data_point.value else {
                                    dropped.unset_value_data_points += 1;
                                    continue;
                                };
                                let metric_type: MetricType = match value {
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(v) => {
                                        MetricType::Counter(v as u64)
                                    },
                                    // Double-typed sums (e.g. USD cost
                                    // counters) keep their fractional value
                                    // instead of being floored to an integer
                                    // at ingest (#252).
                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsDouble(v) => {
                                        MetricType::CounterDouble(v)
                                    },
                                };

                                let mut attributes = convert_attributes(&data_point.attributes);
                                attributes.extend(scope_attrs.clone());

                                metrics.push(Metric {
                                    name: metric.name.clone(),
                                    description: description.clone(),
                                    unit: unit.clone(),
                                    metric_type,
                                    timestamp: data_point.time_unix_nano as i64,
                                    attributes,
                                    resource: resource.clone(),
                                });
                            }
                        },
                        Data::Histogram(histogram) => {
                            for data_point in histogram.data_points {
                                // The OTLP spec gives `bucket_counts` one
                                // more entry than `explicit_bounds`: the
                                // trailing entry is the `+Inf` overflow
                                // bucket. Bounds are finite, so those
                                // observations are counted, not stored —
                                // the reader reconstructs the tail mass
                                // from `count`/`sum` (#256).
                                let bounds = data_point.explicit_bounds.len();
                                let overflow: u64 =
                                    data_point.bucket_counts.iter().skip(bounds).copied().sum();
                                if overflow > 0 {
                                    dropped.histogram_overflow_data_points += 1;
                                    dropped.histogram_overflow_observations += overflow;
                                }

                                let buckets: Vec<HistogramBucket> = data_point
                                    .bucket_counts
                                    .iter()
                                    .take(bounds)
                                    .zip(data_point.explicit_bounds.iter())
                                    .map(|(count, bound)| HistogramBucket {
                                        upper_bound: *bound,
                                        count: *count,
                                    })
                                    .collect();

                                let mut attributes = convert_attributes(&data_point.attributes);
                                attributes.extend(scope_attrs.clone());

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
                        Data::ExponentialHistogram(exp) => {
                            // Not supported in internal types: count the
                            // points so the exporter and the logs know they
                            // did not land (#256).
                            dropped.exponential_histogram_points += exp.data_points.len() as u64;
                        },
                    }
                }
            }
        }
    }

    MetricConversion { metrics, dropped }
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
        Some(any_value::Value::StringValueStrindex(i)) => {
            // Profile string-table reference (opentelemetry-proto 0.32,
            // profiles v1.10+): the table only travels with the profiles
            // signal, so in log/trace/metric envelopes the reference is
            // unresolvable — mark it rather than silently drop.
            format!("(unresolved strindex {i})")
        },
        None => String::new(),
    }
}

/// Convert bytes to lowercase hex string
fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn required_otel_id(
    bytes: &[u8],
    expected_len: usize,
    field: &'static str,
    record_type: &'static str,
) -> Option<String> {
    if is_valid_otel_id(bytes, expected_len) {
        return Some(bytes_to_hex(bytes));
    }

    tracing::warn!(
        field,
        expected_len,
        actual_len = bytes.len(),
        is_zero = !bytes.is_empty() && bytes.iter().all(|byte| *byte == 0),
        record_type,
        "Rejecting telemetry record with invalid OTLP identifier"
    );
    None
}

fn optional_otel_id(
    bytes: &[u8],
    expected_len: usize,
    field: &'static str,
    record_type: &'static str,
) -> Option<String> {
    if bytes.is_empty() {
        return None;
    }

    if is_valid_otel_id(bytes, expected_len) {
        return Some(bytes_to_hex(bytes));
    }

    tracing::warn!(
        field,
        expected_len,
        actual_len = bytes.len(),
        is_zero = bytes.iter().all(|byte| *byte == 0),
        record_type,
        "Omitting invalid optional OTLP identifier"
    );
    None
}

fn is_valid_otel_id(bytes: &[u8], expected_len: usize) -> bool {
    bytes.len() == expected_len && bytes.iter().any(|byte| *byte != 0)
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

/// Map an OTLP span kind ordinal onto the internal SpanKind. OTLP orders the
/// kinds UNSPECIFIED=0, INTERNAL=1, SERVER=2, CLIENT=3, PRODUCER=4,
/// CONSUMER=5, while the core enum's ordinals start at Internal=0 — feeding
/// the raw OTLP value to `SpanKind::from_i32` shifts every kind by one
/// (#233).
fn otlp_span_kind(kind: i32) -> SpanKind {
    match kind {
        2 => SpanKind::Server,
        3 => SpanKind::Client,
        4 => SpanKind::Producer,
        5 => SpanKind::Consumer,
        // UNSPECIFIED (0), INTERNAL (1) and unknown values
        _ => SpanKind::Internal,
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
        metric::Data, number_data_point, summary_data_point::ValueAtQuantile, ExponentialHistogram,
        ExponentialHistogramDataPoint, Gauge, Histogram, HistogramDataPoint, Metric as OtlpMetric,
        NumberDataPoint, ResourceMetrics, ScopeMetrics, Sum, Summary, SummaryDataPoint,
    };
    use opentelemetry_proto::tonic::trace::v1::{
        span::{Event, Link as OtlpSpanLink},
        ResourceSpans, ScopeSpans, Span as OtlpSpan, Status,
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
    fn test_is_valid_otel_id_requires_exact_nonzero_length() {
        assert!(is_valid_otel_id(&[1; TRACE_ID_BYTES], TRACE_ID_BYTES));
        assert!(is_valid_otel_id(&[1; SPAN_ID_BYTES], SPAN_ID_BYTES));
        assert!(!is_valid_otel_id(&[0; TRACE_ID_BYTES], TRACE_ID_BYTES));
        assert!(!is_valid_otel_id(&[1; TRACE_ID_BYTES + 1], TRACE_ID_BYTES));
        assert!(!is_valid_otel_id(&[1; SPAN_ID_BYTES - 1], SPAN_ID_BYTES));
    }

    #[test]
    fn test_convert_traces_rejects_invalid_ids_and_omits_invalid_parent() {
        let valid_trace_id = vec![1; TRACE_ID_BYTES];
        let valid_span_id = vec![2; SPAN_ID_BYTES];
        let request = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: None,
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans: vec![
                        OtlpSpan {
                            trace_id: vec![1; TRACE_ID_BYTES + 1],
                            span_id: valid_span_id.clone(),
                            name: "oversized-trace-id".to_string(),
                            ..Default::default()
                        },
                        OtlpSpan {
                            trace_id: valid_trace_id.clone(),
                            span_id: vec![2; SPAN_ID_BYTES + 1],
                            name: "oversized-span-id".to_string(),
                            ..Default::default()
                        },
                        OtlpSpan {
                            trace_id: valid_trace_id,
                            span_id: valid_span_id,
                            parent_span_id: vec![3; SPAN_ID_BYTES + 1],
                            name: "valid-child".to_string(),
                            ..Default::default()
                        },
                    ],
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        };

        let conversion = convert_traces_with_rejections(request);

        assert_eq!(conversion.rejected_spans, 2);
        assert_eq!(conversion.traces.len(), 1);
        assert_eq!(conversion.traces[0].spans.len(), 1);
        assert_eq!(conversion.traces[0].spans[0].name, "valid-child");
        assert_eq!(conversion.traces[0].spans[0].parent_span_id, None);
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
                ..Default::default()
            },
            KeyValue {
                key: "key2".to_string(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::IntValue(42)),
                }),
                ..Default::default()
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
                ..Default::default()
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
                        ..Default::default()
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
                            ..Default::default()
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
                            ..Default::default()
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
                            ..Default::default()
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
                        ..Default::default()
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
                                ..Default::default()
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
        // OTLP span kind ordinals: UNSPECIFIED=0, INTERNAL=1, SERVER=2,
        // CLIENT=3, PRODUCER=4, CONSUMER=5.
        let kinds = vec![
            (0, SpanKind::Internal),
            (1, SpanKind::Internal),
            (2, SpanKind::Server),
            (3, SpanKind::Client),
            (4, SpanKind::Producer),
            (5, SpanKind::Consumer),
            (99, SpanKind::Internal),
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

    /// A data point with no value set is counted and skipped — not stored
    /// as a fabricated 0 (#256).
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

        let conversion = convert_metrics_with_drops(request);
        assert!(
            conversion.metrics.is_empty(),
            "an unset value must not be stored as a fabricated 0 (#256)"
        );
        assert_eq!(conversion.dropped.unset_value_data_points, 1);
    }

    // ── Edge-case tests for issue #17 ────────────────────────────────────────

    /// NaN gauge value is stored as NaN (not replaced with 0 or panicked on).
    #[test]
    fn test_gauge_nan_value_stored_as_nan() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.nan".to_string(),
                        description: "".to_string(),
                        unit: "".to_string(),
                        metadata: vec![],
                        data: Some(Data::Gauge(Gauge {
                            data_points: vec![NumberDataPoint {
                                attributes: vec![],
                                start_time_unix_nano: 0,
                                time_unix_nano: 1000,
                                value: Some(number_data_point::Value::AsDouble(f64::NAN)),
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
        // NaN f64 is not equal to itself; this documents that NaN propagates unchanged.
        if let MetricType::Gauge(v) = metrics[0].metric_type {
            assert!(v.is_nan(), "expected NaN gauge value, got {v}");
        } else {
            panic!("expected Gauge");
        }
    }

    /// Sum with a negative i64 value: Rust `as u64` wraps (bit-reinterpretation).
    /// This documents the current behaviour so a future change would be caught.
    #[test]
    fn test_sum_negative_int_wraps_to_large_u64() {
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
                                start_time_unix_nano: 0,
                                time_unix_nano: 1000,
                                value: Some(number_data_point::Value::AsInt(-1)),
                                exemplars: vec![],
                                flags: 0,
                            }],
                            aggregation_temporality: 0,
                            is_monotonic: false,
                        })),
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };
        let metrics = convert_metrics(request);
        assert_eq!(metrics.len(), 1);
        // -1i64 as u64 == u64::MAX (two's complement wrap, documented behaviour).
        assert_eq!(metrics[0].metric_type, MetricType::Counter(u64::MAX));
    }

    /// Sum with a negative f64 value (UpDownCounter delta): preserved in the
    /// fractional counter arm instead of being saturated to 0 (#252).
    #[test]
    fn test_sum_negative_double_preserved_as_counter_double() {
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
                                start_time_unix_nano: 0,
                                time_unix_nano: 1000,
                                value: Some(number_data_point::Value::AsDouble(-42.0)),
                                exemplars: vec![],
                                flags: 0,
                            }],
                            aggregation_temporality: 0,
                            is_monotonic: false,
                        })),
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };
        let metrics = convert_metrics(request);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].metric_type, MetricType::CounterDouble(-42.0));
    }

    /// Sum with a fractional f64 value (e.g. USD cost counters): preserved
    /// exactly instead of being floored to an integer at ingest (#252).
    #[test]
    fn test_sum_double_fraction_preserved() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "cost.usage".to_string(),
                        description: "".to_string(),
                        unit: "USD".to_string(),
                        metadata: vec![],
                        data: Some(Data::Sum(Sum {
                            data_points: vec![NumberDataPoint {
                                attributes: vec![],
                                start_time_unix_nano: 0,
                                time_unix_nano: 1000,
                                value: Some(number_data_point::Value::AsDouble(19.87)),
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
        assert_eq!(metrics[0].metric_type, MetricType::CounterDouble(19.87));
    }

    /// ExponentialHistogram data points are not stored (no internal
    /// type) and are counted as dropped telemetry (#256).
    #[test]
    fn test_exponential_histogram_not_stored() {
        use opentelemetry_proto::tonic::metrics::v1::ExponentialHistogram;
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.expo".to_string(),
                        description: "".to_string(),
                        unit: "".to_string(),
                        metadata: vec![],
                        data: Some(Data::ExponentialHistogram(ExponentialHistogram {
                            data_points: vec![],
                            aggregation_temporality: 0,
                        })),
                    }],
                    schema_url: "".to_string(),
                }],
                schema_url: "".to_string(),
            }],
        };
        let metrics = convert_metrics(request);
        assert_eq!(
            metrics.len(),
            0,
            "ExponentialHistogram should produce no metrics"
        );
    }

    /// Histogram with 0 buckets is accepted; count and sum are still populated.
    #[test]
    fn test_histogram_zero_buckets() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.hist".to_string(),
                        description: "".to_string(),
                        unit: "".to_string(),
                        metadata: vec![],
                        data: Some(Data::Histogram(Histogram {
                            data_points: vec![HistogramDataPoint {
                                attributes: vec![],
                                start_time_unix_nano: 0,
                                time_unix_nano: 1000,
                                count: 5,
                                sum: Some(100.0),
                                bucket_counts: vec![],
                                explicit_bounds: vec![],
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
        match &metrics[0].metric_type {
            MetricType::Histogram {
                count,
                sum,
                buckets,
            } => {
                assert_eq!(*count, 5);
                assert_eq!(*sum, 100.0);
                assert!(buckets.is_empty());
            },
            other => panic!("expected Histogram, got {other:?}"),
        }
    }

    /// Timestamp at u64::MAX is stored as the i64 bit-reinterpretation (-1), not panicked on.
    #[test]
    fn test_timestamp_u64_max_stored_without_panic() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics: vec![OtlpMetric {
                        name: "test.ts".to_string(),
                        description: "".to_string(),
                        unit: "".to_string(),
                        metadata: vec![],
                        data: Some(Data::Gauge(Gauge {
                            data_points: vec![NumberDataPoint {
                                attributes: vec![],
                                start_time_unix_nano: 0,
                                time_unix_nano: u64::MAX,
                                value: Some(number_data_point::Value::AsDouble(1.0)),
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
        // u64::MAX as i64 == -1; document and do not panic.
        assert_eq!(metrics[0].timestamp, u64::MAX as i64);
    }

    /// AnyValue ArrayValue containing a nested ArrayValue serialises correctly.
    #[test]
    fn test_any_value_nested_array() {
        use opentelemetry_proto::tonic::common::v1::{AnyValue as Av, ArrayValue};
        let inner = Av {
            value: Some(any_value::Value::ArrayValue(ArrayValue {
                values: vec![Av {
                    value: Some(any_value::Value::IntValue(42)),
                }],
            })),
        };
        let outer = AnyValue {
            value: Some(any_value::Value::ArrayValue(ArrayValue {
                values: vec![
                    AnyValue {
                        value: Some(any_value::Value::StringValue("x".to_string())),
                    },
                    inner,
                ],
            })),
        };
        let s = any_value_to_string(&outer);
        // Should serialise to something containing 42 without panicking.
        assert!(
            s.contains("42"),
            "expected '42' in nested array output: {s}"
        );
        assert!(
            s.starts_with('['),
            "expected array output to start with '[': {s}"
        );
    }

    /// KvlistValue with 3 levels of nesting does not stack-overflow.
    #[test]
    fn test_any_value_deep_kvlist_nesting() {
        use opentelemetry_proto::tonic::common::v1::{AnyValue as Av, KeyValueList};
        fn make_kvlist(depth: u32, leaf: &str) -> Av {
            if depth == 0 {
                return Av {
                    value: Some(any_value::Value::StringValue(leaf.to_string())),
                };
            }
            Av {
                value: Some(any_value::Value::KvlistValue(KeyValueList {
                    values: vec![KeyValue {
                        key: format!("d{depth}"),
                        value: Some(make_kvlist(depth - 1, leaf)),
                        ..Default::default()
                    }],
                })),
            }
        }
        let nested = AnyValue {
            ..make_kvlist(3, "leaf")
        };
        let s = any_value_to_string(&nested);
        assert!(
            s.contains("leaf"),
            "expected 'leaf' in deeply-nested output: {s}"
        );
    }

    /// Attribute with a None value is stored as an empty string (not excluded).
    #[test]
    fn test_attribute_none_value_stored_as_empty_string() {
        let kvs = vec![KeyValue {
            key: "empty_key".to_string(),
            value: None,
            ..Default::default()
        }];
        let attrs = convert_attributes(&kvs);
        assert_eq!(attrs.len(), 1);
        assert_eq!(attrs.get("empty_key"), Some(&String::new()));
    }

    /// 100+ attributes on a single span are all preserved.
    #[test]
    fn test_many_attributes_all_preserved() {
        let kvs: Vec<KeyValue> = (0..120)
            .map(|i| KeyValue {
                key: format!("key.{i}"),
                value: Some(AnyValue {
                    value: Some(any_value::Value::IntValue(i)),
                }),
                ..Default::default()
            })
            .collect();
        let attrs = convert_attributes(&kvs);
        assert_eq!(attrs.len(), 120);
        assert_eq!(attrs.get("key.0"), Some(&"0".to_string()));
        assert_eq!(attrs.get("key.119"), Some(&"119".to_string()));
    }

    // ── #256: dropped telemetry is counted, never silent ─────────────────

    fn proto_gauge(name: &str, value: Option<number_data_point::Value>) -> OtlpMetric {
        OtlpMetric {
            name: name.to_string(),
            description: String::new(),
            unit: String::new(),
            data: Some(Data::Gauge(Gauge {
                data_points: vec![NumberDataPoint {
                    attributes: vec![],
                    start_time_unix_nano: 0,
                    time_unix_nano: 1,
                    value,
                    exemplars: vec![],
                    flags: 0,
                }],
            })),
            metadata: vec![],
        }
    }

    fn proto_metrics_req(metrics: Vec<OtlpMetric>) -> ExportMetricsServiceRequest {
        ExportMetricsServiceRequest {
            resource_metrics: vec![ResourceMetrics {
                resource: None,
                scope_metrics: vec![ScopeMetrics {
                    scope: None,
                    metrics,
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        }
    }

    /// An ExponentialHistogram point has no internal representation: it is
    /// counted (and reported to the exporter), never stored.
    #[test]
    fn test_exponential_histogram_points_counted_not_stored() {
        let metric = OtlpMetric {
            name: "exp".to_string(),
            description: String::new(),
            unit: String::new(),
            data: Some(Data::ExponentialHistogram(ExponentialHistogram {
                aggregation_temporality: 1,
                data_points: vec![ExponentialHistogramDataPoint {
                    count: 5,
                    ..Default::default()
                }],
            })),
            metadata: vec![],
        };
        let conversion = convert_metrics_with_drops(proto_metrics_req(vec![metric]));
        assert!(conversion.metrics.is_empty(), "nothing storable was stored");
        assert_eq!(conversion.dropped.exponential_histogram_points, 1);
        assert_eq!(conversion.dropped.rejected_data_points(), 1);
        assert!(
            conversion
                .dropped
                .summary()
                .contains("exponential-histogram"),
            "the summary must name the drop: {}",
            conversion.dropped.summary()
        );
    }

    /// A gauge/sum data point with no value set must be counted and
    /// skipped — not stored as a fabricated 0.
    #[test]
    fn test_unset_value_data_points_counted_not_stored_as_zero() {
        let conversion =
            convert_metrics_with_drops(proto_metrics_req(vec![proto_gauge("g", None)]));
        assert!(
            conversion.metrics.is_empty(),
            "an unset value must not be stored (old code stored 0.0)"
        );
        assert_eq!(conversion.dropped.unset_value_data_points, 1);
        assert_eq!(conversion.dropped.rejected_data_points(), 1);
    }

    /// The +Inf overflow bucket (the N+1th bucket_counts entry) is counted
    /// as dropped observations on a lossy point — not silently truncated,
    /// not stored (bucket bounds are finite).
    #[test]
    fn test_histogram_overflow_bucket_counted() {
        let metric = OtlpMetric {
            name: "h".to_string(),
            description: String::new(),
            unit: String::new(),
            data: Some(Data::Histogram(Histogram {
                aggregation_temporality: 1,
                data_points: vec![HistogramDataPoint {
                    count: 5,
                    sum: Some(100.0),
                    // 1 bound → 2 bucket entries; the last is the +Inf tail.
                    bucket_counts: vec![2, 3],
                    explicit_bounds: vec![10.0],
                    ..Default::default()
                }],
            })),
            metadata: vec![],
        };
        let conversion = convert_metrics_with_drops(proto_metrics_req(vec![metric]));
        // The data point itself is stored (lossily)…
        assert_eq!(conversion.metrics.len(), 1);
        // …and the overflow observations are counted.
        assert_eq!(conversion.dropped.histogram_overflow_data_points, 1);
        assert_eq!(conversion.dropped.histogram_overflow_observations, 3);
        assert_eq!(conversion.dropped.rejected_data_points(), 1);
    }

    /// A clean histogram (zero overflow) drops nothing.
    #[test]
    fn test_histogram_without_overflow_drops_nothing() {
        let metric = OtlpMetric {
            name: "h".to_string(),
            description: String::new(),
            unit: String::new(),
            data: Some(Data::Histogram(Histogram {
                aggregation_temporality: 1,
                data_points: vec![HistogramDataPoint {
                    count: 5,
                    sum: Some(100.0),
                    bucket_counts: vec![5, 0],
                    explicit_bounds: vec![10.0],
                    ..Default::default()
                }],
            })),
            metadata: vec![],
        };
        let conversion = convert_metrics_with_drops(proto_metrics_req(vec![metric]));
        assert_eq!(conversion.metrics.len(), 1);
        assert!(conversion.dropped.is_empty());
    }

    /// Span links have no internal home: counted per accepted span, kept
    /// out of rejected_spans (the protocol field is for whole spans).
    #[test]
    fn test_span_links_counted_not_rejected() {
        let req = ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: None,
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans: vec![OtlpSpan {
                        trace_id: vec![1; 16],
                        span_id: vec![2; 8],
                        name: "s".to_string(),
                        links: vec![OtlpSpanLink::default(), OtlpSpanLink::default()],
                        ..Default::default()
                    }],
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        };
        let conversion = convert_traces_with_rejections(req);
        assert_eq!(conversion.traces.len(), 1, "the span itself is accepted");
        assert_eq!(conversion.rejected_spans, 0);
        assert_eq!(conversion.dropped.span_links, 2);
        assert_eq!(conversion.dropped.total(), 2);
    }

    /// The thin convert_metrics wrapper still returns the storable metrics
    /// (existing callers — CLI import, older tests — are unchanged).
    #[test]
    fn test_convert_metrics_wrapper_unchanged() {
        let req = proto_metrics_req(vec![proto_gauge(
            "g",
            Some(number_data_point::Value::AsInt(3)),
        )]);
        let metrics = convert_metrics(req);
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "g");
    }
}
