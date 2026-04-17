// Test utilities for gRPC testing

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{AnyValue, InstrumentationScope, KeyValue};
use opentelemetry_proto::tonic::logs::v1::{LogRecord, ResourceLogs, ScopeLogs};
use opentelemetry_proto::tonic::metrics::v1::{
    metric::Data, Gauge, Metric, NumberDataPoint, ResourceMetrics, ScopeMetrics,
};
use opentelemetry_proto::tonic::resource::v1::Resource;
use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span};
use std::time::{SystemTime, UNIX_EPOCH};

/// Create a sample metrics export request for testing
pub fn create_sample_metrics_request() -> ExportMetricsServiceRequest {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let data_point = NumberDataPoint {
        attributes: vec![KeyValue {
            key: "test_key".to_string(),
            value: Some(AnyValue {
                value: Some(
                    opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                        "test_value".to_string(),
                    ),
                ),
            }),
        }],
        start_time_unix_nano: timestamp - 1_000_000_000, // 1 second ago
        time_unix_nano: timestamp,
        value: Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(42)),
        exemplars: vec![],
        flags: 0,
    };

    let gauge = Gauge {
        data_points: vec![data_point],
    };

    let metric = Metric {
        name: "test_metric".to_string(),
        description: "A test metric".to_string(),
        unit: "1".to_string(),
        data: Some(Data::Gauge(gauge)),
    };

    let scope_metrics = ScopeMetrics {
        scope: Some(InstrumentationScope {
            name: "test_scope".to_string(),
            version: "1.0.0".to_string(),
            attributes: vec![],
            dropped_attributes_count: 0,
        }),
        metrics: vec![metric],
        schema_url: "".to_string(),
    };

    let resource_metrics = ResourceMetrics {
        resource: Some(Resource {
            attributes: vec![KeyValue {
                key: "service.name".to_string(),
                value: Some(AnyValue {
                    value: Some(
                        opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                            "test_service".to_string(),
                        ),
                    ),
                }),
            }],
            dropped_attributes_count: 0,
        }),
        scope_metrics: vec![scope_metrics],
        schema_url: "".to_string(),
    };

    ExportMetricsServiceRequest {
        resource_metrics: vec![resource_metrics],
    }
}

/// Create a sample logs export request for testing
pub fn create_sample_logs_request() -> ExportLogsServiceRequest {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let log_record = LogRecord {
        time_unix_nano: timestamp,
        observed_time_unix_nano: timestamp,
        severity_number: 9, // INFO
        severity_text: "INFO".to_string(),
        body: Some(AnyValue {
            value: Some(
                opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                    "Test log message".to_string(),
                ),
            ),
        }),
        attributes: vec![KeyValue {
            key: "log_key".to_string(),
            value: Some(AnyValue {
                value: Some(
                    opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                        "log_value".to_string(),
                    ),
                ),
            }),
        }],
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: vec![],
        span_id: vec![],
    };

    let scope_logs = ScopeLogs {
        scope: Some(InstrumentationScope {
            name: "test_scope".to_string(),
            version: "1.0.0".to_string(),
            attributes: vec![],
            dropped_attributes_count: 0,
        }),
        log_records: vec![log_record],
        schema_url: "".to_string(),
    };

    let resource_logs = ResourceLogs {
        resource: Some(Resource {
            attributes: vec![KeyValue {
                key: "service.name".to_string(),
                value: Some(AnyValue {
                    value: Some(
                        opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                            "test_service".to_string(),
                        ),
                    ),
                }),
            }],
            dropped_attributes_count: 0,
        }),
        scope_logs: vec![scope_logs],
        schema_url: "".to_string(),
    };

    ExportLogsServiceRequest {
        resource_logs: vec![resource_logs],
    }
}

/// Create a sample traces export request for testing
pub fn create_sample_traces_request() -> ExportTraceServiceRequest {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let span = Span {
        trace_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
        span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
        trace_state: "".to_string(),
        parent_span_id: vec![],
        name: "test_span".to_string(),
        kind: 1,                                         // SPAN_KIND_INTERNAL
        start_time_unix_nano: timestamp - 1_000_000_000, // 1 second ago
        end_time_unix_nano: timestamp,
        attributes: vec![KeyValue {
            key: "span_key".to_string(),
            value: Some(AnyValue {
                value: Some(
                    opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                        "span_value".to_string(),
                    ),
                ),
            }),
        }],
        dropped_attributes_count: 0,
        events: vec![],
        dropped_events_count: 0,
        links: vec![],
        dropped_links_count: 0,
        status: None,
        flags: 0,
    };

    let scope_spans = ScopeSpans {
        scope: Some(InstrumentationScope {
            name: "test_scope".to_string(),
            version: "1.0.0".to_string(),
            attributes: vec![],
            dropped_attributes_count: 0,
        }),
        spans: vec![span],
        schema_url: "".to_string(),
    };

    let resource_spans = ResourceSpans {
        resource: Some(Resource {
            attributes: vec![KeyValue {
                key: "service.name".to_string(),
                value: Some(AnyValue {
                    value: Some(
                        opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                            "test_service".to_string(),
                        ),
                    ),
                }),
            }],
            dropped_attributes_count: 0,
        }),
        scope_spans: vec![scope_spans],
        schema_url: "".to_string(),
    };

    ExportTraceServiceRequest {
        resource_spans: vec![resource_spans],
    }
}

/// Create a batch of metrics requests for load testing
pub fn create_metrics_batch(count: usize) -> Vec<ExportMetricsServiceRequest> {
    (0..count)
        .map(|_| create_sample_metrics_request())
        .collect()
}

/// Create a batch of logs requests for load testing
pub fn create_logs_batch(count: usize) -> Vec<ExportLogsServiceRequest> {
    (0..count).map(|_| create_sample_logs_request()).collect()
}

/// Create a batch of traces requests for load testing
pub fn create_traces_batch(count: usize) -> Vec<ExportTraceServiceRequest> {
    (0..count).map(|_| create_sample_traces_request()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_sample_metrics_request() {
        let request = create_sample_metrics_request();
        assert_eq!(request.resource_metrics.len(), 1);
        assert_eq!(request.resource_metrics[0].scope_metrics.len(), 1);
        assert_eq!(
            request.resource_metrics[0].scope_metrics[0].metrics.len(),
            1
        );
    }

    #[test]
    fn test_create_sample_logs_request() {
        let request = create_sample_logs_request();
        assert_eq!(request.resource_logs.len(), 1);
        assert_eq!(request.resource_logs[0].scope_logs.len(), 1);
        assert_eq!(request.resource_logs[0].scope_logs[0].log_records.len(), 1);
    }

    #[test]
    fn test_create_sample_traces_request() {
        let request = create_sample_traces_request();
        assert_eq!(request.resource_spans.len(), 1);
        assert_eq!(request.resource_spans[0].scope_spans.len(), 1);
        assert_eq!(request.resource_spans[0].scope_spans[0].spans.len(), 1);
    }

    #[test]
    fn test_create_metrics_batch() {
        let batch = create_metrics_batch(5);
        assert_eq!(batch.len(), 5);
    }

    #[test]
    fn test_create_logs_batch() {
        let batch = create_logs_batch(5);
        assert_eq!(batch.len(), 5);
    }

    #[test]
    fn test_create_traces_batch() {
        let batch = create_traces_batch(5);
        assert_eq!(batch.len(), 5);
    }
}

// Made with Bob
