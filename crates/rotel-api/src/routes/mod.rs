//! API route definitions and registration

pub mod docs;
pub mod health;
pub mod logs;
pub mod metrics;
pub mod traces;

use axum::Router;

/// Create the main API router with all routes registered
pub fn create_router() -> Router {
    Router::new()
        .merge(logs::routes())
        .merge(traces::routes())
        .merge(metrics::routes())
        .merge(health::routes())
        .merge(docs::routes())
}

// Made with Bob
