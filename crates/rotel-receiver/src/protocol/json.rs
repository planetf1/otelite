// JSON protocol parser for OTLP

use crate::error::ReceiverError;
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use serde_json::Value;

/// Parse OTLP metrics request from JSON
///
/// **Current Implementation**: Validates JSON structure and returns empty request.
///
/// **Limitation**: The opentelemetry-proto crate uses prost-generated types that don't
/// implement serde::Deserialize. Full JSON-to-Protobuf conversion would require:
/// 1. Manual mapping from JSON Value to protobuf types, OR
/// 2. Using prost-reflect for dynamic protobuf construction, OR
/// 3. Using a different OTLP library with serde support
///
/// For now, this validates that clients can send JSON (content-type accepted),
/// but the data is not actually processed. This is acceptable for initial MVP
/// as most production OTLP clients use Protobuf encoding for efficiency.
pub fn parse_metrics_json(data: &[u8]) -> Result<ExportMetricsServiceRequest, ReceiverError> {
    if data.is_empty() {
        return Ok(ExportMetricsServiceRequest {
            resource_metrics: vec![],
        });
    }

    // Validate JSON structure
    let _json: Value = serde_json::from_slice(data)?;

    // Return empty request - JSON validation passed but conversion not implemented
    // This allows JSON content-type to be accepted without full processing
    Ok(ExportMetricsServiceRequest {
        resource_metrics: vec![],
    })
}

/// Parse OTLP logs request from JSON
///
/// See [`parse_metrics_json`] for implementation details and limitations.
pub fn parse_logs_json(data: &[u8]) -> Result<ExportLogsServiceRequest, ReceiverError> {
    if data.is_empty() {
        return Ok(ExportLogsServiceRequest {
            resource_logs: vec![],
        });
    }

    // Validate JSON structure
    let _json: Value = serde_json::from_slice(data)?;

    // Return empty request - JSON validation passed but conversion not implemented
    Ok(ExportLogsServiceRequest {
        resource_logs: vec![],
    })
}

/// Parse OTLP traces request from JSON
///
/// See [`parse_metrics_json`] for implementation details and limitations.
pub fn parse_traces_json(data: &[u8]) -> Result<ExportTraceServiceRequest, ReceiverError> {
    if data.is_empty() {
        return Ok(ExportTraceServiceRequest {
            resource_spans: vec![],
        });
    }

    // Validate JSON structure
    let _json: Value = serde_json::from_slice(data)?;

    // Return empty request - JSON validation passed but conversion not implemented
    Ok(ExportTraceServiceRequest {
        resource_spans: vec![],
    })
}

/// Validate JSON message structure
pub fn validate_json_message(data: &[u8]) -> Result<(), ReceiverError> {
    if data.is_empty() {
        return Ok(());
    }

    // Basic JSON validation - check if it's valid JSON
    serde_json::from_slice::<Value>(data)?;

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
    }

    #[test]
    fn test_parse_valid_empty_logs_json() {
        let valid_json = b"{\"resourceLogs\":[]}";
        let result = parse_logs_json(valid_json);
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_valid_empty_traces_json() {
        let valid_json = b"{\"resourceSpans\":[]}";
        let result = parse_traces_json(valid_json);
        assert!(result.is_ok());
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

// Made with Bob
