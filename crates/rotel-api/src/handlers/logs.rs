//! Log query handlers

use axum::{
    extract::{Path, Query},
    Json,
};

use crate::{
    error::{ApiError, ApiResult},
    models::{
        request::LogQueryParams,
        response::{ListResponse, LogEntry},
        PaginationMetadata,
    },
};

/// Handler for GET /api/v1/logs - List logs with filtering and pagination
///
/// Query logs with optional filtering by severity, search text, trace/span IDs,
/// and time ranges. Results are paginated and sorted by timestamp (newest first).
///
/// # Query Parameters
/// - `severity`: Filter by log severity level (e.g., "ERROR", "WARN", "INFO")
/// - `search`: Full-text search in log body
/// - `trace_id`: Filter by trace ID
/// - `span_id`: Filter by span ID
/// - `start_time`: Start of time range (Unix timestamp in ms)
/// - `end_time`: End of time range (Unix timestamp in ms)
/// - `since`: Relative time range (e.g., "1h", "30m", "7d")
/// - `offset`: Pagination offset (default: 0)
/// - `limit`: Number of results per page (default: 100, max: 1000)
///
/// # Example
/// ```text
/// GET /api/v1/logs?severity=ERROR&limit=50&since=1h
/// ```text
#[utoipa::path(
    get,
    path = "/api/v1/logs",
    params(LogQueryParams),
    responses(
        (status = 200, description = "List of logs", body = ListResponse<LogEntry>),
        (status = 400, description = "Invalid query parameters"),
        (status = 500, description = "Internal server error")
    ),
    tag = "logs"
)]
pub async fn list_logs(
    Query(params): Query<LogQueryParams>,
) -> ApiResult<Json<ListResponse<LogEntry>>> {
    // Validate parameters
    if let Err(e) = validator::Validate::validate(&params) {
        return Err(ApiError::ValidationError(e.to_string()));
    }

    // TODO: Query storage backend
    // For now, return mock data
    let logs = vec![
        create_mock_log("log-1", "INFO", "Application started"),
        create_mock_log("log-2", "DEBUG", "Processing request"),
        create_mock_log("log-3", "ERROR", "Connection failed"),
    ];

    // Apply filters
    let filtered_logs: Vec<LogEntry> = logs
        .into_iter()
        .filter(|log| {
            // Filter by severity
            if let Some(ref severity) = params.severity {
                if !log.severity.eq_ignore_ascii_case(severity) {
                    return false;
                }
            }

            // Filter by search term
            if let Some(ref search) = params.search {
                if !log.message.to_lowercase().contains(&search.to_lowercase()) {
                    return false;
                }
            }

            // Filter by trace_id
            if let Some(ref trace_id) = params.trace_id {
                if log.trace_id.as_ref() != Some(trace_id) {
                    return false;
                }
            }

            // Filter by span_id
            if let Some(ref span_id) = params.span_id {
                if log.span_id.as_ref() != Some(span_id) {
                    return false;
                }
            }

            true
        })
        .collect();

    let total = filtered_logs.len();

    // Apply pagination
    let paginated_logs: Vec<LogEntry> = filtered_logs
        .into_iter()
        .skip(params.offset)
        .take(params.limit)
        .collect();

    let count = paginated_logs.len();

    let pagination = PaginationMetadata::new(total, params.offset, params.limit, count);

    Ok(Json(ListResponse::new(paginated_logs, pagination)))
}

/// Handler for GET /api/v1/logs/{id} - Get log details by ID
///
/// Retrieve detailed information about a specific log entry.
///
/// # Path Parameters
/// - `id`: Unique log entry identifier
///
/// # Example
/// ```text
/// GET /api/v1/logs/log-123
/// ```text
#[utoipa::path(
    get,
    path = "/api/v1/logs/{id}",
    params(
        ("id" = String, Path, description = "Log entry ID")
    ),
    responses(
        (status = 200, description = "Log entry details", body = LogEntry),
        (status = 404, description = "Log not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "logs"
)]
pub async fn get_log(Path(id): Path<String>) -> ApiResult<Json<LogEntry>> {
    // TODO: Query storage backend
    // For now, return mock data or 404
    if id == "log-1" {
        Ok(Json(create_mock_log(&id, "INFO", "Application started")))
    } else {
        Err(ApiError::NotFound(format!(
            "Log with ID '{}' not found",
            id
        )))
    }
}

/// Create a mock log entry for testing
fn create_mock_log(id: &str, severity: &str, message: &str) -> LogEntry {
    use std::collections::HashMap;

    LogEntry {
        id: id.to_string(),
        timestamp: chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0),
        severity: severity.to_string(),
        message: message.to_string(),
        resource: crate::models::response::ResourceAttributes {
            service_name: Some("rotel-api".to_string()),
            service_version: Some("0.1.0".to_string()),
            service_instance_id: Some("instance-1".to_string()),
            attributes: HashMap::new(),
        },
        attributes: HashMap::new(),
        trace_id: None,
        span_id: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_logs() {
        let params = LogQueryParams::default();
        let result = list_logs(Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.items.len(), 3);
        assert_eq!(response.pagination.total, 3);
    }

    #[tokio::test]
    async fn test_list_logs_with_severity_filter() {
        let params = LogQueryParams {
            severity: Some("ERROR".to_string()),
            ..Default::default()
        };
        let result = list_logs(Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.items.len(), 1);
        assert_eq!(response.items[0].severity, "ERROR");
    }

    #[tokio::test]
    async fn test_list_logs_with_search() {
        let params = LogQueryParams {
            search: Some("request".to_string()),
            ..Default::default()
        };
        let result = list_logs(Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.items.len(), 1);
        assert!(response.items[0].message.contains("request"));
    }

    #[tokio::test]
    async fn test_get_log_found() {
        let result = get_log(Path("log-1".to_string())).await;
        assert!(result.is_ok());

        let log = result.unwrap().0;
        assert_eq!(log.id, "log-1");
    }

    #[tokio::test]
    async fn test_get_log_not_found() {
        let result = get_log(Path("nonexistent".to_string())).await;
        assert!(result.is_err());
    }
}

// Made with Bob
