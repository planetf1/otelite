//! Metrics API routes

use axum::{routing::get, Router};

use crate::handlers::metrics;

/// Create metrics routes
///
/// Registers the following endpoints:
/// - GET /api/v1/metrics - List metrics with filtering and pagination
/// - GET /api/v1/metrics/{name}/stats - Get metric statistics
pub fn routes() -> Router {
    Router::new()
        .route("/api/v1/metrics", get(metrics::list_metrics))
        .route(
            "/api/v1/metrics/:name/stats",
            get(metrics::get_metric_stats),
        )
}

// Made with Bob
