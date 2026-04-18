//! Integration tests for trace query endpoints

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

use rotel_api::{
    models::{trace::Trace, ListResponse},
    routes,
};

#[tokio::test]
async fn test_list_traces_endpoint() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<Trace> = serde_json::from_slice(&body).unwrap();

    assert!(!list_response.items.is_empty());
    assert_eq!(list_response.pagination.total, list_response.items.len());
}

#[tokio::test]
async fn test_list_traces_with_limit() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces?limit=2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<Trace> = serde_json::from_slice(&body).unwrap();

    assert!(list_response.items.len() <= 2);
    assert_eq!(list_response.pagination.limit, 2);
}

#[tokio::test]
async fn test_list_traces_with_service_filter() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces?service_name=rotel-api")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<Trace> = serde_json::from_slice(&body).unwrap();

    // All returned traces should be from rotel-api service
    for trace in &list_response.items {
        assert_eq!(
            trace.root_span.resource.service_name.as_deref(),
            Some("rotel-api")
        );
    }
}

#[tokio::test]
async fn test_list_traces_with_duration_filter() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces?min_duration_ns=100000000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<Trace> = serde_json::from_slice(&body).unwrap();

    // All returned traces should meet minimum duration
    for trace in &list_response.items {
        assert!(trace.duration_ns >= 100_000_000);
    }
}

#[tokio::test]
async fn test_list_traces_with_span_name_filter() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces?span_name=operation-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<Trace> = serde_json::from_slice(&body).unwrap();

    // All returned traces should contain the specified span name
    for trace in &list_response.items {
        assert!(trace.spans.iter().any(|s| s.name == "operation-1"));
    }
}

#[tokio::test]
async fn test_list_traces_with_pagination() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces?limit=1&offset=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<Trace> = serde_json::from_slice(&body).unwrap();

    assert_eq!(list_response.pagination.offset, 1);
    assert_eq!(list_response.pagination.limit, 1);
    assert!(list_response.items.len() <= 1);
}

#[tokio::test]
async fn test_list_traces_invalid_limit() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces?limit=2000") // Exceeds max of 1000
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_trace_by_id() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces/trace-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let trace: Trace = serde_json::from_slice(&body).unwrap();

    assert_eq!(trace.trace_id, "trace-1");
    assert!(trace.span_count > 0);
    assert!(!trace.spans.is_empty());
}

#[tokio::test]
async fn test_get_trace_not_found() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces/nonexistent-id")
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
async fn test_trace_response_structure() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces/trace-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let trace: Trace = serde_json::from_slice(&body).unwrap();

    // Verify trace structure
    assert!(!trace.trace_id.is_empty());
    assert!(trace.duration_ns > 0);
    assert!(trace.span_count > 0);
    assert_eq!(trace.spans.len(), trace.span_count);

    // Verify root span
    assert!(trace.root_span.parent_span_id.is_none());
    assert_eq!(trace.root_span.trace_id, trace.trace_id);

    // Verify all spans belong to same trace
    for span in &trace.spans {
        assert_eq!(span.trace_id, trace.trace_id);
        assert!(span.duration_ns > 0);
        assert!(!span.name.is_empty());
        assert!(!span.kind.is_empty());
    }
}

#[tokio::test]
async fn test_trace_hierarchy() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces/trace-1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let trace: Trace = serde_json::from_slice(&body).unwrap();

    // Verify span hierarchy
    let root_span = &trace.root_span;
    assert!(root_span.parent_span_id.is_none());

    // Count child spans (spans with parent_span_id)
    let child_spans: Vec<_> = trace
        .spans
        .iter()
        .filter(|s| s.parent_span_id.is_some())
        .collect();

    assert!(!child_spans.is_empty());

    // Verify each child span has a valid parent
    for child in child_spans {
        let parent_exists = trace
            .spans
            .iter()
            .any(|s| Some(&s.span_id) == child.parent_span_id.as_ref());
        assert!(parent_exists, "Child span has invalid parent reference");
    }
}

#[tokio::test]
async fn test_list_traces_response_structure() {
    let app = routes::create_router();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/traces")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<Trace> = serde_json::from_slice(&body).unwrap();

    // Verify response structure
    assert!(!list_response.items.is_empty());

    // Verify pagination metadata
    assert!(list_response.pagination.total > 0);
    assert_eq!(list_response.pagination.count, list_response.items.len());
    assert_eq!(list_response.pagination.offset, 0);
    assert_eq!(list_response.pagination.limit, 100); // Default limit
}

// Made with Bob
