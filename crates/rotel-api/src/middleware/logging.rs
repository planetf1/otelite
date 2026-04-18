//! Request logging middleware

#[cfg(test)]
use axum::http::StatusCode;
use axum::{extract::Request, middleware::Next, response::Response};
use std::time::Instant;
use tracing::{info, warn};
use uuid::Uuid;

/// Request ID header name
pub const REQUEST_ID_HEADER: &str = "X-Request-ID";

/// Middleware to log incoming requests and responses
pub async fn log_request(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let start = Instant::now();

    // Generate or extract request ID
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    info!(
        request_id = %request_id,
        method = %method,
        uri = %uri,
        "Incoming request"
    );

    // Process the request
    let response = next.run(request).await;

    let duration = start.elapsed();
    let status = response.status();

    // Log based on status code
    if status.is_server_error() {
        warn!(
            request_id = %request_id,
            method = %method,
            uri = %uri,
            status = %status,
            duration_ms = duration.as_millis(),
            "Request completed with server error"
        );
    } else if status.is_client_error() {
        warn!(
            request_id = %request_id,
            method = %method,
            uri = %uri,
            status = %status,
            duration_ms = duration.as_millis(),
            "Request completed with client error"
        );
    } else {
        info!(
            request_id = %request_id,
            method = %method,
            uri = %uri,
            status = %status,
            duration_ms = duration.as_millis(),
            "Request completed successfully"
        );
    }

    // Add request ID to response headers
    let mut response = response;
    response
        .headers_mut()
        .insert(REQUEST_ID_HEADER, request_id.parse().unwrap());

    response
}

/// Middleware to add request ID to all responses
pub async fn add_request_id(mut request: Request, next: Next) -> Response {
    // Generate request ID if not present
    let request_id = request
        .headers()
        .get(REQUEST_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    // Add to request headers
    request
        .headers_mut()
        .insert(REQUEST_ID_HEADER, request_id.parse().unwrap());

    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_request_logging() {
        let app = Router::new()
            .route("/test", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn(log_request));

        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.headers().contains_key(REQUEST_ID_HEADER));
    }
}

// Made with Bob
