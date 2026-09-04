//! Dashboard HTTP server implementation

use crate::cache::LruCache;
use crate::cached::{cache_handler, CacheState};
use crate::config::DashboardConfig;
use crate::pricing_cache::PricingCache;
use crate::static_files;
use axum::{
    routing::{get, post},
    Router,
};
use otelite_core::storage::StorageBackend;
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
        crate::api::traces::get_trace_logs,
        crate::api::traces::export_traces,
        crate::api::metrics::list_metrics,
        crate::api::metrics::list_metric_names,
        crate::api::metrics::aggregate_metrics,
        crate::api::metrics::get_metric_timeseries,
        crate::api::metrics::export_metrics,
        crate::api::genai::get_token_usage,
        crate::api::genai::get_cost_series,
        crate::api::genai::get_top_spans,
        crate::api::genai::get_top_sessions,
        crate::api::genai::get_top_conversations,
        crate::api::genai::get_finish_reasons,
        crate::api::genai::get_distributions,
        crate::api::genai::get_latency_stats,
        crate::api::genai::get_latency_percentiles,
        crate::api::genai::get_genai_capabilities,
        crate::api::genai::get_model_performance,
        crate::api::genai::get_error_rate,
        crate::api::genai::get_tool_usage,
        crate::api::genai::get_retry_stats,
        crate::api::genai::get_retrieval_stats,
        crate::api::genai::get_pricing_metadata,
        crate::api::genai::get_agent_framework_defs,
        crate::api::genai::get_truncation_rate,
        crate::api::genai::get_cache_hit_rate,
        crate::api::genai::get_agent_roles,
        crate::api::genai::get_reasoning_share,
        crate::api::genai::get_agents,
        crate::api::genai::get_projects,
        crate::api::genai::get_provider_mix,
        crate::api::genai::get_request_param_profile,
        crate::api::genai::get_conversation_depth,
        crate::api::genai::get_latency_series,
        crate::api::genai::get_calls_series,
        crate::api::genai::get_latency_by_context,
        crate::api::genai::get_error_types,
        crate::api::genai::get_model_drift,
        crate::api::genai::get_tool_approvals,
        crate::api::genai::get_stop_reasons,
        crate::api::genai::get_context_type_split,
        crate::api::genai::get_tool_errors,
        crate::api::genai::get_hour_of_day,
        crate::api::genai::get_effort_breakdown,
        crate::api::genai::get_efficiency_stats,
        crate::api::genai::get_codex_ttft,
        crate::api::genai::get_agent_project_rollup,
        crate::api::genai::get_mcp_health,
        crate::api::genai::get_guardian_stats,
        crate::api::genai::get_multi_agent_stats,
        crate::api::genai::get_codex_turn_breakdown,
        crate::api::genai::get_session_model_breakdown,
        crate::api::genai::get_speed_distribution,
        crate::api::genai::get_cross_tool_ttft,
        crate::api::genai::get_hook_overhead,
        crate::api::sessions::get_session_costs,
        crate::api::sessions::get_session_cost_distribution,
        crate::api::sessions::get_session_context,
    ),
    components(
        schemas(
            otelite_core::api::ErrorResponse,
            otelite_core::api::LogsResponse,
            otelite_core::api::LogEntry,
            otelite_core::api::Resource,
            otelite_core::api::TracesResponse,
            otelite_core::api::TraceEntry,
            otelite_core::api::TraceDetail,
            otelite_core::api::SpanEntry,
            otelite_core::api::SpanStatus,
            otelite_core::api::SpanEvent,
            otelite_core::api::MetricResponse,
            otelite_core::api::TokenUsageResponse,
            otelite_core::api::TokenUsageSummary,
            otelite_core::api::ModelUsage,
            otelite_core::api::SystemUsage,
            otelite_core::api::CostSeriesPoint,
            otelite_core::api::TopSpan,
            otelite_core::api::FinishReasonCount,
            otelite_core::api::LatencyStats,
            otelite_core::api::LatencyPercentilePoint,
            otelite_core::api::LatencyPercentileSeries,
            otelite_core::api::LatencyPercentilesResponse,
            otelite_core::api::DistributionBucket,
            otelite_core::api::DistributionStats,
            otelite_core::api::DistributionResponse,
            otelite_core::api::GenAiMetricCapability,
            otelite_core::api::GenAiCorrelationProvenance,
            otelite_core::api::GenAiCapabilityReport,
            otelite_core::api::GenAiCapabilityResponse,
            otelite_core::api::ModelPerformanceWindow,
            otelite_core::api::ModelPerformanceQuery,
            otelite_core::api::ModelPerformanceResponse,
            otelite_core::api::ModelPerformanceIdentity,
            otelite_core::api::ModelPerformanceMetric,
            otelite_core::api::ModelPerformanceSample,
            otelite_core::api::ModelPerformancePercentile,
            otelite_core::api::ModelPerformanceDelta,
            otelite_core::api::ModelPerformanceCounts,
            otelite_core::api::ModelPerformanceErrorRate,
            otelite_core::api::ModelPerformanceErrorValue,
            otelite_core::api::ModelPerformanceDiagnosis,
            otelite_core::model_performance::ModelPerformanceAssessment,
            otelite_core::model_performance::ModelPerformanceMetricAssessment,
            otelite_core::model_performance::PerformanceMetricKind,
            otelite_core::model_performance::PerformanceChangeClass,
            otelite_core::model_performance::PerformanceConfidence,
            otelite_core::model_performance::TtftTrust,
            otelite_core::model_performance::WorkloadShiftEvidence,
            crate::api::genai::ModelPerformanceQueryParams,
            otelite_core::api::ErrorRateByModel,
            otelite_core::api::ToolUsage,
            otelite_core::api::RetryStats,
            otelite_core::api::RetrievalStats,
            otelite_core::api::TopRetrievalQuery,
            crate::api::health::HealthResponse,
            crate::api::stats::StatsResponse,
            crate::api::admin::PurgeAllResponse,
            crate::api::metrics::AggregateResponse,
            crate::api::metrics::TimeBucket,
            crate::api::metrics::TimeseriesQuery,
            crate::api::genai::TokenUsageQuery,
            crate::api::genai::CostSeriesQuery,
            crate::api::genai::TopSpansQuery,
            crate::api::genai::TopGroupQuery,
            crate::api::genai::FinishReasonsQuery,
            crate::api::genai::LatencyQuery,
            crate::api::genai::ErrorRateQuery,
            crate::api::genai::ToolUsageQuery,
            crate::api::genai::RetryStatsQuery,
            crate::api::genai::RetrievalStatsQuery,
            crate::api::genai::PricingMetadata,
            otelite_core::agent_frameworks::AgentFrameworkRecognizer,
            otelite_core::api::ToolApprovalStats,
            otelite_core::api::ToolApprovalEntry,
            otelite_core::api::StopReasonCount,
            otelite_core::api::ContextTypeSplit,
            otelite_core::api::ToolErrorEntry,
            otelite_core::api::HourOfDayBucket,
            otelite_core::api::ReasoningShareResponse,
            otelite_core::api::ReasoningShareByModel,
            otelite_core::api::ReasoningEffortEntry,
            otelite_core::api::SessionCost,
            otelite_core::api::SessionCostResponse,
            otelite_core::api::CostBucket,
            otelite_core::api::CostDistributionResponse,
            otelite_core::api::ProjectRollup,
            otelite_core::api::ProjectTopModel,
            otelite_core::api::ProjectRollupResponse,
            otelite_core::api::SessionContextResponse,
            otelite_core::api::SessionContextSession,
            otelite_core::api::SessionContextSpan,
            otelite_core::api::SessionContextLog,
            otelite_core::api::SessionContextMetric,
            otelite_core::api::SessionContextTimelineEvent,
            crate::api::sessions::SessionContextQuery,
            otelite_core::api::EffortBreakdownRow,
            otelite_core::api::EffortBreakdownResponse,
            otelite_core::api::EfficiencyStats,
            otelite_core::api::AgentEfficiencyRow,
            otelite_core::api::CodexTtftByModel,
            otelite_core::api::CodexTtftResponse,
            otelite_core::api::ProjectRollupEntry,
            otelite_core::api::AgentProjectRollupResponse,
            otelite_core::api::McpHealthEntry,
            otelite_core::api::McpHealthResponse,
            otelite_core::api::GuardianRiskLevelEntry,
            otelite_core::api::GuardianActionEntry,
            otelite_core::api::GuardianStatsResponse,
            otelite_core::api::MultiAgentRoleEntry,
            otelite_core::api::MultiAgentStatsResponse,
            otelite_core::api::CodexTurnBreakdownRow,
            otelite_core::api::CodexTurnBreakdownResponse,
            otelite_core::api::SessionModelRow,
            otelite_core::api::SessionModelBreakdown,
            otelite_core::api::SpeedBucket,
            otelite_core::api::SpeedDistribution,
            otelite_core::api::CrossToolTtftRow,
            otelite_core::api::CrossToolTtftResponse,
            otelite_core::api::HookOverheadRow,
            otelite_core::api::HookOverheadResponse,
            otelite_core::api::ToolFailureRow,
            otelite_core::api::ToolFailureRatesResponse,
            otelite_core::api::DailyToolMixRow,
            otelite_core::api::DailyToolMixResponse,
            otelite_core::api::SkillActivityRow,
            otelite_core::api::SkillActivityResponse,
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
        title = "Otelite API",
        version = "1.0.0",
        description = "OpenTelemetry data query and visualization API",
        contact(
            name = "Otelite",
            url = "https://github.com/yourusername/otelite"
        )
    )
)]
struct ApiDoc;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub storage: Arc<dyn StorageBackend>,
    pub cache: QueryCache,
    /// Short-TTL response cache + in-flight dedup for read-only endpoints.
    pub cache_state: Arc<CacheState>,
    /// LiteLLM pricing database, refreshed periodically in the background.
    pub pricing: PricingCache,
    /// Time at which the server started (for uptime calculation)
    pub start_time: Arc<Instant>,
    /// OTLP receiver ports, populated from DashboardConfig
    pub otlp_grpc_port: u16,
    pub otlp_http_port: u16,
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

    /// Clear every cached query result. Called after data-mutating
    /// operations so stale data is never served.
    pub fn clear_all(&self) {
        self.logs.clear();
        self.traces.clear();
        self.metrics.clear();
    }
}

