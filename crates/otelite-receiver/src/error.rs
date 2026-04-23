//! Error types for the OTLP receiver

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

/// Result type alias for receiver operations
pub type Result<T> = std::result::Result<T, ReceiverError>;

/// Errors that can occur in the OTLP receiver
#[derive(Debug, thiserror::Error)]
pub enum ReceiverError {
    /// Invalid OTLP protocol version
    #[error("Invalid OTLP protocol version: {0}")]
    InvalidProtocolVersion(String),

    /// Failed to parse Protobuf message
    #[error("Failed to parse Protobuf message: {0}")]
    ProtobufParseError(#[from] prost::DecodeError),

    /// Failed to parse JSON message
    #[error("Failed to parse JSON message: {0}")]
    JsonParseError(#[from] serde_json::Error),

    /// Invalid content type
    #[error("Invalid content type: {0}")]
    InvalidContentType(String),

    /// Message too large
    #[error("Message too large: {size} bytes (max: {max} bytes)")]
    MessageTooLarge { size: usize, max: usize },

    /// Missing required field
    #[error("Missing required field: {0}")]
    MissingField(String),

    /// Invalid signal type
    #[error("Invalid signal type: {0}")]
    InvalidSignalType(String),

    /// gRPC server error
    #[error("gRPC server error: {0}")]
    GrpcError(#[from] tonic::transport::Error),

    /// HTTP server error
    #[error("HTTP server error: {0}")]
    HttpError(String),

    /// Compression error
    #[error("Compression error: {0}")]
    CompressionError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),

    /// Storage error
    #[error("Storage error: {0}")]
    StorageError(#[from] otelite_core::storage::StorageError),
}

impl ReceiverError {
    /// Create a new HTTP error
    pub fn http_error(msg: impl Into<String>) -> Self {
        Self::HttpError(msg.into())
    }

    /// Create a new compression error
    pub fn compression_error(msg: impl Into<String>) -> Self {
        Self::CompressionError(msg.into())
    }

    /// Create a new configuration error
    pub fn config_error(msg: impl Into<String>) -> Self {
        Self::ConfigError(msg.into())
    }

    /// Create a new internal error
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    /// Convert to gRPC status code
    pub fn to_grpc_status(&self) -> tonic::Status {
        match self {
            Self::InvalidProtocolVersion(_) => tonic::Status::invalid_argument(self.to_string()),
            Self::ProtobufParseError(_) => tonic::Status::invalid_argument(self.to_string()),
            Self::JsonParseError(_) => tonic::Status::invalid_argument(self.to_string()),
            Self::InvalidContentType(_) => tonic::Status::invalid_argument(self.to_string()),
            Self::MessageTooLarge { .. } => tonic::Status::resource_exhausted(self.to_string()),
            Self::MissingField(_) => tonic::Status::invalid_argument(self.to_string()),
            Self::InvalidSignalType(_) => tonic::Status::invalid_argument(self.to_string()),
            Self::GrpcError(_) => tonic::Status::internal(self.to_string()),
            Self::HttpError(_) => tonic::Status::internal(self.to_string()),
            Self::CompressionError(_) => tonic::Status::internal(self.to_string()),
            Self::ConfigError(_) => tonic::Status::failed_precondition(self.to_string()),
            Self::Internal(_) => tonic::Status::internal(self.to_string()),
            Self::StorageError(_) => tonic::Status::internal(self.to_string()),
        }
    }

    /// Convert to HTTP status code
    pub fn to_http_status(&self) -> u16 {
        match self {
            Self::InvalidProtocolVersion(_) => 400,
            Self::ProtobufParseError(_) => 400,
            Self::JsonParseError(_) => 400,
            Self::InvalidContentType(_) => 415, // Unsupported Media Type
            Self::MessageTooLarge { .. } => 413, // Payload Too Large
            Self::MissingField(_) => 400,
            Self::InvalidSignalType(_) => 400,
            Self::GrpcError(_) => 500,
            Self::HttpError(_) => 500,
            Self::CompressionError(_) => 500,
            Self::ConfigError(_) => 500,
            Self::Internal(_) => 500,
            Self::StorageError(_) => 500,
        }
    }
}

// Implement IntoResponse for axum HTTP handlers
impl IntoResponse for ReceiverError {
    fn into_response(self) -> Response {
        let status = StatusCode::from_u16(self.to_http_status())
            .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);

        let body = Json(json!({
            "error": self.to_string(),
            "status": status.as_u16(),
        }));

        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = ReceiverError::InvalidProtocolVersion("1.0.0".to_string());
        assert_eq!(err.to_string(), "Invalid OTLP protocol version: 1.0.0");
    }

    #[test]
    fn test_grpc_status_conversion() {
        let err = ReceiverError::InvalidProtocolVersion("1.0.0".to_string());
        let status = err.to_grpc_status();
        assert_eq!(status.code(), tonic::Code::InvalidArgument);
    }

    #[test]
    fn test_http_status_conversion() {
        let err = ReceiverError::InvalidContentType("text/plain".to_string());
        assert_eq!(err.to_http_status(), 415);

        let err = ReceiverError::MessageTooLarge {
            size: 20_000_000,
            max: 10_000_000,
        };
        assert_eq!(err.to_http_status(), 413);
    }
}
