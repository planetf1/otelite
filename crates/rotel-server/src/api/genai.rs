//! GenAI/LLM token usage API endpoints

use crate::server::AppState;
use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Json,
};
use rotel_core::api::{ErrorResponse, TokenUsageResponse};
use serde::{Deserialize, Serialize};

/// Query parameters for token usage endpoint
#[derive(Debug, Deserialize, Serialize, utoipa::IntoParams, utoipa::ToSchema)]
pub struct TokenUsageQuery {
    /// Start time (nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
}

/// Get token usage statistics for GenAI/LLM spans
///
/// Returns aggregated token usage grouped by model and system (provider).
/// Only includes spans with `gen_ai.system` attribute.
#[utoipa::path(
    get,
    path = "/api/genai/usage",
    params(TokenUsageQuery),
    responses(
        (status = 200, description = "Token usage summary", body = TokenUsageResponse),
        (status = 500, description = "Internal server error", body = ErrorResponse)
    ),
    tag = "genai"
)]
pub async fn get_token_usage(
    State(_state): State<AppState>,
    Query(_query): Query<TokenUsageQuery>,
) -> Result<Json<TokenUsageResponse>, (StatusCode, Json<ErrorResponse>)> {
    // Query storage for token usage
    // Note: This requires adding query_token_usage to StorageBackend trait
    // For now, return empty results as placeholder
    let summary = rotel_core::api::TokenUsageSummary {
        total_input_tokens: 0,
        total_output_tokens: 0,
        total_requests: 0,
    };
    let by_model = Vec::new();
    let by_system = Vec::new();

    Ok(Json(TokenUsageResponse {
        summary,
        by_model,
        by_system,
    }))
}
