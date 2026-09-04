//! Session-level API endpoints.

use crate::server::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::Json,
};
use chrono::{DateTime, Local, Utc};
use otelite_core::api::{
    ErrorResponse, SessionContextGrowth, SessionDiagnoseResponse, SessionInteraction,
    SessionListResponse, SessionSummary,
};
use otelite_core::filters::{GenAiFilters, FILTER_DIMENSIONS};
use otelite_core::query::{Operator, QueryPredicate, QueryValue};
use otelite_core::storage::QueryParams;
use otelite_core::telemetry::trace::StatusCode as SpanStatusCode;
use otelite_core::telemetry::{extract_ttft_secs, GenAiSpanInfo, Span};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};

fn root_llm_span(spans: &[Span]) -> Option<&Span> {
    spans
        .iter()
        .filter(|s| s.parent_span_id.is_none())
        .find(|s| s.attributes.keys().any(|k| k.starts_with("gen_ai.")))
        .or_else(|| {
            spans
                .iter()
                .find(|s| s.attributes.keys().any(|k| k.starts_with("gen_ai.")))
        })
}

/// GET /api/sessions/:session_id/diagnose
///
/// Returns a forensic report for an LLM session: per-interaction token counts,
/// latency, errors, streaming stalls, and context growth.
pub async fn get_session_diagnose(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<SessionDiagnoseResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Single query: all spans where session.id = <session_id>.
    // query_spans applies predicates via json_extract — no per-trace round-trips needed.
    let query = QueryParams {
        predicates: vec![QueryPredicate {
            field: "session.id".to_string(),
            operator: Operator::Equal,
            value: QueryValue::String(session_id.clone()),
        }],
        limit: Some(10_000),
        ..Default::default()
    };

    let all_spans = state.storage.query_spans(&query).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::internal_error(e.to_string())),
        )
    })?;

    if all_spans.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::not_found(format!("session {}", session_id))),
        ));
    }

    // Group spans by trace_id.
    let mut by_trace: HashMap<String, Vec<Span>> = HashMap::new();
    for span in all_spans {
        by_trace
            .entry(span.trace_id.clone())
            .or_default()
            .push(span);
    }

    // Sort trace groups by the earliest span start_time (chronological order).
    let mut trace_groups: Vec<(String, Vec<Span>)> = by_trace.into_iter().collect();
    trace_groups.sort_by_key(|(_, spans)| spans.iter().map(|s| s.start_time).min().unwrap_or(0));

    let mut interactions: Vec<SessionInteraction> = Vec::new();

    for (idx, (trace_id, spans)) in trace_groups.iter().enumerate() {
        let root = match root_llm_span(spans) {
            Some(s) => s,
            None => continue,
        };

        let mut genai = GenAiSpanInfo::from_attributes(&root.attributes);
        let mut ttft = extract_ttft_secs(&root.attributes);
        let duration_ms = (root.end_time - root.start_time) / 1_000_000;
        let is_error = root.status.code == SpanStatusCode::Error;
        let is_stall = is_error && ttft.is_some() && duration_ms > 30_000;

        // Claude Code traces use claude_code.interaction as the root span with
        // claude_code.llm_request children. When the root span carries no token
        // counts, aggregate across all LLM child spans so the modal displays
        // real numbers instead of blanks.
        if genai.input_tokens.is_none() && genai.output_tokens.is_none() {
            let llm_spans: Vec<&Span> = spans
                .iter()
                .filter(|s| {
                    s.name.contains("llm_request")
                        || s.attributes.keys().any(|k| k.starts_with("gen_ai."))
                })
                .collect();

            if !llm_spans.is_empty() {
                // Use the first LLM span for scalar fields (model, TTFT, response_id).
                let first = llm_spans[0];
                let first_genai = GenAiSpanInfo::from_attributes(&first.attributes);
                if genai.model.is_none() {
                    genai.model = first_genai.model;
                }
                if genai.response_id.is_none() {
                    genai.response_id = first_genai.response_id;
                }
                if ttft.is_none() {
                    ttft = extract_ttft_secs(&first.attributes);
                }

                // Sum token counts across all LLM spans in the trace (one
                // trace can contain multiple tool-call round-trips).
                let mut total_input: u64 = 0;
                let mut total_output: u64 = 0;
                let mut total_cache_read: u64 = 0;
                let mut total_cache_creation: u64 = 0;
                for s in &llm_spans {
                    let sg = GenAiSpanInfo::from_attributes(&s.attributes);
                    total_input += sg.input_tokens.unwrap_or(0);
                    total_output += sg.output_tokens.unwrap_or(0);
                    total_cache_read += sg.cache_read_tokens.unwrap_or(0);
                    total_cache_creation += sg.cache_creation_tokens.unwrap_or(0);
                }
                if total_input > 0 || total_output > 0 {
                    genai.input_tokens = Some(total_input);
                    genai.output_tokens = Some(total_output);
                }
                if total_cache_read > 0 {
                    genai.cache_read_tokens = Some(total_cache_read);
                }
                if total_cache_creation > 0 {
                    genai.cache_creation_tokens = Some(total_cache_creation);
                }
            }
        }

        let dt = DateTime::<Utc>::from_timestamp_nanos(root.start_time);
        let time_str = dt.with_timezone(&Local).format("%H:%M:%S").to_string();

        // For errored interactions, fetch the api_request_body log to get body_length.
        // prompt.id is available directly on the span attributes.
        let (body_length, prompt_id) = if is_error {
            let log_params = QueryParams {
                trace_id: Some(trace_id.clone()),
                search_text: Some("api_request_body".to_string()),
                limit: Some(1),
                ..Default::default()
            };
            let log_body_len = state
                .storage
                .query_logs(&log_params)
                .await
                .ok()
                .and_then(|logs| logs.into_iter().next())
                .and_then(|log| {
                    log.attributes
                        .get("body_length")
                        .and_then(|v| v.parse::<u64>().ok())
                });
            let pid = root.attributes.get("prompt.id").cloned();
            (log_body_len, pid)
        } else {
            (None, None)
        };

        interactions.push(SessionInteraction {
            index: idx + 1,
            time: time_str,
            model: genai
                .model
                .clone()
                .or_else(|| root.attributes.get("gen_ai.request.model").cloned()),
            input_tokens: genai.input_tokens,
            output_tokens: genai.output_tokens,
            cache_read_tokens: genai.cache_read_tokens,
            cache_creation_tokens: genai.cache_creation_tokens,
            ttft_secs: ttft,
            duration_ms,
            is_error,
            is_stall,
            response_id: genai.response_id.clone(),
            trace_id: trace_id.clone(),
            start_time_ns: root.start_time,
            body_length,
            prompt_id,
        });
    }

    if interactions.is_empty() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse::not_found(format!(
                "GenAI spans for session {}",
                session_id
            ))),
        ));
    }

    let models: Vec<String> = interactions
        .iter()
        .filter_map(|i| i.model.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();

    let first_ts = interactions.first().map(|i| i.start_time_ns).unwrap_or(0);
    let last_ts = interactions.last().map(|i| i.start_time_ns).unwrap_or(0);
    let start_time = DateTime::<Utc>::from_timestamp_nanos(first_ts)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();
    let end_time = DateTime::<Utc>::from_timestamp_nanos(last_ts)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string();

    let error_count = interactions.iter().filter(|i| i.is_error).count();
    let stall_count = interactions.iter().filter(|i| i.is_stall).count();

    let input_series: Vec<u64> = interactions.iter().filter_map(|i| i.input_tokens).collect();
    let context_growth = if input_series.len() >= 2 {
        Some(SessionContextGrowth {
            first_tokens: *input_series.first().unwrap(),
            last_tokens: *input_series.last().unwrap(),
            peak_tokens: *input_series.iter().max().unwrap(),
            interaction_count: interactions.len(),
        })
    } else {
        None
    };

    Ok(Json(SessionDiagnoseResponse {
        session_id,
        models,
        start_time,
        end_time,
        total_interactions: interactions.len(),
        error_count,
        stall_count,
        interactions,
        context_growth,
    }))
}

