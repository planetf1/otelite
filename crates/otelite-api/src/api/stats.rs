// Storage statistics endpoint

use crate::server::AppState;
use axum::{extract::State, http::StatusCode, Json};
use otelite_core::api::ErrorResponse;
use serde::{Deserialize, Serialize};

/// Storage statistics response
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct StatsResponse {
    /// Total number of log records stored
    pub log_count: u64,
    /// Total number of spans stored
    pub span_count: u64,
    /// Total number of metric data points stored
    pub metric_count: u64,
    /// Total storage size in bytes
    pub storage_size_bytes: u64,
}

/// Storage statistics handler
#[utoipa::path(
    get,
    path = "/api/stats",
    responses(
        (status = 200, description = "Storage statistics", body = StatsResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "stats"
)]
pub async fn get_stats(
    State(state): State<AppState>,
) -> Result<Json<StatsResponse>, (StatusCode, Json<ErrorResponse>)> {
    let storage_stats = state.storage.stats().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::storage_error(format!("get stats: {}", e))),
        )
    })?;

    Ok(Json(StatsResponse {
        log_count: storage_stats.log_count,
        span_count: storage_stats.span_count,
        metric_count: storage_stats.metric_count,
        storage_size_bytes: storage_stats.storage_size_bytes,
    }))
}
