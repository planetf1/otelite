// Admin endpoints

use crate::server::AppState;
use axum::{extract::State, http::StatusCode, Json};
use otelite_core::api::ErrorResponse;
use serde::{Deserialize, Serialize};

/// Response returned by the purge-all endpoint
#[derive(Debug, Serialize, Deserialize, utoipa::ToSchema)]
pub struct PurgeAllResponse {
    /// Number of log records deleted
    pub logs_deleted: u64,
    /// Number of spans deleted
    pub spans_deleted: u64,
    /// Number of metric data points deleted
    pub metrics_deleted: u64,
}

/// Delete all telemetry data
#[utoipa::path(
    post,
    path = "/api/admin/purge",
    responses(
        (status = 200, description = "All telemetry data deleted", body = PurgeAllResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "admin"
)]
pub async fn purge_all(
    State(state): State<AppState>,
) -> Result<Json<PurgeAllResponse>, (StatusCode, Json<ErrorResponse>)> {
    let stats = state.storage.purge_all().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse::storage_error(format!("purge all: {}", e))),
        )
    })?;

    Ok(Json(PurgeAllResponse {
        logs_deleted: stats.logs_deleted,
        spans_deleted: stats.spans_deleted,
        metrics_deleted: stats.metrics_deleted,
    }))
}
