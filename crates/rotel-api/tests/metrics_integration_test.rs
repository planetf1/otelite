//! Integration tests for metrics API endpoints

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use rotel_api::{
    models::{
        metric::{MetricDataPoint, MetricStats},
        ListResponse,
    },
    routes,
};
use serde_json::Value;
use tower::ServiceExt;
use tower_http::cors::CorsLayer;

/// Helper to create test app
fn create_test_app() -> axum::Router {
    routes::create_router().layer(CorsLayer::permissive())
}

#[tokio::test]
async fn test_list_metrics_success() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<MetricDataPoint> = serde_json::from_slice(&body).unwrap();

    assert!(!list_response.items.is_empty());
    assert_eq!(list_response.pagination.total, list_response.items.len());
}

#[tokio::test]
async fn test_list_metrics_with_name_filter() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics?name=http_requests_total")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<MetricDataPoint> = serde_json::from_slice(&body).unwrap();

    for metric in &list_response.items {
        assert_eq!(metric.name, "http_requests_total");
    }
}

#[tokio::test]
async fn test_list_metrics_with_type_filter() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics?metric_type=GAUGE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<MetricDataPoint> = serde_json::from_slice(&body).unwrap();

    for metric in &list_response.items {
        assert_eq!(metric.metric_type, "GAUGE");
    }
}

#[tokio::test]
async fn test_list_metrics_with_service_filter() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics?service_name=rotel-api")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<MetricDataPoint> = serde_json::from_slice(&body).unwrap();

    for metric in &list_response.items {
        assert_eq!(metric.resource.service_name.as_deref(), Some("rotel-api"));
    }
}

#[tokio::test]
async fn test_list_metrics_with_pagination() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics?limit=2&offset=0")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let list_response: ListResponse<MetricDataPoint> = serde_json::from_slice(&body).unwrap();

    assert!(list_response.items.len() <= 2);
    assert_eq!(list_response.pagination.limit, 2);
    assert_eq!(list_response.pagination.offset, 0);
}

#[tokio::test]
async fn test_list_metrics_invalid_limit() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics?limit=2000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_get_metric_stats_success() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics/request_duration_ms/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    let stats: MetricStats = serde_json::from_slice(&body).unwrap();

    assert_eq!(stats.name, "request_duration_ms");
    assert!(stats.count > 0);
    assert!(stats.min <= stats.max);
    assert!(stats.percentiles.is_some());

    let percentiles = stats.percentiles.unwrap();
    assert!(percentiles.p50 >= stats.min);
    assert!(percentiles.p50 <= stats.max);
    assert!(percentiles.p95 >= percentiles.p50);
    assert!(percentiles.p99 >= percentiles.p95);
}

#[tokio::test]
async fn test_get_metric_stats_not_found() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics/nonexistent_metric/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_metrics_response_structure() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics")
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
    assert!(json.get("items").is_some());
    assert!(json.get("pagination").is_some());

    let pagination = json.get("pagination").unwrap();
    assert!(pagination.get("total").is_some());
    assert!(pagination.get("offset").is_some());
    assert!(pagination.get("limit").is_some());
    assert!(pagination.get("count").is_some());

    // Verify metric structure
    let items = json.get("items").unwrap().as_array().unwrap();
    if !items.is_empty() {
        let metric = &items[0];
        assert!(metric.get("id").is_some());
        assert!(metric.get("name").is_some());
        assert!(metric.get("metric_type").is_some());
        assert!(metric.get("timestamp").is_some());
        assert!(metric.get("value").is_some());
        assert!(metric.get("resource").is_some());
        assert!(metric.get("attributes").is_some());
    }
}

#[tokio::test]
async fn test_metric_stats_structure() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics/request_duration_ms/stats")
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

    // Verify stats structure
    assert!(json.get("name").is_some());
    assert!(json.get("count").is_some());
    assert!(json.get("min").is_some());
    assert!(json.get("max").is_some());
    assert!(json.get("avg").is_some());
    assert!(json.get("sum").is_some());
    assert!(json.get("stddev").is_some());
    assert!(json.get("percentiles").is_some());

    let percentiles = json.get("percentiles").unwrap();
    assert!(percentiles.get("p50").is_some());
    assert!(percentiles.get("p95").is_some());
    assert!(percentiles.get("p99").is_some());
}

#[tokio::test]
async fn test_metrics_cors_headers() {
    let app = create_test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/metrics")
                .header("Origin", "http://localhost:3000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let headers = response.headers();
    assert!(headers.contains_key("access-control-allow-origin"));
}

// Made with Bob
