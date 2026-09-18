// JSON protocol parser for OTLP

use crate::error::ReceiverError;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;

/// Parse OTLP metrics request from JSON
pub fn parse_metrics_json(data: &[u8]) -> Result<ExportMetricsServiceRequest, ReceiverError> {
    if data.is_empty() {
        return Ok(ExportMetricsServiceRequest {
            resource_metrics: vec![],
        });
    }

    let mut value: serde_json::Value = serde_json::from_slice(data)?;
    normalize_int64_strings(&mut value)?;
    let request: ExportMetricsServiceRequest = serde_json::from_value(value)?;
    Ok(request)
}

/// JSON keys of the int64 oneof arms in the OTLP proto types: `asInt`
/// (number/exemplar data-point values) and `intValue` (AnyValue — log
/// bodies, log/span/metric attribute values).
const INT64_KEYS: [&str; 2] = ["asInt", "intValue"];

/// JSON keys of the uint64 fields in metrics data points. opentelemetry-proto
/// 0.32 accepts both string and number forms, but a *mismatched* value
/// (e.g. a non-numeric string count) makes `serde(flatten)` silently drop
/// the whole histogram/summary oneof — so validate here and fail loudly,
/// the same contract as INT64_KEYS (#233/#247/#255).
const UINT64_KEYS: [&str; 5] = [
    "count",
    "bucketCounts",
    "zeroCount",
    "positiveBucketCounts",
    "negativeBucketCounts",
];

/// Proto3 JSON encodes int64 fields as strings, and spec-compliant SDKs send
/// `"asInt": "42"` / `"intValue": "42"`. The `with-serde` derive of
/// `opentelemetry-proto` expects a JSON number for these oneof arms (its
/// `serde(flatten)` silently drops the string form — metric data points
/// stored as 0, #233; attribute/body values dropped, #247), so convert
/// string-encoded int64 fields to numbers before deserializing. A value
/// that is a string but not a valid int64 is an error, not a silent
/// zero/omission.
fn normalize_int64_strings(value: &mut serde_json::Value) -> Result<(), ReceiverError> {
    match value {
        serde_json::Value::Object(map) => {
            for (key, v) in map.iter_mut() {
                let k = key.as_str();
                if INT64_KEYS.contains(&k) {
                    if let serde_json::Value::String(s) = v {
                        let parsed: i64 = s.parse().map_err(|_| {
                            ReceiverError::MissingField(format!(
                                "{k} value {s:?} is not a valid int64"
                            ))
                        })?;
                        *v = serde_json::Value::Number(serde_json::Number::from(parsed));
                    }
                } else if UINT64_KEYS.contains(&k) {
                    match v {
                        serde_json::Value::String(s) => {
                            let parsed: u64 = s.parse().map_err(|_| {
                                ReceiverError::MissingField(format!(
                                    "{k} value {s:?} is not a valid uint64"
                                ))
                            })?;
                            *v = serde_json::Value::Number(serde_json::Number::from(parsed));
                        },
                        serde_json::Value::Array(items) => {
                            for item in items.iter_mut() {
                                if let serde_json::Value::String(s) = item {
                                    let parsed: u64 = s.parse().map_err(|_| {
                                        ReceiverError::MissingField(format!(
                                            "{k} element {s:?} is not a valid uint64"
                                        ))
                                    })?;
                                    *item =
                                        serde_json::Value::Number(serde_json::Number::from(parsed));
                                }
                            }
                        },
                        _ => {},
                    }
                }
                normalize_int64_strings(v)?;
            }
        },
        serde_json::Value::Array(items) => {
            for v in items.iter_mut() {
                normalize_int64_strings(v)?;
            }
        },
        _ => {},
    }
    Ok(())
}

/// Parse OTLP logs request from JSON
pub fn parse_logs_json(data: &[u8]) -> Result<ExportLogsServiceRequest, ReceiverError> {
    if data.is_empty() {
        return Ok(ExportLogsServiceRequest {
            resource_logs: vec![],
        });
    }

    let mut value: serde_json::Value = serde_json::from_slice(data)?;
    normalize_int64_strings(&mut value)?;
    let request: ExportLogsServiceRequest = serde_json::from_value(value)?;
    Ok(request)
}

