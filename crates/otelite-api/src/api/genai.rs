//! GenAI/LLM token usage API endpoints

use crate::server::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use otelite_core::api::{
    AgentRolesResponse, AgentRollup, AgentRollupResponse, ConversationCostRow,
    ConversationDepthStats, CostSeriesPoint, ErrorResponse, GenAiCapabilityResponse,
    LatencyPercentilesResponse, ProjectRollupResponse, ProviderMixResponse, ReasoningShareResponse,
    RequestParamProfile, RetrievalStats, RetryStats, SessionCostRow, TokenUsageResponse,
    ToolApprovalStats, TopSpan, TopSpanSort,
};
use otelite_core::filters::{GenAiFilters, FILTER_DIMENSIONS};
use otelite_core::pricing::{PricingDatabase, TokenUsage};
use serde::{Deserialize, Serialize};

/// Every GenAI endpoint accepts the five filter-bar dimensions as query
/// params (#135); each endpoint applies the subset it genuinely supports
/// and echoes that set back as `filters_applied`. Unsupported params are
/// ignored — never a 400.
/// Build a `GenAiFilters` from a query struct carrying the five filter
/// fields.
macro_rules! genai_filter_impl {
    ($t:ident) => {
        impl $t {
            /// Build the effective filter set from this query (borrowing —
            /// the handler still needs the raw query fields).
            fn filters(&self) -> GenAiFilters {
                GenAiFilters {
                    agent: self.agent.clone(),
                    model: self.model.clone(),
                    // The filter bar sends a single exact model; the
                    // repeatable `models` patterns are CLI-only for now.
                    models: None,
                    provider: self.provider.clone(),
                    project: self.project.clone(),
                    session: self.session.clone(),
                }
            }
        }
    };
}

/// Wrapper for array-shaped GenAI responses so every endpoint can echo the
/// filter dimensions it actually applied (`filters_applied`).
#[derive(Debug, Serialize, utoipa::ToSchema)]
pub struct GenAiItemsResponse {
    /// Response items (the same rows the endpoint returned before #135)
    pub items: serde_json::Value,
    /// Filter dimensions this endpoint actually applied
    pub filters_applied: Vec<String>,
}

/// Enrich a batch of TopSpan rows with computed cost fields.
fn enrich_top_spans(rows: &mut [TopSpan], db: &PricingDatabase) {
    for row in rows {
        let usage = TokenUsage {
            input: row.input_tokens,
            output: row.output_tokens,
            cache_creation: row.cache_creation_tokens,
            cache_read: row.cache_read_tokens,
        };
        let result = db.compute_cost(row.model.as_deref(), usage, row.system.as_deref());
        row.cost = result.cost;
        row.cost_source = Some(result.source.as_str().to_string());
        row.cost_reason = result.reason;
        let duration_ms = row.duration / 1_000_000;
        if row.output_tokens > 0 && duration_ms > 0 {
            row.derived_output_tokens_per_sec =
                Some(row.output_tokens as f64 / (duration_ms as f64 / 1000.0));
        }
    }
}

/// Enrich cost-series bucket rows. Cost is computed per-bucket using the
/// bucket's aggregate token counts and the model that dominates the bucket.
/// Provider isn't carried at the bucket level so we pass `None` for system —
/// the fallback table matches on model name alone.
fn enrich_cost_series(rows: &mut [CostSeriesPoint], db: &PricingDatabase) {
    for row in rows {
        let usage = TokenUsage {
            input: row.input_tokens,
            output: row.output_tokens,
            cache_creation: row.cache_creation_tokens,
            cache_read: row.cache_read_tokens,
        };
        let result = db.compute_cost(row.model.as_deref(), usage, None);
        row.cost = result.cost;
        row.cost_source = Some(result.source.as_str().to_string());
    }
}

/// Query parameters for token usage endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct TokenUsageQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Get token usage statistics for GenAI/LLM spans
///
/// Returns aggregated token usage grouped by model and system (provider).
/// Only includes spans with `gen_ai.system` attribute.
#[utoipa::path(
    get,
    path = "/api/genai/usage",
    params(TokenUsageQuery),
    responses(
        (status = 200, description = "Token usage summary", body = TokenUsageResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_token_usage(
    State(state): State<AppState>,
    Query(query): Query<TokenUsageQuery>,
) -> Result<Json<TokenUsageResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let (summary, by_model, by_system) = state
        .storage
        .query_token_usage(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query token usage: {}",
                    e
                ))),
            )
        })?;

    Ok(Json(TokenUsageResponse {
        summary,
        by_model,
        by_system,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Query parameters for cost-over-time endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct CostSeriesQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Bucket size in seconds (defaults to 3600 = 1 hour)
    pub bucket: Option<i64>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Get time-bucketed token usage (cost-over-time)
///
/// Aggregates input/output/cache tokens and request counts into fixed-size time buckets
/// grouped by model. Use for charting cost trends.
#[utoipa::path(
    get,
    path = "/api/genai/cost_series",
    params(CostSeriesQuery),
    responses(
        (status = 200, description = "Cost series points", body = GenAiItemsResponse),
        (status = 400, description = "Invalid bucket parameter", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_cost_series(
    State(state): State<AppState>,
    Query(query): Query<CostSeriesQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let bucket_seconds = query.bucket.unwrap_or(3600);
    if bucket_seconds <= 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request(
                "bucket must be a positive number of seconds",
            )),
        ));
    }
    let bucket_ns = bucket_seconds.saturating_mul(1_000_000_000);
    let filters = query.filters();

    let mut series = state
        .storage
        .query_cost_series(query.start_time, query.end_time, bucket_ns, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query cost series: {}",
                    e
                ))),
            )
        })?;

    let pricing = state.pricing.snapshot().await;
    enrich_cost_series(&mut series, &pricing.db);

    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&series).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize cost series: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Query parameters for top-spans endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct TopSpansQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Maximum number of spans to return (default 20, capped at 100)
    pub limit: Option<usize>,
    /// Sort dimension: total_tokens (default), duration, output_input_ratio
    /// (output divided by all input context), cache_efficiency
    #[serde(default)]
    pub sort_by: TopSpanSort,
    /// When true, return only spans with finish_reason max_tokens or length
    #[serde(default)]
    pub truncated_only: bool,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Query parameters for top-sessions endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct TopGroupQuery {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: Option<usize>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Get the top-N LLM spans by the requested sort dimension
#[utoipa::path(
    get,
    path = "/api/genai/top_spans",
    params(TopSpansQuery),
    responses(
        (status = 200, description = "Top spans", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_top_spans(
    State(state): State<AppState>,
    Query(query): Query<TopSpansQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let filters = query.filters();

    let mut spans = state
        .storage
        .query_top_spans(
            query.start_time,
            query.end_time,
            &filters,
            limit,
            query.sort_by,
            query.truncated_only,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query top spans: {}",
                    e
                ))),
            )
        })?;

    let pricing = state.pricing.snapshot().await;
    enrich_top_spans(&mut spans, &pricing.db);

    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&spans).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize top spans: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

