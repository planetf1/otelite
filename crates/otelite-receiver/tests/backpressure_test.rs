// Backpressure and write-health tests (#256):
//
//   * HTTP exports are bounded by the configured concurrency limit; the
//     excess is rejected with 503 (which OTLP exporters retry), and
//     /health + reads stay responsive while the limit is saturated.
//   * gRPC exports are bounded the same way; the excess gets UNAVAILABLE.
//   * /health flips unhealthy after sustained PERSISTENT write failures
//     (fault injection: a storage double that fails N times with
//     DiskFullError) and re-arms after a success.
//
// All servers bind 127.0.0.1:0 (OS-assigned free port). Slow/faulty
// storage doubles wrap a real tempdir SQLite backend: the single-row
// write methods are overridden (sleep / fail), the trait's batch defaults
// loop over them, and every read delegates to the real backend.

mod http_test_utils;

use async_trait::async_trait;
use http_test_utils::{create_logs_protobuf, create_metrics_protobuf};
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_client::MetricsServiceClient;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use otelite_core::api::{
    AgentRolesResponse, AgentRollupStorage, CacheEconomicsResponse, CacheHitRateByModel,
    CallsSeriesPoint, ContextTypeSplit, ConversationCostRow, ConversationDepthStats,
    CostSeriesPoint, DistributionResponse, ErrorRateByModel, ErrorTypeBreakdown, FinishReasonCount,
    HourOfDayBucket, LatencyByContextBin, LatencyPercentilesResponse, LatencySeriesPoint,
    LatencyStats, ModelDriftPair, ModelUsage, ProjectRollupStorage, ProviderMixResponse,
    ReasoningShareResponse, RequestParamProfile, RetrievalStats, RetryStats,
    SessionContextResponse, SessionCostRow, SessionCostStorage, StopReasonCount, SystemUsage,
    TokenUsageSummary, ToolApprovalStats, ToolErrorEntry, ToolUsage, TopSpan, TopSpanSort,
    TruncationRateByModel,
};
use otelite_core::filters::GenAiFilters;
use otelite_core::storage::{
    PurgeAllStats, PurgeOptions, QueryParams, Result, StorageBackend, StorageStats,
};
use otelite_core::telemetry::{LogRecord, Metric, Span};
use otelite_receiver::config::ReceiverConfig;
use otelite_receiver::grpc::GrpcServer;
use otelite_receiver::http::HttpServer;
use otelite_storage::{sqlite::SqliteBackend, StorageConfig};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;
use tokio::time::sleep;

const WRITE_DELAY: Duration = Duration::from_millis(400);

// ---------------------------------------------------------------------------
// Storage doubles
// ---------------------------------------------------------------------------

/// Writes sleep `delay` (holding the transport's concurrency slot); reads
/// hit a real tempdir database.
struct SlowStorage {
    inner: SqliteBackend,
    delay: Duration,
}

impl SlowStorage {
    async fn new(delay: Duration) -> (Arc<SlowStorage>, TempDir) {
        let temp_dir = TempDir::new().expect("temp dir");
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut inner = SqliteBackend::new(config);
        inner.initialize().await.expect("init inner storage");
        (Arc::new(SlowStorage { inner, delay }), temp_dir)
    }
}

/// Fails its next `remaining` writes (per signal) with a persistent
/// DiskFullError, then succeeds forever.
struct FaultyStorage {
    inner: SqliteBackend,
    remaining: AtomicUsize,
}

impl FaultyStorage {
    async fn new(failures: usize) -> (Arc<FaultyStorage>, TempDir) {
        let temp_dir = TempDir::new().expect("temp dir");
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut inner = SqliteBackend::new(config);
        inner.initialize().await.expect("init inner storage");
        (
            Arc::new(FaultyStorage {
                inner,
                remaining: AtomicUsize::new(failures),
            }),
            temp_dir,
        )
    }

    fn next_write(&self) -> Result<()> {
        let left = self.remaining.fetch_sub(1, Ordering::SeqCst);
        if left > 0 {
            Err(otelite_core::storage::StorageError::DiskFullError(
                "simulated full disk (fault injection)".to_string(),
            ))
        } else {
            Ok(())
        }
    }
}

fn query_unsupported() -> otelite_core::storage::StorageError {
    otelite_core::storage::StorageError::QueryError(
        "backpressure test double: queries are not supported".to_string(),
    )
}

