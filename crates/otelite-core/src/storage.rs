//! Storage abstraction layer for otelite.
//!
//! Defines the `StorageBackend` trait and all associated types so that
//! downstream crates (`otelite-receiver`, `otelite-api`) can depend only on
//! `otelite-core` rather than the concrete SQLite implementation.

use async_trait::async_trait;
use thiserror::Error;

use crate::api::{
    AgentRolesResponse, AgentRollupStorage, CacheEconomicsResponse, CacheHitRateByModel,
    CallsSeriesPoint, ConversationCostRow, ConversationDepthStats, CostSeriesPoint,
    DistributionResponse, ErrorRateByModel, ErrorTypeBreakdown, FinishReasonCount,
    GenAiCapabilityResponse, LatencyByContextBin, LatencyPercentilesResponse, LatencySeriesPoint,
    LatencyStats, ModelDriftPair, ModelUsage, ProjectRollupStorage, ProviderMixResponse,
    ReasoningShareResponse, RequestParamProfile, RetrievalStats, RetryStats,
    SessionContextResponse, SessionCostRow, SessionCostStorage, SystemUsage, TokenUsageSummary,
    ToolUsage, TopSpan, TopSpanSort, TruncationRateByModel,
};
use crate::filters::GenAiFilters;
// New types referenced via crate::api:: in the trait methods below.
use crate::query::QueryPredicate;
use crate::telemetry::log::SeverityLevel;
use crate::telemetry::{LogRecord, Metric, Span};

/// Result type for storage operations.
pub type Result<T> = std::result::Result<T, StorageError>;

/// Generic storage errors returned by `StorageBackend` implementations.
///
/// All variants carry string payloads so this type has no dependency on any
/// database library. Backend-specific error types should convert to these via
/// a `From` impl.
#[derive(Error, Debug)]
pub enum StorageError {
    /// Storage initialization failed.
    #[error("Failed to initialize storage: {0}")]
    InitializationError(String),

    /// Write operation failed.
    #[error("Failed to write data: {0}")]
    WriteError(String),

    /// Query operation failed.
    #[error("Failed to query data: {0}")]
    QueryError(String),

    /// Disk is full or insufficient space.
    #[error("Insufficient disk space: {0}")]
    DiskFullError(String),

    /// Storage corruption detected.
    #[error("Storage corruption detected: {0}")]
    CorruptionError(String),

    /// Permission denied.
    #[error("Permission denied: {0}")]
    PermissionError(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Purge operation failed.
    #[error("Purge operation failed: {0}")]
    PurgeError(String),

    /// Underlying database error (string representation).
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// I/O error (string representation).
    #[error("I/O error: {0}")]
    IoError(String),

    /// Serialization error (string representation).
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

impl StorageError {
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            StorageError::WriteError(_) | StorageError::QueryError(_) | StorageError::PurgeError(_)
        )
    }

    pub fn is_corruption(&self) -> bool {
        matches!(self, StorageError::CorruptionError(_))
    }

    pub fn is_disk_full(&self) -> bool {
        matches!(self, StorageError::DiskFullError(_))
    }
}

/// Statistics returned after a `purge_all` operation.
#[derive(Debug, Clone)]
pub struct PurgeAllStats {
    pub logs_deleted: u64,
    pub spans_deleted: u64,
    pub metrics_deleted: u64,
}

/// Statistics about stored telemetry data.
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub log_count: u64,
    pub span_count: u64,
    pub metric_count: u64,
    /// Oldest record timestamp (nanoseconds since Unix epoch).
    pub oldest_timestamp: Option<i64>,
    /// Newest record timestamp (nanoseconds since Unix epoch).
    pub newest_timestamp: Option<i64>,
    pub storage_size_bytes: u64,
}

/// Query parameters for filtering telemetry data.
#[derive(Debug, Clone, Default)]
pub struct QueryParams {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: Option<usize>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub min_severity: Option<SeverityLevel>,
    pub search_text: Option<String>,
    pub predicates: Vec<QueryPredicate>,
}