fn enrich_session_rows(rows: &mut [SessionCostRow], db: &PricingDatabase) {
    for row in rows {
        let usage = TokenUsage {
            input: row.input_tokens,
            output: row.output_tokens,
            ..Default::default()
        };
        let result = db.compute_cost(None, usage, None);
        row.cost = result.cost;
        row.cost_source = Some(result.source.as_str().to_string());
    }
}

fn enrich_conversation_rows(rows: &mut [ConversationCostRow], db: &PricingDatabase) {
    for row in rows {
        let usage = TokenUsage {
            input: row.input_tokens,
            output: row.output_tokens,
            ..Default::default()
        };
        let result = db.compute_cost(None, usage, None);
        row.cost = result.cost;
        row.cost_source = Some(result.source.as_str().to_string());
    }
}

/// Get the top-N sessions by total token usage
#[utoipa::path(
    get,
    path = "/api/genai/top_sessions",
    params(TopGroupQuery),
    responses(
        (status = 200, description = "Top sessions", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_top_sessions(
    State(state): State<AppState>,
    Query(query): Query<TopGroupQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let filters = query.filters();

    let mut rows = state
        .storage
        .query_top_sessions(query.start_time, query.end_time, &filters, limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query top sessions: {}",
                    e
                ))),
            )
        })?;

    let pricing = state.pricing.snapshot().await;
    enrich_session_rows(&mut rows, &pricing.db);

    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize top sessions: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Get the top-N conversations (gen_ai.conversation.id) by total token usage
#[utoipa::path(
    get,
    path = "/api/genai/top_conversations",
    params(TopGroupQuery),
    responses(
        (status = 200, description = "Top conversations", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_top_conversations(
    State(state): State<AppState>,
    Query(query): Query<TopGroupQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let filters = query.filters();

    let mut rows = state
        .storage
        .query_top_conversations(query.start_time, query.end_time, &filters, limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query top conversations: {}",
                    e
                ))),
            )
        })?;

    let pricing = state.pricing.snapshot().await;
    enrich_conversation_rows(&mut rows, &pricing.db);

    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize top conversations: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Query parameters for finish-reason distribution endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct FinishReasonsQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Get the distribution of finish / stop reasons across LLM spans
///
/// Combines OTel plural `gen_ai.response.finish_reasons`, singular `gen_ai.response.finish_reason`,
/// and Claude Code `stop_reason` values from `claude_code.api_response_body` log bodies.
#[utoipa::path(
    get,
    path = "/api/genai/finish_reasons",
    params(FinishReasonsQuery),
    responses(
        (status = 200, description = "Finish reason counts", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_finish_reasons(
    State(state): State<AppState>,
    Query(query): Query<FinishReasonsQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_finish_reasons(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query finish reasons: {}",
                    e
                ))),
            )
        })?;

    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize finish_reasons: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Query parameters for latency endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct LatencyQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Query parameters for the latency percentile endpoint.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct LatencyPercentileQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Bucket size in seconds (default 3600 = 1 hour). Ignored in
    /// calendar-day mode.
    pub bucket_secs: Option<u64>,
    /// Comma-separated metrics: "duration,ttft" (default: both).
    pub metrics: Option<String>,
    /// Calendar-day bucketing: `1` (or `true`) buckets by local day in
    /// `timezone` instead of the fixed `bucket_secs` grid. DST days are
    /// 23 or 25 hours; empty days are present with null percentiles; calls
    /// are attributed by start time. Requires explicit `start_time` and
    /// `end_time` (#119/#141).
    pub calendar_day: Option<String>,
    /// IANA timezone for `calendar_day=1` (e.g. `Europe/London`,
    /// `America/New_York`). Defaults to UTC. Ignored (and rejected)
    /// without `calendar_day=1`.
    pub timezone: Option<String>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Get latency / TTFT percentile statistics per model for LLM spans.
#[utoipa::path(
    get,
    path = "/api/genai/latency_stats",
    params(LatencyQuery),
    responses(
        (status = 200, description = "Latency statistics per model", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_latency_stats(
    State(state): State<AppState>,
    Query(query): Query<LatencyQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_latency_stats(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query latency stats: {}",
                    e
                ))),
            )
        })?;

    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize latency_stats: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Bucketed p50/p90/p95/p99 latency percentiles by model (issue #132).
/// Duration comes from LLM request spans (all harnesses); TTFT from the
/// spans' normalized TTFT attribute plus the codex turn TTFT histogram
/// (disjoint cohort — codex request spans carry no TTFT attribute).
#[utoipa::path(
    get,
    path = "/api/genai/latency_percentiles",
    params(LatencyPercentileQuery),
    responses(
        (status = 200, description = "Percentile series per metric and model", body = LatencyPercentilesResponse),
        (status = 400, description = "Invalid bucket_secs or metrics", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_latency_percentiles(
    State(state): State<AppState>,
    Query(query): Query<LatencyPercentileQuery>,
) -> Result<Json<LatencyPercentilesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let bucket_secs = query.bucket_secs.unwrap_or(3600);
    if bucket_secs == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request(
                "bucket_secs must be a positive number of seconds",
            )),
        ));
    }
    let metrics: Vec<&str> = query
        .metrics
        .as_deref()
        .unwrap_or("duration,ttft")
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();
    for m in &metrics {
        if *m != "duration" && *m != "ttft" {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::bad_request(format!(
                    "unknown metric '{m}' — expected \"duration\" and/or \"ttft\""
                ))),
            ));
        }
    }
    if metrics.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request(
                "metrics must include \"duration\" and/or \"ttft\"",
            )),
        ));
    }

    // Calendar-day mode (#141): explicit, never a silent switch. The
    // timezone must validate as an IANA zone; the window must be explicit.
    let calendar_day = match query.calendar_day.as_deref() {
        None => false,
        Some("1" | "true" | "yes") => true,
        Some("0" | "false" | "no") => false,
        Some(v) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::bad_request(format!(
                    "calendar_day must be 1/0, true/false or yes/no, got '{v}'"
                ))),
            ))
        },
    };
    let timezone: Option<String> = match (query.timezone.as_deref(), calendar_day) {
        (Some(tz), true) => {
            let tz = tz.trim();
            let _parsed: chrono_tz::Tz = std::str::FromStr::from_str(tz).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(ErrorResponse::bad_request(format!(
                        "unknown IANA timezone '{tz}': {e} (e.g. Europe/London, America/New_York, UTC)"
                    ))),
                )
            })?;
            Some(tz.to_string())
        },
        (None, true) => Some("UTC".to_string()),
        (Some(tz), false) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::bad_request(format!(
                    "timezone '{tz}' is only used with calendar_day=1"
                ))),
            ))
        },
        (None, false) => None,
    };
    if calendar_day && (query.start_time.is_none() || query.end_time.is_none()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request(
                "calendar_day=1 requires explicit start_time and end_time",
            )),
        ));
    }
    let timezone = timezone.as_deref();

    let mut rows = state
        .storage
        .query_latency_percentiles(
            query.start_time,
            query.end_time,
            &filters,
            bucket_secs,
            &metrics,
            timezone,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query latency percentiles: {}",
                    e
                ))),
            )
        })?;

    rows.filters_applied = filters.applied(&FILTER_DIMENSIONS);

    Ok(Json(rows))
}

