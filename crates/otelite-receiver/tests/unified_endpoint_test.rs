// Tests for the legacy unified OTLP endpoint (/v1/otlp).
//
// #16 added it as a sniff-and-route fallback; #256 removed the trial
// decode: protobuf wire types routinely parse as the *wrong* message, so
// a logs export could be silently stored as traces, and the endpoint
// ignored Content-Encoding. The endpoint now fails loudly with a 400
// pointing at the per-signal endpoints — a client error, so exporters do
// not loop retrying it as if it were a transient server fault.
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

/// A valid trace protobuf posted to the deprecated unified endpoint must
/// be rejected with a 400 that names the per-signal endpoints — and must
/// NOT be stored as anything (the old trial-decode stored it as a trace).
#[tokio::test]
async fn test_unified_endpoint_rejects_valid_trace_with_400_and_stores_nothing() {
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
        400,
        "the deprecated endpoint must fail loudly with 400 (not 200, not 5xx)"
    );
    let body: serde_json::Value = response.json().await.expect("json body");
    let error = body["error"].as_str().expect("error message");
    for endpoint in ["/v1/traces", "/v1/metrics", "/v1/logs"] {
        assert!(
            error.contains(endpoint),
            "the 400 must point at {endpoint}: {error}"
        );
    }

    // Nothing may have been stored — no mis-routing.
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
    assert!(
        spans.is_empty() && logs.is_empty() && metrics.is_empty(),
        "the deprecated endpoint must never store anything"
    );

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// Same contract for logs and garbage bodies: 400, nothing stored.
#[tokio::test]
async fn test_unified_endpoint_rejects_logs_and_garbage() {
    let (base, server, _temp, storage) = start_server().await;
    let client = reqwest::Client::new();

    for (name, body) in [
        ("logs protobuf", create_logs_protobuf()),
        ("unparseable body", create_invalid_protobuf()),
    ] {
        let response = client
            .post(format!("{base}/v1/otlp"))
            .header("Content-Type", "application/x-protobuf")
            .body(body)
            .send()
            .await
            .expect("send");
        assert_eq!(response.status(), 400, "{name} via /v1/otlp must be a 400");
    }

    let spans = storage
        .query_spans(&QueryParams::default())
        .await
        .expect("query spans");
    let logs = storage
        .query_logs(&QueryParams::default())
        .await
        .expect("query logs");
    assert!(spans.is_empty() && logs.is_empty(), "nothing may be stored");

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}

/// The per-signal endpoints keep working (the 400 points at them): a trace
/// POSTed to /v1/traces is still stored.
#[tokio::test]
async fn test_per_signal_endpoints_still_work() {
    let (base, server, _temp, storage) = start_server().await;
    let client = reqwest::Client::new();

    let response = client
        .post(format!("{base}/v1/traces"))
        .header("Content-Type", "application/x-protobuf")
        .body(create_traces_protobuf())
        .send()
        .await
        .expect("send");
    assert_eq!(
        response.status(),
        200,
        "/v1/traces must keep accepting traces"
    );

    let spans = storage
        .query_spans(&QueryParams::default())
        .await
        .expect("query spans");
    assert!(!spans.is_empty(), "span from /v1/traces must be persisted");

    server.shutdown();
    tokio::time::sleep(Duration::from_millis(100)).await;
}
