//! Log query routes

use axum::{routing::get, Router};

use crate::handlers::logs;

/// Create log routes
///
/// Registers the following endpoints:
/// - GET /api/v1/logs - List logs with filtering and pagination
/// - GET /api/v1/logs/{id} - Get log details by ID
pub fn routes() -> Router {
    Router::new()
        .route("/api/v1/logs", get(logs::list_logs))
        .route("/api/v1/logs/:id", get(logs::get_log))
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
    async fn test_list_logs_route() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/logs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_log_route() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/logs/log-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_log_not_found() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/logs/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}

// Made with Bob