/// Query parameters for the generic distribution endpoint.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct DistributionQuery {
    /// Metric cohort: session_cost | tool_duration | llm_duration | ttft | output_tokens.
    pub metric: String,
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Number of buckets (default 20, cap 100).
    pub buckets: Option<usize>,
    /// Binning scale: linear | log (default linear).
    pub scale: Option<String>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Generic distribution over a named metric cohort (issue #133). One
/// endpoint serving chart, table and CLI from the same JSON.
/// `session_cost` is resolved here (claude sessions are priced from tokens);
/// the span/metric cohorts are resolved in the storage layer.
#[utoipa::path(
    get,
    path = "/api/genai/distributions",
    params(DistributionQuery),
    responses(
        (status = 200, description = "Distribution buckets + summary stats", body = otelite_core::api::DistributionResponse),
        (status = 400, description = "Invalid metric, scale or buckets", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_distributions(
    State(state): State<AppState>,
    Query(query): Query<DistributionQuery>,
) -> Result<Json<otelite_core::api::DistributionResponse>, (StatusCode, Json<ErrorResponse>)> {
    use otelite_core::distribution;
    use otelite_core::session_cost;

    const KNOWN: &[&str] = &[
        "session_cost",
        "tool_duration",
        "llm_duration",
        "ttft",
        "output_tokens",
    ];
    if !KNOWN.contains(&query.metric.as_str()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request(format!(
                "unknown metric '{}' — expected one of: {}",
                query.metric,
                KNOWN.join(" | ")
            ))),
        ));
    }
    let scale = query.scale.as_deref().unwrap_or("linear");
    if scale != "linear" && scale != "log" {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request(format!(
                "unknown scale '{scale}' — expected \"linear\" or \"log\""
            ))),
        ));
    }
    let buckets = query.buckets.unwrap_or(20);
    if buckets == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request(
                "buckets must be a positive integer",
            )),
        ));
    }

    let filters = query.filters();

    let resp = if query.metric == "session_cost" {
        // Same source as the sessions cost panel (#126): opencode's own cost
        // counter ("actual") plus claude sessions priced from tokens.
        let rows = state
            .storage
            .query_session_costs(query.start_time, query.end_time)
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::storage_error(format!(
                        "query session costs: {e}"
                    ))),
                )
            })?;
        let pricing = state.pricing.snapshot().await;
        let sessions = session_cost::build_session_costs(rows, &pricing.db);
        let values: Vec<f64> = sessions.iter().filter_map(|s| s.cost_usd).collect();
        distribution::build("session_cost", "usd", scale, buckets, values)
    } else {
        state
            .storage
            .query_distribution(
                &query.metric,
                query.start_time,
                query.end_time,
                &filters,
                buckets,
                scale,
            )
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::storage_error(format!(
                        "query distribution: {e}"
                    ))),
                )
            })?
    };

    Ok(Json(resp))
}

/// Get native GenAI telemetry capability coverage and provenance.
#[utoipa::path(
    get,
    path = "/api/genai/capabilities",
    params(ModelAnalyticsQuery),
    responses(
        (status = 200, description = "GenAI telemetry capability report", body = GenAiCapabilityResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_genai_capabilities(
    State(state): State<AppState>,
    Query(query): Query<ModelAnalyticsQuery>,
) -> Result<Json<GenAiCapabilityResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let mut report = state
        .storage
        .query_genai_capabilities(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query GenAI capabilities: {e}"
                ))),
            )
        })?;
    report.filters_applied = filters.applied(&FILTER_DIMENSIONS);

    Ok(Json(report))
}

/// Query parameters for the model-performance diagnosis endpoint
/// (#121/#153). The current interval is explicit and required; the rolling
/// baseline is a length, placed immediately before the derived preceding
/// window so it can never overlap either comparison window.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ModelPerformanceQueryParams {
    /// Start of the current interval (nanoseconds since Unix epoch)
    pub start_time: i64,
    /// End of the current interval (nanoseconds since Unix epoch)
    pub end_time: i64,
    /// Rolling baseline length in nanoseconds; the baseline sits immediately
    /// before the derived preceding window. Omit to disable it.
    pub rolling_ns: Option<i64>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// IANA timezone echoed back for calendar alignment (e.g. Europe/London)
    pub timezone: Option<String>,
}

/// Get the model-performance diagnosis: the canonical comparison of a
/// current interval against its preceding interval and an optional rolling
/// baseline, with the deterministic classification and confidence for each
/// identity (#121).
#[utoipa::path(
    get,
    path = "/api/genai/model-performance",
    params(ModelPerformanceQueryParams),
    responses(
        (status = 200, description = "Model-performance diagnosis", body = otelite_core::api::ModelPerformanceDiagnosis),
        (status = 400, description = "Invalid interval or baseline", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_model_performance(
    State(state): State<AppState>,
    Query(query): Query<ModelPerformanceQueryParams>,
) -> Result<Json<otelite_core::api::ModelPerformanceDiagnosis>, (StatusCode, Json<ErrorResponse>)> {
    if query.end_time <= query.start_time {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request(format!(
                "end_time must be after start_time (got start={} end={})",
                query.start_time, query.end_time
            ))),
        ));
    }
    if let Some(len) = query.rolling_ns {
        if len <= 0 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::bad_request(format!(
                    "rolling_ns must be a positive length in nanoseconds (got {len})"
                ))),
            ));
        }
    }
    if let Some(tz) = query.timezone.as_deref() {
        let tz = tz.trim();
        let _parsed: chrono_tz::Tz = std::str::FromStr::from_str(tz).map_err(|e| {
            (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::bad_request(format!(
                    "unknown IANA timezone '{tz}': {e} (e.g. Europe/London, America/New_York, UTC)"
                ))),
            )
        })?;
    }

    // Preceding window: equal length immediately before the current one.
    // Rolling baseline: the requested length immediately before the
    // preceding window — structurally excluding both comparison windows.
    let current_len = query.end_time - query.start_time;
    let preceding_start = query.start_time - current_len;
    let query_core = otelite_core::api::ModelPerformanceQuery {
        current: otelite_core::api::ModelPerformanceWindow {
            start_time: query.start_time,
            end_time: query.end_time,
        },
        rolling: query
            .rolling_ns
            .map(|len| otelite_core::api::ModelPerformanceWindow {
                start_time: preceding_start - len,
                end_time: preceding_start,
            }),
        model: query.model.clone(),
        provider: query.provider.clone(),
    };

    let response = state
        .storage
        .query_model_performance(&query_core)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query model performance: {e}"
                ))),
            )
        })?;
    let capability = state
        .storage
        .query_genai_capabilities(
            Some(query_core.current.start_time),
            Some(query_core.current.end_time),
            &GenAiFilters {
                model: query.model.clone(),
                provider: query.provider.clone(),
                ..Default::default()
            },
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query GenAI capabilities for TTFT trust: {e}"
                ))),
            )
        })?;

    Ok(Json(otelite_core::model_performance::build_diagnosis(
        &response,
        &capability,
        query.timezone.clone(),
    )))
}

