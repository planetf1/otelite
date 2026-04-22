// HTTP request handlers for OTLP endpoints

use crate::error::ReceiverError;
use crate::http::routes::AppState;
use crate::protocol::{json as json_parser, protobuf};
use axum::{
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use serde_json::json;
use tracing::{debug, error};

/// Health check endpoint handler
pub async fn handle_health(State(state): State<AppState>) -> Response {
    if state.health_checker.is_ready() {
        (StatusCode::OK, Json(json!({"status": "healthy"}))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "unhealthy"})),
        )
            .into_response()
    }
}

/// Metrics endpoint handler
pub async fn handle_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    debug!("Received metrics request: {} bytes", body.len());

    // Determine content type and parse accordingly
    let content_type = match get_content_type(&headers) {
        Ok(ct) => ct,
        Err(e) => return e.into_response(),
    };

    let request: ExportMetricsServiceRequest = match content_type.as_str() {
        "application/x-protobuf" => match protobuf::parse_message(&body) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse protobuf metrics request: {}", e);
                return e.into_response();
            },
        },
        "application/json" => match json_parser::parse_metrics_json(&body) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse JSON metrics request: {}", e);
                return e.into_response();
            },
        },
        _ => {
            return ReceiverError::InvalidContentType(format!(
                "Unsupported Content-Type: {}. Expected application/x-protobuf or application/json",
                content_type
            ))
            .into_response();
        },
    };

    // Process metrics
    match state.metrics_handler.process(request).await {
        Ok(_) => (StatusCode::OK, Json(json!({"status": "success"}))).into_response(),
        Err(e) => {
            error!("Failed to process metrics: {}", e);
            e.into_response()
        },
    }
}

/// Logs endpoint handler
pub async fn handle_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    debug!("Received logs request: {} bytes", body.len());

    // Determine content type and parse accordingly
    let content_type = match get_content_type(&headers) {
        Ok(ct) => ct,
        Err(e) => return e.into_response(),
    };

    let request: ExportLogsServiceRequest = match content_type.as_str() {
        "application/x-protobuf" => match protobuf::parse_message(&body) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse protobuf logs request: {}", e);
                return e.into_response();
            },
        },
        "application/json" => match json_parser::parse_logs_json(&body) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse JSON logs request: {}", e);
                return e.into_response();
            },
        },
        _ => {
            return ReceiverError::InvalidContentType(format!(
                "Unsupported Content-Type: {}. Expected application/x-protobuf or application/json",
                content_type
            ))
            .into_response();
        },
    };

    // Process logs
    match state.logs_handler.process(request).await {
        Ok(_) => (StatusCode::OK, Json(json!({"status": "success"}))).into_response(),
        Err(e) => {
            error!("Failed to process logs: {}", e);
            e.into_response()
        },
    }
}

/// Traces endpoint handler
pub async fn handle_traces(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    debug!("Received traces request: {} bytes", body.len());

    // Determine content type and parse accordingly
    let content_type = match get_content_type(&headers) {
        Ok(ct) => ct,
        Err(e) => return e.into_response(),
    };

    let request: ExportTraceServiceRequest = match content_type.as_str() {
        "application/x-protobuf" => match protobuf::parse_message(&body) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse protobuf traces request: {}", e);
                return e.into_response();
            },
        },
        "application/json" => match json_parser::parse_traces_json(&body) {
            Ok(req) => req,
            Err(e) => {
                error!("Failed to parse JSON traces request: {}", e);
                return e.into_response();
            },
        },
        _ => {
            return ReceiverError::InvalidContentType(format!(
                "Unsupported Content-Type: {}. Expected application/x-protobuf or application/json",
                content_type
            ))
            .into_response();
        },
    };

    // Process traces
    match state.traces_handler.process(request).await {
        Ok(_) => (StatusCode::OK, Json(json!({"status": "success"}))).into_response(),
        Err(e) => {
            error!("Failed to process traces: {}", e);
            e.into_response()
        },
    }
}

