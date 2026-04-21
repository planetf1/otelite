//! Dashboard HTTP server implementation

use crate::cache::LruCache;
use crate::config::DashboardConfig;
use crate::static_files;
use axum::{
    routing::{get, post},
    Router,
};
use rotel_storage::StorageBackend;
use serde::Serialize;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tower_http::trace::TraceLayer;
use tracing::info;
use utoipa::OpenApi;

/// OpenAPI documentation
#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::health::health_check,
        crate::api::stats::get_stats,
        crate::api::admin::purge_all,
        crate::api::help::api_help,
        crate::api::logs::list_logs,
        crate::api::logs::get_log,
        crate::api::logs::export_logs,
        crate::api::traces::list_traces,
        crate::api::traces::get_trace,
        crate::api::traces::export_traces,
        crate::api::metrics::list_metrics,
        crate::api::metrics::list_metric_names,
        crate::api::metrics::aggregate_metrics,
        crate::api::metrics::get_metric_timeseries,
        crate::api::metrics::export_metrics,
        crate::api::genai::get_token_usage,
    ),
    components(
        schemas(
            rotel_core::api::ErrorResponse,
            rotel_core::api::LogsResponse,
            rotel_core::api::LogEntry,
            rotel_core::api::Resource,
            rotel_core::api::TracesResponse,
            rotel_core::api::TraceEntry,
            rotel_core::api::TraceDetail,
            rotel_core::api::SpanEntry,
            rotel_core::api::SpanStatus,
            rotel_core::api::SpanEvent,
            rotel_core::api::MetricResponse,
            rotel_core::api::TokenUsageResponse,
            rotel_core::api::TokenUsageSummary,
            rotel_core::api::ModelUsage,
            rotel_core::api::SystemUsage,
            crate::api::health::HealthResponse,
            crate::api::stats::StatsResponse,
            crate::api::admin::PurgeAllResponse,
            crate::api::metrics::AggregateResponse,
            crate::api::metrics::TimeBucket,
            crate::api::metrics::TimeseriesQuery,
            crate::api::genai::TokenUsageQuery,
        )
    ),
    tags(
        (name = "health", description = "Health check endpoints"),
        (name = "stats", description = "Storage statistics endpoints"),
        (name = "help", description = "API documentation and help"),
        (name = "logs", description = "Log query and export endpoints"),
        (name = "traces", description = "Trace query and export endpoints"),
        (name = "metrics", description = "Metric query and aggregation endpoints"),
        (name = "genai", description = "GenAI/LLM token usage and analytics endpoints"),
        (name = "admin", description = "Administrative endpoints for data management")
    ),
    info(
        title = "Rotel API",
        version = "1.0.0",
        description = "OpenTelemetry data query and visualization API",
        contact(
            name = "Rotel",
            url = "https://github.com/yourusername/rotel"
        )
    )
)]
struct ApiDoc;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn StorageBackend>,
    pub cache: QueryCache,
    /// Time at which the server started (for uptime calculation)
    pub start_time: Arc<Instant>,
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
            start_time: Arc::new(Instant::now()),
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
            // API routes - Help
            .route("/api/help", get(crate::api::api_help))
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
            .route("/api/metrics/{name}/timeseries", get(crate::api::metrics::get_metric_timeseries))
            .route("/api/metrics/export", get(crate::api::metrics::export_metrics))
            // API routes - Resource keys typeahead
            .route("/api/resource-keys", get(crate::api::resource_keys::get_resource_keys))
            // API routes - Stats
            .route("/api/stats", get(crate::api::stats::get_stats))
            // API routes - Admin
            .route("/api/admin/purge", post(crate::api::admin::purge_all))
            // API routes - GenAI
            .route("/api/genai/usage", get(crate::api::get_token_usage))
            // OpenAPI spec endpoint
            .route("/api/openapi.json", get(|| async {
                axum::Json(ApiDoc::openapi())
            }))
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
