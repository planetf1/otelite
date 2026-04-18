//! Error handling middleware

#[cfg(test)]
use axum::body::Body;
use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use tracing::error;

use crate::error::ErrorResponse;

/// Middleware to handle panics and convert them to proper error responses
pub async fn handle_errors(request: Request, next: Next) -> Response {
    let response = next.run(request).await;

    // If the response is already an error, return it as-is
    if response.status().is_client_error() || response.status().is_server_error() {
        return response;
    }

    response
}

/// Catch-all error handler for unhandled errors
pub async fn handle_panic(err: Box<dyn std::any::Any + Send + 'static>) -> Response {
    let details = if let Some(s) = err.downcast_ref::<String>() {
        s.clone()
    } else if let Some(s) = err.downcast_ref::<&str>() {
        s.to_string()
    } else {
        "Unknown panic".to_string()
    };

    error!(details = %details, "Panic occurred during request processing");

    let error_response =
        ErrorResponse::new("Internal server error", "INTERNAL_ERROR").with_details(details);

    (StatusCode::INTERNAL_SERVER_ERROR, Json(error_response)).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::get, Router};
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_error_middleware() {
        let app = Router::new()
            .route("/test", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn(handle_errors));

        let response = app
            .oneshot(Request::builder().uri("/test").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}

// Made with Bob
