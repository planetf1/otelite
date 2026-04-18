//! Health check and readiness handlers

use axum::Json;

use crate::{
    error::ApiResult,
    models::{HealthResponse, ReadinessResponse},
};

/// Handler for GET /health - Health check endpoint
///
/// Returns comprehensive health information including system statistics,
/// component health, and overall service status.
///
/// # Example
/// ```text
/// GET /health
/// ```text
#[utoipa::path(
    get,
    path = "/health",
    responses(
        (status = 200, description = "Health check response", body = HealthResponse),
        (status = 503, description = "Service unavailable")
    ),
    tag = "health"
)]
pub async fn health_check() -> ApiResult<Json<HealthResponse>> {
    // TODO: Implement real health checks
    // For now, return mock data
    Ok(Json(HealthResponse::mock()))
}

/// Handler for GET /ready - Readiness check endpoint
///
/// Returns whether the service is ready to accept traffic. Used by
/// orchestrators (Kubernetes, Docker Swarm) to determine if the service
/// should receive requests.
///
/// # Example
/// ```text
/// GET /ready
/// ```text
#[utoipa::path(
    get,
    path = "/ready",
    responses(
        (status = 200, description = "Service is ready", body = ReadinessResponse),
        (status = 503, description = "Service is not ready")
    ),
    tag = "health"
)]
pub async fn readiness_check() -> ApiResult<Json<ReadinessResponse>> {
    // TODO: Implement real readiness checks
    // For now, return mock data
    Ok(Json(ReadinessResponse::mock()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_health_check() {
        let result = health_check().await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert!(matches!(
            response.status,
            crate::models::HealthStatus::Healthy
        ));
        assert!(!response.version.is_empty());
        assert!(response.uptime_seconds > 0);
    }

    #[tokio::test]
    async fn test_readiness_check() {
        let result = readiness_check().await;
        assert!(result.is_ok());

        let response = result.unwrap().0;
        assert!(response.ready);
        assert!(response.checks.storage_ready);
        assert!(response.checks.api_ready);
        assert!(response.checks.config_valid);
    }
}

// Made with Bob
