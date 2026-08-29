// Regression tests (#16): storage write failures must surface as proper
// responses — gRPC INTERNAL, HTTP 500 — not panics or silent drops.
//
// A FailingStorage test double wraps the StorageBackend trait with
// per-signal write-failure flags; everything else returns a clear error.
// All servers bind 127.0.0.1:0 (OS-assigned free port).

mod http_test_utils;

use async_trait::async_trait;
use http_test_utils::{create_logs_protobuf, create_metrics_protobuf, create_traces_protobuf};
use opentelemetry_proto::tonic::collector::logs::v1::logs_service_server::LogsService;
use opentelemetry_proto::tonic::collector::metrics::v1::metrics_service_server::MetricsService;
use opentelemetry_proto::tonic::collector::trace::v1::trace_service_server::TraceService;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
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
    PurgeAllStats, PurgeOptions, QueryParams, Result, StorageBackend, StorageError, StorageStats,
};
use otelite_core::telemetry::{LogRecord, Metric, Span};
use otelite_receiver::config::ReceiverConfig;
use otelite_receiver::grpc::{
    logs::LogsServiceImpl, metrics::MetricsServiceImpl, traces::TraceServiceImpl,
};
use otelite_receiver::http::HttpServer;
use otelite_receiver::signals::{LogsHandler, MetricsHandler, TracesHandler};
use std::sync::Arc;
use std::time::Duration;
use tonic::Request;

/// Storage test double: write methods fail per signal flag, everything
/// else returns a clear "not supported" query error (these tests only
/// exercise the write-failure paths).
struct FailingStorage {
    fail_logs: bool,
    fail_spans: bool,
    fail_metrics: bool,
}

impl FailingStorage {
    fn write_error(&self, what: &str) -> StorageError {
        StorageError::WriteError(format!("{what}: simulated storage failure"))
    }
}

fn query_not_supported() -> StorageError {
    StorageError::QueryError(
        "FailingStorage: query methods are not supported by this test double".to_string(),
    )
}

#[async_trait]
impl StorageBackend for FailingStorage {
    async fn initialize(&mut self) -> Result<()> {
        Ok(())
    }

    async fn write_log(&self, _log: &LogRecord) -> Result<()> {
        if self.fail_logs {
            Err(self.write_error("write_log"))
        } else {
            Ok(())
        }
    }

    async fn write_span(&self, _span: &Span) -> Result<()> {
        if self.fail_spans {
            Err(self.write_error("write_span"))
        } else {
            Ok(())
        }
    }

    async fn write_metric(&self, _metric: &Metric) -> Result<()> {
        if self.fail_metrics {
            Err(self.write_error("write_metric"))
        } else {
            Ok(())
        }
    }

