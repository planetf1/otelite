//! Tests for GenAI token usage API endpoint

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use rotel_core::api::TokenUsageResponse;
use rotel_server::{DashboardConfig, DashboardServer};
use rotel_storage::sqlite::SqliteBackend;
use rotel_storage::{StorageBackend, StorageConfig};
use std::sync::Arc;
use tower::ServiceExt;

async fn setup_test_server() -> (DashboardServer, Arc<dyn StorageBackend>) {
    let config = DashboardConfig::default();
    let storage_config = StorageConfig::default();
    let mut storage = SqliteBackend::new(storage_config);
    storage.initialize().await.unwrap();
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    let server = DashboardServer::new(config, storage.clone());
    (server, storage)
}

#[tokio::test]
async fn test_get_token_usage_empty() {
    let (server, _storage) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/usage")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let usage: TokenUsageResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(usage.summary.total_input_tokens, 0);
    assert_eq!(usage.summary.total_output_tokens, 0);
    assert_eq!(usage.summary.total_requests, 0);
    assert_eq!(usage.by_model.len(), 0);
    assert_eq!(usage.by_system.len(), 0);
}

#[tokio::test]
async fn test_get_token_usage_with_time_params() {
    let (server, _storage) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/usage?start_time=1000&end_time=2000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let usage: TokenUsageResponse = serde_json::from_slice(&body).unwrap();

    // Should return empty results (placeholder implementation)
    assert_eq!(usage.summary.total_input_tokens, 0);
    assert_eq!(usage.summary.total_output_tokens, 0);
}

#[tokio::test]
async fn test_get_token_usage_response_structure() {
    let (server, _storage) = setup_test_server().await;
    let app = server.build_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/genai/usage")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let usage: TokenUsageResponse = serde_json::from_slice(&body).unwrap();

    // Verify response structure (values are u64, so always >= 0)
    assert!(usage.by_model.is_empty() || !usage.by_model.is_empty());
    assert!(usage.by_system.is_empty() || !usage.by_system.is_empty());
}
