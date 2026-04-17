// Test utilities for HTTP testing

use axum::body::Bytes;
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
use prost::Message;
use std::time::{SystemTime, UNIX_EPOCH};

/// Encode an OTLP request to protobuf bytes
pub fn encode_protobuf<T: Message>(request: &T) -> Bytes {
    let mut buf = Vec::new();
    request.encode(&mut buf).expect("Failed to encode protobuf");
    Bytes::from(buf)
}

/// Create sample metrics request
fn create_sample_metrics_request() -> ExportMetricsServiceRequest {
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
        start_time_unix_nano: timestamp - 1_000_000_000,
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

/// Create sample logs request
fn create_sample_logs_request() -> ExportLogsServiceRequest {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let log_record = LogRecord {
        time_unix_nano: timestamp,
        observed_time_unix_nano: timestamp,
        severity_number: 9,
        severity_text: "INFO".to_string(),
        body: Some(AnyValue {
            value: Some(
                opentelemetry_proto::tonic::common::v1::any_value::Value::StringValue(
                    "Test log message".to_string(),
                ),
            ),
        }),
        attributes: vec![],
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

/// Create sample traces request
fn create_sample_traces_request() -> ExportTraceServiceRequest {
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
        kind: 1,
        start_time_unix_nano: timestamp - 1_000_000_000,
        end_time_unix_nano: timestamp,
        attributes: vec![],
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

/// Create sample metrics request and encode to protobuf
pub fn create_metrics_protobuf() -> Bytes {
    let request = create_sample_metrics_request();
    encode_protobuf(&request)
}

/// Create sample logs request and encode to protobuf
pub fn create_logs_protobuf() -> Bytes {
    let request = create_sample_logs_request();
    encode_protobuf(&request)
}

/// Create sample traces request and encode to protobuf
pub fn create_traces_protobuf() -> Bytes {
    let request = create_sample_traces_request();
    encode_protobuf(&request)
}

/// Create invalid protobuf data
pub fn create_invalid_protobuf() -> Bytes {
    Bytes::from(vec![0xFF, 0xFF, 0xFF, 0xFF])
}

/// Create empty protobuf data
pub fn create_empty_protobuf() -> Bytes {
    Bytes::new()
}

/// Decode protobuf bytes to metrics request
pub fn decode_metrics_protobuf(
    data: &[u8],
) -> Result<ExportMetricsServiceRequest, prost::DecodeError> {
    ExportMetricsServiceRequest::decode(data)
}

/// Decode protobuf bytes to logs request
pub fn decode_logs_protobuf(data: &[u8]) -> Result<ExportLogsServiceRequest, prost::DecodeError> {
    ExportLogsServiceRequest::decode(data)
}

/// Decode protobuf bytes to traces request
pub fn decode_traces_protobuf(
    data: &[u8],
) -> Result<ExportTraceServiceRequest, prost::DecodeError> {
    ExportTraceServiceRequest::decode(data)
}

/// Create sample metrics JSON
#[allow(dead_code)]
pub fn create_metrics_json() -> String {
    serde_json::json!({
        "resourceMetrics": [{
            "resource": {
                "attributes": [{
                    "key": "service.name",
                    "value": {"stringValue": "test_service"}
                }]
            },
            "scopeMetrics": [{
                "scope": {
                    "name": "test_scope",
                    "version": "1.0.0"
                },
                "metrics": [{
                    "name": "test_metric",
                    "description": "A test metric",
                    "unit": "1",
                    "gauge": {
                        "dataPoints": [{
                            "attributes": [{
                                "key": "test_key",
                                "value": {"stringValue": "test_value"}
                            }],
                            "timeUnixNano": "1234567890000000000",
                            "asInt": "42"
                        }]
                    }
                }]
            }]
        }]
    })
    .to_string()
}

/// Create sample logs JSON
#[allow(dead_code)]
pub fn create_logs_json() -> String {
    serde_json::json!({
        "resourceLogs": [{
            "resource": {
                "attributes": [{
                    "key": "service.name",
                    "value": {"stringValue": "test_service"}
                }]
            },
            "scopeLogs": [{
                "scope": {
                    "name": "test_scope",
                    "version": "1.0.0"
                },
                "logRecords": [{
                    "timeUnixNano": "1234567890000000000",
                    "observedTimeUnixNano": "1234567890000000000",
                    "severityNumber": 9,
                    "severityText": "INFO",
                    "body": {"stringValue": "Test log message"}
                }]
            }]
        }]
    })
    .to_string()
}

/// Create sample traces JSON
#[allow(dead_code)]
pub fn create_traces_json() -> String {
    serde_json::json!({
        "resourceSpans": [{
            "resource": {
                "attributes": [{
                    "key": "service.name",
                    "value": {"stringValue": "test_service"}
                }]
            },
            "scopeSpans": [{
                "scope": {
                    "name": "test_scope",
                    "version": "1.0.0"
                },
                "spans": [{
                    "traceId": "0102030405060708090a0b0c0d0e0f10",
                    "spanId": "0102030405060708",
                    "name": "test_span",
                    "kind": 1,
                    "startTimeUnixNano": "1234567890000000000",
                    "endTimeUnixNano": "1234567891000000000"
                }]
            }]
        }]
    })
    .to_string()
}

/// Create invalid JSON
#[allow(dead_code)]
pub fn create_invalid_json() -> String {
    "{invalid json".to_string()
}

/// Create malformed JSON (valid JSON but wrong structure)
#[allow(dead_code)]
pub fn create_malformed_json() -> String {
    serde_json::json!({
        "wrongField": "wrongValue"
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_metrics_protobuf() {
        let data = create_metrics_protobuf();
        assert!(!data.is_empty());

        let result = decode_metrics_protobuf(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_logs_protobuf() {
        let data = create_logs_protobuf();
        assert!(!data.is_empty());

        let result = decode_logs_protobuf(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_traces_protobuf() {
        let data = create_traces_protobuf();
        assert!(!data.is_empty());

        let result = decode_traces_protobuf(&data);
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_invalid_protobuf() {
        let data = create_invalid_protobuf();
        assert_eq!(data.len(), 4);

        let result = decode_metrics_protobuf(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_empty_protobuf() {
        let data = create_empty_protobuf();
        assert!(data.is_empty());
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let original = create_sample_metrics_request();
        let encoded = encode_protobuf(&original);
        let decoded = decode_metrics_protobuf(&encoded).expect("Failed to decode");

        assert_eq!(
            decoded.resource_metrics.len(),
            original.resource_metrics.len()
        );
    }
}

// Made with Bob
