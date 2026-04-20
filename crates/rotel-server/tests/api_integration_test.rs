use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use http_body_util::BodyExt;
use rotel_core::api::{LogEntry, LogsResponse, MetricResponse, TraceDetail, TracesResponse};
use rotel_core::telemetry::log::{LogRecord, SeverityLevel};
use rotel_core::telemetry::metric::{Metric, MetricType};
use rotel_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode as SpanStatusCode};
use rotel_core::telemetry::Resource;
use rotel_server::api::health::HealthResponse;
use rotel_server::api::metrics::AggregateResponse;
use rotel_server::server::{AppState, QueryCache};
use rotel_storage::sqlite::SqliteBackend;
use rotel_storage::{StorageBackend, StorageConfig};
use std::collections::HashMap;
use std::sync::Arc;
use tempfile::TempDir;
use tower::ServiceExt;

async fn setup_test_storage() -> (Arc<dyn StorageBackend>, TempDir) {
    let tmp = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(tmp.path().to_path_buf());
    let mut storage = SqliteBackend::new(config);
    storage.initialize().await.unwrap();
    let storage_arc: Arc<dyn StorageBackend> = Arc::new(storage);
    (storage_arc, tmp)
}

fn build_test_router(storage: Arc<dyn StorageBackend>) -> Router {
    let state = AppState {
        storage,
        cache: QueryCache::default(),
    };

    Router::new()
        .route(
            "/api/health",
            axum::routing::get(rotel_server::api::health::health_check),
        )
        .route(
            "/api/help",
            axum::routing::get(rotel_server::api::help::api_help),
        )
        .route(
            "/api/logs",
            axum::routing::get(rotel_server::api::logs::list_logs),
        )
        .route(
            "/api/logs/export",
            axum::routing::get(rotel_server::api::logs::export_logs),
        )
        .route(
            "/api/logs/{timestamp}",
            axum::routing::get(rotel_server::api::logs::get_log),
        )
        .route(
            "/api/traces",
            axum::routing::get(rotel_server::api::traces::list_traces),
        )
        .route(
            "/api/traces/export",
            axum::routing::get(rotel_server::api::traces::export_traces),
        )
        .route(
            "/api/traces/{trace_id}",
            axum::routing::get(rotel_server::api::traces::get_trace),
        )
        .route(
            "/api/metrics",
            axum::routing::get(rotel_server::api::metrics::list_metrics),
        )
        .route(
            "/api/metrics/names",
            axum::routing::get(rotel_server::api::metrics::list_metric_names),
        )
        .route(
            "/api/metrics/aggregate",
            axum::routing::get(rotel_server::api::metrics::aggregate_metrics),
        )
        .route(
            "/api/metrics/export",
            axum::routing::get(rotel_server::api::metrics::export_metrics),
        )
        .route(
            "/api/metrics/{name}/timeseries",
            axum::routing::get(rotel_server::api::metrics::get_metric_timeseries),
        )
        .route(
            "/api/openapi.json",
            axum::routing::get(|| async {
                use utoipa::OpenApi;
                #[derive(OpenApi)]
                #[openapi(
                    paths(rotel_server::api::health::health_check,),
                    info(title = "Rotel API", version = "1.0.0",)
                )]
                struct ApiDoc;
                axum::Json(ApiDoc::openapi())
            }),
        )
        .with_state(state)
}

fn create_test_log(timestamp: i64, severity: SeverityLevel, body: &str) -> LogRecord {
    LogRecord {
        timestamp,
        observed_timestamp: Some(timestamp),
        severity,
        severity_text: Some(severity.as_str().to_string()),
        body: body.to_string(),
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: {
                let mut attrs = HashMap::new();
                attrs.insert("service.name".to_string(), "test-service".to_string());
                attrs
            },
        }),
        trace_id: None,
        span_id: None,
    }
}

fn create_test_span(trace_id: &str, span_id: &str, name: &str, start: i64, end: i64) -> Span {
    Span {
        trace_id: trace_id.to_string(),
        span_id: span_id.to_string(),
        parent_span_id: None,
        name: name.to_string(),
        kind: SpanKind::Internal,
        start_time: start,
        end_time: end,
        attributes: HashMap::new(),
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
    }
}

