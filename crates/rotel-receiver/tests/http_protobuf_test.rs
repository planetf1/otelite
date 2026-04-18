// Integration tests for HTTP/Protobuf endpoints

mod http_test_utils;

use http_test_utils::{
    create_empty_protobuf, create_invalid_protobuf, create_logs_protobuf, create_metrics_protobuf,
    create_traces_protobuf, decode_logs_protobuf, decode_metrics_protobuf, decode_traces_protobuf,
};
use rotel_receiver::config::ReceiverConfig;
use rotel_receiver::http::HttpServer;
use rotel_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
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
async fn test_http_metrics_endpoint_success() {
    let (base_url, server) = start_test_server().await;

    // Create test request
    let protobuf_data = create_metrics_protobuf();
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/metrics", base_url))
        .header("Content-Type", "application/x-protobuf")
        .body(protobuf_data.to_vec())
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    // Cleanup
    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_http_logs_endpoint_success() {
    let (base_url, server) = start_test_server().await;

    // Create test request
    let protobuf_data = create_logs_protobuf();
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/logs", base_url))
        .header("Content-Type", "application/x-protobuf")
        .body(protobuf_data.to_vec())
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    // Cleanup
    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_http_traces_endpoint_success() {
    let (base_url, server) = start_test_server().await;

    // Create test request
    let protobuf_data = create_traces_protobuf();
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/traces", base_url))
        .header("Content-Type", "application/x-protobuf")
        .body(protobuf_data.to_vec())
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    // Cleanup
    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_http_invalid_protobuf() {
    let (base_url, server) = start_test_server().await;

    // Create invalid protobuf data
    let body = create_invalid_protobuf();
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/metrics", base_url))
        .header("Content-Type", "application/x-protobuf")
        .body(body.to_vec())
        .send()
        .await
        .expect("Failed to send request");

    // Should return 400 Bad Request for invalid protobuf
    assert_eq!(response.status(), 400);

    // Cleanup
    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_http_empty_protobuf() {
    let (base_url, server) = start_test_server().await;

    // Create empty protobuf data
    let body = create_empty_protobuf();
    let client = reqwest::Client::new();
    let response = client
        .post(format!("{}/v1/metrics", base_url))
        .header("Content-Type", "application/x-protobuf")
        .body(body.to_vec())
        .send()
        .await
        .expect("Failed to send request");

    // Empty protobuf should be accepted (200 OK)
    assert_eq!(response.status(), 200);

    // Cleanup
    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_http_server_health_check() {
    let (base_url, server) = start_test_server().await;

    // Check health endpoint
    let client = reqwest::Client::new();
    let response = client
        .get(format!("{}/health", base_url))
        .send()
        .await
        .expect("Failed to send request");

    assert_eq!(response.status(), 200);

    // Verify health checker is ready
    assert!(server.health_checker().is_ready());
    assert!(server.health_checker().is_alive());

    // Cleanup
    server.shutdown();
    sleep(Duration::from_millis(100)).await;

    // After shutdown, should not be ready
    assert!(!server.health_checker().is_ready());
}

#[tokio::test]
async fn test_http_concurrent_requests() {
    let (base_url, server) = start_test_server().await;

    // Send concurrent requests
    let client = reqwest::Client::new();
    let mut handles = vec![];

    for _ in 0..10 {
        let client = client.clone();
        let base_url = base_url.clone();
        let protobuf_data = create_metrics_protobuf();
        let handle = tokio::spawn(async move {
            client
                .post(format!("{}/v1/metrics", base_url))
                .header("Content-Type", "application/x-protobuf")
                .body(protobuf_data.to_vec())
                .send()
                .await
                .expect("Failed to send request")
        });
        handles.push(handle);
    }

    // Wait for all requests to complete
    for handle in handles {
        let response = handle.await.expect("Task failed");
        assert_eq!(response.status(), 200);
    }

    // Cleanup
    server.shutdown();
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_http_protobuf_encoding_decoding() {
    // Test that we can encode and decode protobuf correctly
    let metrics_data = create_metrics_protobuf();
    let logs_data = create_logs_protobuf();
    let traces_data = create_traces_protobuf();

    // Verify all data is non-empty
    assert!(!metrics_data.is_empty());
    assert!(!logs_data.is_empty());
    assert!(!traces_data.is_empty());

    // Verify data can be decoded
    let metrics_result = decode_metrics_protobuf(&metrics_data);
    let logs_result = decode_logs_protobuf(&logs_data);
    let traces_result = decode_traces_protobuf(&traces_data);

    assert!(metrics_result.is_ok());
    assert!(logs_result.is_ok());
    assert!(traces_result.is_ok());
}

// Made with Bob
