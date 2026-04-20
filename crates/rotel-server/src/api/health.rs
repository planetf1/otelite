// Health check endpoint

use axum::{http::StatusCode, Json};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Health check response
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub storage: String,
    pub uptime_seconds: u64,
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
pub async fn health_check() -> Result<Json<HealthResponse>, StatusCode> {
    // Calculate uptime (simplified - would need actual start time tracking)
    let uptime = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let response = HealthResponse {
        status: "healthy".to_string(),
        version: crate::VERSION.to_string(),
        storage: "connected".to_string(),
        uptime_seconds: uptime,
    };

    Ok(Json(response))
}