fn create_test_metric(name: &str, timestamp: i64, value: f64) -> Metric {
    Metric {
        name: name.to_string(),
        description: Some("Test metric".to_string()),
        unit: Some("count".to_string()),
        metric_type: MetricType::Gauge(value),
        timestamp,
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: {
                let mut attrs = HashMap::new();
                attrs.insert("service.name".to_string(), "test-service".to_string());
                attrs
            },
        }),
    }
}

#[tokio::test]
async fn test_health_check() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let health: HealthResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(health.status, "healthy");
    assert_eq!(health.storage, "connected");
    assert!(!health.version.is_empty());
}

// NEW TEST: GET /api/help
#[tokio::test]
async fn test_help_endpoint() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/help")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let help_text = String::from_utf8(body.to_vec()).unwrap();

    assert!(!help_text.is_empty());
    assert!(help_text.contains("API"));
}

#[tokio::test]
async fn test_list_logs_empty() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/logs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let logs_response: LogsResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(logs_response.logs.len(), 0);
    assert_eq!(logs_response.total, 0);
}

#[tokio::test]
async fn test_list_logs_with_data() {
    let (storage, _tmp) = setup_test_storage().await;

    // Write test logs
    let log1 = create_test_log(1000, SeverityLevel::Info, "Test log 1");
    let log2 = create_test_log(2000, SeverityLevel::Error, "Test log 2");
    let log3 = create_test_log(3000, SeverityLevel::Warn, "Test log 3");

    storage.write_log(&log1).await.unwrap();
    storage.write_log(&log2).await.unwrap();
    storage.write_log(&log3).await.unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/logs")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let logs_response: LogsResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(logs_response.logs.len(), 3);
    assert_eq!(logs_response.total, 3);
}

#[tokio::test]
async fn test_list_logs_with_severity_filter() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_log(&create_test_log(1000, SeverityLevel::Info, "Info log"))
        .await
        .unwrap();
    storage
        .write_log(&create_test_log(2000, SeverityLevel::Error, "Error log"))
        .await
        .unwrap();
    storage
        .write_log(&create_test_log(3000, SeverityLevel::Warn, "Warn log"))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/logs?severity=ERROR")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let logs_response: LogsResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(logs_response.logs.len(), 1);
    assert_eq!(logs_response.logs[0].severity, "ERROR");
}

#[tokio::test]
async fn test_list_logs_with_search() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_log(&create_test_log(
            1000,
            SeverityLevel::Info,
            "User logged in",
        ))
        .await
        .unwrap();
    storage
        .write_log(&create_test_log(
            2000,
            SeverityLevel::Error,
            "Database error",
        ))
        .await
        .unwrap();
    storage
        .write_log(&create_test_log(
            3000,
            SeverityLevel::Info,
            "User logged out",
        ))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/logs?search=logged")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let logs_response: LogsResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(logs_response.logs.len(), 2);
}

#[tokio::test]
async fn test_get_log_by_timestamp() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_log(&create_test_log(1000, SeverityLevel::Info, "Specific log"))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/logs/1000")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let log_entry: LogEntry = serde_json::from_slice(&body).unwrap();

    assert_eq!(log_entry.timestamp, 1000);
    assert_eq!(log_entry.body, "Specific log");
}

#[tokio::test]
async fn test_export_logs_json() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_log(&create_test_log(1000, SeverityLevel::Info, "Log 1"))
        .await
        .unwrap();
    storage
        .write_log(&create_test_log(2000, SeverityLevel::Error, "Log 2"))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/logs/export?format=json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );
}