/// Query parameters for error-rate endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ErrorRateQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Get error rate per model across LLM spans.
#[utoipa::path(
    get,
    path = "/api/genai/error_rate",
    params(ErrorRateQuery),
    responses(
        (status = 200, description = "Error rate per model", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_error_rate(
    State(state): State<AppState>,
    Query(query): Query<ErrorRateQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_error_rate(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query error rate: {}",
                    e
                ))),
            )
        })?;

    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize error_rate: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Query parameters for tool-usage endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ToolUsageQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Maximum number of tools to return (default 20, capped at 100)
    pub limit: Option<usize>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Get aggregated per-tool usage for tool-execution spans.
#[utoipa::path(
    get,
    path = "/api/genai/tool_usage",
    params(ToolUsageQuery),
    responses(
        (status = 200, description = "Tool usage aggregates", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_tool_usage(
    State(state): State<AppState>,
    Query(query): Query<ToolUsageQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = query.limit.unwrap_or(20).clamp(1, 100);
    let filters = query.filters();
    let rows = state
        .storage
        .query_tool_usage(query.start_time, query.end_time, &filters, limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query tool usage: {}",
                    e
                ))),
            )
        })?;

    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize tool_usage: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Query parameters for retry-stats endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct RetryStatsQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Get retry statistics across LLM spans.
#[utoipa::path(
    get,
    path = "/api/genai/retry_stats",
    params(RetryStatsQuery),
    responses(
        (status = 200, description = "Retry statistics", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_retry_stats(
    State(state): State<AppState>,
    Query(query): Query<RetryStatsQuery>,
) -> Result<Json<RetryStats>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let mut stats = state
        .storage
        .query_retry_stats(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query retry stats: {}",
                    e
                ))),
            )
        })?;

    stats.filters_applied = filters.applied(&FILTER_DIMENSIONS);

    Ok(Json(stats))
}

/// Query parameters for retrieval-stats endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct RetrievalStatsQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Maximum number of top queries to return (default 5, capped at 20)
    pub limit: Option<usize>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Get aggregated retrieval / RAG statistics across retriever spans.
///
/// Retriever spans are identified by `openinference.span.kind = 'RETRIEVER'` or
/// the presence of a `retrieval.query` attribute. Returns total counts, average
/// documents per query, average top-1 document score, and the top-N most-frequent
/// queries.
#[utoipa::path(
    get,
    path = "/api/genai/retrieval_stats",
    params(RetrievalStatsQuery),
    responses(
        (status = 200, description = "Retrieval statistics", body = RetrievalStats),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_retrieval_stats(
    State(state): State<AppState>,
    Query(query): Query<RetrievalStatsQuery>,
) -> Result<Json<RetrievalStats>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let limit = query.limit.unwrap_or(5).clamp(1, 20);

    let mut stats = state
        .storage
        .query_retrieval_stats(query.start_time, query.end_time, &filters, limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query retrieval stats: {}",
                    e
                ))),
            )
        })?;

    stats.filters_applied = filters.applied(&FILTER_DIMENSIONS);

    Ok(Json(stats))
}

/// Metadata about the pricing database currently in use by the server.
#[derive(Debug, Clone, Serialize, utoipa::ToSchema)]
pub struct PricingMetadata {
    /// "litellm" when the upstream LiteLLM fetch has succeeded at least once;
    /// "fallback" when only the hardcoded Claude 4.x table is available.
    pub source: &'static str,
    /// Number of entries in the active pricing database (0 for fallback-only).
    pub entry_count: usize,
    /// Unix milliseconds of the last successful LiteLLM fetch, if any.
    pub last_fetched_unix_ms: Option<i64>,
    /// Unix milliseconds of the last failed LiteLLM fetch, if any.
    pub last_failed_unix_ms: Option<i64>,
    /// Date the hardcoded Claude 4.x fallback table was last verified against
    /// Anthropic's list rates.
    pub fallback_last_verified: &'static str,
    /// URL to the LiteLLM source file for attribution / deep-linking.
    pub source_url: &'static str,
    /// MIT-license acknowledgement for the LiteLLM data.
    pub license: &'static str,
    /// User-facing disclaimer text — safe to render inline.
    pub disclaimer: &'static str,
    /// Filter dimensions the endpoint actually applied (global filter bar, #135)
    pub filters_applied: Vec<String>,
}

/// Return the list of agent-framework recognizers (CrewAI, AutoGen, LangGraph).
/// The web UI and any other client consumes this to know which attributes to
/// group under each framework section — keeps the vocabulary in one place.
#[utoipa::path(
    get,
    path = "/api/genai/agent_framework_defs",
    responses(
        (status = 200, description = "Agent framework recognizers"),
    ),
    tag = "genai"
)]
pub async fn get_agent_framework_defs(
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let items =
        serde_json::to_value(otelite_core::agent_frameworks::AGENT_FRAMEWORKS).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize agent framework defs: {e}"
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items,
        // Static reference data — the bar's dimensions don't apply to it.
        filters_applied: Vec::new(),
    }))
}

const PRICING_DISCLAIMER: &str =
    "Cost figures are best-effort estimates. Per-token rates sourced from the LiteLLM \
     community pricing database (MIT-licensed, © 2023 Berri AI). When the upstream \
     fetch is unavailable, a small hand-curated Claude 4.x fallback table is used.";

/// Return metadata describing which pricing database the server is currently
/// using. The frontend reads this once to render the disclaimer banner and a
/// source/freshness badge.
#[utoipa::path(
    get,
    path = "/api/genai/pricing_metadata",
    responses(
        (status = 200, description = "Pricing metadata", body = PricingMetadata),
    ),
    tag = "genai"
)]
pub async fn get_pricing_metadata(State(state): State<AppState>) -> Json<PricingMetadata> {
    let snapshot = state.pricing.snapshot().await;
    Json(PricingMetadata {
        source: if snapshot.db.is_litellm() {
            "litellm"
        } else {
            "fallback"
        },
        entry_count: snapshot.db.len(),
        last_fetched_unix_ms: snapshot.last_fetched_unix_ms,
        last_failed_unix_ms: snapshot.last_failed_unix_ms,
        fallback_last_verified: otelite_core::pricing::FALLBACK_LAST_VERIFIED,
        source_url: otelite_core::pricing::LITELLM_SOURCE_URL,
        license: otelite_core::pricing::LITELLM_LICENSE,
        disclaimer: PRICING_DISCLAIMER,
        filters_applied: Vec::new(),
    })
}

/// Query parameters shared by the new per-model analytics endpoints.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ModelAnalyticsQuery {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Query parameters for the cache hit rate / cache economics endpoint.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct CacheQuery {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    // Filter dimensions: model only applies without `by_model`; the economics
    // payload is always per-model.
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
    /// Pass `1` (or `true`) to return the cache-economics payload
    /// (per-model read/write split, hit rate, read:write ratio, estimated
    /// savings, plus a time-bucketed series). Without it, the original
    /// per-model hit-rate list is returned unchanged.
    pub by_model: Option<String>,
    /// Bucket size in seconds for the economics series (default 3600).
    /// Only used with `by_model=1`.
    pub bucket_secs: Option<u64>,
}

