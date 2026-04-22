// Integration tests for HTTP/JSON OTLP endpoints

mod http_test_utils;

use http_test_utils::{
    create_invalid_json, create_logs_json, create_malformed_json, create_metrics_json,
    create_traces_json,
};
use otelite_receiver::config::ReceiverConfig;
use otelite_receiver::http::HttpServer;
use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
use reqwest::StatusCode;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

/// Helper to start HTTP server on random port and return the address
async fn start_test_server() -> (String, HttpServer) {
    let mut config = ReceiverConfig::new();
    // Use port 0 to let OS assign a random available port
    config.http_addr = "127.0.0.1:0".parse().expect("Failed to parse address");

    let server = HttpServer::new(config);

    // Create storage backend
    let mut storage = SqliteBackend::new(StorageConfig::default());
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    server.start(storage).await.expect("Failed to start server");

    // Wait for server to be ready and get actual bound address
    sleep(Duration::from_millis(100)).await;
    let addr = server
        .local_addr()
        .await
        .expect("Failed to get local address");

    (format!("http://{}", addr), server)
}

#[tokio::test]
async fn test_http_json_metrics_success() {
    let (base_url, _server) = start_test_server().await;

    let client = reqwest::Client::new();
    let json_data = create_metrics_json();

    let response = client
        .post(format!("{}/v1/metrics", base_url))
        .header("Content-Type", "application/json")
        .body(json_data)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["status"], "success");
}

#[tokio::test]
async fn test_http_json_logs_success() {
    let (base_url, _server) = start_test_server().await;

    let client = reqwest::Client::new();
    let json_data = create_logs_json();

    let response = client
        .post(format!("{}/v1/logs", base_url))
        .header("Content-Type", "application/json")
        .body(json_data)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["status"], "success");
}

#[tokio::test]
async fn test_http_json_traces_success() {
    let (base_url, _server) = start_test_server().await;

    let client = reqwest::Client::new();
    let json_data = create_traces_json();

    let response = client
        .post(format!("{}/v1/traces", base_url))
        .header("Content-Type", "application/json")
        .body(json_data)
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), StatusCode::OK);

    let body: serde_json::Value = response.json().await.expect("Failed to parse response");
    assert_eq!(body["status"], "success");
}

#[tokio::test]
async fn test_http_json_invalid_json() {
    let (base_url, _server) = start_test_server().await;

    let client = reqwest::Client::new();
    let invalid_json = create_invalid_json();

    let response = client
        .post(format!("{}/v1/metrics", base_url))
        .header("Content-Type", "application/json")
        .body(invalid_json)
        .send()
        .await
        .expect("Failed to send request");

    // Should return 400 Bad Request for invalid JSON
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_http_json_malformed_structure() {
    let (base_url, _server) = start_test_server().await;

    let client = reqwest::Client::new();
    let malformed_json = create_malformed_json();

    let response = client
        .post(format!("{}/v1/metrics", base_url))
        .header("Content-Type", "application/json")
        .body(malformed_json)
        .send()
        .await
        .expect("Failed to send request");

    // Should reject malformed JSON structure with 400 Bad Request
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_http_json_empty_body() {
    let (base_url, _server) = start_test_server().await;

    let client = reqwest::Client::new();

    let response = client
        .post(format!("{}/v1/metrics", base_url))
        .header("Content-Type", "application/json")
        .body("")
        .send()
        .await
        .expect("Failed to send request");

    // Empty body is treated as valid JSON (empty object) by serde_json
    // Our current implementation accepts it and returns OK with empty protobuf structures
    // This is acceptable behavior - empty telemetry data is valid
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_http_json_charset_handling() {
    let (base_url, _server) = start_test_server().await;

    let client = reqwest::Client::new();
    let json_data = create_metrics_json();

    let response = client
        .post(format!("{}/v1/metrics", base_url))
        .header("Content-Type", "application/json; charset=utf-8")
        .body(json_data)
        .send()
        .await
        .expect("Failed to send request");

    // Should handle charset parameter in Content-Type
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_http_json_all_signals() {
    let (base_url, _server) = start_test_server().await;

    let client = reqwest::Client::new();

    // Test metrics
    let metrics_response = client
        .post(format!("{}/v1/metrics", base_url))
        .header("Content-Type", "application/json")
        .body(create_metrics_json())
        .send()
        .await
        .expect("Failed to send metrics");
    assert_eq!(metrics_response.status(), StatusCode::OK);

    // Test logs
    let logs_response = client
        .post(format!("{}/v1/logs", base_url))
        .header("Content-Type", "application/json")
        .body(create_logs_json())
        .send()
        .await
        .expect("Failed to send logs");
    assert_eq!(logs_response.status(), StatusCode::OK);

    // Test traces
    let traces_response = client
        .post(format!("{}/v1/traces", base_url))
        .header("Content-Type", "application/json")
        .body(create_traces_json())
        .send()
        .await
        .expect("Failed to send traces");
    assert_eq!(traces_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_http_json_concurrent_requests() {
    let (base_url, _server) = start_test_server().await;

    let client = reqwest::Client::new();
    let mut handles = vec![];

    // Send 10 concurrent JSON requests
    for _ in 0..10 {
        let client = client.clone();
        let url = format!("{}/v1/metrics", base_url);
        let json_data = create_metrics_json();

        let handle = tokio::spawn(async move {
            client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(json_data)
                .send()
                .await
                .expect("Failed to send request")
        });

        handles.push(handle);
    }

    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.expect("Task panicked");
        assert_eq!(response.status(), StatusCode::OK);
    }
}
