use crate::server::{AppState, QueryCache};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use rotel_core::telemetry::{LogRecord, Resource};
use rotel_storage::QueryParams;
use serde::{Deserialize, Serialize};

/// Query parameters for log listing
#[derive(Debug, Deserialize, Serialize)]
pub struct LogsQuery {
    /// Filter by severity level (e.g., "ERROR", "WARN", "INFO")
    #[serde(default)]
    pub severity: Option<String>,

    /// Filter by resource attribute (e.g., "service.name=my-service")
    #[serde(default)]
    pub resource: Option<String>,

    /// Full-text search in log body
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

/// Response for log listing
#[derive(Debug, Serialize)]
pub struct LogsResponse {
    pub logs: Vec<LogEntry>,
    pub total: usize,
    pub limit: usize,
    pub offset: usize,
}

/// Individual log entry for API response
#[derive(Debug, Serialize)]
pub struct LogEntry {
    pub timestamp: i64,
    pub severity: String,
    pub severity_text: Option<String>,
    pub body: String,
    pub attributes: std::collections::HashMap<String, String>,
    pub resource: Option<Resource>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
}

impl From<LogRecord> for LogEntry {
    fn from(log: LogRecord) -> Self {
        Self {
            timestamp: log.timestamp,
            severity: log.severity.as_str().to_string(),
            severity_text: log.severity_text,
            body: log.body,
            attributes: log.attributes,
            resource: log.resource,
            trace_id: log.trace_id,
            span_id: log.span_id,
        }
    }
}

/// Handler for GET /api/logs
pub async fn list_logs(
    State(state): State<AppState>,
    Query(params): Query<LogsQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Check cache first
    let cache_key = QueryCache::make_key(&params);
    if let Some(cached) = state.cache.logs.get(&cache_key) {
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
    let mut query = QueryParams {
        start_time: params.start_time,
        end_time: params.end_time,
        limit: Some(limit),
        search_text: params.search.clone(),
        ..Default::default()
    };

    // Parse severity filter if provided
    if let Some(severity_str) = &params.severity {
        query.min_severity = parse_severity(severity_str);
    }

    // Query logs from storage
    let logs = state
        .storage
        .query_logs(&query)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Filter by resource if specified (post-query filtering for now)
    let filtered_logs: Vec<LogRecord> = if let Some(resource_filter) = &params.resource {
        logs.into_iter()
            .filter(|log| matches_resource_filter(log, resource_filter))
            .collect()
    } else {
        logs
    };

    // Apply offset for pagination
    let total = filtered_logs.len();
    let paginated_logs: Vec<LogRecord> = filtered_logs
        .into_iter()
        .skip(params.offset)
        .take(limit)
        .collect();

    // Convert to API format
    let log_entries: Vec<LogEntry> = paginated_logs.into_iter().map(LogEntry::from).collect();

    let response = LogsResponse {
        logs: log_entries,
        total,
        limit,
        offset: params.offset,
    };

    // Cache the response
    if let Ok(json) = serde_json::to_string(&response) {
        state.cache.logs.insert(cache_key, json.clone());
        Ok((StatusCode::OK, [("content-type", "application/json")], json).into_response())
    } else {
        Ok(Json(response).into_response())
    }
}

/// Handler for GET /api/logs/:timestamp
/// Note: Using timestamp as ID since LogRecord doesn't have a separate ID field
pub async fn get_log(
    State(state): State<AppState>,
    Path(timestamp): Path<i64>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Query logs around this timestamp
    let query = QueryParams {
        start_time: Some(timestamp),
        end_time: Some(timestamp + 1),
        limit: Some(1),
        ..Default::default()
    };

    let logs = state
        .storage
        .query_logs(&query)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let log = logs
        .into_iter()
        .next()
        .ok_or_else(|| (StatusCode::NOT_FOUND, "Log not found".to_string()))?;

    Ok(Json(LogEntry::from(log)))
}

/// Export format for logs
#[derive(Debug, Deserialize)]
pub struct ExportQuery {
    /// Export format: "json" or "csv"
    #[serde(default = "default_format")]
    pub format: String,

    /// Same filters as LogsQuery
    #[serde(flatten)]
    pub filters: LogsQuery,
}

fn default_format() -> String {
    "json".to_string()
}

/// Handler for GET /api/logs/export
pub async fn export_logs(
    State(state): State<AppState>,
    Query(params): Query<ExportQuery>,
) -> Result<impl IntoResponse, (StatusCode, String)> {
    // Build query parameters (no limit for export, but cap at 10000)
    let mut query = QueryParams {
        start_time: params.filters.start_time,
        end_time: params.filters.end_time,
        limit: Some(10000),
        search_text: params.filters.search.clone(),
        ..Default::default()
    };

    if let Some(severity_str) = &params.filters.severity {
        query.min_severity = parse_severity(severity_str);
    }

    let logs = state
        .storage
        .query_logs(&query)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Filter by resource if specified
    let filtered_logs: Vec<LogRecord> = if let Some(resource_filter) = &params.filters.resource {
        logs.into_iter()
            .filter(|log| matches_resource_filter(log, resource_filter))
            .collect()
    } else {
        logs
    };

    match params.format.as_str() {
        "json" => {
            let log_entries: Vec<LogEntry> =
                filtered_logs.into_iter().map(LogEntry::from).collect();

            Ok((
                [
                    ("Content-Type", "application/json"),
                    ("Content-Disposition", "attachment; filename=\"logs.json\""),
                ],
                Json(log_entries),
            )
                .into_response())
        },
        "csv" => {
            // Simple CSV export
            let mut csv = String::from("timestamp,severity,body,trace_id,span_id\n");
            for log in filtered_logs {
                csv.push_str(&format!(
                    "{},{},{},{},{}\n",
                    log.timestamp,
                    log.severity.as_str(),
                    log.body.replace(',', ";").replace('\n', " "),
                    log.trace_id.unwrap_or_default(),
                    log.span_id.unwrap_or_default(),
                ));
            }

            Ok((
                [
                    ("Content-Type", "text/csv"),
                    ("Content-Disposition", "attachment; filename=\"logs.csv\""),
                ],
                csv,
            )
                .into_response())
        },
        _ => Err((
            StatusCode::BAD_REQUEST,
            "Invalid format. Use 'json' or 'csv'".to_string(),
        )),
    }
}

/// Parse severity string to SeverityLevel
fn parse_severity(s: &str) -> Option<rotel_core::telemetry::log::SeverityLevel> {
    use rotel_core::telemetry::log::SeverityLevel;
    match s.to_uppercase().as_str() {
        "TRACE" => Some(SeverityLevel::Trace),
        "DEBUG" => Some(SeverityLevel::Debug),
        "INFO" => Some(SeverityLevel::Info),
        "WARN" => Some(SeverityLevel::Warn),
        "ERROR" => Some(SeverityLevel::Error),
        "FATAL" => Some(SeverityLevel::Fatal),
        _ => None,
    }
}

/// Check if log matches resource filter (simple key=value matching)
fn matches_resource_filter(log: &LogRecord, filter: &str) -> bool {
    if let Some(resource) = &log.resource {
        if let Some((key, value)) = filter.split_once('=') {
            return resource.attributes.get(key).is_some_and(|v| v == value);
        }
    }
    false
}

// Made with Bob