/// Query parameters for time-series endpoints.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct TimeSeriesQuery {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    /// Bucket size in seconds (default 3600 = 1 hour).
    pub bucket_secs: Option<u64>,
    /// Span filter: "llm" (default) = LLM spans only; "all" = all OTel spans grouped by name.
    pub span_filter: Option<String>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Query parameters for time-series endpoints that also accept a model filter.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct ModelTimeSeriesQuery {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    /// Bucket size in seconds (default 3600 = 1 hour).
    pub bucket_secs: Option<u64>,
    /// Optional model filter (e.g. "claude-opus-4-7").
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
    /// Span filter: "llm" (default) = LLM spans only; "all" = all OTel spans grouped by name.
    pub span_filter: Option<String>,
}

/// Query parameters for endpoints that only filter by time.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct TimeRangeQuery {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    /// Agent family filter: `claude`, `opencode`, or `codex`
    pub agent: Option<String>,
    /// Model name filter
    pub model: Option<String>,
    /// Provider filter
    pub provider: Option<String>,
    /// Project id filter (opencode data only)
    pub project: Option<String>,
    /// Session id filter
    pub session: Option<String>,
}

/// Truncation rate (finish_reason = max_tokens / length) per model.
#[utoipa::path(
    get,
    path = "/api/genai/truncation_rate",
    params(ModelAnalyticsQuery),
    responses(
        (status = 200, description = "Truncation rate by model", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_truncation_rate(
    State(state): State<AppState>,
    Query(query): Query<ModelAnalyticsQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_truncation_rate(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query truncation rate: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize truncation_rate: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// `by_model=1` (or `true`) enables the cache-economics payload.
fn by_model_enabled(v: Option<&str>) -> bool {
    matches!(v, Some("1") | Some("true"))
}

/// Cache token hit rate per model; with `by_model=1` returns the full cache
/// economics payload instead (`CacheEconomicsResponse`).
#[utoipa::path(
    get,
    path = "/api/genai/cache_hit_rate",
    params(CacheQuery),
    responses(
        (status = 200, description = "Per-model cache hit rate (default) or cache economics with by_model=1", body = serde_json::Value),
        (status = 400, description = "Invalid bucket_secs", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_cache_hit_rate(
    State(state): State<AppState>,
    Query(query): Query<CacheQuery>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if by_model_enabled(query.by_model.as_deref()) {
        let bucket_secs = query.bucket_secs.unwrap_or(3600);
        if bucket_secs == 0 {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse::bad_request(
                    "bucket_secs must be a positive number of seconds",
                )),
            ));
        }
        let mut response = state
            .storage
            .query_cache_economics(
                query.start_time,
                query.end_time,
                (bucket_secs as i64) * 1_000_000_000,
            )
            .await
            .map_err(|e| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse::storage_error(format!(
                        "query cache economics: {}",
                        e
                    ))),
                )
            })?;
        // Enrich per-model estimated savings. The cache-read price is
        // unknown when the pricing table has no entry or no cache-read rate
        // for the model — savings stay null and savings_known is false.
        let pricing = state.pricing.snapshot().await;
        for m in &mut response.models {
            let r = pricing
                .db
                .compute_cache_savings(Some(&m.model), m.cache_read_tokens, None);
            m.est_savings_usd = r.cost;
            m.savings_known = r.cost.is_some();
        }
        let value = serde_json::to_value(response).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize cache economics: {e}"
                ))),
            )
        })?;
        // The economics payload is always per-model and drawn from
        // opencode metrics — nothing from the bar is applied to it.
        let obj = value.as_object().ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(
                    "cache economics payload is not a JSON object".to_string(),
                )),
            )
        })?;
        let mut obj = obj.clone();
        obj.insert(
            "filters_applied".to_string(),
            serde_json::Value::Array(Vec::new()),
        );
        return Ok(Json(serde_json::Value::Object(obj)));
    }

    let filters = query.filters();
    let rows = state
        .storage
        .query_cache_hit_rate(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query cache hit rate: {}",
                    e
                ))),
            )
        })?;
    let items = serde_json::to_value(rows).map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::storage_error(format!(
                "serialize cache hit rate: {e}"
            ))),
        )
    })?;
    // The per-model rows are an array, so — like the other list endpoints —
    // they are wrapped in the standard { items, filters_applied } envelope
    // rather than merged into the payload (issue #139: merging required an
    // object and 500'd on the array).
    Ok(Json(serde_json::json!({
        "items": items,
        "filters_applied": filters.applied(&FILTER_DIMENSIONS),
    })))
}

/// Reasoning ("thinking") token share per model, plus a global
/// per-effort breakdown. Reasoning tokens are priced at the model's output
/// rate — that is what thinking costs.
#[utoipa::path(
    get,
    path = "/api/genai/reasoning_share",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Reasoning token share by model and effort", body = ReasoningShareResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_reasoning_share(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<ReasoningShareResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let mut response = state
        .storage
        .query_reasoning_share(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query reasoning share: {e}"
                ))),
            )
        })?;

    // Enrich per-model cost: reasoning tokens billed at the output rate.
    let pricing = state.pricing.snapshot().await;
    for m in &mut response.models {
        let usage = TokenUsage {
            input: 0,
            output: m.reasoning_tokens,
            cache_creation: 0,
            cache_read: 0,
        };
        m.cost_usd = pricing
            .db
            .compute_cost(Some(m.model.as_str()), usage, None)
            .cost;
    }
    // The rollup mixes opencode metrics and codex spans; the filter bar's
    // dimensions aren't addressable across both parts, so nothing is applied
    // and the UI greys the whole bar for this section.
    response.filters_applied = filters.applied(&[]);

    Ok(Json(response))
}

/// Per-harness rollup: sessions, cost, tokens, tool calls and retries for
/// opencode/codex/claude, sorted by cost descending. opencode's cost is its
/// own spend counter ("actual"); codex and claude are estimated from tokens
/// x pricing (their cost counters under-report on the live data).
#[utoipa::path(
    get,
    path = "/api/genai/agents",
    params(TimeSeriesQuery),
    responses(
        (status = 200, description = "Per-harness sessions, cost, tokens, tool calls and retries", body = AgentRollupResponse),
        (status = 400, description = "Invalid bucket_secs", body = ErrorResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_agents(
    State(state): State<AppState>,
    Query(query): Query<TimeSeriesQuery>,
) -> Result<Json<AgentRollupResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let bucket_secs = query.bucket_secs.unwrap_or(3600);
    if bucket_secs == 0 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse::bad_request(
                "bucket_secs must be a positive number of seconds",
            )),
        ));
    }

    let rollups = state
        .storage
        .query_agent_rollup(query.start_time, query.end_time, bucket_secs)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query agent rollup: {e}"
                ))),
            )
        })?;

    let pricing = state.pricing.snapshot().await;
    let mut agents: Vec<AgentRollup> = rollups.into_iter().map(|r| r.enrich(&pricing.db)).collect();
    agents.sort_by(|a, b| {
        b.cost_usd
            .unwrap_or(0.0)
            .partial_cmp(&a.cost_usd.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.agent.cmp(&b.agent))
    });

    Ok(Json(AgentRollupResponse {
        agents,
        filters_applied: filters.applied(&[]),
    }))
}

