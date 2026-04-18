//! Trace query handlers

use axum::{
    extract::{Path, Query},
    Json,
};

use crate::{
    error::{ApiError, ApiResult},
    models::{
        trace::{SpanEvent, SpanStatus, Trace, TraceQueryParams, TraceSpan},
        ListResponse, PaginationMetadata, ResourceAttributes,
    },
};

/// Handler for GET /api/v1/traces - List traces with filtering and pagination
///
/// Query traces with optional filtering by service name, span name, duration,
/// status, and time ranges. Results are paginated and sorted by start time (newest first).
///
/// # Query Parameters
/// - `service_name`: Filter by service name
/// - `span_name`: Filter by span name
/// - `min_duration_ns`: Minimum duration in nanoseconds
/// - `max_duration_ns`: Maximum duration in nanoseconds
/// - `status`: Filter by status (OK, ERROR, UNSET)
/// - `start_time`: Start of time range (Unix timestamp in ms)
/// - `end_time`: End of time range (Unix timestamp in ms)
/// - `since`: Relative time range (e.g., "1h", "30m", "7d")
/// - `offset`: Pagination offset (default: 0)
/// - `limit`: Number of results per page (default: 100, max: 1000)
///
/// # Example
/// ```text
/// GET /api/v1/traces?service_name=rotel-api&min_duration_ns=1000000&limit=50
/// ```text
#[utoipa::path(
    get,
    path = "/api/v1/traces",
    params(TraceQueryParams),
    responses(
        (status = 200, description = "List of traces", body = ListResponse<Trace>),
        (status = 400, description = "Invalid query parameters"),
        (status = 500, description = "Internal server error")
    ),
    tag = "traces"
)]
pub async fn list_traces(
    Query(params): Query<TraceQueryParams>,
) -> ApiResult<Json<ListResponse<Trace>>> {
    // Validate parameters
    if let Err(e) = validator::Validate::validate(&params) {
        return Err(ApiError::ValidationError(e.to_string()));
    }

    // TODO: Query storage backend
    // For now, return mock data
    let traces = vec![
        create_mock_trace("trace-1", "rotel-api", 3, 150_000_000),
        create_mock_trace("trace-2", "rotel-worker", 5, 250_000_000),
        create_mock_trace("trace-3", "rotel-api", 2, 50_000_000),
    ];

    // Apply filters
    let filtered_traces: Vec<Trace> = traces
        .into_iter()
        .filter(|trace| {
            // Filter by service name
            if let Some(ref service_name) = params.service_name {
                if !trace
                    .root_span
                    .resource
                    .service_name
                    .as_ref()
                    .map(|s| s.eq_ignore_ascii_case(service_name))
                    .unwrap_or(false)
                {
                    return false;
                }
            }

            // Filter by span name
            if let Some(ref span_name) = params.span_name {
                if !trace
                    .spans
                    .iter()
                    .any(|s| s.name.eq_ignore_ascii_case(span_name))
                {
                    return false;
                }
            }

            // Filter by minimum duration
            if let Some(min_duration) = params.min_duration_ns {
                if trace.duration_ns < min_duration {
                    return false;
                }
            }

            // Filter by maximum duration
            if let Some(max_duration) = params.max_duration_ns {
                if trace.duration_ns > max_duration {
                    return false;
                }
            }

            // Filter by status
            if let Some(ref status) = params.status {
                if !trace
                    .spans
                    .iter()
                    .any(|s| s.status.code.eq_ignore_ascii_case(status))
                {
                    return false;
                }
            }

            true
        })
        .collect();

    let total = filtered_traces.len();

    // Apply pagination
    let paginated_traces: Vec<Trace> = filtered_traces
        .into_iter()
        .skip(params.offset)
        .take(params.limit)
        .collect();

    let count = paginated_traces.len();

    let pagination = PaginationMetadata::new(total, params.offset, params.limit, count);

    Ok(Json(ListResponse::new(paginated_traces, pagination)))
}

/// Handler for GET /api/v1/traces/{id} - Get trace details by ID
///
/// Retrieve complete trace information including all spans and their hierarchy.
///
/// # Path Parameters
/// - `id`: Unique trace identifier
///
/// # Example
/// ```text
/// GET /api/v1/traces/trace-123
/// ```text
#[utoipa::path(
    get,
    path = "/api/v1/traces/{id}",
    params(
        ("id" = String, Path, description = "Trace ID")
    ),
    responses(
        (status = 200, description = "Trace details with all spans", body = Trace),
        (status = 404, description = "Trace not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "traces"
)]
pub async fn get_trace(Path(id): Path<String>) -> ApiResult<Json<Trace>> {
    // TODO: Query storage backend
    // For now, return mock data or 404
    if id == "trace-1" {
        Ok(Json(create_mock_trace(&id, "rotel-api", 3, 150_000_000)))
    } else {
        Err(ApiError::NotFound(format!(
            "Trace with ID '{}' not found",
            id
        )))
    }
}