/// Options for manual data cleanup.
#[derive(Debug, Clone)]
pub struct PurgeOptions {
    /// Purge data older than this timestamp (nanoseconds since Unix epoch).
    pub older_than: Option<i64>,
    pub signal_types: Vec<SignalType>,
    pub dry_run: bool,
}

/// Signal type discriminator used in purge operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    Logs,
    Traces,
    Metrics,
}

/// Pluggable storage backend trait.
///
/// Both `otelite-receiver` (writes) and `otelite-api` (reads) depend only on
/// this trait; neither needs a direct dependency on the SQLite implementation.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn initialize(&mut self) -> Result<()>;
    async fn write_log(&self, log: &LogRecord) -> Result<()>;
    async fn write_span(&self, span: &Span) -> Result<()>;
    async fn write_metric(&self, metric: &Metric) -> Result<()>;

    /// Write a batch of logs atomically: either all records are stored or
    /// none are. A failure partway through the batch rolls the whole batch
    /// back, so an exporter retry of the rejected export cannot store
    /// duplicates of the records that already committed.
    ///
    /// The default implementation falls back to per-record writes;
    /// backends should override this with a single transaction.
    async fn write_log_batch(&self, logs: &[LogRecord]) -> Result<()> {
        for log in logs {
            self.write_log(log).await?;
        }
        Ok(())
    }

    /// Write a batch of spans atomically (all-or-nothing). See
    /// [`write_log_batch`](Self::write_log_batch) for the semantics.
    ///
    /// The default implementation falls back to per-record writes;
    /// backends should override this with a single transaction.
    async fn write_span_batch(&self, spans: &[Span]) -> Result<()> {
        for span in spans {
            self.write_span(span).await?;
        }
        Ok(())
    }

    /// Write a batch of metrics atomically (all-or-nothing). See
    /// [`write_log_batch`](Self::write_log_batch) for the semantics.
    ///
    /// The default implementation falls back to per-record writes;
    /// backends should override this with a single transaction.
    async fn write_metric_batch(&self, metrics: &[Metric]) -> Result<()> {
        for metric in metrics {
            self.write_metric(metric).await?;
        }
        Ok(())
    }
    async fn query_logs(&self, params: &QueryParams) -> Result<Vec<LogRecord>>;
    async fn query_spans(&self, params: &QueryParams) -> Result<Vec<Span>>;
    /// Query all spans for the N most-recent distinct traces matching the filters.
    async fn query_spans_for_trace_list(
        &self,
        params: &QueryParams,
        trace_limit: usize,
    ) -> Result<Vec<Span>>;
    /// Query metrics (raw time-series rows, latest first).
    async fn query_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>>;
    /// Query metrics returning the single most-recent data point per unique name.
    async fn query_latest_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>>;
    /// Distinct metric names, sorted ascending.
    async fn query_distinct_metric_names(&self) -> Result<Vec<String>>;
    async fn stats(&self) -> Result<StorageStats>;
    async fn purge(&self, options: &PurgeOptions) -> Result<u64>;
    async fn purge_all(&self) -> Result<PurgeAllStats>;
    async fn close(&mut self) -> Result<()>;
    /// Return distinct resource attribute keys for the given signal type.
    /// `signal` must be one of `"logs"`, `"spans"`, or `"metrics"`.
    async fn distinct_resource_keys(&self, signal: &str) -> Result<Vec<String>>;
    async fn query_token_usage(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<(TokenUsageSummary, Vec<ModelUsage>, Vec<SystemUsage>)>;

    /// Time-bucketed token usage grouped by model for cost-over-time analysis.
    ///
    /// `bucket_ns` is the bucket size in nanoseconds (e.g. 3_600_000_000_000 for 1h).
    async fn query_cost_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_ns: i64,
        filters: &GenAiFilters,
    ) -> Result<Vec<CostSeriesPoint>>;

    /// Top-N LLM spans ordered by the given sort dimension.
    ///
    /// When `truncated_only` is true, only spans whose finish reason is
    /// `max_tokens` or `length` are returned.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)] // filter bar (#135) pushed us past 5
    async fn query_top_spans(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
        sort_by: TopSpanSort,
        truncated_only: bool,
    ) -> Result<Vec<TopSpan>>;

    /// Top-N sessions by total tokens, suitable for cost enrichment.
    async fn query_top_sessions(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<SessionCostRow>>;

    /// Top-N conversations (gen_ai.conversation.id) by total tokens.
    async fn query_top_conversations(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<ConversationCostRow>>;

    /// Finish-reason distribution across LLM spans and Claude Code api_response_body logs.
    async fn query_finish_reasons(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<FinishReasonCount>>;

    /// Latency (and optional TTFT) percentile statistics per model for LLM spans.
    async fn query_latency_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<LatencyStats>>;

    /// Bucketed latency percentiles by model for the requested metrics
    /// ("duration", "ttft"). Rolling buckets (fixed `bucket_secs` grid from
    /// the epoch, non-empty only) when `timezone` is `None`; calendar-day
    /// buckets in the given IANA timezone (DST-aware, empty days included
    /// with null percentiles, explicit `start_time`/`end_time` required)
    /// otherwise (#119/#141).
    #[allow(clippy::too_many_arguments)] // filter bar (#135) pushed us past 5
    async fn query_latency_percentiles(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        bucket_secs: u64,
        metrics: &[&str],
        timezone: Option<&str>,
    ) -> Result<LatencyPercentilesResponse>;

    /// Generic distribution over a named metric cohort (issue #133):
    /// session_cost | tool_duration | llm_duration | ttft | output_tokens.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_arguments)] // filter bar (#135) pushed us past 5
    async fn query_distribution(
        &self,
        metric: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        buckets: usize,
        scale: &str,
    ) -> Result<DistributionResponse>;

    /// Session context (issue #134): spans, logs and aggregated metrics for
    /// one session id over the window. Spans/logs truncated to `limit`
    /// (counts in `*_total`); metrics aggregated per name. 404-able:
    /// `None` response when the session has no data in any store.
    async fn query_session_context(
        &self,
        session_id: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
        limit: u64,
    ) -> Result<Option<SessionContextResponse>>;

    /// Native GenAI telemetry capability coverage and quality.
    ///
    /// Backends without this optional analytic can retain source compatibility.
    async fn query_genai_capabilities(
        &self,
        _start_time: Option<i64>,
        _end_time: Option<i64>,
        _filters: &GenAiFilters,
    ) -> Result<GenAiCapabilityResponse> {
        Err(StorageError::QueryError(
            "GenAI capability reporting is not supported by this backend".to_string(),
        ))
    }

    /// Error rate by model across LLM spans.
    async fn query_error_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ErrorRateByModel>>;

    /// Aggregated tool-execution usage counts and durations.
    async fn query_tool_usage(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<ToolUsage>>;

    /// Retry statistics across LLM spans.
    async fn query_retry_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<RetryStats>;

    /// Aggregated retrieval / RAG statistics across retriever spans.
    async fn query_retrieval_stats(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        top_queries_limit: usize,
    ) -> Result<RetrievalStats>;

    /// Truncation rate (finish_reason = max_tokens / length) per model.
    async fn query_truncation_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<TruncationRateByModel>>;

    /// Cache token hit rate per model.
    async fn query_cache_hit_rate(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<CacheHitRateByModel>>;

    /// Cache economics (read/write split, hit rate, read:write ratio) per
    /// model and per time bucket, combining the three harness sources
    /// (opencode counters, codex turn histograms, claude llm_request spans).
    /// Savings fields are left unenriched (None / false) for the API layer.
    async fn query_cache_economics(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_ns: i64,
    ) -> Result<CacheEconomicsResponse>;

    /// Reasoning-token share per model plus a global per-effort breakdown,
    /// combining opencode `token.usage` counters (types `reasoning`/`output`)
    /// and codex `turn.token_usage` histograms. Claude Code is absent: its
    /// spans carry no thinking-token attributes. `cost_usd` is left
    /// unenriched (None) for the API layer.
    async fn query_reasoning_share(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<ReasoningShareResponse>;

    /// Per-harness rollup (opencode/codex/claude): sessions, per-model
    /// tokens, tool calls and retries over the window, with per-bucket
    /// series data. Harnesses with no activity in the window are omitted.
    /// Cost is unenriched (the API layer prices codex/claude from the
    /// per-model totals; opencode's own counter is in
    /// [`AgentRollupStorage::counter_cost_usd`]).
    async fn query_agent_rollup(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_secs: u64,
    ) -> Result<Vec<AgentRollupStorage>>;

    /// Per-project rollup over the window: opencode sessions attributed
    /// by their `project.id` label (token deltas, session counts, cost
    /// counter deltas), plus one `"unattributed"` row for codex/claude
    /// (no project label today) and label-less opencode sessions. Cost is
    /// unenriched — see [`ProjectRollupStorage`].
    async fn query_project_rollup(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<ProjectRollupStorage>>;

    /// Per-session costs (opencode + claude) over the window: last
    /// per-session value of opencode's cumulative `session.cost.total` /
    /// `session.duration` / `session.token.total` metrics, and per-session
    /// token sums from claude `claude_code.llm_request` span attributes.
    /// Codex is absent: its metrics carry no per-session identifier. Cost
    /// is unenriched (opencode's counter in
    /// [`SessionCostStorage::counter_cost_usd`], claude priced by the API
    /// layer from the per-model totals).
    async fn query_session_costs(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<SessionCostStorage>>;

    /// Sub-agent role attribution (cost and tokens per opencode `agent`
    /// label) over the time window.
    async fn query_agent_roles(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<AgentRolesResponse>;

    /// Provider × model mix (tokens, sessions, estimated cost share) over
    /// the time window, across opencode, codex and claude_code.
    async fn query_provider_mix(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<ProviderMixResponse>;

    /// Distribution of request parameter settings (temperature, max_tokens).
    async fn query_request_param_profile(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<RequestParamProfile>;

    /// Turn-count distribution across conversations with a known conversation_id.
    async fn query_conversation_depth(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<ConversationDepthStats>;

    /// Latency (min/avg/p95/max + TTFT) per time bucket grouped by model or span name.
    /// When `all_spans` is true the LLM guard is lifted and results are grouped by span name.
    #[allow(clippy::too_many_arguments)]
    async fn query_latency_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        bucket_secs: u64,
        filters: &GenAiFilters,
        all_spans: bool,
        timezone: Option<&str>,
    ) -> Result<Vec<LatencySeriesPoint>>;

    /// Call volume per time bucket grouped by model or span name.
    /// When `all_spans` is true the LLM guard is lifted and results are grouped by span name.
    #[allow(clippy::too_many_arguments)] // filter bar (#135) pushed us past 5
    async fn query_calls_series(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        bucket_secs: u64,
        all_spans: bool,
    ) -> Result<Vec<CallsSeriesPoint>>;

    /// LLM latency broken down by input-token context size bin × model.
    async fn query_latency_by_context(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<LatencyByContextBin>>;

    /// Per-(model, error_type) breakdown of error spans, bucketed into actionable categories.
    async fn query_error_types(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ErrorTypeBreakdown>>;

    /// All observed (request_model, response_model) pairs with a `differs` flag.
    async fn query_model_drift(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<ModelDriftPair>>;

    /// Approval/rejection summary for tool gating events.
    async fn query_tool_approvals(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<crate::api::ToolApprovalStats>;

    /// Distribution of stop_reason values across LLM spans.
    async fn query_stop_reasons(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<crate::api::StopReasonCount>>;

    /// Token usage broken down by llm_request.context type.
    async fn query_context_type_split(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<crate::api::ContextTypeSplit>>;

    /// Top error messages from failed tool executions.
    async fn query_tool_errors(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
        limit: usize,
    ) -> Result<Vec<crate::api::ToolErrorEntry>>;

    /// Hour-of-day activity buckets (UTC).
    async fn query_hour_of_day(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
        filters: &GenAiFilters,
    ) -> Result<Vec<crate::api::HourOfDayBucket>>;
}
