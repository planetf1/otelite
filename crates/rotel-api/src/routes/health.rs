//! Health and readiness check routes

use axum::{routing::get, Router};

use crate::handlers::health;

/// Create health check routes
///
/// Registers the following endpoints:
/// - GET /health - Comprehensive health check with system statistics
/// - GET /ready - Readiness check for orchestrators
pub fn routes() -> Router {
    Router::new()
        .route("/health", get(health::health_check))
        .route("/ready", get(health::readiness_check))
}

// Made with Bob