/// Per-project usage: which project/repo drove the bill. opencode
/// attributes by `project.id`; codex/claude emit no project label today, so
/// their activity lands in the `"unattributed"` row (stated as
/// `cost_source`/`project_id`, not a mapping invented in the query).
#[utoipa::path(
    get,
    path = "/api/genai/projects",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Cost, sessions and tokens per project", body = ProjectRollupResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_projects(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<ProjectRollupResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let rollups = state
        .storage
        .query_project_rollup(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query project rollup: {e}"
                ))),
            )
        })?;

    let pricing = state.pricing.snapshot().await;
    let mut projects: Vec<_> = rollups.into_iter().map(|r| r.enrich(&pricing.db)).collect();
    projects.sort_by(|a, b| {
        b.cost_usd
            .unwrap_or(0.0)
            .partial_cmp(&a.cost_usd.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.project_id.cmp(&b.project_id))
    });

    Ok(Json(ProjectRollupResponse {
        projects,
        filters_applied: filters.applied(&[]),
    }))
}

/// Sub-agent role attribution: cost and tokens per opencode `agent` label.
#[utoipa::path(
    get,
    path = "/api/genai/agent_roles",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Cost and token attribution per sub-agent role", body = AgentRolesResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_agent_roles(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<AgentRolesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let mut response = state
        .storage
        .query_agent_roles(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query agent roles: {}",
                    e
                ))),
            )
        })?;

    // Enrich per-model cost from the pricing table. opencode's own cost
    // counter is zero-valued in the wire data, so tokens x price is the only
    // source. Reasoning tokens are not priced (no separate rate in the
    // pricing table); role.cost covers the top-5 models and is None when any
    // of them lacks pricing (e.g. local models).
    let pricing = state.pricing.snapshot().await;
    for role in &mut response.roles {
        let mut total: f64 = 0.0;
        let mut all_priced = true;
        for m in &mut role.top_models {
            let usage = TokenUsage {
                input: m.tokens.input,
                output: m.tokens.output,
                cache_creation: m.tokens.cache_write,
                cache_read: m.tokens.cache_read,
            };
            let result = pricing.db.compute_cost(Some(m.model.as_str()), usage, None);
            m.cost = result.cost;
            m.cost_source = Some(result.source.as_str().to_string());
            m.cost_reason = result.reason;
            match result.cost {
                Some(c) => total += c,
                None => all_priced = false,
            }
        }
        role.cost = if all_priced && !role.top_models.is_empty() {
            Some(total)
        } else {
            None
        };
    }
    response.filters_applied = filters.applied(&[]);

    Ok(Json(response))
}

/// Provider × model mix: tokens, sessions and estimated cost per provider
/// and model, across opencode, codex and claude_code.
#[utoipa::path(
    get,
    path = "/api/genai/provider_mix",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Provider x model token and cost mix", body = ProviderMixResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_provider_mix(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<ProviderMixResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let mut response = state
        .storage
        .query_provider_mix(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query provider mix: {}",
                    e
                ))),
            )
        })?;

    // Enrich per-model cost from the pricing table (opencode's own cost
    // counter is zero-valued in the wire data, so tokens x price is the
    // source; reasoning tokens are not priced). Provider cost covers its
    // priced models; None when none of them has known pricing.
    let pricing = state.pricing.snapshot().await;
    for provider in &mut response.providers {
        let mut total: f64 = 0.0;
        let mut any_priced = false;
        for m in &mut provider.models {
            let usage = TokenUsage {
                input: m.tokens.input,
                output: m.tokens.output,
                cache_creation: m.tokens.cache_write,
                cache_read: m.tokens.cache_read,
            };
            let result = pricing.db.compute_cost(Some(m.model.as_str()), usage, None);
            if let Some(c) = result.cost {
                total += c;
                any_priced = true;
            }
            m.cost_usd = result.cost;
            m.cost_source = Some(result.source.as_str().to_string());
        }
        provider.cost_usd = if any_priced { Some(total) } else { None };
    }
    response.filters_applied = filters.applied(&[]);

    Ok(Json(response))
}

/// Distribution of request parameter settings (temperature, max_tokens).
#[utoipa::path(
    get,
    path = "/api/genai/request_param_profile",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Request parameter profile", body = RequestParamProfile),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_request_param_profile(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<RequestParamProfile>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let mut profile = state
        .storage
        .query_request_param_profile(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query request param profile: {}",
                    e
                ))),
            )
        })?;
    profile.filters_applied = filters.applied(&FILTER_DIMENSIONS);

    Ok(Json(profile))
}

/// Turn-count distribution across conversations.
#[utoipa::path(
    get,
    path = "/api/genai/conversation_depth",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Conversation depth statistics", body = ConversationDepthStats),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_conversation_depth(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<ConversationDepthStats>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let mut stats = state
        .storage
        .query_conversation_depth(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query conversation depth: {}",
                    e
                ))),
            )
        })?;
    stats.filters_applied = filters.applied(&FILTER_DIMENSIONS);

    Ok(Json(stats))
}

