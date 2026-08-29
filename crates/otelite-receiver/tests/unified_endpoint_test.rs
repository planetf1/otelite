// Tests for the legacy unified OTLP endpoint (/v1/otlp) (#16):
// it sniffs the protobuf payload and routes to the matching signal
// handler, and rejects bodies that parse as no known signal type.
//
// The server binds 127.0.0.1:0 (OS-assigned free port) and uses a fresh
// TempDir database.

mod http_test_utils;

use http_test_utils::{create_invalid_protobuf, create_logs_protobuf, create_traces_protobuf};
use otelite_core::storage::{QueryParams, StorageBackend};
use otelite_receiver::config::ReceiverConfig;
use otelite_receiver::http::HttpServer;
use otelite_storage::{sqlite::SqliteBackend, StorageConfig};
use std::sync::Arc;
use std::time::Duration;
use tempfile::TempDir;

async fn start_server() -> (String, HttpServer, TempDir, Arc<dyn StorageBackend>) {
    let config = ReceiverConfig::new().with_http_addr("127.0.0.1:0".parse().unwrap());
    let server = HttpServer::new(config);

    let temp_dir = TempDir::new().expect("temp dir");
    let storage_config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut storage = SqliteBackend::new(storage_config);
    storage.initialize().await.expect("init storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    server.start(storage.clone()).await.expect("start server");
    tokio::time::sleep(Duration::from_millis(100)).await;
    let addr = server.local_addr().await.expect("bound address");

    (format!("http://{addr}"), server, temp_dir, storage)
}

/// A trace protobuf posted to the legacy unified endpoint must be
/// processed as a trace — not just accepted.
#[tokio::test]
async fn test_unified_endpoint_routes_trace_protobuf() {
    let (base, server, _temp, storage) = start_server().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/v1/otlp"))
        .header("Content-Type", "application/x-protobuf")
        .body(create_traces_protobuf())
        .send()
        .await
        .expect("send");

    assert_eq!(
        response.status(),
        200,
        "unified endpoint must accept a trace protobuf"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    assert_eq!(body["status"], "success");

    // The span must actually be stored, not merely acknowledged.
    let spans = storage
        .query_spans(&QueryParams::default())
        .await
        .expect("query spans");
    assert!(
        !spans.is_empty(),
        "span from unified endpoint must be persisted"
    );

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// A log protobuf posted to the legacy unified endpoint must be
/// processed as a log.
#[tokio::test]
async fn test_unified_endpoint_routes_log_protobuf() {
    let (base, server, _temp, storage) = start_server().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/v1/otlp"))
        .header("Content-Type", "application/x-protobuf")
        .body(create_logs_protobuf())
        .send()
        .await
        .expect("send");

    assert_eq!(
        response.status(),
        200,
        "unified endpoint must accept a log protobuf"
    );

    // The log must actually be stored, not merely acknowledged.
    let logs = storage
        .query_logs(&QueryParams::default())
        .await
        .expect("query logs");
    assert!(
        !logs.is_empty(),
        "log from unified endpoint must be persisted"
    );

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// A body that parses as no known OTLP signal type must be rejected with
/// a 4xx client error, not a 500.
#[tokio::test]
async fn test_unified_endpoint_rejects_unknown_content() {
    let (base, server, _temp, storage) = start_server().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/v1/otlp"))
        .header("Content-Type", "application/x-protobuf")
        .body(create_invalid_protobuf())
        .send()
        .await
        .expect("send");

    let status = response.status();
    assert!(
        (400..500).contains(&status.as_u16()),
        "unknown content must be a client error, got {status}"
    );

    // Nothing must have been stored.
    let spans = storage
        .query_spans(&QueryParams::default())
        .await
        .expect("query spans");
    let logs = storage
        .query_logs(&QueryParams::default())
        .await
        .expect("query logs");
    let metrics = storage
        .query_metrics(&QueryParams::default())
        .await
        .expect("query metrics");
    assert!(spans.is_empty() && logs.is_empty() && metrics.is_empty());

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}
