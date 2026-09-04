// End-to-end test: OTLP through API query
// Tests the complete flow: OTLP ingestion → storage → dashboard API → HTTP response

use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};
use opentelemetry_proto::tonic::logs::v1::LogRecord as OtlpLogRecord;
use opentelemetry_proto::tonic::metrics::v1::{
    metric::Data, number_data_point::Value as MetricValue, Gauge, Metric, NumberDataPoint,
    ResourceMetrics, ScopeMetrics,
};
use opentelemetry_proto::tonic::trace::v1::{
    span::SpanKind, ResourceSpans, ScopeSpans, Span, Status,
};
use otelite_api::config::DashboardConfig;
use otelite_api::server::DashboardServer;
use otelite_receiver::signals::{LogsHandler, MetricsHandler, TracesHandler};
use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
use serde_json::Value as JsonValue;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;
use tokio::net::TcpListener;

/// Create file-backed temp storage for testing.
///
/// Returns both the storage backend and the TempDir — the caller must hold the
/// TempDir for the lifetime of the test or the directory is deleted mid-run.
async fn create_test_storage() -> (Arc<SqliteBackend>, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut storage = SqliteBackend::new(config);
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    (Arc::new(storage), temp_dir)
}

/// Start dashboard server on a random available port
async fn start_test_server(
    storage: Arc<dyn StorageBackend>,
) -> (SocketAddr, tokio::task::JoinHandle<()>) {
    // Bind to port 0 to get a random available port
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("Failed to bind to random port");
    let addr = listener.local_addr().expect("Failed to get local address");

    let config = DashboardConfig::default().with_bind_address(addr);

    let server = DashboardServer::new(config, storage);
    let router = server.build_router();

    let handle = tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .expect("Server failed to start");
    });

    // Give server a moment to start
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    (addr, handle)
}