#[derive(Debug, Deserialize, Default)]
pub struct SessionListQuery {
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

/// GET /api/sessions
///
/// Lists distinct GenAI sessions seen in the time window with summary stats:
/// model(s), interaction count, total tokens, error count, first/last seen.
///
/// Strategy: single `query_spans` over the window for any span carrying
/// `session.id`, group by session.id in memory, aggregate.
pub async fn list_sessions(
    State(state): State<AppState>,
    Query(params): Query<SessionListQuery>,
) -> Result<Json<SessionListResponse>, (StatusCode, Json<ErrorResponse>)> {
    let limit = params.limit.unwrap_or(200);

    // No "exists" predicate available; scan all spans in the window and
    // filter in memory. Limit caps the worst case at 20k spans (typically
    // many fewer once a time window is applied).
    let query = QueryParams {
        start_time: params.start_time,
        end_time: params.end_time,
        limit: Some(20_000),
        ..Default::default()
    };

    let all_spans = state.storage.query_spans(&query).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::internal_error(e.to_string())),
        )
    })?;

    // Group by session.id, then by trace_id (one interaction = one trace).
    let mut by_session: HashMap<String, HashMap<String, Vec<Span>>> = HashMap::new();
    for span in all_spans {
        let sid = match span.attributes.get("session.id") {
            Some(s) if !s.is_empty() => s.clone(),
            _ => continue,
        };
        by_session
            .entry(sid)
            .or_default()
            .entry(span.trace_id.clone())
            .or_default()
            .push(span);
    }

    let mut summaries: Vec<SessionSummary> = Vec::with_capacity(by_session.len());

    for (session_id, by_trace) in by_session {
        let mut models: BTreeSet<String> = BTreeSet::new();
        let mut projects: BTreeSet<String> = BTreeSet::new();
        let mut providers: BTreeSet<String> = BTreeSet::new();
        let mut families: BTreeSet<String> = BTreeSet::new();
        let mut interaction_count = 0usize;
        let mut total_input: u64 = 0;
        let mut total_output: u64 = 0;
        let mut error_count = 0usize;
        let mut first_seen_ns = i64::MAX;
        let mut last_seen_ns = i64::MIN;

        // Filter-bar dimensions collected from the session's spans:
        // opencode carries project.id; provider is gen_ai.system (claude)
        // or gen_ai.provider.name / llm.provider (opencode); the agent
        // family is derived from span name / scope exactly like the
        // storage-layer filter predicates.
        for spans in by_trace.values() {
            for s in spans {
                if let Some(p) = s.attributes.get("project.id") {
                    if !p.is_empty() {
                        projects.insert(p.clone());
                    }
                }
                for key in ["gen_ai.system", "gen_ai.provider.name", "llm.provider"] {
                    if let Some(v) = s.attributes.get(key) {
                        if !v.is_empty() {
                            providers.insert(v.clone());
                        }
                    }
                }
                let scope = s
                    .attributes
                    .get("otel.scope.name")
                    .cloned()
                    .unwrap_or_default();
                if s.name.starts_with("claude_code.")
                    || scope.starts_with("com.anthropic.claude_code")
                {
                    families.insert("claude".to_string());
                }
                if s.name.starts_with("opencode.") || scope == "com.opencode" {
                    families.insert("opencode".to_string());
                }
                if scope == "codex_cli_rs" {
                    families.insert("codex".to_string());
                }
            }
        }

        for (_trace_id, spans) in by_trace {
            let root = match root_llm_span(&spans) {
                Some(s) => s,
                None => continue,
            };
            interaction_count += 1;
            let mut genai = GenAiSpanInfo::from_attributes(&root.attributes);

            // Claude Code: root span (claude_code.interaction) carries no token
            // counts — aggregate across child LLM spans, same as diagnose does.
            if genai.input_tokens.is_none() && genai.output_tokens.is_none() {
                let llm_spans: Vec<&Span> = spans
                    .iter()
                    .filter(|s| {
                        s.name.contains("llm_request")
                            || s.attributes.keys().any(|k| k.starts_with("gen_ai."))
                    })
                    .collect();
                if !llm_spans.is_empty() {
                    let first_genai = GenAiSpanInfo::from_attributes(&llm_spans[0].attributes);
                    if genai.model.is_none() {
                        genai.model = first_genai.model;
                    }
                    let mut t_in: u64 = 0;
                    let mut t_out: u64 = 0;
                    for s in &llm_spans {
                        let sg = GenAiSpanInfo::from_attributes(&s.attributes);
                        t_in += sg.input_tokens.unwrap_or(0);
                        t_out += sg.output_tokens.unwrap_or(0);
                    }
                    if t_in > 0 || t_out > 0 {
                        genai.input_tokens = Some(t_in);
                        genai.output_tokens = Some(t_out);
                    }
                }
            }

            if let Some(m) = genai.model.as_deref().or_else(|| {
                root.attributes
                    .get("gen_ai.request.model")
                    .map(|s| s.as_str())
            }) {
                models.insert(m.to_string());
            }
            if let Some(v) = genai.input_tokens {
                total_input += v;
            }
            if let Some(v) = genai.output_tokens {
                total_output += v;
            }
            if root.status.code == SpanStatusCode::Error {
                error_count += 1;
            }
            if root.start_time < first_seen_ns {
                first_seen_ns = root.start_time;
            }
            if root.start_time > last_seen_ns {
                last_seen_ns = root.start_time;
            }
        }

        if interaction_count == 0 {
            continue;
        }

        summaries.push(SessionSummary {
            session_id,
            models: models.into_iter().collect(),
            projects: projects.into_iter().collect(),
            providers: providers.into_iter().collect(),
            agent_families: families.into_iter().collect(),
            interaction_count,
            total_input_tokens: total_input,
            total_output_tokens: total_output,
            error_count,
            first_seen_ns,
            last_seen_ns,
        });
    }

    // Apply the global filter bar in memory (#135): the list is built from
    // span attributes, so all five dimensions are addressable.
    if let Some(ref agent) = params.agent {
        summaries.retain(|s| s.agent_families.iter().any(|f| f == agent));
    }
    if let Some(ref model) = params.model {
        summaries.retain(|s| s.models.iter().any(|m| m == model));
    }
    if let Some(ref provider) = params.provider {
        summaries.retain(|s| s.providers.iter().any(|p| p == provider));
    }
    if let Some(ref project) = params.project {
        summaries.retain(|s| s.projects.iter().any(|p| p == project));
    }
    if let Some(ref session) = params.session {
        summaries.retain(|s| s.session_id == *session);
    }

    // Sort newest-first by last_seen, then truncate.
    summaries.sort_by_key(|s| std::cmp::Reverse(s.last_seen_ns));
    let total = summaries.len();
    summaries.truncate(limit);

    let filters = GenAiFilters {
        agent: params.agent.clone(),
        model: params.model.clone(),
        models: None,
        provider: params.provider.clone(),
        project: params.project.clone(),
        session: params.session.clone(),
    };

    Ok(Json(SessionListResponse {
        sessions: summaries,
        total,
        filters_applied: filters.applied(&FILTER_DIMENSIONS),
    }))
}

