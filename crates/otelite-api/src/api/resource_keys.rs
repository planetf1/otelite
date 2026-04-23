use crate::server::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use otelite_core::api::ErrorResponse;
use serde::{Deserialize, Serialize};

/// Query parameters for the resource-keys endpoint
#[derive(Debug, Deserialize)]
pub struct ResourceKeysQuery {
    /// Signal type: "logs", "spans", or "metrics"
    pub signal: String,
}

/// Response containing distinct resource attribute keys
#[derive(Debug, Serialize)]
pub struct ResourceKeysResponse {
    pub keys: Vec<String>,
}

/// Handler for GET /api/resource-keys?signal=<signal>
pub async fn get_resource_keys(
    State(state): State<AppState>,
    Query(params): Query<ResourceKeysQuery>,
) -> Result<Json<ResourceKeysResponse>, (StatusCode, Json<ErrorResponse>)> {
    let keys = state
        .storage
        .distinct_resource_keys(&params.signal)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse::storage_error(format!(
                    "distinct resource keys: {}",
                    e
                ))),
            )
        })?;

    Ok(Json(ResourceKeysResponse { keys }))
}
