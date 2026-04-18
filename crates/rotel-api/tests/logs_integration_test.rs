//! Integration tests for log query endpoints

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use rotel_api::{
    models::response::{ListResponse, LogEntry},
    routes,
};

#[tokio::test]
async fn test_list_logs_endpoint() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<LogEntry> = serde_json::from_slice(&body).unwrap();

    assert!(list_response.items.len() > 0);
    assert_eq!(list_response.pagination.total, list_response.items.len());
}

#[tokio::test]
async fn test_list_logs_with_limit() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs?limit=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<LogEntry> = serde_json::from_slice(&body).unwrap();

    assert!(list_response.items.len() <= 2);
    assert_eq!(list_response.pagination.limit, 2);
}

#[tokio::test]
async fn test_list_logs_with_severity_filter() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs?severity=ERROR")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<LogEntry> = serde_json::from_slice(&body).unwrap();

    // All returned logs should have ERROR severity
    for log in &list_response.items {
        assert_eq!(log.severity, "ERROR");
    }
}

#[tokio::test]
async fn test_list_logs_with_search() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs?search=request")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<LogEntry> = serde_json::from_slice(&body).unwrap();

    // All returned logs should contain "request" in message
    for log in &list_response.items {
        assert!(log.message.to_lowercase().contains("request"));
    }
}

#[tokio::test]
async fn test_list_logs_with_pagination() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs?limit=1&offset=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<LogEntry> = serde_json::from_slice(&body).unwrap();

    assert_eq!(list_response.pagination.offset, 1);
    assert_eq!(list_response.pagination.limit, 1);
    assert!(list_response.items.len() <= 1);
}

#[tokio::test]
async fn test_list_logs_invalid_limit() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs?limit=2000") // Exceeds max of 1000
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_log_by_id() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs/log-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let log: LogEntry = serde_json::from_slice(&body).unwrap();

    assert_eq!(log.id, "log-1");
    assert!(!log.message.is_empty());
}

#[tokio::test]
async fn test_get_log_not_found() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs/nonexistent-id")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let error: Value = serde_json::from_slice(&body).unwrap();

    assert!(error["error"].as_str().unwrap().contains("not found"));
}

#[tokio::test]
async fn test_list_logs_response_structure() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/logs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<LogEntry> = serde_json::from_slice(&body).unwrap();

    // Verify response structure
    assert!(list_response.items.len() > 0);

    // Verify pagination metadata
    assert!(list_response.pagination.total > 0);
    assert_eq!(list_response.pagination.count, list_response.items.len());
    assert_eq!(list_response.pagination.offset, 0);
    assert_eq!(list_response.pagination.limit, 100); // Default limit

    // Verify log entry structure
    let log = &list_response.items[0];
    assert!(!log.id.is_empty());
    assert!(log.timestamp > 0);
    assert!(!log.severity.is_empty());
    assert!(!log.message.is_empty());
    assert!(log.resource.service_name.is_some());
}

// Made with Bob