#[tokio::test]
async fn test_logs_e2e_flow() {
    // Setup: Create storage and handlers
    let (storage, _temp_dir) = create_test_storage().await;
    let logs_handler = Arc::new(LogsHandler::new(storage.clone() as Arc<dyn StorageBackend>));

    // Start dashboard server
    let (addr, _server_handle) = start_test_server(storage.clone()).await;

    // Create and send OTLP log data
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let log_record = OtlpLogRecord {
        time_unix_nano: timestamp,
        observed_time_unix_nano: timestamp,
        severity_number: 9, // INFO
        severity_text: "INFO".to_string(),
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue(
                "E2E test log message".to_string(),
            )),
        }),
        attributes: vec![KeyValue {
            key: "test_key".to_string(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue("test_value".to_string())),
            }),
        }],
        dropped_attributes_count: 0,
        flags: 0,
        trace_id: vec![],
        span_id: vec![],
        event_name: String::new(),
    };

    let request = ExportLogsServiceRequest {
        resource_logs: vec![opentelemetry_proto::tonic::logs::v1::ResourceLogs {
            resource: None,
            scope_logs: vec![opentelemetry_proto::tonic::logs::v1::ScopeLogs {
                scope: None,
                log_records: vec![log_record],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };

    // Process through handler
    logs_handler
        .process(request)
        .await
        .expect("Failed to process logs");

    // Query via HTTP API
    let client = reqwest::Client::new();
    let url = format!("http://{}/api/logs", addr);
    let response = client.get(&url).send().await.expect("Failed to query API");

    assert_eq!(
        response.status(),
        200,
        "API should return 200 OK: {:?}",
        response.text().await
    );

    let json: JsonValue = response.json().await.expect("Failed to parse JSON");

    // Verify response structure
    assert!(json["logs"].is_array(), "Response should have logs array");
    let logs = json["logs"].as_array().unwrap();
    assert!(!logs.is_empty(), "Should have at least one log");

    // Verify log content
    let log = &logs[0];
    assert_eq!(
        log["body"].as_str().unwrap(),
        "E2E test log message",
        "Log body should match"
    );
    assert_eq!(
        log["severity"].as_str().unwrap(),
        "INFO",
        "Severity should match"
    );
    assert_eq!(
        log["attributes"]["test_key"].as_str().unwrap(),
        "test_value",
        "Attributes should match"
    );
}

#[tokio::test]
async fn test_traces_e2e_flow() {
    // Setup
    let (storage, _temp_dir) = create_test_storage().await;
    let traces_handler = Arc::new(TracesHandler::new(
        storage.clone() as Arc<dyn StorageBackend>
    ));

    // Start dashboard server
    let (addr, _server_handle) = start_test_server(storage.clone()).await;

    // Create and send OTLP trace data
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let trace_id = vec![1u8; 16];
    let span_id = vec![2u8; 8];

    let span = Span {
        trace_id: trace_id.clone(),
        span_id: span_id.clone(),
        trace_state: String::new(),
        parent_span_id: vec![],
        name: "e2e_test_span".to_string(),
        kind: SpanKind::Internal as i32,
        start_time_unix_nano: timestamp,
        end_time_unix_nano: timestamp + 1_000_000_000, // 1 second later
        attributes: vec![KeyValue {
            key: "span_attr".to_string(),
            value: Some(AnyValue {
                value: Some(any_value::Value::StringValue("span_value".to_string())),
            }),
        }],
        dropped_attributes_count: 0,
        events: vec![],
        dropped_events_count: 0,
        links: vec![],
        dropped_links_count: 0,
        status: Some(Status {
            message: String::new(),
            code: 0,
        }),
        flags: 0,
    };

    let request = ExportTraceServiceRequest {
        resource_spans: vec![ResourceSpans {
            resource: None,
            scope_spans: vec![ScopeSpans {
                scope: None,
                spans: vec![span],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };

    // Process through handler
    traces_handler
        .process(request)
        .await
        .expect("Failed to process traces");

    // Query via HTTP API
    let client = reqwest::Client::new();
    let url = format!("http://{}/api/traces", addr);
    let response = client.get(&url).send().await.expect("Failed to query API");

    assert_eq!(response.status(), 200, "API should return 200 OK");

    let json: JsonValue = response.json().await.expect("Failed to parse JSON");

    // Verify response structure
    assert!(
        json["traces"].is_array(),
        "Response should have traces array: {:?}",
        json
    );
    let traces = json["traces"].as_array().unwrap();
    assert!(!traces.is_empty(), "Should have at least one trace");

    // Verify trace content (API returns aggregated TraceEntry, not individual spans)
    let trace = &traces[0];
    assert_eq!(
        trace["root_span_name"].as_str().unwrap(),
        "e2e_test_span",
        "Root span name should match"
    );
    assert!(
        !trace["trace_id"].as_str().unwrap().is_empty(),
        "Trace ID should be present"
    );
    assert!(
        trace["span_count"].as_u64().unwrap() > 0,
        "Should have at least one span"
    );
}

#[tokio::test]
async fn test_metrics_e2e_flow() {
    // Setup
    let (storage, _temp_dir) = create_test_storage().await;
    let metrics_handler = Arc::new(MetricsHandler::new(
        storage.clone() as Arc<dyn StorageBackend>
    ));

    // Start dashboard server
    let (addr, _server_handle) = start_test_server(storage.clone()).await;

    // Create and send OTLP metric data
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let metric = Metric {
        name: "e2e_test_metric".to_string(),
        description: "Test metric for e2e".to_string(),
        unit: "count".to_string(),
        data: Some(Data::Gauge(Gauge {
            data_points: vec![NumberDataPoint {
                attributes: vec![KeyValue {
                    key: "metric_attr".to_string(),
                    value: Some(AnyValue {
                        value: Some(any_value::Value::StringValue("metric_value".to_string())),
                    }),
                }],
                start_time_unix_nano: 0,
                time_unix_nano: timestamp,
                value: Some(MetricValue::AsDouble(123.45)),
                exemplars: vec![],
                flags: 0,
            }],
        })),
        metadata: vec![],
    };

    let request = ExportMetricsServiceRequest {
        resource_metrics: vec![ResourceMetrics {
            resource: None,
            scope_metrics: vec![ScopeMetrics {
                scope: None,
                metrics: vec![metric],
                schema_url: String::new(),
            }],
            schema_url: String::new(),
        }],
    };

    // Process through handler
    metrics_handler
        .process(request)
        .await
        .expect("Failed to process metrics");

    // Query via HTTP API
    let client = reqwest::Client::new();
    let url = format!("http://{}/api/metrics", addr);
    let response = client.get(&url).send().await.expect("Failed to query API");

    assert_eq!(response.status(), 200, "API should return 200 OK");

    let json: JsonValue = response.json().await.expect("Failed to parse JSON");

    // Verify response structure - API returns array directly for metrics
    let metrics = json.as_array().expect("Response should be an array");
    assert!(!metrics.is_empty(), "Should have at least one metric");

    // Verify metric content
    let metric = &metrics[0];
    assert_eq!(
        metric["name"].as_str().unwrap(),
        "e2e_test_metric",
        "Metric name should match"
    );

    // Verify metric value (Gauge type)
    assert_eq!(
        metric["metric_type"].as_str().unwrap(),
        "gauge",
        "Metric type should be gauge"
    );

    // Verify attributes
    assert_eq!(
        metric["attributes"]["metric_attr"].as_str().unwrap(),
        "metric_value",
        "Attributes should match"
    );
}

#[tokio::test]
async fn test_logs_severity_filter() {
    // Setup
    let (storage, _temp_dir) = create_test_storage().await;
    let logs_handler = Arc::new(LogsHandler::new(storage.clone() as Arc<dyn StorageBackend>));

    // Start dashboard server
    let (addr, _server_handle) = start_test_server(storage.clone()).await;

    // Create 10 logs with different severities
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let severities = vec![
        (5, "DEBUG"), // 2 DEBUG
        (5, "DEBUG"),
        (9, "INFO"), // 3 INFO
        (9, "INFO"),
        (9, "INFO"),
        (13, "WARN"), // 2 WARN
        (13, "WARN"),
        (17, "ERROR"), // 3 ERROR
        (17, "ERROR"),
        (17, "ERROR"),
    ];

    for (i, (severity_num, severity_text)) in severities.iter().enumerate() {
        let log_record = OtlpLogRecord {
            time_unix_nano: timestamp + (i as u64 * 1000),
            observed_time_unix_nano: timestamp + (i as u64 * 1000),
            severity_number: *severity_num,
            severity_text: severity_text.to_string(),
            body: Some(AnyValue {
                value: Some(any_value::Value::StringValue(format!(
                    "Log message {} with severity {}",
                    i, severity_text
                ))),
            }),
            attributes: vec![],
            dropped_attributes_count: 0,
            flags: 0,
            trace_id: vec![],
            span_id: vec![],
            event_name: String::new(),
        };

        let request = ExportLogsServiceRequest {
            resource_logs: vec![opentelemetry_proto::tonic::logs::v1::ResourceLogs {
                resource: None,
                scope_logs: vec![opentelemetry_proto::tonic::logs::v1::ScopeLogs {
                    scope: None,
                    log_records: vec![log_record],
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        };

        logs_handler
            .process(request)
            .await
            .expect("Failed to process log");
    }

    // Query all logs
    let client = reqwest::Client::new();
    let url = format!("http://{}/api/logs", addr);
    let response = client.get(&url).send().await.expect("Failed to query API");
    let json: JsonValue = response.json().await.expect("Failed to parse JSON");
    let all_logs = json["logs"].as_array().unwrap();
    assert_eq!(all_logs.len(), 10, "Should have all 10 logs");

    // Query with severity=ERROR filter
    let url = format!("http://{}/api/logs?severity=ERROR", addr);
    let response = client.get(&url).send().await.expect("Failed to query API");
    assert_eq!(response.status(), 200, "API should return 200 OK");

    let json: JsonValue = response.json().await.expect("Failed to parse JSON");
    let error_logs = json["logs"].as_array().unwrap();

    // Verify only ERROR logs are returned
    assert_eq!(
        error_logs.len(),
        3,
        "Should have exactly 3 ERROR logs, got: {:?}",
        error_logs
    );

    for log in error_logs {
        assert_eq!(
            log["severity"].as_str().unwrap(),
            "ERROR",
            "All returned logs should have ERROR severity"
        );
    }

    // Query with severity=WARN filter
    let url = format!("http://{}/api/logs?severity=WARN", addr);
    let response = client.get(&url).send().await.expect("Failed to query API");
    let json: JsonValue = response.json().await.expect("Failed to parse JSON");
    let warn_logs = json["logs"].as_array().unwrap();

    // Should return WARN and ERROR (min_severity filter)
    assert!(
        warn_logs.len() >= 2,
        "Should have at least 2 WARN logs (may include ERROR)"
    );

    for log in warn_logs {
        let severity = log["severity"].as_str().unwrap();
        assert!(
            severity == "WARN" || severity == "ERROR",
            "Logs should be WARN or higher severity"
        );
    }
}

// ── #83: full-stack e2e: HTTP OTLP receiver → shared storage → API → otelite-client ──

mod http_test_utils_e2e {
    // Re-declare the minimal payload helper inline so this test module
    // is self-contained without re-exporting from http_test_utils.
    pub fn logs_json(service: &str, body: &str) -> String {
        serde_json::json!({
            "resourceLogs": [{
                "resource": {
                    "attributes": [{
                        "key": "service.name",
                        "value": {"stringValue": service}
                    }]
                },
                "scopeLogs": [{
                    "scope": {"name": "e2e_test", "version": "1.0.0"},
                    "logRecords": [{
                        "timeUnixNano": "1700000000000000000",
                        "observedTimeUnixNano": "1700000000000000000",
                        "severityNumber": 9,
                        "severityText": "INFO",
                        "body": {"stringValue": body}
                    }]
                }]
            }]
        })
        .to_string()
    }
}

/// End-to-end wiring test (#83):
///
/// 1. OTLP HTTP receiver starts on an ephemeral port backed by shared SQLite storage.
/// 2. A real OTLP HTTP/JSON log payload is POSTed to `POST /v1/logs`.
/// 3. The dashboard API starts on a second ephemeral port, sharing the same storage.
/// 4. `otelite_client::ApiClient` queries `GET /api/logs` and asserts the log arrived.
///
/// This catches wiring regressions in the OTLP receiver → storage → API router chain
/// that in-process handler tests or API tests writing directly to storage would not.
#[tokio::test]
async fn test_e2e_otlp_http_receiver_to_api_client() {
    use otelite_api::{config::DashboardConfig, server::DashboardServer};
    use otelite_client::ApiClient;
    use otelite_receiver::{config::ReceiverConfig, http::HttpServer};
    use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
    use std::{sync::Arc, time::Duration};
    use tempfile::TempDir;
    use tokio::net::TcpListener;
    use tokio::time::sleep;

    // ── shared storage ────────────────────────────────────────────────────────
    let temp_dir = TempDir::new().expect("temp dir");
    let storage_config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(storage_config);
    backend.initialize().await.expect("storage init");
    let storage: Arc<dyn StorageBackend> = Arc::new(backend);

    // ── OTLP HTTP receiver on port 0 ─────────────────────────────────────────
    let mut recv_cfg = ReceiverConfig::new();
    recv_cfg.http_addr = "127.0.0.1:0".parse().unwrap();
    let receiver = HttpServer::new(recv_cfg);
    receiver
        .start(Arc::clone(&storage))
        .await
        .expect("receiver start");
    sleep(Duration::from_millis(50)).await;
    let recv_addr = receiver.local_addr().await.expect("receiver local_addr");

    // ── POST a real OTLP JSON log payload ─────────────────────────────────────
    let payload = http_test_utils_e2e::logs_json("e2e_service", "hello from e2e test");
    let http = reqwest::Client::new();
    let status = http
        .post(format!("http://{}/v1/logs", recv_addr))
        .header("Content-Type", "application/json")
        .body(payload)
        .send()
        .await
        .expect("POST /v1/logs")
        .status();
    assert!(
        status.is_success(),
        "OTLP receiver rejected the payload: {status}"
    );

    // Give the receiver a moment to flush to storage
    sleep(Duration::from_millis(100)).await;

    // ── Dashboard API on a second ephemeral port ──────────────────────────────
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("API listener");
    let api_addr = listener.local_addr().expect("API local_addr");
    let api_cfg = DashboardConfig::default().with_bind_address(api_addr);
    let server = DashboardServer::new(api_cfg, Arc::clone(&storage));
    let router = server.build_router();
    tokio::spawn(async move {
        axum::serve(listener, router.into_make_service())
            .await
            .expect("API serve");
    });
    sleep(Duration::from_millis(50)).await;

    // ── Query via ApiClient ───────────────────────────────────────────────────
    let client = ApiClient::new(format!("http://{}", api_addr), Duration::from_secs(10)).unwrap();
    let logs = client
        .fetch_logs(vec![("limit", "50".to_string())])
        .await
        .expect("fetch_logs via ApiClient");

    assert!(
        !logs.logs.is_empty(),
        "Expected at least one log from the OTLP payload, got none"
    );
    let found = logs.logs.iter().any(|l| l.body == "hello from e2e test");
    assert!(
        found,
        "Log body 'hello from e2e test' not found in API response; got: {:?}",
        logs.logs
    );
    // Keep temp_dir alive until the end of the test
    drop(temp_dir);
}