macro_rules! delegate {
    ($self:ident, $method:ident) => {
        $self.inner.$method().await
    };
    ($self:ident, $method:ident, $($arg:ident : $ty:ty),+ $(,)?) => {
        $self.inner.$method($($arg),*).await
    };
}

#[async_trait]
impl StorageBackend for SlowStorage {
    async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }
    async fn write_log(&self, _log: &LogRecord) -> Result<()> {
        sleep(self.delay).await;
        Ok(())
    }
    async fn write_span(&self, _span: &Span) -> Result<()> {
        sleep(self.delay).await;
        Ok(())
    }
    async fn write_metric(&self, _metric: &Metric) -> Result<()> {
        sleep(self.delay).await;
        Ok(())
    }

    async fn query_logs(&self, params: &QueryParams) -> Result<Vec<LogRecord>> {
        delegate!(self, query_logs, params: &QueryParams)
    }
    async fn query_spans(&self, params: &QueryParams) -> Result<Vec<Span>> {
        delegate!(self, query_spans, params: &QueryParams)
    }
    async fn query_spans_for_trace_list(
        &self,
        params: &QueryParams,
        trace_limit: usize,
    ) -> Result<Vec<Span>> {
        delegate!(self, query_spans_for_trace_list, params: &QueryParams, trace_limit: usize)
    }
    async fn query_trace_summaries(
        &self,
        _params: &QueryParams,
        _trace_limit: usize,
    ) -> Result<Vec<otelite_core::api::TraceEntry>> {
        Err(query_unsupported())
    }
    async fn query_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>> {
        delegate!(self, query_metrics, params: &QueryParams)
    }
    async fn query_latest_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>> {
        delegate!(self, query_latest_metrics, params: &QueryParams)
    }
    async fn query_distinct_metric_names(&self) -> Result<Vec<String>> {
        delegate!(self, query_distinct_metric_names)
    }
    async fn stats(&self) -> Result<StorageStats> {
        delegate!(self, stats)
    }
    async fn purge(&self, options: &PurgeOptions) -> Result<u64> {
        delegate!(self, purge, options: &PurgeOptions)
    }
    async fn purge_all(&self) -> Result<PurgeAllStats> {
        delegate!(self, purge_all)
    }
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
    async fn distinct_resource_keys(&self, signal: &str) -> Result<Vec<String>> {
        delegate!(self, distinct_resource_keys, signal: &str)
    }
    async fn query_token_usage(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<(TokenUsageSummary, Vec<ModelUsage>, Vec<SystemUsage>)> {
        delegate!(
            self,
            query_token_usage,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_cost_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_ns: i64,
        filters: &GenAiFilters,
    ) -> Result<Vec<CostSeriesPoint>> {
        delegate!(
            self,
            query_cost_series,
            start_time: Option<i64>,
            end_time: Option<i64>,
            bucket_ns: i64,
            filters: &GenAiFilters
        )
    }
    async fn query_top_spans(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
        sort_by: TopSpanSort,
        truncated_only: bool,
    ) -> Result<Vec<TopSpan>> {
        delegate!(
            self,
            query_top_spans,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize,
            sort_by: TopSpanSort,
            truncated_only: bool
        )
    }
    async fn query_top_sessions(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<SessionCostRow>> {
        delegate!(
            self,
            query_top_sessions,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize
        )
    }
    async fn query_top_conversations(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<ConversationCostRow>> {
        delegate!(
            self,
            query_top_conversations,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize
        )
    }
    async fn query_finish_reasons(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<FinishReasonCount>> {
        delegate!(
            self,
            query_finish_reasons,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_latency_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<LatencyStats>> {
        delegate!(
            self,
            query_latency_stats,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_latency_percentiles(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        bucket_secs: u64,
        metrics: &[&str],
        timezone: Option<&str>,
    ) -> Result<LatencyPercentilesResponse> {
        delegate!(
            self,
            query_latency_percentiles,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            bucket_secs: u64,
            metrics: &[&str],
            timezone: Option<&str>
        )
    }
    async fn query_distribution(
        &self,
        metric: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        buckets: usize,
        scale: &str,
    ) -> Result<DistributionResponse> {
        delegate!(
            self,
            query_distribution,
            metric: &str,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            buckets: usize,
            scale: &str
        )
    }
    async fn query_session_context(
        &self,
        session_id: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: u64,
    ) -> Result<Option<SessionContextResponse>> {
        delegate!(
            self,
            query_session_context,
            session_id: &str,
            start_time: Option<i64>,
            end_time: Option<i64>,
            limit: u64
        )
    }
    async fn query_error_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ErrorRateByModel>> {
        delegate!(
            self,
            query_error_rate,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_tool_usage(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<ToolUsage>> {
        delegate!(
            self,
            query_tool_usage,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize
        )
    }
    async fn query_retry_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<RetryStats> {
        delegate!(
            self,
            query_retry_stats,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_retrieval_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        top_queries_limit: usize,
    ) -> Result<RetrievalStats> {
        delegate!(
            self,
            query_retrieval_stats,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            top_queries_limit: usize
        )
    }
    async fn query_truncation_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<TruncationRateByModel>> {
        delegate!(
            self,
            query_truncation_rate,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_cache_hit_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<CacheHitRateByModel>> {
        delegate!(
            self,
            query_cache_hit_rate,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_cache_economics(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_ns: i64,
    ) -> Result<CacheEconomicsResponse> {
        delegate!(
            self,
            query_cache_economics,
            start_time: Option<i64>,
            end_time: Option<i64>,
            bucket_ns: i64
        )
    }
    async fn query_reasoning_share(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<ReasoningShareResponse> {
        delegate!(
            self,
            query_reasoning_share,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_agent_rollup(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_secs: u64,
    ) -> Result<Vec<AgentRollupStorage>> {
        delegate!(
            self,
            query_agent_rollup,
            start_time: Option<i64>,
            end_time: Option<i64>,
            bucket_secs: u64
        )
    }
    async fn query_project_rollup(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<ProjectRollupStorage>> {
        delegate!(
            self,
            query_project_rollup,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_session_costs(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<SessionCostStorage>> {
        delegate!(
            self,
            query_session_costs,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_agent_roles(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<AgentRolesResponse> {
        delegate!(
            self,
            query_agent_roles,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_provider_mix(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<ProviderMixResponse> {
        delegate!(
            self,
            query_provider_mix,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_request_param_profile(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<RequestParamProfile> {
        delegate!(
            self,
            query_request_param_profile,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_conversation_depth(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<ConversationDepthStats> {
        delegate!(
            self,
            query_conversation_depth,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_latency_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_secs: u64,
        filters: &GenAiFilters,
        all_spans: bool,
        timezone: Option<&str>,
    ) -> Result<Vec<LatencySeriesPoint>> {
        delegate!(
            self,
            query_latency_series,
            start_time: Option<i64>,
            end_time: Option<i64>,
            bucket_secs: u64,
            filters: &GenAiFilters,
            all_spans: bool,
            timezone: Option<&str>
        )
    }
    async fn query_calls_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        bucket_secs: u64,
        all_spans: bool,
    ) -> Result<Vec<CallsSeriesPoint>> {
        delegate!(
            self,
            query_calls_series,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            bucket_secs: u64,
            all_spans: bool
        )
    }
    async fn query_latency_by_context(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<LatencyByContextBin>> {
        delegate!(
            self,
            query_latency_by_context,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_error_types(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ErrorTypeBreakdown>> {
        delegate!(
            self,
            query_error_types,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_model_drift(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ModelDriftPair>> {
        delegate!(
            self,
            query_model_drift,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_tool_approvals(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<ToolApprovalStats> {
        delegate!(
            self,
            query_tool_approvals,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_stop_reasons(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<StopReasonCount>> {
        delegate!(
            self,
            query_stop_reasons,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_context_type_split(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ContextTypeSplit>> {
        delegate!(
            self,
            query_context_type_split,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_tool_errors(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<ToolErrorEntry>> {
        delegate!(
            self,
            query_tool_errors,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize
        )
    }
    async fn query_hour_of_day(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<HourOfDayBucket>> {
        delegate!(
            self,
            query_hour_of_day,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
}

#[async_trait]
impl StorageBackend for FaultyStorage {
    async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }
    async fn write_log(&self, _log: &LogRecord) -> Result<()> {
        self.next_write()
    }
    async fn write_span(&self, _span: &Span) -> Result<()> {
        self.next_write()
    }
    async fn write_metric(&self, _metric: &Metric) -> Result<()> {
        self.next_write()
    }

    async fn query_logs(&self, params: &QueryParams) -> Result<Vec<LogRecord>> {
        delegate!(self, query_logs, params: &QueryParams)
    }
    async fn query_spans(&self, params: &QueryParams) -> Result<Vec<Span>> {
        delegate!(self, query_spans, params: &QueryParams)
    }
    async fn query_spans_for_trace_list(
        &self,
        params: &QueryParams,
        trace_limit: usize,
    ) -> Result<Vec<Span>> {
        delegate!(self, query_spans_for_trace_list, params: &QueryParams, trace_limit: usize)
    }
    async fn query_trace_summaries(
        &self,
        _params: &QueryParams,
        _trace_limit: usize,
    ) -> Result<Vec<otelite_core::api::TraceEntry>> {
        Err(query_unsupported())
    }
    async fn query_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>> {
        delegate!(self, query_metrics, params: &QueryParams)
    }
    async fn query_latest_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>> {
        delegate!(self, query_latest_metrics, params: &QueryParams)
    }
    async fn query_distinct_metric_names(&self) -> Result<Vec<String>> {
        delegate!(self, query_distinct_metric_names)
    }
    async fn stats(&self) -> Result<StorageStats> {
        delegate!(self, stats)
    }
    async fn purge(&self, options: &PurgeOptions) -> Result<u64> {
        delegate!(self, purge, options: &PurgeOptions)
    }
    async fn purge_all(&self) -> Result<PurgeAllStats> {
        delegate!(self, purge_all)
    }
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
    async fn distinct_resource_keys(&self, signal: &str) -> Result<Vec<String>> {
        delegate!(self, distinct_resource_keys, signal: &str)
    }
    async fn query_token_usage(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<(TokenUsageSummary, Vec<ModelUsage>, Vec<SystemUsage>)> {
        delegate!(
            self,
            query_token_usage,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_cost_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_ns: i64,
        filters: &GenAiFilters,
    ) -> Result<Vec<CostSeriesPoint>> {
        delegate!(
            self,
            query_cost_series,
            start_time: Option<i64>,
            end_time: Option<i64>,
            bucket_ns: i64,
            filters: &GenAiFilters
        )
    }
    async fn query_top_spans(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
        sort_by: TopSpanSort,
        truncated_only: bool,
    ) -> Result<Vec<TopSpan>> {
        delegate!(
            self,
            query_top_spans,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize,
            sort_by: TopSpanSort,
            truncated_only: bool
        )
    }
    async fn query_top_sessions(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<SessionCostRow>> {
        delegate!(
            self,
            query_top_sessions,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize
        )
    }
    async fn query_top_conversations(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<ConversationCostRow>> {
        delegate!(
            self,
            query_top_conversations,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize
        )
    }
    async fn query_finish_reasons(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<FinishReasonCount>> {
        delegate!(
            self,
            query_finish_reasons,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_latency_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<LatencyStats>> {
        delegate!(
            self,
            query_latency_stats,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_latency_percentiles(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        bucket_secs: u64,
        metrics: &[&str],
        timezone: Option<&str>,
    ) -> Result<LatencyPercentilesResponse> {
        delegate!(
            self,
            query_latency_percentiles,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            bucket_secs: u64,
            metrics: &[&str],
            timezone: Option<&str>
        )
    }
    async fn query_distribution(
        &self,
        metric: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        buckets: usize,
        scale: &str,
    ) -> Result<DistributionResponse> {
        delegate!(
            self,
            query_distribution,
            metric: &str,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            buckets: usize,
            scale: &str
        )
    }
    async fn query_session_context(
        &self,
        session_id: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: u64,
    ) -> Result<Option<SessionContextResponse>> {
        delegate!(
            self,
            query_session_context,
            session_id: &str,
            start_time: Option<i64>,
            end_time: Option<i64>,
            limit: u64
        )
    }
    async fn query_error_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ErrorRateByModel>> {
        delegate!(
            self,
            query_error_rate,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_tool_usage(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<ToolUsage>> {
        delegate!(
            self,
            query_tool_usage,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize
        )
    }
    async fn query_retry_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<RetryStats> {
        delegate!(
            self,
            query_retry_stats,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_retrieval_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        top_queries_limit: usize,
    ) -> Result<RetrievalStats> {
        delegate!(
            self,
            query_retrieval_stats,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            top_queries_limit: usize
        )
    }
    async fn query_truncation_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<TruncationRateByModel>> {
        delegate!(
            self,
            query_truncation_rate,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_cache_hit_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<CacheHitRateByModel>> {
        delegate!(
            self,
            query_cache_hit_rate,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_cache_economics(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_ns: i64,
    ) -> Result<CacheEconomicsResponse> {
        delegate!(
            self,
            query_cache_economics,
            start_time: Option<i64>,
            end_time: Option<i64>,
            bucket_ns: i64
        )
    }
    async fn query_reasoning_share(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<ReasoningShareResponse> {
        delegate!(
            self,
            query_reasoning_share,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_agent_rollup(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_secs: u64,
    ) -> Result<Vec<AgentRollupStorage>> {
        delegate!(
            self,
            query_agent_rollup,
            start_time: Option<i64>,
            end_time: Option<i64>,
            bucket_secs: u64
        )
    }
    async fn query_project_rollup(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<ProjectRollupStorage>> {
        delegate!(
            self,
            query_project_rollup,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_session_costs(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<SessionCostStorage>> {
        delegate!(
            self,
            query_session_costs,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_agent_roles(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<AgentRolesResponse> {
        delegate!(
            self,
            query_agent_roles,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_provider_mix(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<ProviderMixResponse> {
        delegate!(
            self,
            query_provider_mix,
            start_time: Option<i64>,
            end_time: Option<i64>
        )
    }
    async fn query_request_param_profile(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<RequestParamProfile> {
        delegate!(
            self,
            query_request_param_profile,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_conversation_depth(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<ConversationDepthStats> {
        delegate!(
            self,
            query_conversation_depth,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_latency_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_secs: u64,
        filters: &GenAiFilters,
        all_spans: bool,
        timezone: Option<&str>,
    ) -> Result<Vec<LatencySeriesPoint>> {
        delegate!(
            self,
            query_latency_series,
            start_time: Option<i64>,
            end_time: Option<i64>,
            bucket_secs: u64,
            filters: &GenAiFilters,
            all_spans: bool,
            timezone: Option<&str>
        )
    }
    async fn query_calls_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        bucket_secs: u64,
        all_spans: bool,
    ) -> Result<Vec<CallsSeriesPoint>> {
        delegate!(
            self,
            query_calls_series,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            bucket_secs: u64,
            all_spans: bool
        )
    }
    async fn query_latency_by_context(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<LatencyByContextBin>> {
        delegate!(
            self,
            query_latency_by_context,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_error_types(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ErrorTypeBreakdown>> {
        delegate!(
            self,
            query_error_types,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_model_drift(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ModelDriftPair>> {
        delegate!(
            self,
            query_model_drift,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_tool_approvals(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<ToolApprovalStats> {
        delegate!(
            self,
            query_tool_approvals,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_stop_reasons(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<StopReasonCount>> {
        delegate!(
            self,
            query_stop_reasons,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_context_type_split(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ContextTypeSplit>> {
        delegate!(
            self,
            query_context_type_split,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
    async fn query_tool_errors(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<ToolErrorEntry>> {
        delegate!(
            self,
            query_tool_errors,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters,
            limit: usize
        )
    }
    async fn query_hour_of_day(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<HourOfDayBucket>> {
        delegate!(
            self,
            query_hour_of_day,
            start_time: Option<i64>,
            end_time: Option<i64>,
            filters: &GenAiFilters
        )
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Start an HTTP receiver with the given storage, limit 2, on a free port.
async fn start_http(
    storage: Arc<dyn StorageBackend>,
    max_concurrent: usize,
) -> (String, HttpServer) {
    let config = ReceiverConfig::new()
        .with_http_addr("127.0.0.1:0".parse().expect("addr"))
        .with_max_concurrent_requests(max_concurrent);
    let server = HttpServer::new(config);
    server
        .start(storage)
        .await
        .expect("Failed to start HTTP receiver");
    sleep(Duration::from_millis(100)).await;
    let addr = server.local_addr().await.expect("bound address");
    (format!("http://{}", addr), server)
}

/// Start a gRPC receiver with the given storage, limit 2, on a free port.
async fn start_grpc(storage: Arc<dyn StorageBackend>, max_concurrent: usize) -> (u16, GrpcServer) {
    let config = ReceiverConfig::new()
        .with_grpc_addr("127.0.0.1:0".parse().expect("addr"))
        .with_max_concurrent_requests(max_concurrent);
    let server = GrpcServer::new(config, storage);
    server.start().await.expect("Failed to start gRPC receiver");
    sleep(Duration::from_millis(100)).await;
    let port = server.local_addr().await.expect("bound address").port();
    (port, server)
}

// ---------------------------------------------------------------------------
// HTTP: burst bounded, 503 over the limit, reads unaffected
// ---------------------------------------------------------------------------

#[tokio::test]
async fn http_burst_is_bounded_and_over_limit_gets_503() {
    let (storage, _dir) = SlowStorage::new(WRITE_DELAY).await;
    let (base_url, server) = start_http(storage.clone(), 2).await;
    let client = reqwest::Client::new();

    // 6 concurrent exports against a limit of 2: exactly 2 may be
    // in-flight; the other 4 must be rejected with 503 (retryable).
    let body = create_metrics_protobuf();
    let mut handles = Vec::new();
    for _ in 0..6 {
        let client = client.clone();
        let base_url = base_url.clone();
        let body = body.clone();
        handles.push(tokio::spawn(async move {
            client
                .post(format!("{base_url}/v1/metrics"))
                .header("Content-Type", "application/x-protobuf")
                .body(body.to_vec())
                .send()
                .await
                .expect("request sent")
                .status()
        }));
    }

    // While the 2 in-flight exports are sleeping, /health (outside the
    // limit) must still answer promptly.
    let health_probe = async {
        sleep(Duration::from_millis(80)).await;
        client
            .get(format!("{base_url}/health"))
            .send()
            .await
            .expect("health sent")
            .status()
    };
    let (statuses_raw, health) =
        tokio::join!(futures_util::future::join_all(handles), health_probe);
    let statuses: Vec<u16> = statuses_raw
        .into_iter()
        .map(|r| r.expect("burst request completed").as_u16())
        .collect();

    let ok = statuses.iter().filter(|s| **s == 200).count();
    let unavailable = statuses.iter().filter(|s| **s == 503).count();
    assert_eq!(ok, 2, "exactly `limit` exports may be in-flight");
    assert_eq!(
        unavailable, 4,
        "the excess must be rejected with 503, not queued or dropped silently"
    );
    assert_eq!(health, 200, "/health must stay reachable while saturated");

    // A storage read must complete promptly while exports are in flight —
    // the bounded backlog cannot stall reads (the read pool is separate,
    // and the in-flight writes are capped, not unbounded).
    let read = async {
        sleep(Duration::from_millis(80)).await;
        let start = std::time::Instant::now();
        storage.stats().await.expect("stats query");
        start.elapsed()
    };
    let elapsed = read.await;
    assert!(
        elapsed < Duration::from_secs(2),
        "read must not be stalled by saturated writes (took {elapsed:?})"
    );

    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

// ---------------------------------------------------------------------------
// gRPC: burst bounded, UNAVAILABLE over the limit
// ---------------------------------------------------------------------------

#[tokio::test]
async fn grpc_burst_is_bounded_and_over_limit_gets_unavailable() {
    let (storage, _dir) = SlowStorage::new(WRITE_DELAY).await;
    let (port, server) = start_grpc(storage, 2).await;

    let endpoint = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{port}"))
        .expect("endpoint")
        .connect_timeout(Duration::from_secs(5));
    let channel = endpoint.connect().await.expect("connect");
    let client = MetricsServiceClient::new(channel);

    // A NON-EMPTY request: the handler only reaches the (slow) write when
    // there is a metric to store, so an empty export would complete in
    // milliseconds and the burst would never overlap.
    let inner = ExportMetricsServiceRequest {
        resource_metrics: vec![
            opentelemetry_proto::tonic::metrics::v1::ResourceMetrics {
                resource: None,
                scope_metrics: vec![opentelemetry_proto::tonic::metrics::v1::ScopeMetrics {
                    scope: None,
                    metrics: vec![
                        opentelemetry_proto::tonic::metrics::v1::Metric {
                            name: "burst".to_string(),
                            description: String::new(),
                            unit: String::new(),
                            data: Some(
                                opentelemetry_proto::tonic::metrics::v1::metric::Data::Gauge(
                                    opentelemetry_proto::tonic::metrics::v1::Gauge {
                                        data_points: vec![
                                            opentelemetry_proto::tonic::metrics::v1::NumberDataPoint {
                                                attributes: vec![],
                                                start_time_unix_nano: 0,
                                                time_unix_nano: 1,
                                                value: Some(
                                                    opentelemetry_proto::tonic::metrics::v1::number_data_point::Value::AsInt(1),
                                                ),
                                                exemplars: vec![],
                                                flags: 0,
                                            },
                                        ],
                                    },
                                ),
                            ),
                            metadata: vec![],
                        },
                    ],
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            },
        ],
    };
    let mut handles = Vec::new();
    for _ in 0..6 {
        let mut client = client.clone();
        let inner = inner.clone(); // the message is Clone; tonic::Request is not
        handles.push(tokio::spawn(async move {
            match client.export(tonic::Request::new(inner)).await {
                Ok(_) => 0u16, // OK
                Err(status) => status.code() as u16,
            }
        }));
    }
    let outcomes_raw = futures_util::future::join_all(handles).await;
    let outcomes: Vec<u16> = outcomes_raw
        .into_iter()
        .map(|r| r.expect("burst export completed"))
        .collect();

    let ok = outcomes.iter().filter(|c| **c == 0).count();
    let unavailable = outcomes
        .iter()
        .filter(|c| **c == tonic::Code::Unavailable as u16)
        .count();
    assert_eq!(ok, 2, "exactly `limit` exports may be in-flight");
    assert_eq!(
        unavailable, 4,
        "the excess must be rejected with UNAVAILABLE (retryable)"
    );

    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

// ---------------------------------------------------------------------------
// /health: flips unhealthy on sustained persistent write failure
// ---------------------------------------------------------------------------

#[tokio::test]
async fn health_flips_unhealthy_on_sustained_persistent_write_failure() {
    use otelite_receiver::health::WRITE_FAILURE_THRESHOLD;

    // The double fails its first WRITE_FAILURE_THRESHOLD writes with a
    // persistent DiskFullError, then succeeds.
    let (storage, _dir) = FaultyStorage::new(WRITE_FAILURE_THRESHOLD as usize).await;
    let (base_url, server) = start_http(storage, 100).await;
    let client = reqwest::Client::new();
    let body = create_logs_protobuf();

    // Baseline: healthy (ready, no failures yet).
    let status = client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .expect("health sent")
        .status();
    assert_eq!(status, 200, "healthy before any write failure");

    // Three persistent failures, each surfacing as HTTP 500 to the
    // exporter.
    for i in 0..WRITE_FAILURE_THRESHOLD {
        let status = client
            .post(format!("{base_url}/v1/logs"))
            .header("Content-Type", "application/x-protobuf")
            .body(body.clone().to_vec())
            .send()
            .await
            .expect("export sent")
            .status();
        assert_eq!(
            status, 500,
            "persistent write failure {i} must surface as 500"
        );
    }

    // The sustained failure has flipped /health unhealthy — the receiver
    // is no longer ingesting, and it says so.
    let status = client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .expect("health sent")
        .status();
    assert_eq!(
        status, 503,
        "/health must be unhealthy after sustained persistent write failure"
    );

    // The next write succeeds (the fault window is over) and re-arms health.
    let status = client
        .post(format!("{base_url}/v1/logs"))
        .header("Content-Type", "application/x-protobuf")
        .body(body.to_vec())
        .send()
        .await
        .expect("export sent")
        .status();
    assert_eq!(status, 200, "write after the fault window must succeed");

    let status = client
        .get(format!("{base_url}/health"))
        .send()
        .await
        .expect("health sent")
        .status();
    assert_eq!(status, 200, "a success must re-arm /health to healthy");

    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

// ---------------------------------------------------------------------------
// Legacy /v1/otlp: explicit 400, no trial decode
// ---------------------------------------------------------------------------

#[tokio::test]
async fn legacy_unified_endpoint_is_explicit_400() {
    let (storage, _dir) = SlowStorage::new(Duration::from_millis(1)).await;
    let (base_url, server) = start_http(storage, 100).await;
    let client = reqwest::Client::new();

    // A perfectly valid traces protobuf — before #256 this would be
    // trial-decoded and stored; now it must be rejected with a 400 that
    // points at the per-signal endpoints (and NOT retried as a 5xx).
    let body = http_test_utils::create_traces_protobuf();
    let response = client
        .post(format!("{base_url}/v1/otlp"))
        .header("Content-Type", "application/x-protobuf")
        .body(body.to_vec())
        .send()
        .await
        .expect("request sent");
    assert_eq!(response.status(), 400);
    let text = response.text().await.expect("body");
    assert!(
        text.contains("/v1/traces") && text.contains("no longer supported"),
        "the 400 must point at the per-signal endpoints, got: {text}"
    );

    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}