/// Query parameters for GET /api/sessions/costs.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct SessionCostsQuery {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    /// Max sessions to return (default 50, cap 500). The anomaly flag is
    /// computed over the full window before truncation.
    pub limit: Option<usize>,
}

/// GET /api/sessions/costs
///
/// Per-session cost, tokens and duration for opencode and claude sessions
/// in the window, sorted by cost descending. opencode's cost is its own
/// cumulative session-cost counter ("actual"); claude is estimated from
/// span token counts x pricing ("estimated") because its cost counter
/// under-reports. A session is anomalous when its cost exceeds three times
/// the median session cost (formula in `anomaly_rule`).
#[utoipa::path(
    get,
    path = "/api/sessions/costs",
    params(SessionCostsQuery),
    responses(
        (status = 200, description = "Per-session costs, sorted by cost descending", body = otelite_core::api::SessionCostResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_session_costs(
    State(state): State<AppState>,
    Query(params): Query<SessionCostsQuery>,
) -> Result<Json<otelite_core::api::SessionCostResponse>, (StatusCode, Json<ErrorResponse>)> {
    use otelite_core::session_cost;

    let limit = params.limit.unwrap_or(50).min(500);

    let (rows, quality_map) = tokio::try_join!(
        state
            .storage
            .query_session_costs(params.start_time, params.end_time),
        state
            .storage
            .query_session_quality_map(params.start_time, params.end_time),
    )
    .map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::storage_error(format!(
                "query session costs: {e}"
            ))),
        )
    })?;

    let pricing = state.pricing.snapshot().await;
    let mut sessions =
        session_cost::build_session_costs_with_quality(rows, &pricing.db, &quality_map);
    let median = session_cost::apply_anomaly_flags(&mut sessions).map(|(m, _)| m);
    sessions.truncate(limit);

    Ok(Json(otelite_core::api::SessionCostResponse {
        sessions,
        median_cost_usd: median,
        anomaly_rule: session_cost::ANOMALY_RULE.to_string(),
        // Per-session costs come from opencode counters + claude token
        // pricing; the bar's dimensions aren't addressable across both.
        filters_applied: Vec::new(),
    }))
}

