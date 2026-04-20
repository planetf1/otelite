//! Dashboard HTTP server implementation

use crate::cache::LruCache;
use crate::config::DashboardConfig;
use crate::static_files;
use axum::{routing::get, Router};
use rotel_storage::StorageBackend;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;
use tower_http::trace::TraceLayer;
use tracing::info;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn StorageBackend>,
    pub cache: QueryCache,
}

/// Cache for query results
#[derive(Clone)]
pub struct QueryCache {
    /// Cache for logs queries (key: query params hash, value: JSON response)
    pub logs: LruCache<String, String>,
    /// Cache for traces queries
    pub traces: LruCache<String, String>,
    /// Cache for metrics queries
    pub metrics: LruCache<String, String>,
}

impl QueryCache {
    /// Create a new query cache with default settings
    pub fn new() -> Self {
        // Cache up to 100 queries per type, with 5 minute TTL
        let max_size = 100;
        let ttl = Duration::from_secs(300);

        Self {
            logs: LruCache::new(max_size, ttl),
            traces: LruCache::new(max_size, ttl),
            metrics: LruCache::new(max_size, ttl),
        }
    }

    /// Create cache key from query parameters
    pub fn make_key<T: Serialize>(params: &T) -> String {
        // Simple serialization-based key
        serde_json::to_string(params).unwrap_or_default()
    }
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Dashboard server
pub struct DashboardServer {
    config: Arc<DashboardConfig>,
    state: AppState,
}

impl DashboardServer {
    /// Create a new dashboard server
    pub fn new(config: DashboardConfig, storage: Arc<dyn StorageBackend>) -> Self {
        let state = AppState {
            storage,
            cache: QueryCache::new(),
        };

        Self {
            config: Arc::new(config),
            state,
        }
    }

    /// Build the router with all routes
    pub fn build_router(&self) -> Router {
        Router::new()
            // API routes - Health
            .route("/api/health", get(crate::api::health_check))
            // API routes - Logs
            .route("/api/logs", get(crate::api::logs::list_logs))
            .route("/api/logs/export", get(crate::api::logs::export_logs))
            .route("/api/logs/{timestamp}", get(crate::api::logs::get_log))
            // API routes - Traces
            .route("/api/traces", get(crate::api::traces::list_traces))
            .route("/api/traces/export", get(crate::api::traces::export_traces))
            .route("/api/traces/{trace_id}", get(crate::api::traces::get_trace))
            // API routes - Metrics
            .route("/api/metrics", get(crate::api::metrics::list_metrics))
            .route("/api/metrics/names", get(crate::api::metrics::list_metric_names))
            .route("/api/metrics/aggregate", get(crate::api::metrics::aggregate_metrics))
            .route("/api/metrics/export", get(crate::api::metrics::export_metrics))
            // Static file serving (index.html, CSS, JS)
            .fallback(static_files::serve_static_file)
            // Add shared state
            .with_state(self.state.clone())
            // Add tracing middleware
            .layer(TraceLayer::new_for_http())
    }

    /// Start the dashboard server
    pub async fn start(self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = self.config.bind_address;
        let router = self.build_router();

        info!("Starting dashboard server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        axum::serve(listener, router).await?;

        Ok(())
    }
}

// Made with Bob
