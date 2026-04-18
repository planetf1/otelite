//! Integration tests for health and readiness endpoints

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use rotel_api::{
    models::{HealthResponse, ReadinessResponse},
    routes,
};
use serde_json::Value;
use tower::ServiceExt;

/// Helper to create test app
fn create_test_app() -> axum::Router {
    routes::create_router()
}

#[tokio::test]
async fn test_health_check_success() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let health: HealthResponse = serde_json::from_slice(&body).unwrap();

    assert!(!health.version.is_empty());
    assert!(health.uptime_seconds > 0);
}

#[tokio::test]
async fn test_health_check_structure() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    // Verify response structure
    assert!(json.get("status").is_some());
    assert!(json.get("version").is_some());
    assert!(json.get("uptime_seconds").is_some());
    assert!(json.get("system").is_some());
    assert!(json.get("components").is_some());

    // Verify system stats structure
    let system = json.get("system").unwrap();
    assert!(system.get("memory_used_bytes").is_some());
    assert!(system.get("memory_total_bytes").is_some());
    assert!(system.get("memory_usage_percent").is_some());
    assert!(system.get("cpu_usage_percent").is_some());
    assert!(system.get("active_connections").is_some());
    assert!(system.get("total_requests").is_some());
    assert!(system.get("requests_per_second").is_some());

    // Verify components structure
    let components = json.get("components").unwrap();
    assert!(components.get("storage").is_some());
    assert!(components.get("api").is_some());
    assert!(components.get("metrics").is_some());

    // Verify component status structure
    let storage = components.get("storage").unwrap();
    assert!(storage.get("status").is_some());
    assert!(storage.get("message").is_some());
    assert!(storage.get("last_check").is_some());
    assert!(storage.get("response_time_ms").is_some());
}

#[tokio::test]
async fn test_readiness_check_success() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let readiness: ReadinessResponse = serde_json::from_slice(&body).unwrap();

    assert!(readiness.ready);
    assert!(readiness.checks.storage_ready);
    assert!(readiness.checks.api_ready);
    assert!(readiness.checks.config_valid);
}

#[tokio::test]
async fn test_readiness_check_structure() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let json: Value = serde_json::from_slice(&body).unwrap();

    // Verify response structure
    assert!(json.get("ready").is_some());
    assert!(json.get("checks").is_some());

    // Verify checks structure
    let checks = json.get("checks").unwrap();
    assert!(checks.get("storage_ready").is_some());
    assert!(checks.get("api_ready").is_some());
    assert!(checks.get("config_valid").is_some());
}

#[tokio::test]
async fn test_health_system_stats_values() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let health: HealthResponse = serde_json::from_slice(&body).unwrap();

    // Verify system stats have reasonable values
    assert!(health.system.memory_used_bytes > 0);
    assert!(health.system.memory_total_bytes > health.system.memory_used_bytes);
    assert!(health.system.memory_usage_percent >= 0.0);
    assert!(health.system.memory_usage_percent <= 100.0);
    assert!(health.system.cpu_usage_percent >= 0.0);
    assert!(health.system.cpu_usage_percent <= 100.0);
    // total_requests is u64, so always >= 0
    assert!(health.system.requests_per_second >= 0.0);
}

#[tokio::test]
async fn test_health_component_status() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let health: HealthResponse = serde_json::from_slice(&body).unwrap();

    // Verify all components have status
    assert!(!health.components.storage.message.is_empty());
    assert!(health.components.storage.last_check > 0);
    assert!(health.components.storage.response_time_ms.is_some());

    assert!(!health.components.api.message.is_empty());
    assert!(health.components.api.last_check > 0);
    assert!(health.components.api.response_time_ms.is_some());

    assert!(!health.components.metrics.message.is_empty());
    assert!(health.components.metrics.last_check > 0);
    assert!(health.components.metrics.response_time_ms.is_some());
}

#[tokio::test]
async fn test_health_endpoint_idempotent() {
    let app = create_test_app();

    // Call health endpoint multiple times
    for _ in 0..3 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn test_readiness_endpoint_idempotent() {
    let app = create_test_app();

    // Call readiness endpoint multiple times
    for _ in 0..3 {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/ready")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}

// Made with Bob
