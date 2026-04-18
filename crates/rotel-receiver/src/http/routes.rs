// HTTP routes for OTLP receiver

use crate::health::HealthChecker;
use crate::http::handlers::{
    handle_health, handle_logs, handle_metrics, handle_traces, handle_unified,
};
use crate::signals::{LogsHandler, MetricsHandler, TracesHandler};
use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;

/// Create the main router with all OTLP endpoints
pub fn create_router(
    metrics_handler: Arc<MetricsHandler>,
    logs_handler: Arc<LogsHandler>,
    traces_handler: Arc<TracesHandler>,
    health_checker: Arc<HealthChecker>,
) -> Router {
    Router::new()
        // Health check endpoint
        .route("/health", get(handle_health))
        .route("/healthz", get(handle_health))
        // OTLP v1 signal-specific endpoints (recommended)
        .route("/v1/metrics", post(handle_metrics))
        .route("/v1/logs", post(handle_logs))
        .route("/v1/traces", post(handle_traces))
        // Legacy unified endpoint (for backward compatibility)
        .route("/v1/otlp", post(handle_unified))
        // Add shared state
        .with_state(AppState {
            metrics_handler,
            logs_handler,
            traces_handler,
            health_checker,
        })
}

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub metrics_handler: Arc<MetricsHandler>,
    pub logs_handler: Arc<LogsHandler>,
    pub traces_handler: Arc<TracesHandler>,
    pub health_checker: Arc<HealthChecker>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rotel_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};

    #[tokio::test]
    async fn test_create_router() {
        let mut storage = SqliteBackend::new(StorageConfig::default());
        storage
            .initialize()
            .await
            .expect("Failed to initialize storage");
        let storage = Arc::new(storage);

        let metrics_handler = Arc::new(MetricsHandler::new(storage.clone()));
        let logs_handler = Arc::new(LogsHandler::new(storage.clone()));
        let traces_handler = Arc::new(TracesHandler::new(storage));
        let health_checker = Arc::new(HealthChecker::new());

        let _router = create_router(
            metrics_handler,
            logs_handler,
            traces_handler,
            health_checker,
        );

        // Router created successfully - test passes if no panic
    }
}

// Made with Bob