    async fn query_logs(&self, _params: &QueryParams) -> Result<Vec<LogRecord>> {
        Err(query_not_supported())
    }
    async fn query_spans(&self, _params: &QueryParams) -> Result<Vec<Span>> {
        Err(query_not_supported())
    }
    async fn query_spans_for_trace_list(
        &self,
        _params: &QueryParams,
        _trace_limit: usize,
    ) -> Result<Vec<Span>> {
        Err(query_not_supported())
    }
    async fn query_metrics(&self, _params: &QueryParams) -> Result<Vec<Metric>> {
        Err(query_not_supported())
    }
    async fn query_latest_metrics(&self, _params: &QueryParams) -> Result<Vec<Metric>> {
        Err(query_not_supported())
    }
    async fn query_distinct_metric_names(&self) -> Result<Vec<String>> {
        Err(query_not_supported())
    }
    async fn stats(&self) -> Result<StorageStats> {
        Err(query_not_supported())
    }
    async fn purge(&self, _options: &PurgeOptions) -> Result<u64> {
        Err(query_not_supported())
    }
    async fn purge_all(&self) -> Result<PurgeAllStats> {
        Err(query_not_supported())
    }
    async fn close(&mut self) -> Result<()> {
        Ok(())
    }
    async fn distinct_resource_keys(&self, _signal: &str) -> Result<Vec<String>> {
        Err(query_not_supported())
    }
    async fn query_token_usage(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<(TokenUsageSummary, Vec<ModelUsage>, Vec<SystemUsage>)> {
        Err(query_not_supported())
    }
    async fn query_cost_series(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _bucket_ns: i64,
        _filters: &GenAiFilters,
    ) -> Result<Vec<CostSeriesPoint>> {
        Err(query_not_supported())
    }
    async fn query_top_spans(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
        _limit: usize,
        _sort_by: TopSpanSort,
        _truncated_only: bool,
    ) -> Result<Vec<TopSpan>> {
        Err(query_not_supported())
    }
    async fn query_top_sessions(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
        _limit: usize,
    ) -> Result<Vec<SessionCostRow>> {
        Err(query_not_supported())
    }
    async fn query_top_conversations(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
        _limit: usize,
    ) -> Result<Vec<ConversationCostRow>> {
        Err(query_not_supported())
    }
    async fn query_finish_reasons(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<FinishReasonCount>> {
        Err(query_not_supported())
    }
    async fn query_latency_stats(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<LatencyStats>> {
        Err(query_not_supported())
    }
    async fn query_latency_percentiles(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
        _bucket_secs: u64,
        _metrics: &[&str],
        _timezone: Option<&str>,
    ) -> Result<LatencyPercentilesResponse> {
        Err(query_not_supported())
    }
    async fn query_distribution(
        &self,
        _metric: &str,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
        _buckets: usize,
        _scale: &str,
    ) -> Result<DistributionResponse> {
        Err(query_not_supported())
    }
    async fn query_session_context(
        &self,
        _session_id: &str,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _limit: u64,
    ) -> Result<Option<SessionContextResponse>> {
        Err(query_not_supported())
    }
    async fn query_error_rate(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<ErrorRateByModel>> {
        Err(query_not_supported())
    }
    async fn query_tool_usage(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
        _limit: usize,
    ) -> Result<Vec<ToolUsage>> {
        Err(query_not_supported())
    }
    async fn query_retry_stats(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<RetryStats> {
        Err(query_not_supported())
    }
    async fn query_retrieval_stats(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
        _top_queries_limit: usize,
    ) -> Result<RetrievalStats> {
        Err(query_not_supported())
    }
    async fn query_truncation_rate(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<TruncationRateByModel>> {
        Err(query_not_supported())
    }
    async fn query_cache_hit_rate(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<CacheHitRateByModel>> {
        Err(query_not_supported())
    }
    async fn query_cache_economics(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _bucket_ns: i64,
    ) -> Result<CacheEconomicsResponse> {
        Err(query_not_supported())
    }
    async fn query_reasoning_share(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
    ) -> Result<ReasoningShareResponse> {
        Err(query_not_supported())
    }
    async fn query_agent_rollup(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _bucket_secs: u64,
    ) -> Result<Vec<AgentRollupStorage>> {
        Err(query_not_supported())
    }
    async fn query_project_rollup(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
    ) -> Result<Vec<ProjectRollupStorage>> {
        Err(query_not_supported())
    }
    async fn query_session_costs(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
    ) -> Result<Vec<SessionCostStorage>> {
        Err(query_not_supported())
    }
    async fn query_agent_roles(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
    ) -> Result<AgentRolesResponse> {
        Err(query_not_supported())
    }
    async fn query_provider_mix(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
    ) -> Result<ProviderMixResponse> {
        Err(query_not_supported())
    }
    async fn query_request_param_profile(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<RequestParamProfile> {
        Err(query_not_supported())
    }
    async fn query_conversation_depth(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<ConversationDepthStats> {
        Err(query_not_supported())
    }
    async fn query_latency_series(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _bucket_secs: u64,
        _filters: &GenAiFilters,
        _all_spans: bool,
        _timezone: Option<&str>,
    ) -> Result<Vec<LatencySeriesPoint>> {
        Err(query_not_supported())
    }
    async fn query_calls_series(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
        _bucket_secs: u64,
        _all_spans: bool,
    ) -> Result<Vec<CallsSeriesPoint>> {
        Err(query_not_supported())
    }
    async fn query_latency_by_context(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<LatencyByContextBin>> {
        Err(query_not_supported())
    }
    async fn query_error_types(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<ErrorTypeBreakdown>> {
        Err(query_not_supported())
    }
    async fn query_model_drift(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<ModelDriftPair>> {
        Err(query_not_supported())
    }
    async fn query_tool_approvals(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<ToolApprovalStats> {
        Err(query_not_supported())
    }
    async fn query_stop_reasons(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<StopReasonCount>> {
        Err(query_not_supported())
    }
    async fn query_context_type_split(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<ContextTypeSplit>> {
        Err(query_not_supported())
    }
    async fn query_tool_errors(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
        _limit: usize,
    ) -> Result<Vec<ToolErrorEntry>> {
        Err(query_not_supported())
    }
    async fn query_hour_of_day(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<Vec<HourOfDayBucket>> {
        Err(query_not_supported())
    }
}

// ---------------------------------------------------------------------------
// gRPC transport: write failures must map to INTERNAL status, not panics
// ---------------------------------------------------------------------------

fn storage(fail_logs: bool, fail_spans: bool, fail_metrics: bool) -> Arc<dyn StorageBackend> {
    Arc::new(FailingStorage {
        fail_logs,
        fail_spans,
        fail_metrics,
    })
}

#[tokio::test]
async fn test_grpc_traces_storage_failure_returns_internal() {
    let handler = Arc::new(TracesHandler::new(storage(false, true, false)));
    let service = TraceServiceImpl::new(handler);

    let request = decode_protobuf_traces();
    let result = service.export(Request::new(request)).await;

    assert!(
        result.is_err(),
        "span write failure must surface as an error"
    );
    let status = result.unwrap_err();
    assert_eq!(
        status.code(),
        tonic::Code::Internal,
        "expected INTERNAL, got {}",
        status.code()
    );
}

#[tokio::test]
async fn test_grpc_logs_storage_failure_returns_internal() {
    let handler = Arc::new(LogsHandler::new(storage(true, false, false)));
    let service = LogsServiceImpl::new(handler);

    let request =
        http_test_utils::decode_logs_protobuf(&create_logs_protobuf()).expect("decode sample logs");
    let result = service.export(Request::new(request)).await;

    assert!(
        result.is_err(),
        "log write failure must surface as an error"
    );
    let status = result.unwrap_err();
    assert_eq!(
        status.code(),
        tonic::Code::Internal,
        "expected INTERNAL, got {}",
        status.code()
    );
}

#[tokio::test]
async fn test_grpc_metrics_storage_failure_returns_internal() {
    let handler = Arc::new(MetricsHandler::new(storage(false, false, true)));
    let service = MetricsServiceImpl::new(handler);

    let request = http_test_utils::decode_metrics_protobuf(&create_metrics_protobuf())
        .expect("decode sample metrics");
    let result = service.export(Request::new(request)).await;

    assert!(
        result.is_err(),
        "metric write failure must surface as an error"
    );
    let status = result.unwrap_err();
    assert_eq!(
        status.code(),
        tonic::Code::Internal,
        "expected INTERNAL, got {}",
        status.code()
    );
}

fn decode_protobuf_traces() -> ExportTraceServiceRequest {
    http_test_utils::decode_traces_protobuf(&create_traces_protobuf())
        .expect("decode sample traces")
}

// ---------------------------------------------------------------------------
// HTTP transport: write failures must map to 500, not panics or 2xx
// ---------------------------------------------------------------------------

async fn start_http_server(storage: Arc<dyn StorageBackend>) -> (String, HttpServer) {
    let config = ReceiverConfig::new().with_http_addr("127.0.0.1:0".parse().unwrap());
    let server = HttpServer::new(config);
    server
        .start(storage)
        .await
        .expect("start server with failing storage");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let addr = server.local_addr().await.expect("bound address");
    (format!("http://{addr}"), server)
}

#[tokio::test]
async fn test_http_logs_storage_failure_returns_500() {
    let (base, server) = start_http_server(storage(true, false, false)).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/v1/logs"))
        .header("Content-Type", "application/x-protobuf")
        .body(create_logs_protobuf())
        .send()
        .await
        .expect("send");

    assert_eq!(
        response.status(),
        500,
        "log write failure must return 500, got {}",
        response.status()
    );

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_http_traces_storage_failure_returns_500() {
    let (base, server) = start_http_server(storage(false, true, false)).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/v1/traces"))
        .header("Content-Type", "application/x-protobuf")
        .body(create_traces_protobuf())
        .send()
        .await
        .expect("send");

    assert_eq!(
        response.status(),
        500,
        "span write failure must return 500, got {}",
        response.status()
    );

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_http_metrics_storage_failure_returns_500() {
    let (base, server) = start_http_server(storage(false, false, true)).await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/v1/metrics"))
        .header("Content-Type", "application/x-protobuf")
        .body(create_metrics_protobuf())
        .send()
        .await
        .expect("send");

    assert_eq!(
        response.status(),
        500,
        "metric write failure must return 500, got {}",
        response.status()
    );

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

// ---------------------------------------------------------------------------
// Control: with no failure flags set, the same exports succeed — proving
// the failures above come from the storage double, not the test wiring.
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_grpc_exports_succeed_when_storage_is_healthy() {
    let store = storage(false, false, false);

    let traces = TraceServiceImpl::new(Arc::new(TracesHandler::new(store.clone())));
    let logs = LogsServiceImpl::new(Arc::new(LogsHandler::new(store.clone())));
    let metrics = MetricsServiceImpl::new(Arc::new(MetricsHandler::new(store)));

    assert!(traces
        .export(Request::new(decode_protobuf_traces()))
        .await
        .is_ok());
    assert!(logs
        .export(Request::new(
            http_test_utils::decode_logs_protobuf(&create_logs_protobuf()).unwrap()
        ))
        .await
        .is_ok());
    assert!(metrics
        .export(Request::new(
            http_test_utils::decode_metrics_protobuf(&create_metrics_protobuf()).unwrap()
        ))
        .await
        .is_ok());
}

#[tokio::test]
async fn test_http_exports_succeed_when_storage_is_healthy() {
    let (base, server) = start_http_server(storage(false, false, false)).await;
    let client = reqwest::Client::new();

    for (path, body) in [
        ("/v1/logs", create_logs_protobuf()),
        ("/v1/traces", create_traces_protobuf()),
        ("/v1/metrics", create_metrics_protobuf()),
    ] {
        let response = client
            .post(format!("{base}{path}"))
            .header("Content-Type", "application/x-protobuf")
            .body(body)
            .send()
            .await
            .expect("send");
        assert_eq!(
            response.status(),
            200,
            "healthy export to {path} must return 200, got {}",
            response.status()
        );
    }

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}