/// LLM span latency (min/avg/p95/max + TTFT) per time bucket, grouped by model.
#[utoipa::path(
    get,
    path = "/api/genai/latency_series",
    params(ModelTimeSeriesQuery),
    responses(
        (status = 200, description = "Latency stats per time bucket", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_latency_series(
    State(state): State<AppState>,
    Query(query): Query<ModelTimeSeriesQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let bucket_secs = query.bucket_secs.unwrap_or(3600).clamp(60, 86400);
    let all_spans = query.span_filter.as_deref() == Some("all");
    let rows = state
        .storage
        .query_latency_series(
            query.start_time,
            query.end_time,
            bucket_secs,
            &filters,
            all_spans,
            None,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query latency series: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize latency_series: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// LLM call volume over time (parallel to cost_series).
#[utoipa::path(
    get,
    path = "/api/genai/calls_series",
    params(TimeSeriesQuery),
    responses(
        (status = 200, description = "Calls per time bucket", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_calls_series(
    State(state): State<AppState>,
    Query(query): Query<TimeSeriesQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let bucket_secs = query.bucket_secs.unwrap_or(3600).clamp(60, 86400);
    let all_spans = query.span_filter.as_deref() == Some("all");
    let rows = state
        .storage
        .query_calls_series(
            query.start_time,
            query.end_time,
            &filters,
            bucket_secs,
            all_spans,
        )
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query calls series: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize calls_series: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// LLM latency broken down by input-token context size bin × model.
/// Useful for answering "do larger prompts cause slower responses?"
#[utoipa::path(
    get,
    path = "/api/genai/latency_by_context",
    params(ModelAnalyticsQuery),
    responses(
        (status = 200, description = "Latency per context size bin", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_latency_by_context(
    State(state): State<AppState>,
    Query(query): Query<ModelAnalyticsQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_latency_by_context(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query latency by context: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize latency_by_context: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Per-(model, error_type) breakdown of error spans, bucketed into actionable categories.
#[utoipa::path(
    get,
    path = "/api/genai/error_types",
    params(ModelAnalyticsQuery),
    responses(
        (status = 200, description = "Error type breakdown per model", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_error_types(
    State(state): State<AppState>,
    Query(query): Query<ModelAnalyticsQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_error_types(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query error types: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize error_types: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// All observed (request_model → response_model) pairs with a `differs` flag.
/// `differs == true` indicates silent provider rerouting.
#[utoipa::path(
    get,
    path = "/api/genai/model_drift",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Request→response model pairs", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_model_drift(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_model_drift(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query model drift: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize model_drift: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Tool approval/rejection summary (claude_code.tool.blocked_on_user spans).
#[utoipa::path(
    get,
    path = "/api/genai/tool_approvals",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Tool approval statistics", body = ToolApprovalStats),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_tool_approvals(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<ToolApprovalStats>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();
    let mut stats = state
        .storage
        .query_tool_approvals(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query tool approvals: {}",
                    e
                ))),
            )
        })?;
    stats.filters_applied = filters.applied(&FILTER_DIMENSIONS);

    Ok(Json(stats))
}

/// Distribution of stop_reason values across LLM spans.
#[utoipa::path(
    get,
    path = "/api/genai/stop_reasons",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Stop reason distribution", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_stop_reasons(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_stop_reasons(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query stop reasons: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize stop_reasons: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Token usage broken down by llm_request.context type.
#[utoipa::path(
    get,
    path = "/api/genai/context_type_split",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Context type token split", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_context_type_split(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_context_type_split(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query context type split: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize context_type_split: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Top error messages from failed tool executions.
#[utoipa::path(
    get,
    path = "/api/genai/tool_errors",
    params(ToolUsageQuery),
    responses(
        (status = 200, description = "Tool error messages", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_tool_errors(
    State(state): State<AppState>,
    Query(query): Query<ToolUsageQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = query.limit.unwrap_or(30).clamp(1, 100);
    let filters = query.filters();
    let rows = state
        .storage
        .query_tool_errors(query.start_time, query.end_time, &filters, limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query tool errors: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize tool_errors: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Hour-of-day activity distribution (UTC).
#[utoipa::path(
    get,
    path = "/api/genai/hour_of_day",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Hour-of-day buckets", body = GenAiItemsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_hour_of_day(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<GenAiItemsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let filters = query.filters();

    let rows = state
        .storage
        .query_hour_of_day(query.start_time, query.end_time, &filters)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query hour of day: {}",
                    e
                ))),
            )
        })?;
    Ok(Json(GenAiItemsResponse {
        items: serde_json::to_value(&rows).map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "serialize hour_of_day: {e}"
                ))),
            )
        })?,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Claude Code effort breakdown by effort level × model × token type (#157).
#[utoipa::path(
    get,
    path = "/api/genai/effort_breakdown",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Effort breakdown", body = otelite_core::api::EffortBreakdownResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_effort_breakdown(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::EffortBreakdownResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut response = state
        .storage
        .query_effort_breakdown(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query effort breakdown: {e}"
                ))),
            )
        })?;
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// Cross-agent efficiency stats: tokens/commit, tokens/LOC (#158).
#[utoipa::path(
    get,
    path = "/api/genai/efficiency_stats",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Efficiency stats", body = otelite_core::api::EfficiencyStats),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_efficiency_stats(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::EfficiencyStats>, (StatusCode, Json<ErrorResponse>)> {
    let mut response = state
        .storage
        .query_efficiency_stats(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query efficiency stats: {e}"
                ))),
            )
        })?;
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// Codex TTFT histogram percentiles per model (#159).
#[utoipa::path(
    get,
    path = "/api/genai/codex_ttft",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Codex TTFT percentiles", body = otelite_core::api::CodexTtftResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_codex_ttft(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::CodexTtftResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut response = state
        .storage
        .query_codex_ttft(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query codex ttft: {e}"
                ))),
            )
        })?;
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// Per-project token + commit rollup across all agents (#160).
#[utoipa::path(
    get,
    path = "/api/genai/project_rollup",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Per-project rollup", body = otelite_core::api::AgentProjectRollupResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_agent_project_rollup(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::AgentProjectRollupResponse>, (StatusCode, Json<ErrorResponse>)>
{
    let mut response = state
        .storage
        .query_agent_project_rollup(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query project rollup: {e}"
                ))),
            )
        })?;
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// MCP call success/error health per server+tool (#161).
#[utoipa::path(
    get,
    path = "/api/genai/mcp_health",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "MCP health", body = otelite_core::api::McpHealthResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_mcp_health(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::McpHealthResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut response = state
        .storage
        .query_mcp_health(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query mcp health: {e}"
                ))),
            )
        })?;
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// Codex Guardian review summary by risk level and action (#162).
#[utoipa::path(
    get,
    path = "/api/genai/guardian_stats",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Guardian stats", body = otelite_core::api::GuardianStatsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_guardian_stats(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::GuardianStatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut response = state
        .storage
        .query_guardian_stats(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query guardian stats: {e}"
                ))),
            )
        })?;
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// Codex multi-agent spawn/resume topology by role (#163).
#[utoipa::path(
    get,
    path = "/api/genai/multi_agent_stats",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Multi-agent topology", body = otelite_core::api::MultiAgentStatsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_multi_agent_stats(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::MultiAgentStatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let mut response = state
        .storage
        .query_multi_agent_stats(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query multi_agent stats: {e}"
                ))),
            )
        })?;
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// Codex turn busy vs idle breakdown per model and project (#164).
#[utoipa::path(
    get,
    path = "/api/genai/codex_turn_breakdown",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Codex turn busy/idle breakdown", body = otelite_core::api::CodexTurnBreakdownResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_codex_turn_breakdown(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::CodexTurnBreakdownResponse>, (StatusCode, Json<ErrorResponse>)>
{
    let mut response = state
        .storage
        .query_codex_turn_breakdown(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query codex_turn_breakdown: {e}"
                ))),
            )
        })?;
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// Session × model cross-tab: tokens and cost per (session_id, model) pair (#115).
#[utoipa::path(
    get,
    path = "/api/genai/session_model_breakdown",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Session × model cross-tab", body = otelite_core::api::SessionModelBreakdown),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_session_model_breakdown(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::SessionModelBreakdown>, (StatusCode, Json<ErrorResponse>)> {
    let mut response = state
        .storage
        .query_session_model_breakdown(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query session_model_breakdown: {e}"
                ))),
            )
        })?;
    // Enrich each row with pricing cost from model identifier.
    let pricing_db = state.pricing.snapshot().await;
    for row in &mut response.rows {
        let usage = TokenUsage {
            input: row.input_tokens,
            output: row.output_tokens,
            cache_creation: 0,
            cache_read: 0,
        };
        let cr = pricing_db.db.compute_cost(Some(&row.model), usage, None);
        row.cost = cr.cost;
    }
    // Re-sort by cost desc (nulls last), then requests desc.
    response.rows.sort_by(|a, b| match (a.cost, b.cost) {
        (Some(ac), Some(bc)) => bc.partial_cmp(&ac).unwrap_or(std::cmp::Ordering::Equal),
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (None, None) => b.requests.cmp(&a.requests),
    });
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// Speed/effort attribute distribution across Claude Code LLM spans (#114).
#[utoipa::path(
    get,
    path = "/api/genai/speed_distribution",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Speed/effort distribution by model", body = otelite_core::api::SpeedDistribution),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_speed_distribution(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::SpeedDistribution>, (StatusCode, Json<ErrorResponse>)> {
    let mut response = state
        .storage
        .query_speed_distribution(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query speed_distribution: {e}"
                ))),
            )
        })?;
    response.filters_applied = query.filters().applied(&[]);
    Ok(Json(response))
}