// ENHANCED TEST: GET /api/logs/export CSV with proper validation
#[tokio::test]
async fn test_export_logs_csv_with_validation() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_log(&create_test_log(1000, SeverityLevel::Info, "Log 1"))
        .await
        .unwrap();
    storage
        .write_log(&create_test_log(2000, SeverityLevel::Error, "Log 2"))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/logs/export?format=csv")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers().get("content-type").unwrap(), "text/csv");

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let csv = String::from_utf8(body.to_vec()).unwrap();

    // Parse CSV and validate structure
    let lines: Vec<&str> = csv.lines().collect();
    assert!(
        lines.len() >= 3,
        "Expected at least 3 lines (header + 2 data rows), got {}",
        lines.len()
    ); // header + 2 data rows

    // Validate header - actual format is: timestamp,severity,body,trace_id,span_id
    let header = lines[0];
    assert_eq!(header, "timestamp,severity,body,trace_id,span_id");

    // Validate data rows exist - logs may be in any order
    let csv_content = lines[1..].join("\n");
    assert!(
        csv_content.contains("1000"),
        "CSV should contain timestamp 1000"
    );
    assert!(
        csv_content.contains("INFO"),
        "CSV should contain INFO severity"
    );
    assert!(csv_content.contains("Log 1"), "CSV should contain 'Log 1'");
    assert!(
        csv_content.contains("2000"),
        "CSV should contain timestamp 2000"
    );
    assert!(
        csv_content.contains("ERROR"),
        "CSV should contain ERROR severity"
    );
    assert!(csv_content.contains("Log 2"), "CSV should contain 'Log 2'");
}

#[tokio::test]
async fn test_list_traces_empty() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/traces")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let traces_response: TracesResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(traces_response.traces.len(), 0);
}

#[tokio::test]
async fn test_list_traces_with_data() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_span(&create_test_span("trace1", "span1", "root", 1000, 2000))
        .await
        .unwrap();
    storage
        .write_span(&create_test_span("trace1", "span2", "child", 1100, 1900))
        .await
        .unwrap();
    storage
        .write_span(&create_test_span("trace2", "span3", "root", 3000, 4000))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/traces")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let traces_response: TracesResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(traces_response.traces.len(), 2);
    assert_eq!(traces_response.total, 2);
}

#[tokio::test]
async fn test_get_trace_by_id() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_span(&create_test_span("trace1", "span1", "root", 1000, 2000))
        .await
        .unwrap();
    storage
        .write_span(&create_test_span("trace1", "span2", "child", 1100, 1900))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/traces/trace1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let trace_detail: TraceDetail = serde_json::from_slice(&body).unwrap();

    assert_eq!(trace_detail.trace_id, "trace1");
    assert_eq!(trace_detail.spans.len(), 2);
    assert_eq!(trace_detail.span_count, 2);
}

// NEW TEST: GET /api/traces/export - empty state
#[tokio::test]
async fn test_export_traces_empty() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/traces/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let traces: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(traces.len(), 0);
}

// NEW TEST: GET /api/traces/export - with data
#[tokio::test]
async fn test_export_traces_with_data() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_span(&create_test_span("trace1", "span1", "root", 1000, 2000))
        .await
        .unwrap();
    storage
        .write_span(&create_test_span("trace2", "span2", "root", 3000, 4000))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/traces/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let traces: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(traces.len(), 2);
}

// NEW TEST: GET /api/traces/export - invalid format
#[tokio::test]
async fn test_export_traces_invalid_format() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/traces/export?format=invalid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_list_metrics_empty() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let metrics: Vec<MetricResponse> = serde_json::from_slice(&body).unwrap();

    assert_eq!(metrics.len(), 0);
}

#[tokio::test]
async fn test_list_metrics_with_data() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_metric(&create_test_metric("cpu.usage", 1000, 45.5))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("memory.usage", 2000, 78.2))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("disk.usage", 3000, 62.1))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let metrics_response: Vec<MetricResponse> = serde_json::from_slice(&body).unwrap();

    assert_eq!(metrics_response.len(), 3);
}

// NEW TEST: GET /api/metrics/names - empty DB
#[tokio::test]
async fn test_list_metric_names_empty() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics/names")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let names: Vec<String> = serde_json::from_slice(&body).unwrap();

    assert_eq!(names.len(), 0);
}

// NEW TEST: GET /api/metrics/names - with distinct names
#[tokio::test]
async fn test_list_metric_names_with_data() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_metric(&create_test_metric("cpu.usage", 1000, 45.5))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("memory.usage", 2000, 78.2))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("disk.usage", 3000, 62.1))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics/names")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let names: Vec<String> = serde_json::from_slice(&body).unwrap();

    assert_eq!(names.len(), 3);
    assert!(names.contains(&"cpu.usage".to_string()));
    assert!(names.contains(&"memory.usage".to_string()));
    assert!(names.contains(&"disk.usage".to_string()));
}

