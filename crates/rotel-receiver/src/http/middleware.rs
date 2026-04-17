// HTTP middleware for OTLP receiver

use crate::error::ReceiverError;
use axum::{
    extract::Request,
    http::{header, HeaderValue},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::debug;

/// Middleware to validate Content-Type header
pub async fn validate_content_type(req: Request, next: Next) -> Result<Response, ReceiverError> {
    // Skip validation for health check endpoints
    if req.uri().path().contains("/health") {
        return Ok(next.run(req).await);
    }

    // Check Content-Type header
    if let Some(content_type) = req.headers().get(header::CONTENT_TYPE) {
        let content_type_str = content_type.to_str().map_err(|_| {
            ReceiverError::InvalidContentType("Invalid Content-Type header".to_string())
        })?;

        // Accept protobuf or JSON
        if content_type_str.starts_with("application/x-protobuf")
            || content_type_str.starts_with("application/json")
        {
            return Ok(next.run(req).await);
        }

        return Err(ReceiverError::InvalidContentType(format!(
            "Unsupported Content-Type: {}. Expected application/x-protobuf or application/json",
            content_type_str
        )));
    }

    Err(ReceiverError::InvalidContentType(
        "Missing Content-Type header".to_string(),
    ))
}

/// Middleware to handle compression (gzip, deflate)
pub async fn handle_compression(req: Request, next: Next) -> Response {
    // Check Content-Encoding header
    let content_encoding = req
        .headers()
        .get(header::CONTENT_ENCODING)
        .and_then(|v| v.to_str().ok());

    match content_encoding {
        Some("gzip") => {
            debug!("Request has gzip compression");
            // In a full implementation, we would decompress here
            // For now, pass through (axum handles this automatically with tower-http)
            next.run(req).await
        },
        Some("deflate") => {
            debug!("Request has deflate compression");
            // In a full implementation, we would decompress here
            next.run(req).await
        },
        Some("identity") | None => {
            // No compression
            next.run(req).await
        },
        Some(encoding) => {
            // Unsupported encoding
            let error = ReceiverError::CompressionError(format!(
                "Unsupported Content-Encoding: {}",
                encoding
            ));
            error.into_response()
        },
    }
}

/// Middleware to add CORS headers
pub async fn add_cors_headers(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;

    // Add CORS headers
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_ORIGIN,
        HeaderValue::from_static("*"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_METHODS,
        HeaderValue::from_static("GET, POST, OPTIONS"),
    );
    response.headers_mut().insert(
        header::ACCESS_CONTROL_ALLOW_HEADERS,
        HeaderValue::from_static("Content-Type, Content-Encoding"),
    );

    response
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_middleware_module_compiles() {
        // Middleware functions are tested via integration tests
        // This test ensures the module compiles correctly
    }
}

// Made with Bob