/// Cross-tool TTFT comparison from span-level ttft_ms attribute.
#[utoipa::path(
    get,
    path = "/api/genai/cross_tool_ttft",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "TTFT statistics by tool and model", body = otelite_core::api::CrossToolTtftResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_cross_tool_ttft(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::CrossToolTtftResponse>, (StatusCode, Json<ErrorResponse>)> {
    let response = state
        .storage
        .query_cross_tool_ttft(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query cross_tool_ttft: {e}"
                ))),
            )
        })?;
    Ok(Json(response))
}

/// Codex hook overhead: total and average duration per hook event type.
#[utoipa::path(
    get,
    path = "/api/genai/hook_overhead",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Hook overhead by event type", body = otelite_core::api::HookOverheadResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_hook_overhead(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::HookOverheadResponse>, (StatusCode, Json<ErrorResponse>)> {
    let response = state
        .storage
        .query_hook_overhead(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query hook_overhead: {e}"
                ))),
            )
        })?;
    Ok(Json(response))
}

/// Tool failure rates from opencode.tool.duration.
#[utoipa::path(
    get,
    path = "/api/genai/tool_failure_rates",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Tool failure rates sorted by failure count", body = otelite_core::api::ToolFailureRatesResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_tool_failure_rates(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::ToolFailureRatesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let response = state
        .storage
        .query_tool_failure_rates(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query tool_failure_rates: {e}"
                ))),
            )
        })?;
    Ok(Json(response))
}

/// Daily tool activity mix (claude_code / opencode / codex datapoints per day).
#[utoipa::path(
    get,
    path = "/api/genai/daily_tool_mix",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Datapoints per tool per calendar day", body = otelite_core::api::DailyToolMixResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_daily_tool_mix(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::DailyToolMixResponse>, (StatusCode, Json<ErrorResponse>)> {
    let response = state
        .storage
        .query_daily_tool_mix(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query daily_tool_mix: {e}"
                ))),
            )
        })?;
    Ok(Json(response))
}

/// Codex skill injection activity.
#[utoipa::path(
    get,
    path = "/api/genai/skill_activity",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Skill injection counts by skill name and invoke type", body = otelite_core::api::SkillActivityResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_skill_activity(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::SkillActivityResponse>, (StatusCode, Json<ErrorResponse>)> {
    let response = state
        .storage
        .query_skill_activity(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query skill_activity: {e}"
                ))),
            )
        })?;
    Ok(Json(response))
}

#[utoipa::path(
    get,
    path = "/api/genai/session_quality_summary",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Aggregate session quality counts: clean / degraded / errored", body = otelite_core::api::SessionQualitySummary),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_session_quality_summary(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::SessionQualitySummary>, (StatusCode, Json<ErrorResponse>)> {
    let response = state
        .storage
        .query_session_quality_summary(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query session_quality_summary: {e}"
                ))),
            )
        })?;
    Ok(Json(response))
}

#[utoipa::path(
    get,
    path = "/api/genai/skill_outcomes",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Token comparison for sessions with vs without each skill", body = otelite_core::api::SkillOutcomesResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_skill_outcomes(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::SkillOutcomesResponse>, (StatusCode, Json<ErrorResponse>)> {
    let response = state
        .storage
        .query_skill_outcomes(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query skill_outcomes: {e}"
                ))),
            )
        })?;
    Ok(Json(response))
}

#[utoipa::path(
    get,
    path = "/api/genai/model_selection_heatmap",
    params(TimeRangeQuery),
    responses(
        (status = 200, description = "Model-selection heatmap: (role, tool, model) → request count and token share", body = otelite_core::api::ModelSelectionHeatmapResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_model_selection_heatmap(
    State(state): State<AppState>,
    Query(query): Query<TimeRangeQuery>,
) -> Result<Json<otelite_core::api::ModelSelectionHeatmapResponse>, (StatusCode, Json<ErrorResponse>)>
{
    let response = state
        .storage
        .query_model_selection_heatmap(query.start_time, query.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query model_selection_heatmap: {e}"
                ))),
            )
        })?;
    Ok(Json(response))
}

/// Query parameters for the `GET /api/genai/recent_errors` endpoint.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct RecentErrorsQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Filter by tool harness label (`claude_code`, `opencode`, `codex`, `pi`, …)
    pub tool: Option<String>,
    /// Maximum number of rows to return (default 50, max 200)
    pub limit: Option<usize>,
}

#[utoipa::path(
    get,
    path = "/api/genai/recent_errors",
    params(RecentErrorsQuery),
    responses(
        (status = 200, description = "Most recent error events from spans and logs", body = otelite_core::api::RecentErrorsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_recent_errors(
    State(state): State<AppState>,
    Query(query): Query<RecentErrorsQuery>,
) -> Result<Json<otelite_core::api::RecentErrorsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let response = state
        .storage
        .query_recent_errors(query.start_time, query.end_time, query.tool, query.limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query recent_errors: {e}"
                ))),
            )
        })?;
    Ok(Json(response))
}

genai_filter_impl!(TokenUsageQuery);
genai_filter_impl!(CostSeriesQuery);
genai_filter_impl!(FinishReasonsQuery);
genai_filter_impl!(LatencyQuery);
genai_filter_impl!(ErrorRateQuery);
genai_filter_impl!(ModelAnalyticsQuery);
genai_filter_impl!(CacheQuery);
genai_filter_impl!(ModelTimeSeriesQuery);
genai_filter_impl!(TopSpansQuery);
genai_filter_impl!(TopGroupQuery);
genai_filter_impl!(LatencyPercentileQuery);
genai_filter_impl!(DistributionQuery);
genai_filter_impl!(ToolUsageQuery);
genai_filter_impl!(RetryStatsQuery);
genai_filter_impl!(RetrievalStatsQuery);
genai_filter_impl!(TimeRangeQuery);
genai_filter_impl!(TimeSeriesQuery);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_model_flag_accepts_one_and_true_only() {
        assert!(by_model_enabled(Some("1")));
        assert!(by_model_enabled(Some("true")));
        assert!(!by_model_enabled(Some("0")));
        assert!(!by_model_enabled(Some("yes")));
        assert!(!by_model_enabled(Some("2")));
        assert!(!by_model_enabled(None));
    }
}