/// Query parameters for GET /api/sessions/cost-distribution.
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct SessionCostDistributionQuery {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    /// Number of log-spaced buckets (default 20, cap 100).
    pub buckets: Option<usize>,
}

/// GET /api/sessions/cost-distribution
///
/// Log-spaced histogram of per-session costs over the window. Bucket 0
/// covers zero-cost sessions; the remaining buckets span equal decades up
/// to the most expensive session.
#[utoipa::path(
    get,
    path = "/api/sessions/cost-distribution",
    params(SessionCostDistributionQuery),
    responses(
        (status = 200, description = "Log-spaced per-session cost distribution", body = otelite_core::api::CostDistributionResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    )
)]
pub async fn get_session_cost_distribution(
    State(state): State<AppState>,
    Query(params): Query<SessionCostDistributionQuery>,
) -> Result<Json<otelite_core::api::CostDistributionResponse>, (StatusCode, Json<ErrorResponse>)> {
    use otelite_core::session_cost;

    let buckets = params.buckets.unwrap_or(20).min(100);

    let rows = state
        .storage
        .query_session_costs(params.start_time, params.end_time)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query session cost distribution: {e}"
                ))),
            )
        })?;

    let pricing = state.pricing.snapshot().await;
    let sessions = session_cost::build_session_costs(rows, &pricing.db);

    Ok(Json(session_cost::build_cost_distribution(
        &sessions, buckets,
    )))
}

