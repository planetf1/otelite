// Health check endpoint

use crate::server::AppState;
use axum::{extract::State, http::StatusCode, Json};
use serde::{Deserialize, Serialize};

/// Health check response
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub storage: String,
    pub uptime_seconds: u64,
    /// Resolved OTLP gRPC receiver address
    pub otlp_grpc_addr: String,
    /// Resolved OTLP HTTP receiver address
    pub otlp_http_addr: String,
}

/// Health check handler
#[utoipa::path(
    get,
    path = "/api/health",
    responses(
        (status = 200, description = "Service is healthy", body = HealthResponse)
    ),
    tag = "health"
)]
pub async fn health_check(
    State(state): State<AppState>,
) -> Result<Json<HealthResponse>, StatusCode> {
    let uptime = state.start_time.elapsed().as_secs();

    let response = HealthResponse {
        status: "healthy".to_string(),
        version: crate::VERSION.to_string(),
        storage: "connected".to_string(),
        uptime_seconds: uptime,
        otlp_grpc_addr: state.otlp_grpc_addr,
        otlp_http_addr: state.otlp_http_addr,
    };

    Ok(Json(response))
}
