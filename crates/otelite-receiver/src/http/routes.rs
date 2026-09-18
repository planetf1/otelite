// HTTP routes for OTLP receiver

use crate::health::HealthChecker;
use crate::http::handlers::{
    handle_health, handle_logs, handle_metrics, handle_traces, handle_unified,
};
use crate::signals::{LogsHandler, MetricsHandler, TracesHandler};
use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware,
    response::Response,
    routing::{get, post},
    Json, Router,
};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// Create the main router with all OTLP endpoints.
///
/// `max_body_size` bounds the decoded (post-decompression) request
/// body; see [`crate::http::handlers::decode_body`].
///
/// `max_concurrent` bounds in-flight exports (#256): the four export
/// routes sit under a shared `try_acquire` semaphore — a burst of
/// exporters cannot pile up unbounded in-flight work (each holding a
/// 10 MB body, a conversion, and a blocking-pool write). Requests beyond
/// the limit are rejected immediately with 503, which OTLP exporters
/// retry. (tower's `ConcurrencyLimitLayer` was considered first, but it
/// *queues* excess requests rather than rejecting them — the point is
/// bounded memory plus a fast, retryable failure.) `/health` is outside
/// the limit so the saturation itself stays observable and reads are
/// unaffected.
#[allow(clippy::too_many_arguments)] // router factory: every component is a required peer
pub fn create_router(
    metrics_handler: Arc<MetricsHandler>,
    logs_handler: Arc<LogsHandler>,
    traces_handler: Arc<TracesHandler>,
    health_checker: Arc<HealthChecker>,
    max_body_size: usize,
    max_concurrent: usize,
) -> Router {
    let state = AppState {
        metrics_handler,
        logs_handler,
        traces_handler,
        health_checker,
        max_body_size,
    };

    let concurrency = Arc::new(Semaphore::new(max_concurrent));

    let exports = Router::new()
        // OTLP v1 signal-specific endpoints (recommended)
        .route("/v1/metrics", post(handle_metrics))
        .route("/v1/logs", post(handle_logs))
        .route("/v1/traces", post(handle_traces))
        // Legacy unified endpoint (deprecated — see handle_unified)
        .route("/v1/otlp", post(handle_unified))
        .layer(middleware::from_fn_with_state(
            concurrency,
            concurrency_limit,
        ))
        .with_state(state.clone());

    let health = Router::new()
        .route("/health", get(handle_health))
        .route("/healthz", get(handle_health))
        .with_state(state);

    health.merge(exports)
}

/// Reject with 503 the moment the export concurrency limit is reached.
///
/// The permit is held for the whole request (body decode + parse + write)
/// and released when the response is produced.
async fn concurrency_limit(
    State(concurrency): State<Arc<Semaphore>>,
    req: Request,
    next: axum::middleware::Next,
) -> Result<Response, (StatusCode, Json<serde_json::Value>)> {
    let _permit = concurrency.try_acquire_owned().map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "otelite receiver at its concurrency limit; retry the export",
                "status": 503,
            })),
        )
    })?;
    let response = next.run(req).await;
    drop(_permit);
    Ok(response)
}

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub metrics_handler: Arc<MetricsHandler>,
    pub logs_handler: Arc<LogsHandler>,
    pub traces_handler: Arc<TracesHandler>,
    pub health_checker: Arc<HealthChecker>,
    pub max_body_size: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_create_router() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut storage = SqliteBackend::new(config);
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
            10 * 1024 * 1024,
            100,
        );

        // Router created successfully - test passes if no panic
    }
}
