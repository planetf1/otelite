use axum::http::StatusCode;

/// Plain-text API help for AI agents
#[utoipa::path(
    get,
    path = "/api/help",
    responses(
        (status = 200, description = "Plain-text API reference")
    ),
    tag = "help"
)]
pub async fn api_help() -> (StatusCode, &'static str) {
    (StatusCode::OK, HELP_TEXT)
}

const HELP_TEXT: &str = r#"Rotel API Quick Reference

Base URL: http://localhost:3000/api

=== Health ===
GET /health
  Returns service health status

=== Logs ===
GET /logs?severity=ERROR&search=timeout&limit=100&offset=0
  Query parameters:
    - severity: Filter by severity (TRACE, DEBUG, INFO, WARN, ERROR, FATAL)
    - resource: Filter by resource attribute (format: key=value)
    - search: Full-text search in log body
    - start_time: Start time (Unix timestamp in nanoseconds)
    - end_time: End time (Unix timestamp in nanoseconds)
    - limit: Maximum results (default: 100, max: 1000)
    - offset: Pagination offset (default: 0)

GET /logs/{timestamp}
  Get a specific log entry by timestamp

GET /logs/export?format=json&severity=ERROR
  Export logs in JSON or CSV format
  Query parameters: same as /logs plus format (json or csv)

=== Traces ===
GET /traces?trace_id=abc123&service=my-service&limit=100
  Query parameters:
    - trace_id: Filter by trace ID
    - service: Filter by service name
    - search: Full-text search in span names
    - start_time: Start time (Unix timestamp in nanoseconds)
    - end_time: End time (Unix timestamp in nanoseconds)
    - limit: Maximum results (default: 100, max: 1000)
    - offset: Pagination offset (default: 0)

GET /traces/{trace_id}
  Get detailed trace with all spans

GET /traces/export?format=json
  Export traces in JSON format
  Query parameters: same as /traces

=== Metrics ===
GET /metrics?name=http_requests&limit=100
  Query parameters:
    - name: Filter by metric name (partial match)
    - resource: Filter by resource attribute (format: key=value)
    - start_time: Start time (Unix timestamp in nanoseconds)
    - end_time: End time (Unix timestamp in nanoseconds)
    - limit: Maximum results (default: 100)
    - offset: Pagination offset (default: 0)

GET /metrics/names
  Get list of unique metric names

GET /metrics/aggregate?name=http_requests&function=sum&bucket_size=60
  Aggregate metrics by function
  Query parameters:
    - name: Metric name to aggregate (required)
    - function: Aggregation function (sum, avg, min, max) (required)
    - bucket_size: Time bucket size in seconds (optional, for time-series)
    - start_time: Start time (Unix timestamp in nanoseconds)
    - end_time: End time (Unix timestamp in nanoseconds)

GET /metrics/export?name=http_requests
  Export metrics in JSON format
  Query parameters: same as /metrics

=== Documentation ===
GET /openapi.json
  OpenAPI 3.0 specification in JSON format

GET /docs
  Interactive Swagger UI documentation

GET /help
  This plain-text reference guide

=== Response Format ===
All endpoints return JSON (except /help and exports)
Error responses have format:
{
  "error": "Human-readable message",
  "code": "ERROR_CODE",
  "details": "Optional additional information"
}

=== Common Error Codes ===
- BAD_REQUEST: Invalid query parameters
- NOT_FOUND: Resource not found
- INTERNAL_ERROR: Server error
- STORAGE_ERROR: Database operation failed

=== Time Format ===
All timestamps are Unix nanoseconds (e.g., 1713628800000000000)
To convert from seconds: multiply by 1_000_000_000
"#;