impl Default for QueryCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Dashboard server
/// A handle that, when triggered, starts the dashboard's graceful
/// shutdown (stop accepting, finish in-flight requests).
#[derive(Clone)]
pub struct DashboardShutdown(Arc<tokio::sync::Notify>);

impl DashboardShutdown {
    /// Start the graceful shutdown.
    pub fn trigger(&self) {
        self.0.notify_waiters();
    }
}

pub struct DashboardServer {
    config: Arc<DashboardConfig>,
    state: AppState,
    shutdown_notify: Arc<tokio::sync::Notify>,
}

impl DashboardServer {
    /// Create a new dashboard server
    pub fn new(config: DashboardConfig, storage: Arc<dyn StorageBackend>) -> Self {
        let pricing = PricingCache::new().spawn_refresher();
        let state = AppState {
            storage,
            cache: QueryCache::new(),
            cache_state: CacheState::new(),
            pricing,
            start_time: Arc::new(Instant::now()),
            otlp_grpc_port: config.otlp_grpc_port,
            otlp_http_port: config.otlp_http_port,
        };

        Self {
            config: Arc::new(config),
            state,
            shutdown_notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Handle for triggering graceful shutdown from a signal handler.
    pub fn shutdown_handle(&self) -> DashboardShutdown {
        DashboardShutdown(Arc::clone(&self.shutdown_notify))
    }

    /// Build the router with all routes
    pub fn build_router(&self) -> Router {
        let api = Router::new()
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
            .route("/api/traces/{trace_id}/logs", get(crate::api::traces::get_trace_logs))
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
            .route("/api/genai/cost_series", get(crate::api::genai::get_cost_series))
            .route("/api/genai/top_spans", get(crate::api::genai::get_top_spans))
            .route("/api/genai/top_sessions", get(crate::api::genai::get_top_sessions))
            .route("/api/genai/top_conversations", get(crate::api::genai::get_top_conversations))
            .route("/api/genai/finish_reasons", get(crate::api::genai::get_finish_reasons))
            .route("/api/genai/latency_stats", get(crate::api::genai::get_latency_stats))
            .route(
                "/api/genai/latency_percentiles",
                get(crate::api::genai::get_latency_percentiles),
            )
            .route("/api/genai/distributions", get(crate::api::genai::get_distributions))
            .route("/api/genai/capabilities", get(crate::api::genai::get_genai_capabilities))
            .route(
                "/api/genai/model-performance",
                get(crate::api::genai::get_model_performance),
            )
            .route("/api/genai/error_rate", get(crate::api::genai::get_error_rate))
            .route("/api/genai/tool_usage", get(crate::api::genai::get_tool_usage))
            .route("/api/genai/retry_stats", get(crate::api::genai::get_retry_stats))
            .route("/api/genai/retrieval_stats", get(crate::api::genai::get_retrieval_stats))
            .route("/api/genai/pricing_metadata", get(crate::api::genai::get_pricing_metadata))
            .route("/api/genai/agent_framework_defs", get(crate::api::genai::get_agent_framework_defs))
            .route("/api/genai/truncation_rate", get(crate::api::genai::get_truncation_rate))
            .route("/api/genai/cache_hit_rate", get(crate::api::genai::get_cache_hit_rate))
            .route("/api/genai/agent_roles", get(crate::api::genai::get_agent_roles))
            .route(
                "/api/genai/reasoning_share",
                get(crate::api::genai::get_reasoning_share),
            )
            .route("/api/genai/agents", get(crate::api::genai::get_agents))
            .route("/api/genai/projects", get(crate::api::genai::get_projects))
            .route("/api/genai/provider_mix", get(crate::api::genai::get_provider_mix))
            .route("/api/genai/request_param_profile", get(crate::api::genai::get_request_param_profile))
            .route("/api/genai/conversation_depth", get(crate::api::genai::get_conversation_depth))
            .route("/api/genai/latency_series", get(crate::api::genai::get_latency_series))
            .route("/api/genai/calls_series", get(crate::api::genai::get_calls_series))
            .route("/api/genai/latency_by_context", get(crate::api::genai::get_latency_by_context))
            .route("/api/genai/error_types", get(crate::api::genai::get_error_types))
            .route("/api/genai/model_drift", get(crate::api::genai::get_model_drift))
            .route("/api/genai/tool_approvals", get(crate::api::genai::get_tool_approvals))
            .route("/api/genai/stop_reasons", get(crate::api::genai::get_stop_reasons))
            .route("/api/genai/context_type_split", get(crate::api::genai::get_context_type_split))
            .route("/api/genai/tool_errors", get(crate::api::genai::get_tool_errors))
            .route("/api/genai/hour_of_day", get(crate::api::genai::get_hour_of_day))
            .route("/api/genai/effort_breakdown", get(crate::api::genai::get_effort_breakdown))
            .route("/api/genai/efficiency_stats", get(crate::api::genai::get_efficiency_stats))
            .route("/api/genai/codex_ttft", get(crate::api::genai::get_codex_ttft))
            .route("/api/genai/project_rollup", get(crate::api::genai::get_agent_project_rollup))
            .route("/api/genai/mcp_health", get(crate::api::genai::get_mcp_health))
            .route("/api/genai/guardian_stats", get(crate::api::genai::get_guardian_stats))
            .route("/api/genai/multi_agent_stats", get(crate::api::genai::get_multi_agent_stats))
            .route("/api/genai/codex_turn_breakdown", get(crate::api::genai::get_codex_turn_breakdown))
            .route("/api/genai/session_model_breakdown", get(crate::api::genai::get_session_model_breakdown))
            .route("/api/genai/speed_distribution", get(crate::api::genai::get_speed_distribution))
            .route("/api/genai/cross_tool_ttft", get(crate::api::genai::get_cross_tool_ttft))
            .route("/api/genai/hook_overhead", get(crate::api::genai::get_hook_overhead))
            .route("/api/genai/tool_failure_rates", get(crate::api::genai::get_tool_failure_rates))
            .route("/api/genai/daily_tool_mix", get(crate::api::genai::get_daily_tool_mix))
            .route("/api/genai/skill_activity", get(crate::api::genai::get_skill_activity))
            // API routes - Sessions
            .route("/api/sessions", get(crate::api::sessions::list_sessions))
            .route("/api/sessions/costs", get(crate::api::sessions::get_session_costs))
            .route(
                "/api/sessions/cost-distribution",
                get(crate::api::sessions::get_session_cost_distribution),
            )
            .route("/api/sessions/{session_id}/diagnose", get(crate::api::sessions::get_session_diagnose))
            .route(
                "/api/sessions/{session_id}/context",
                get(crate::api::sessions::get_session_context),
            )
            // OpenAPI spec endpoint
            .route("/api/openapi.json", get(|| async {
                axum::Json(ApiDoc::openapi())
            }))
            // Add shared state
            .with_state(self.state.clone());

        api
            // Short-TTL response cache + in-flight dedup for read-only
            // endpoints (applies only to the whitelisted API routes below;
            // static files and mutating endpoints pass through untouched)
            .layer(axum::middleware::from_fn_with_state(
                Arc::clone(&self.state.cache_state),
                cache_handler,
            ))
            // Static file serving (index.html, CSS, JS)
            .fallback(static_files::serve_static_file)
            // Add tracing middleware
            .layer(TraceLayer::new_for_http())
    }

    /// Start the dashboard server, serving until an error or a graceful
    /// shutdown triggered through [`DashboardServer::shutdown_handle`].
    pub async fn start(self) -> Result<(), Box<dyn std::error::Error>> {
        let addr = self.config.bind_address;
        let router = self.build_router();

        info!("Starting dashboard server on {}", addr);

        let listener = tokio::net::TcpListener::bind(addr).await?;
        let shutdown_notify = Arc::clone(&self.shutdown_notify);
        axum::serve(listener, router)
            .with_graceful_shutdown(async move {
                shutdown_notify.notified().await;
                info!("Shutting down dashboard server");
            })
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod server_shutdown_tests {
    use super::*;
    use otelite_storage::sqlite::SqliteBackend;
    use otelite_storage::StorageConfig;
    use tempfile::TempDir;

    /// M16: `start()` must return promptly once the shutdown handle is
    /// triggered (the signal path in `otelite serve`), instead of
    /// blocking on `axum::serve` forever.
    #[tokio::test]
    async fn test_start_returns_on_shutdown_trigger() {
        let temp = TempDir::new().expect("temp dir");
        let config = StorageConfig::default().with_data_dir(temp.path().join("otelite.db"));
        let mut storage = SqliteBackend::new(config);
        storage.initialize().await.expect("storage initializes");
        let storage: Arc<dyn StorageBackend> = Arc::new(storage);

        let port = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind free port");
            l.local_addr().expect("local addr").port()
        };
        let dashboard = DashboardServer::new(
            DashboardConfig::default()
                .with_bind_address(format!("127.0.0.1:{port}").parse().unwrap()),
            storage,
        );
        let handle = dashboard.shutdown_handle();
        let serve = tokio::spawn(async move { dashboard.start().await.map_err(|e| e.to_string()) });

        // Give the server a moment to bind, then trigger the shutdown.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        handle.trigger();

        let result = tokio::time::timeout(std::time::Duration::from_secs(5), serve)
            .await
            .expect("start() must return after the shutdown trigger")
            .expect("serve task must not panic");
        result.expect("a clean shutdown is not a server error");
    }
}
