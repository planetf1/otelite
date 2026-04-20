use crate::server::{AppState, QueryCache};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use rotel_core::api::{SpanEntry, TraceDetail, TraceEntry, TracesResponse};
use rotel_core::telemetry::Span;
use rotel_storage::QueryParams;
use serde::{Deserialize, Serialize};

/// Query parameters for trace listing
#[derive(Debug, Deserialize, Serialize)]
pub struct TracesQuery {
    /// Filter by trace ID
    #[serde(default)]
    pub trace_id: Option<String>,

    /// Filter by service name
    #[serde(default)]
    pub service: Option<String>,

    /// Full-text search in span names
    #[serde(default)]
    pub search: Option<String>,

    /// Start time (Unix timestamp in nanoseconds)
    #[serde(default)]
    pub start_time: Option<i64>,

    /// End time (Unix timestamp in nanoseconds)
    #[serde(default)]
    pub end_time: Option<i64>,

    /// Maximum number of results (default: 100, max: 1000)
    #[serde(default = "default_limit")]
    pub limit: usize,

    /// Offset for pagination
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    100
}

/// Handler for GET /api/traces
pub async fn list_traces(
    State(state): State<AppState>,
    Query(params): Query<TracesQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Check cache first
    let cache_key = QueryCache::make_key(&params);
    if let Some(cached) = state.cache.traces.get(&cache_key) {
        return Ok((
            StatusCode::OK,
            [("content-type", "application/json")],
            cached,
        )
            .into_response());
    }

    // Validate and cap limit
    let limit = params.limit.min(1000);

    // Build query parameters
    let query = QueryParams {
        start_time: params.start_time,
        end_time: params.end_time,
        limit: Some(limit * 10), // Get more spans to aggregate into traces
        trace_id: params.trace_id.clone(),
        ..Default::default()
    };

    // Query spans from storage
    let spans = state
        .storage
        .query_spans(&query)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Group spans by trace_id
    let mut traces_map: std::collections::HashMap<String, Vec<Span>> =
        std::collections::HashMap::new();
    for span in spans {
        traces_map
            .entry(span.trace_id.clone())
            .or_default()
            .push(span);
    }

    // Convert to trace entries
    let mut trace_entries: Vec<TraceEntry> = traces_map
        .into_iter()
        .map(|(trace_id, spans)| {
            let start_time = spans.iter().map(|s| s.start_time).min().unwrap_or(0);
            let end_time = spans.iter().map(|s| s.end_time).max().unwrap_or(0);
            let duration = end_time - start_time;

            let root_span = spans
                .iter()
                .find(|s| s.parent_span_id.is_none())
                .or_else(|| spans.first());

            let root_span_name = root_span
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Unknown".to_string());

            // Note: Resource is on Trace, not individual Spans in rotel-core
            // For now, we'll use empty service names until we can access trace-level resource
            let service_names: Vec<String> = Vec::new();

            let has_errors = spans.iter().any(|s| {
                matches!(
                    s.status.code,
                    rotel_core::telemetry::trace::StatusCode::Error
                )
            });

            TraceEntry {
                trace_id,
                root_span_name,
                start_time,
                duration,
                span_count: spans.len(),
                service_names,
                has_errors,
            }
        })
        .collect();

    // Sort by start time (newest first)
    trace_entries.sort_by_key(|b| std::cmp::Reverse(b.start_time));

    // Apply pagination
    let total = trace_entries.len();
    let paginated_traces: Vec<TraceEntry> = trace_entries
        .into_iter()
        .skip(params.offset)
        .take(limit)
        .collect();

    let response = TracesResponse {
        traces: paginated_traces,
        total,
        limit,
        offset: params.offset,
    };

    // Cache the response
    if let Ok(json) = serde_json::to_string(&response) {
        state.cache.traces.insert(cache_key, json.clone());
        Ok((StatusCode::OK, [("content-type", "application/json")], json).into_response())
    } else {
        Ok(Json(response).into_response())
    }
}

/// Handler for GET /api/traces/:trace_id
pub async fn get_trace(
    State(state): State<AppState>,
    Path(trace_id): Path<String>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Query all spans for this trace
    let query = QueryParams {
        trace_id: Some(trace_id.clone()),
        limit: Some(1000), // Max spans per trace
        ..Default::default()
    };

    let spans = state
        .storage
        .query_spans(&query)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if spans.is_empty() {
        return Err((StatusCode::NOT_FOUND, "Trace not found".to_string()));
    }

    let start_time = spans.iter().map(|s| s.start_time).min().unwrap_or(0);
    let end_time = spans.iter().map(|s| s.end_time).max().unwrap_or(0);
    let duration = end_time - start_time;

    // Note: Resource is on Trace, not individual Spans in rotel-core
    // For now, we'll use empty service names until we can access trace-level resource
    let service_names: Vec<String> = Vec::new();

    let span_entries: Vec<SpanEntry> = spans.into_iter().map(SpanEntry::from).collect();

    let span_count = span_entries.len();

    let trace_detail = TraceDetail {
        trace_id,
        spans: span_entries,
        start_time,
        end_time,
        duration,
        span_count,
        service_names,
    };

    Ok(Json(trace_detail))
}

/// Export format for traces
#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    /// Export format: "json"
    #[serde(default = "default_format")]
    pub format: String,

    /// Same filters as TracesQuery
    #[serde(flatten)]
    pub filters: TracesQuery,
}

fn default_format() -> String {
    "json".to_string()
}

/// Handler for GET /api/traces/export
pub async fn export_traces(
    State(state): State<AppState>,
    Query(params): Query<ExportQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Build query parameters (no limit for export, but cap at 10000)
    let query = QueryParams {
        start_time: params.filters.start_time,
        end_time: params.filters.end_time,
        limit: Some(10000),
        trace_id: params.filters.trace_id.clone(),
        ..Default::default()
    };

    let spans = state
        .storage
        .query_spans(&query)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    match params.format.as_str() {
        "json" => {
            let span_entries: Vec<SpanEntry> = spans.into_iter().map(SpanEntry::from).collect();

            Ok((
                [
                    ("Content-Type", "application/json"),
                    (
                        "Content-Disposition",
                        "attachment; filename=\"traces.json\"",
                    ),
                ],
                Json(span_entries),
            )
                .into_response())
        },
        _ => Err((
            StatusCode::BAD_REQUEST,
            "Invalid format. Use 'json'".to_string(),
        )),
    }
}

// Made with Bob