// NEW TEST: GET /api/metrics/names - deduplication
#[tokio::test]
async fn test_list_metric_names_deduplicated() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_metric(&create_test_metric("requests", 1000, 10.0))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("requests", 2000, 20.0))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("errors", 3000, 5.0))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics/names")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let names: Vec<String> = serde_json::from_slice(&body).unwrap();

    assert_eq!(names.len(), 2); // Only unique names
    assert!(names.contains(&"requests".to_string()));
    assert!(names.contains(&"errors".to_string()));
}

// NEW TEST: GET /api/metrics/export - empty
#[tokio::test]
async fn test_export_metrics_empty() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let metrics: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(metrics.len(), 0);
}

// NEW TEST: GET /api/metrics/export - with data
#[tokio::test]
async fn test_export_metrics_with_data() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_metric(&create_test_metric("cpu.usage", 1000, 45.5))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("memory.usage", 2000, 78.2))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics/export")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let metrics: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();

    assert_eq!(metrics.len(), 2);
    // Verify it's parseable JSON
    assert!(metrics[0].is_object());
    assert!(metrics[1].is_object());
}

#[tokio::test]
async fn test_aggregate_metrics_sum() {
    let (storage, _tmp) = setup_test_storage().await;

    storage
        .write_metric(&create_test_metric("requests", 1000, 10.0))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("requests", 2000, 20.0))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("requests", 3000, 30.0))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics/aggregate?name=requests&function=sum")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let agg: AggregateResponse = serde_json::from_slice(&body).unwrap();

    assert_eq!(agg.name, "requests");
    assert_eq!(agg.function, "sum");
    assert_eq!(agg.result, 60.0);
    assert_eq!(agg.count, 3);
}

#[tokio::test]
async fn test_get_metric_timeseries() {
    let (storage, _tmp) = setup_test_storage().await;

    // Write metrics at different timestamps
    storage
        .write_metric(&create_test_metric("cpu.usage", 1_000_000_000, 45.5))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("cpu.usage", 61_000_000_000, 50.2))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("cpu.usage", 121_000_000_000, 55.8))
        .await
        .unwrap();

    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics/cpu.usage/timeseries?step=60")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let timeseries: Vec<rotel_server::api::metrics::TimeBucket> =
        serde_json::from_slice(&body).unwrap();

    assert!(!timeseries.is_empty());
    assert!(timeseries.iter().all(|b| b.count > 0));
}

#[tokio::test]
async fn test_get_metric_timeseries_not_found() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics/nonexistent/timeseries")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_metric_timeseries_with_time_range() {
    let (storage, _tmp) = setup_test_storage().await;

    // Write metrics spanning a time range
    storage
        .write_metric(&create_test_metric("requests", 1_000_000_000, 10.0))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("requests", 2_000_000_000, 20.0))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("requests", 3_000_000_000, 30.0))
        .await
        .unwrap();
    storage
        .write_metric(&create_test_metric("requests", 4_000_000_000, 40.0))
        .await
        .unwrap();

    let app = build_test_router(storage);

    // Query with time range
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/metrics/requests/timeseries?start_time=1000000000&end_time=3000000000&step=1")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let timeseries: Vec<rotel_server::api::metrics::TimeBucket> =
        serde_json::from_slice(&body).unwrap();

    // Should only include metrics within the time range
    assert!(!timeseries.is_empty());
    assert!(timeseries
        .iter()
        .all(|b| b.timestamp >= 1_000_000_000 && b.timestamp <= 3_000_000_000));
}

// NEW TEST: GET /api/openapi.json
#[tokio::test]
async fn test_openapi_spec() {
    let (storage, _tmp) = setup_test_storage().await;
    let app = build_test_router(storage);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/openapi.json")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "application/json"
    );

    let body = response.into_body().collect().await.unwrap().to_bytes();
    let spec: serde_json::Value = serde_json::from_slice(&body).unwrap();

    // Verify it's valid OpenAPI JSON
    assert!(spec.is_object());
    assert!(spec.get("openapi").is_some());
}
