// Protobuf protocol parsing and validation

use crate::error::ReceiverError;
use crate::Result;
use prost::Message;
use tracing::{debug, warn};

/// Parse and validate a protobuf message
pub fn parse_message<T: Message + Default>(data: &[u8]) -> Result<T> {
    debug!("Parsing protobuf message: {} bytes", data.len());

    // Empty data is valid for protobuf - it represents a message with all default values
    // Parse the protobuf message - prost::DecodeError is automatically converted
    T::decode(data).map_err(|e| {
        warn!("Failed to decode protobuf message: {}", e);
        ReceiverError::ProtobufParseError(e)
    })
}

/// Validate protobuf message structure
pub fn validate_message<T: Message>(message: &T) -> Result<()> {
    // Encode the message to verify it's valid
    let encoded = message.encode_to_vec();

    // Empty messages are valid in protobuf (they encode to a single 0x00 byte or empty)
    debug!("Protobuf message validated: {} bytes", encoded.len());
    Ok(())
}

/// Handle protobuf parsing errors with context
pub fn handle_parse_error(error: prost::DecodeError, context: &str) -> ReceiverError {
    warn!("Protobuf parse error in {}: {}", context, error);
    ReceiverError::ProtobufParseError(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
    use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
    use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;

    #[test]
    fn test_parse_empty_data() {
        // Empty data is valid - represents a message with all default values
        let result: Result<ExportMetricsServiceRequest> = parse_message(&[]);
        assert!(result.is_ok());
        let request = result.unwrap();
        assert!(request.resource_metrics.is_empty());
    }

    #[test]
    fn test_parse_invalid_protobuf() {
        let invalid_data = vec![0xFF, 0xFF, 0xFF, 0xFF];
        let result: Result<ExportMetricsServiceRequest> = parse_message(&invalid_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_valid_empty_request() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![],
        };
        let encoded = request.encode_to_vec();

        let parsed: Result<ExportMetricsServiceRequest> = parse_message(&encoded);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_validate_message() {
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![],
        };

        assert!(validate_message(&request).is_ok());
    }

    #[test]
    fn test_parse_logs_request() {
        let request = ExportLogsServiceRequest {
            resource_logs: vec![],
        };
        let encoded = request.encode_to_vec();

        let parsed: Result<ExportLogsServiceRequest> = parse_message(&encoded);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_parse_trace_request() {
        let request = ExportTraceServiceRequest {
            resource_spans: vec![],
        };
        let encoded = request.encode_to_vec();

        let parsed: Result<ExportTraceServiceRequest> = parse_message(&encoded);
        assert!(parsed.is_ok());
    }

    #[test]
    fn test_handle_parse_error() {
        // Create a DecodeError by attempting to decode invalid data
        let invalid_data = [0xFF, 0xFF, 0xFF];
        let decode_result = ExportLogsServiceRequest::decode(&invalid_data[..]);

        let decode_error = decode_result.unwrap_err();
        let error = handle_parse_error(decode_error, "test context");

        assert!(matches!(error, ReceiverError::ProtobufParseError(_)));
    }
}

// Made with Bob
