//! Trace query routes

use axum::{routing::get, Router};

use crate::handlers::traces;

/// Create trace routes
///
/// Registers the following endpoints:
/// - GET /api/v1/traces - List traces with filtering and pagination
/// - GET /api/v1/traces/{id} - Get trace details by ID
pub fn routes() -> Router {
    Router::new()
        .route("/api/v1/traces", get(traces::list_traces))
        .route("/api/v1/traces/:id", get(traces::get_trace))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_list_traces_route() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/traces")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_trace_route() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/traces/trace-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_trace_not_found() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/traces/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

// Made with Bob