/// Create a mock trace for testing
fn create_mock_trace(
    trace_id: &str,
    service_name: &str,
    span_count: usize,
    duration_ns: i64,
) -> Trace {
    use std::collections::HashMap;

    let base_time = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);
    let resource = ResourceAttributes {
        service_name: Some(service_name.to_string()),
        service_version: Some("0.1.0".to_string()),
        service_instance_id: Some("instance-1".to_string()),
        attributes: HashMap::new(),
    };

    // Create root span
    let root_span = TraceSpan {
        span_id: format!("{}-span-0", trace_id),
        trace_id: trace_id.to_string(),
        parent_span_id: None,
        name: "HTTP GET /api/endpoint".to_string(),
        kind: "SERVER".to_string(),
        start_time: base_time,
        end_time: base_time + duration_ns,
        duration_ns,
        status: SpanStatus {
            code: "OK".to_string(),
            message: None,
        },
        resource: resource.clone(),
        attributes: {
            let mut attrs = HashMap::new();
            attrs.insert("http.method".to_string(), "GET".to_string());
            attrs.insert("http.route".to_string(), "/api/endpoint".to_string());
            attrs.insert("http.status_code".to_string(), "200".to_string());
            attrs
        },
        events: vec![SpanEvent {
            name: "request.received".to_string(),
            timestamp: base_time,
            attributes: HashMap::new(),
        }],
        links: vec![],
    };

    // Create child spans
    let mut spans = vec![root_span.clone()];
    let child_duration = duration_ns / (span_count as i64);

    for i in 1..span_count {
        let span_start = base_time + (i as i64 * child_duration);
        let span = TraceSpan {
            span_id: format!("{}-span-{}", trace_id, i),
            trace_id: trace_id.to_string(),
            parent_span_id: Some(format!("{}-span-{}", trace_id, i - 1)),
            name: format!("operation-{}", i),
            kind: "INTERNAL".to_string(),
            start_time: span_start,
            end_time: span_start + child_duration,
            duration_ns: child_duration,
            status: SpanStatus {
                code: "OK".to_string(),
                message: None,
            },
            resource: resource.clone(),
            attributes: {
                let mut attrs = HashMap::new();
                attrs.insert("operation.type".to_string(), format!("type-{}", i));
                attrs
            },
            events: vec![],
            links: vec![],
        };
        spans.push(span);
    }

    Trace {
        trace_id: trace_id.to_string(),
        root_span,
        spans,
        duration_ns,
        span_count,
        start_time: base_time,
        end_time: base_time + duration_ns,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_list_traces() {
        let params = TraceQueryParams::default();
        let result = list_traces(Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.items.len(), 3);
        assert_eq!(response.pagination.total, 3);
    }

    #[tokio::test]
    async fn test_list_traces_with_service_filter() {
        let params = TraceQueryParams {
            service_name: Some("rotel-api".to_string()),
            ..Default::default()
        };
        let result = list_traces(Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert_eq!(response.items.len(), 2);
        for trace in &response.items {
            assert_eq!(
                trace.root_span.resource.service_name.as_deref(),
                Some("rotel-api")
            );
        }
    }

    #[tokio::test]
    async fn test_list_traces_with_duration_filter() {
        let params = TraceQueryParams {
            min_duration_ns: Some(100_000_000),
            ..Default::default()
        };
        let result = list_traces(Query(params)).await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        for trace in &response.items {
            assert!(trace.duration_ns >= 100_000_000);
        }
    }

    #[tokio::test]
    async fn test_get_trace_found() {
        let result = get_trace(Path("trace-1".to_string())).await;
        assert!(result.is_ok());

        let trace = result.unwrap().0;
        assert_eq!(trace.trace_id, "trace-1");
        assert!(trace.span_count > 0);
    }

    #[tokio::test]
    async fn test_get_trace_not_found() {
        let result = get_trace(Path("nonexistent".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_trace_hierarchy() {
        let trace = create_mock_trace("test-trace", "test-service", 5, 1_000_000_000);

        // Verify root span has no parent
        assert!(trace.root_span.parent_span_id.is_none());

        // Verify all spans belong to same trace
        for span in &trace.spans {
            assert_eq!(span.trace_id, "test-trace");
        }

        // Verify span count
        assert_eq!(trace.span_count, 5);
        assert_eq!(trace.spans.len(), 5);
    }
}

// Made with Bob
