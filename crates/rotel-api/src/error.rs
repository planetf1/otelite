//! Error types for the API

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Result type alias for API operations
pub type ApiResult<T> = Result<T, ApiError>;

/// API error types
#[derive(Debug, Error)]
pub enum ApiError {
    /// Resource not found
    #[error("Resource not found: {0}")]
    NotFound(String),

    /// Invalid request parameters
    #[error("Invalid request: {0}")]
    BadRequest(String),

    /// Validation error
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// Storage backend error
    #[error("Storage error: {0}")]
    StorageError(String),

    /// Internal server error
    #[error("Internal server error: {0}")]
    InternalError(String),

    /// Service unavailable
    #[error("Service unavailable: {0}")]
    ServiceUnavailable(String),

    /// Timeout error
    #[error("Request timeout")]
    Timeout,
}

/// Error response structure sent to clients
#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// Error message
    pub error: String,

    /// Error code
    pub code: String,

    /// Optional details
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,

    /// Request ID for tracking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

impl ErrorResponse {
    /// Create a new error response
    pub fn new(error: impl Into<String>, code: impl Into<String>) -> Self {
        Self {
            error: error.into(),
            code: code.into(),
            details: None,
            request_id: None,
        }
    }

    /// Add details to the error response
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    /// Add request ID to the error response
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            ApiError::NotFound(_) => (StatusCode::NOT_FOUND, "NOT_FOUND"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "BAD_REQUEST"),
            ApiError::ValidationError(_) => (StatusCode::BAD_REQUEST, "VALIDATION_ERROR"),
            ApiError::StorageError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "STORAGE_ERROR"),
            ApiError::InternalError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR"),
            ApiError::ServiceUnavailable(_) => {
                (StatusCode::SERVICE_UNAVAILABLE, "SERVICE_UNAVAILABLE")
            },
            ApiError::Timeout => (StatusCode::REQUEST_TIMEOUT, "TIMEOUT"),
        };

        let error_response = ErrorResponse::new(self.to_string(), code);

        (status, Json(error_response)).into_response()
    }
}

// Implement From conversions for common error types
impl From<std::io::Error> for ApiError {
    fn from(err: std::io::Error) -> Self {
        ApiError::InternalError(err.to_string())
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(err: serde_json::Error) -> Self {
        ApiError::BadRequest(format!("JSON error: {}", err))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_response_creation() {
        let err = ErrorResponse::new("Test error", "TEST_ERROR")
            .with_details("Additional details")
            .with_request_id("req-123");

        assert_eq!(err.error, "Test error");
        assert_eq!(err.code, "TEST_ERROR");
        assert_eq!(err.details, Some("Additional details".to_string()));
        assert_eq!(err.request_id, Some("req-123".to_string()));
    }

    #[test]
    fn test_api_error_display() {
        let err = ApiError::NotFound("User".to_string());
        assert_eq!(err.to_string(), "Resource not found: User");

        let err = ApiError::BadRequest("Invalid ID".to_string());
        assert_eq!(err.to_string(), "Invalid request: Invalid ID");
    }
}

// Made with Bob