/// Parse OTLP traces request from JSON
pub fn parse_traces_json(data: &[u8]) -> Result<ExportTraceServiceRequest, ReceiverError> {
    if data.is_empty() {
        return Ok(ExportTraceServiceRequest {
            resource_spans: vec![],
        });
    }

    let mut value: serde_json::Value = serde_json::from_slice(data)?;
    normalize_int64_strings(&mut value)?;
    let request: ExportTraceServiceRequest = serde_json::from_value(value)?;
    Ok(request)
}

/// Validate JSON message structure
pub fn validate_json_message(data: &[u8]) -> Result<(), ReceiverError> {
    if data.is_empty() {
        return Ok(());
    }

    // Basic JSON validation - check if it's valid JSON
    serde_json::from_slice::<serde_json::Value>(data)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_json() {
        let result = parse_metrics_json(&[]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().resource_metrics.len(), 0);
    }

    #[test]
    fn test_parse_invalid_json() {
        let invalid_json = b"{ invalid json }";
        let result = parse_metrics_json(invalid_json);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            ReceiverError::JsonParseError(_)
        ));
    }

    #[test]
    fn test_parse_valid_empty_metrics_json() {
        let valid_json = b"{\"resourceMetrics\":[]}";
        let result = parse_metrics_json(valid_json);
        assert!(result.is_ok());
        let request = result.unwrap();
        assert_eq!(request.resource_metrics.len(), 0);
    }

    #[test]
    fn test_parse_valid_empty_logs_json() {
        let valid_json = b"{\"resourceLogs\":[]}";
        let result = parse_logs_json(valid_json);
        assert!(result.is_ok());
        let request = result.unwrap();
        assert_eq!(request.resource_logs.len(), 0);
    }

    #[test]
    fn test_parse_valid_empty_traces_json() {
        let valid_json = b"{\"resourceSpans\":[]}";
        let result = parse_traces_json(valid_json);
        assert!(result.is_ok());
        let request = result.unwrap();
        assert_eq!(request.resource_spans.len(), 0);
    }

    fn sum_value(
        json: &[u8],
    ) -> Option<opentelemetry_proto::tonic::metrics::v1::number_data_point::Value> {
        let req = parse_metrics_json(json).expect("parse should succeed");
        let dp = &req.resource_metrics[0].scope_metrics[0].metrics[0];
        match &dp.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Sum(s)) => {
                s.data_points[0].value
            },
            _ => panic!("expected a Sum data point"),
        }
    }

    const AS_INT_SUM_JSON: &str = r#"{
        "resourceMetrics": [{
            "scopeMetrics": [{
                "metrics": [{
                    "name": "t",
                    "sum": {
                        "dataPoints": [{ "asInt": __V__ }],
                        "aggregationTemporality": 2,
                        "isMonotonic": true
                    }
                }]
            }]
        }]
    }"#;

    #[test]
    fn test_parse_metrics_json_as_int_string_datapoint() {
        // Proto3 JSON string form — the issue's literal repro, previously
        // stored as 0.
        let json = AS_INT_SUM_JSON.replace("__V__", r#""42""#);
        assert_eq!(
            sum_value(json.as_bytes()),
            Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(42))
        );
    }

    #[test]
    fn test_parse_metrics_json_as_int_number_datapoint() {
        // Numeric form kept working before the fix; must keep working.
        let json = AS_INT_SUM_JSON.replace("__V__", "42");
        assert_eq!(
            sum_value(json.as_bytes()),
            Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(42))
        );
    }

    #[test]
    fn test_parse_metrics_json_as_int_negative_string_datapoint() {
        let json = AS_INT_SUM_JSON.replace("__V__", r#""-7""#);
        assert_eq!(
            sum_value(json.as_bytes()),
            Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(-7))
        );
    }

    #[test]
    fn test_parse_metrics_json_as_int_gauge_datapoint() {
        // The asInt oneof also appears on gauge data points.
        let json = br#"{
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "g",
                        "gauge": { "dataPoints": [{ "asInt": "9" }] }
                    }]
                }]
            }]
        }"#;
        let req = parse_metrics_json(json).unwrap();
        let dp = &req.resource_metrics[0].scope_metrics[0].metrics[0];
        let value = match &dp.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(g)) => {
                g.data_points[0].value
            },
            _ => panic!("expected a Gauge data point"),
        };
        assert_eq!(
            value,
            Some(opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(9))
        );
    }

    #[test]
    fn test_parse_metrics_json_as_int_invalid_string_is_error() {
        // A string that is not an int64 must fail loudly, not store 0.
        let json = AS_INT_SUM_JSON.replace("__V__", r#""not-a-number""#);
        let result = parse_metrics_json(json.as_bytes());
        assert!(
            matches!(result, Err(ReceiverError::MissingField(_))),
            "expected MissingField, got {:?}",
            result.is_ok()
        );
    }

    // --- AnyValue intValue (proto3 JSON string int64, #247) -------------

    /// The issue's literal repro: a log record with a stringValue body and
    /// an intValue attribute sent in the spec-compliant string form.
    const LOGS_INT_VALUE_JSON: &str = r#"{
        "resourceLogs": [{
            "scopeLogs": [{
                "logRecords": [{
                    "body": { "stringValue": "x" },
                    "timeUnixNano": "1758000000000000000",
                    "attributes": [{ "key": "n", "value": { "intValue": "42" } }]
                }]
            }]
        }]
    }"#;

    fn log_record(
        logs: &ExportLogsServiceRequest,
    ) -> &opentelemetry_proto::tonic::logs::v1::LogRecord {
        &logs.resource_logs[0].scope_logs[0].log_records[0]
    }

    #[test]
    fn test_parse_logs_json_int_value_string_attribute() {
        let req = parse_logs_json(LOGS_INT_VALUE_JSON.as_bytes()).unwrap();
        let record = log_record(&req);
        let attr = &record.attributes[0];
        assert_eq!(
            attr.value.as_ref().expect("value present").value,
            Some(opentelemetry_proto::tonic::common::v1::any_value::Value::IntValue(42))
        );
    }

    #[test]
    fn test_parse_logs_json_int_value_number_body() {
        // Numeric form worked before the #233-class fixes and must keep
        // working.
        let json = r#"{
            "resourceLogs": [{
                "scopeLogs": [{
                    "logRecords": [{ "body": { "intValue": -7 } }]
                }]
            }]
        }"#;
        let req = parse_logs_json(json.as_bytes()).unwrap();
        assert_eq!(
            log_record(&req).body.as_ref().expect("body present").value,
            Some(opentelemetry_proto::tonic::common::v1::any_value::Value::IntValue(-7))
        );
    }

    #[test]
    fn test_parse_logs_json_int_value_string_body() {
        let json = r#"{
            "resourceLogs": [{
                "scopeLogs": [{
                    "logRecords": [{ "body": { "intValue": "-7" } }]
                }]
            }]
        }"#;
        let req = parse_logs_json(json.as_bytes()).unwrap();
        assert_eq!(
            log_record(&req).body.as_ref().expect("body present").value,
            Some(opentelemetry_proto::tonic::common::v1::any_value::Value::IntValue(-7))
        );
    }

    #[test]
    fn test_parse_logs_json_int_value_invalid_string_is_error() {
        // A string that is not an int64 must fail loudly, not drop the
        // value.
        let json = r#"{
            "resourceLogs": [{
                "scopeLogs": [{
                    "logRecords": [{ "body": { "intValue": "not-a-number" } }]
                }]
            }]
        }"#;
        let result = parse_logs_json(json.as_bytes());
        assert!(
            matches!(result, Err(ReceiverError::MissingField(_))),
            "expected MissingField, got {:?}",
            result.is_ok()
        );
    }

    #[test]
    fn test_parse_traces_json_int_value_string_attribute() {
        // Span attributes use the same AnyValue type — the same drop
        // applied to traces.
        let json = r#"{
            "resourceSpans": [{
                "scopeSpans": [{
                    "spans": [{
                        "traceId": "aa32619a4f3a4511b8c5f3f5f0e6d001",
                        "spanId": "0f0e0d0c0b0a0908",
                        "name": "s",
                        "attributes": [{ "key": "n", "value": { "intValue": "42" } }]
                    }]
                }]
            }]
        }"#;
        let req = parse_traces_json(json.as_bytes()).unwrap();
        let span = &req.resource_spans[0].scope_spans[0].spans[0];
        assert_eq!(
            span.attributes[0]
                .value
                .as_ref()
                .expect("value present")
                .value,
            Some(opentelemetry_proto::tonic::common::v1::any_value::Value::IntValue(42))
        );
    }

    #[test]
    fn test_parse_metrics_json_int_value_string_attribute() {
        // Metric data-point attributes use the same AnyValue type.
        let json = r#"{
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "t",
                        "sum": {
                            "dataPoints": [{
                                "asInt": "1",
                                "attributes": [{ "key": "n", "value": { "intValue": "42" } }]
                            }],
                            "aggregationTemporality": 2,
                            "isMonotonic": true
                        }
                    }]
                }]
            }]
        }"#;
        let req = parse_metrics_json(json.as_bytes()).unwrap();
        let dp = &req.resource_metrics[0].scope_metrics[0].metrics[0];
        let value = match &dp.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Sum(s)) => {
                &s.data_points[0].attributes[0].value
            },
            _ => panic!("expected a Sum data point"),
        };
        assert_eq!(
            value.as_ref().expect("value present").value,
            Some(opentelemetry_proto::tonic::common::v1::any_value::Value::IntValue(42))
        );
    }

    // --- Histogram/summary string counts (proto3 JSON uint64, #255) -----
    // opentelemetry-proto 0.32 deserialises the spec-compliant string form
    // of the fixed64 count fields; 0.31 rejected the whole export with a
    // 400.

    #[test]
    fn test_parse_metrics_json_histogram_string_counts() {
        let json = r#"{
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "h",
                        "histogram": {
                            "dataPoints": [{
                                "count": "3",
                                "sum": 6.0,
                                "bucketCounts": ["1", "2", "0"],
                                "explicitBounds": [1.0, 2.0]
                            }]
                        }
                    }]
                }]
            }]
        }"#;
        let req = parse_metrics_json(json.as_bytes()).unwrap();
        let dp = &req.resource_metrics[0].scope_metrics[0].metrics[0];
        match &dp.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Histogram(h)) => {
                let p = &h.data_points[0];
                assert_eq!(p.count, 3);
                assert_eq!(p.bucket_counts, vec![1, 2, 0]);
            },
            _ => panic!("expected a Histogram data point"),
        }
    }

    #[test]
    fn test_parse_metrics_json_histogram_number_counts() {
        // Number form must keep working alongside the string form.
        let json = r#"{
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "h",
                        "histogram": {
                            "dataPoints": [{
                                "count": 3,
                                "sum": 6.0,
                                "bucketCounts": [1, 2, 0],
                                "explicitBounds": [1.0, 2.0]
                            }]
                        }
                    }]
                }]
            }]
        }"#;
        let req = parse_metrics_json(json.as_bytes()).unwrap();
        let dp = &req.resource_metrics[0].scope_metrics[0].metrics[0];
        match &dp.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Histogram(h)) => {
                assert_eq!(h.data_points[0].count, 3);
            },
            _ => panic!("expected a Histogram data point"),
        }
    }

    #[test]
    fn test_parse_metrics_json_summary_string_counts() {
        let json = r#"{
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "s",
                        "summary": {
                            "dataPoints": [{
                                "count": "2",
                                "sum": 5.0,
                                "quantileValues": [{ "quantile": 0.5, "value": 2.5 }]
                            }]
                        }
                    }]
                }]
            }]
        }"#;
        let req = parse_metrics_json(json.as_bytes()).unwrap();
        let dp = &req.resource_metrics[0].scope_metrics[0].metrics[0];
        match &dp.data {
            Some(opentelemetry_proto::tonic::metrics::v1::metric::Data::Summary(s)) => {
                assert_eq!(s.data_points[0].count, 2);
            },
            _ => panic!("expected a Summary data point"),
        }
    }

    #[test]
    fn test_parse_metrics_json_histogram_invalid_count_string_is_error() {
        let json = r#"{
            "resourceMetrics": [{
                "scopeMetrics": [{
                    "metrics": [{
                        "name": "h",
                        "histogram": {
                            "dataPoints": [{ "count": "not-a-number" }]
                        }
                    }]
                }]
            }]
        }"#;
        assert!(parse_metrics_json(json.as_bytes()).is_err());
    }

    #[test]
    fn test_validate_json_message_valid() {
        let valid_json = b"{\"test\":\"value\"}";
        let result = validate_json_message(valid_json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_json_message_invalid() {
        let invalid_json = b"{ invalid }";
        let result = validate_json_message(invalid_json);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_json_message_empty() {
        let result = validate_json_message(&[]);
        assert!(result.is_ok());
    }
}
