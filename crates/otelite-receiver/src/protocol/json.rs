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

/// Proto3 JSON encodes int64 fields as strings, and spec-compliant SDKs send
/// `"asInt": "42"`. The `with-serde` derive of `opentelemetry-proto` expects
/// a JSON number for the `asInt` oneof arm (its `serde(flatten)` silently
/// drops the string form, storing the data point as 0 — #233), so convert
/// string-encoded `asInt` fields to numbers before deserializing. A value
/// that is a string but not a valid int64 is an error, not a silent zero.
fn normalize_int64_strings(value: &mut serde_json::Value) -> Result<(), ReceiverError> {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(serde_json::Value::String(s)) = map.get_mut("asInt") {
                let parsed: i64 = s.parse().map_err(|_| {
                    ReceiverError::MissingField(format!("asInt value {s:?} is not a valid int64"))
                })?;
                *map.get_mut("asInt").expect("checked above") =
                    serde_json::Value::Number(serde_json::Number::from(parsed));
            }
            for v in map.values_mut() {
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

    let request: ExportLogsServiceRequest = serde_json::from_slice(data)?;
    Ok(request)
}

/// Parse OTLP traces request from JSON
pub fn parse_traces_json(data: &[u8]) -> Result<ExportTraceServiceRequest, ReceiverError> {
    if data.is_empty() {
        return Ok(ExportTraceServiceRequest {
            resource_spans: vec![],
        });
    }

    let request: ExportTraceServiceRequest = serde_json::from_slice(data)?;
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
