// HTTP request handlers for OTLP endpoints

use crate::error::ReceiverError;
use crate::health::HealthStatus;
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
use tracing::{debug, error, warn};

/// Health check endpoint handler
///
/// Reports the *combined* health (#256): the bind-time ready flag AND the
/// write-path state. A full disk or corruption that degrades writes makes
/// `/health` return 503 even though the socket is still bound and serving.
pub async fn handle_health(State(state): State<AppState>) -> Response {
    match state.health_checker.status() {
        HealthStatus::Healthy => {
            (StatusCode::OK, Json(json!({"status": "healthy"}))).into_response()
        },
        HealthStatus::Unhealthy => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"status": "unhealthy"})),
        )
            .into_response(),
    }
}

/// Metrics endpoint handler
pub async fn handle_metrics(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    debug!("Received metrics request: {} bytes", body.len());

    let body = match decode_body(&headers, body, state.max_body_size) {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };

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
        Ok(result) => {
            // partialSuccess (OTLP HTTP JSON shape) reports the data
            // points conversion dropped — exponential histograms, unset
            // values, +Inf overflow tails — so spec-aware exporters learn
            // they did not fully land (#256). Absent when nothing was
            // dropped.
            let rejected = result.dropped.rejected_data_points();
            let mut payload = serde_json::Map::new();
            payload.insert("status".into(), json!("success"));
            if rejected > 0 {
                payload.insert(
                    "partialSuccess".into(),
                    json!({
                        "rejectedDataPoints": rejected,
                        "errorMessage": format!(
                            "otelite dropped telemetry it cannot store: {}",
                            result.dropped.summary()
                        ),
                    }),
                );
            }
            (StatusCode::OK, Json(serde_json::Value::Object(payload))).into_response()
        },
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

    let body = match decode_body(&headers, body, state.max_body_size) {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };

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

    let body = match decode_body(&headers, body, state.max_body_size) {
        Ok(b) => b,
        Err(e) => return e.into_response(),
    };

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
        Ok(result) => (
            StatusCode::OK,
            Json(json!({
                "status": "success",
                "accepted_spans": result.accepted_spans,
                "rejected_spans": result.rejected_spans,
            })),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to process traces: {}", e);
            e.into_response()
        },
    }
}

/// Unified endpoint handler (legacy support)
///
/// The OTLP spec defines per-signal endpoints only; this legacy unified
/// path used to guess the signal by trial-decoding the body as each type
/// in turn — protobuf wire types routinely parse as the *wrong* message,
/// so a logs export could be silently stored as traces (#256). It also
/// ignored Content-Encoding. Rather than keep a mis-routing hazard, the
/// endpoint now fails loudly with a 400 pointing at the per-signal
/// endpoints. A 400 (not 5xx) is deliberate: it tells exporters to stop
/// retrying this path instead of looping on a "server error".
pub async fn handle_unified(
    State(_state): State<AppState>,
    _headers: HeaderMap,
    body: Bytes,
) -> Response {
    warn!(
        bytes = body.len(),
        "Legacy unified OTLP endpoint /v1/otlp hit — it is deprecated; POST to /v1/traces, /v1/metrics or /v1/logs"
    );
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "error": "The legacy unified OTLP endpoint /v1/otlp is no longer supported: trial-decoding the body could mis-route exports to the wrong signal. POST to the per-signal endpoints instead: /v1/traces, /v1/metrics, or /v1/logs.",
            "status": 400,
        })),
    )
        .into_response()
}

/// Decode the request body, honouring the Content-Encoding header.
///
/// OTLP/HTTP requires gzip support; deflate is accepted as well. The
/// decompressed output is bounded by `max_bytes` (the configured
/// maximum message size) — a "decompression bomb" that would expand
/// past the limit is rejected with 413, and the decoder stops reading
/// at the cap so memory stays bounded regardless of the compression
/// ratio. Unknown encodings are rejected with 415.
fn decode_body(headers: &HeaderMap, body: Bytes, max_bytes: usize) -> Result<Bytes, ReceiverError> {
    let encoding = headers
        .get("content-encoding")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .unwrap_or("identity");

    // A list of encodings (e.g. "gzip, br") would apply each layer; we
    // only support a single encoding, so anything else is rejected.
    if encoding.contains(',') {
        return Err(ReceiverError::InvalidContentType(format!(
            "Unsupported Content-Encoding: {encoding}. Supported: identity, gzip, deflate"
        )));
    }

    match encoding {
        "identity" => Ok(body),
        "gzip" => decompress_bounded(
            &mut flate2::read::GzDecoder::new(&body[..]),
            "gzip",
            max_bytes,
        ),
        "deflate" => decompress_bounded(
            &mut flate2::read::DeflateDecoder::new(&body[..]),
            "deflate",
            max_bytes,
        ),
        other => Err(ReceiverError::InvalidContentType(format!(
            "Unsupported Content-Encoding: {other}. Supported: identity, gzip, deflate"
        ))),
    }
}

/// Read from `reader` until EOF, stopping (and failing) once the output
/// exceeds `max_bytes`.
fn decompress_bounded<R: std::io::Read>(
    reader: &mut R,
    name: &str,
    max_bytes: usize,
) -> Result<Bytes, ReceiverError> {
    let mut out = Vec::with_capacity(64 * 1024);
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf).map_err(|e| {
            ReceiverError::CompressionError(format!("Failed to decode {name} body: {e}"))
        })?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() > max_bytes {
            return Err(ReceiverError::MessageTooLarge {
                size: out.len(),
                max: max_bytes,
            });
        }
    }
    Ok(Bytes::from(out))
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
}