/// GET /api/sessions/:session_id/context
///
/// Everything observed for one session on one timeline (issue #134):
/// spans and logs truncated to `limit` (true counts in `*_total`) and
/// per-name metric aggregates. 404 when the session has no data in any
/// of the three stores.
#[utoipa::path(
    get,
    path = "/api/sessions/{session_id}/context",
    params(
        ("session_id" = String, Path, description = "Session id"),
        SessionContextQuery,
    ),
    responses(
        (status = 200, description = "Session context", body = otelite_core::api::SessionContextResponse),
        (status = 400, description = "Invalid query parameters"),
        (status = 404, description = "No data for this session", body = ErrorResponse),
        (status = 500, description = "Internal error", body = ErrorResponse),
    )
)]
pub async fn get_session_context(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Query(params): Query<SessionContextQuery>,
) -> Result<Json<otelite_core::api::SessionContextResponse>, (StatusCode, Json<ErrorResponse>)> {
    // No "exists" check needed: the storage layer returns None when the
    // session has no data in spans, logs or metrics.
    let limit = params.limit.unwrap_or(500).min(5000) as u64;
    let resp = state
        .storage
        .query_session_context(&session_id, params.start_time, params.end_time, limit)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "query session context: {e}"
                ))),
            )
        })?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse::not_found(format!("session {}", session_id))),
            )
        })?;

    Ok(Json(resp))
}

#[derive(Debug, Deserialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct SessionContextQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Max spans and logs to return (default 500, cap 5000)
    pub limit: Option<usize>,
}