/// Unified endpoint handler (legacy support)
/// Routes to appropriate handler based on URL path or content inspection
pub async fn handle_unified(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    debug!("Received unified OTLP request: {} bytes", body.len());

    // Validate Content-Type
    if let Err(e) = validate_content_type(&headers, "application/x-protobuf") {
        return e.into_response();
    }

    // Try to parse as each type and route accordingly
    // In practice, clients should use signal-specific endpoints
    // This is a fallback for legacy clients

    // Try metrics first
    if let Ok(request) = protobuf::parse_message::<ExportMetricsServiceRequest>(&body) {
        return match state.metrics_handler.process(request).await {
            Ok(_) => (StatusCode::OK, Json(json!({"status": "success"}))).into_response(),
            Err(e) => e.into_response(),
        };
    }

    // Try logs
    if let Ok(request) = protobuf::parse_message::<ExportLogsServiceRequest>(&body) {
        return match state.logs_handler.process(request).await {
            Ok(_) => (StatusCode::OK, Json(json!({"status": "success"}))).into_response(),
            Err(e) => e.into_response(),
        };
    }

    // Try traces
    if let Ok(request) = protobuf::parse_message::<ExportTraceServiceRequest>(&body) {
        return match state.traces_handler.process(request).await {
            Ok(_) => (StatusCode::OK, Json(json!({"status": "success"}))).into_response(),
            Err(e) => e.into_response(),
        };
    }

    // Could not parse as any known type
    ReceiverError::InvalidSignalType("Could not parse as any OTLP signal type".to_string())
        .into_response()
}

/// Get and normalize Content-Type header
fn get_content_type(headers: &HeaderMap) -> Result<String, ReceiverError> {
    if let Some(content_type) = headers.get("content-type") {
        let content_type_str = content_type.to_str().map_err(|_| {
            ReceiverError::InvalidContentType("Invalid Content-Type header".to_string())
        })?;

        // Extract base content type (before semicolon for charset, etc.)
        let base_type = content_type_str
            .split(';')
            .next()
            .unwrap_or(content_type_str)
            .trim();

        Ok(base_type.to_string())
    } else {
        Err(ReceiverError::InvalidContentType(
            "Missing Content-Type header".to_string(),
        ))
    }
}

/// Validate Content-Type header (legacy function for backward compatibility)
fn validate_content_type(headers: &HeaderMap, expected: &str) -> Result<(), ReceiverError> {
    let content_type = get_content_type(headers)?;
    if content_type.starts_with(expected) {
        Ok(())
    } else {
        Err(ReceiverError::InvalidContentType(format!(
            "Expected Content-Type: {}, got: {}",
            expected, content_type
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::header::CONTENT_TYPE;

    #[test]
    fn test_get_content_type_protobuf() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/x-protobuf".parse().unwrap());

        assert_eq!(
            get_content_type(&headers).unwrap(),
            "application/x-protobuf"
        );
    }

    #[test]
    fn test_get_content_type_json() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

        assert_eq!(get_content_type(&headers).unwrap(), "application/json");
    }

    #[test]
    fn test_get_content_type_with_charset() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            "application/x-protobuf; charset=utf-8".parse().unwrap(),
        );

        assert_eq!(
            get_content_type(&headers).unwrap(),
            "application/x-protobuf"
        );
    }

    #[test]
    fn test_get_content_type_missing() {
        let headers = HeaderMap::new();
        assert!(get_content_type(&headers).is_err());
    }

    #[test]
    fn test_validate_content_type_success() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/x-protobuf".parse().unwrap());

        assert!(validate_content_type(&headers, "application/x-protobuf").is_ok());
    }

    #[test]
    fn test_validate_content_type_with_charset() {
        let mut headers = HeaderMap::new();
        headers.insert(
            CONTENT_TYPE,
            "application/x-protobuf; charset=utf-8".parse().unwrap(),
        );

        assert!(validate_content_type(&headers, "application/x-protobuf").is_ok());
    }

    #[test]
    fn test_validate_content_type_missing() {
        let headers = HeaderMap::new();
        assert!(validate_content_type(&headers, "application/x-protobuf").is_err());
    }

    #[test]
    fn test_validate_content_type_wrong() {
        let mut headers = HeaderMap::new();
        headers.insert(CONTENT_TYPE, "application/json".parse().unwrap());

        assert!(validate_content_type(&headers, "application/x-protobuf").is_err());
    }
}
