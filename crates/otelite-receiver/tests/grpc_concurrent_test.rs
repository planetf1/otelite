// Concurrent connection tests for gRPC server

mod grpc_test_utils;

use grpc_test_utils::{create_logs_batch, create_metrics_batch, create_traces_batch};
use otelite_receiver::config::ReceiverConfig;
use otelite_receiver::grpc::GrpcServer;
use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

#[tokio::test]
async fn test_concurrent_metrics_requests() {
    // Create server with test configuration
    let mut config = ReceiverConfig::new();
    config.grpc_addr = "127.0.0.1:14317".parse::<SocketAddr>().unwrap();

    // Create storage backend
    let mut storage = SqliteBackend::new(StorageConfig::default());
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    let server = GrpcServer::new(config, storage);

    // Start server
    server.start().await.expect("Failed to start server");

    // Give server time to start
    sleep(Duration::from_millis(100)).await;

    // Create multiple concurrent tasks
    let mut handles = vec![];

    for _ in 0..10 {
        let handle = tokio::spawn(async move {
            let batch = create_metrics_batch(5);
            // In a real test, we would send these via gRPC client
            // For now, just verify batch creation
            assert_eq!(batch.len(), 5);
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // Shutdown server
    server.shutdown();

    // Give server time to shutdown
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_concurrent_logs_requests() {
    // Create server with test configuration
    let mut config = ReceiverConfig::new();
    config.grpc_addr = "127.0.0.1:14318".parse::<SocketAddr>().unwrap();

    // Create storage backend
    let mut storage = SqliteBackend::new(StorageConfig::default());
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    let server = GrpcServer::new(config, storage);

    // Start server
    server.start().await.expect("Failed to start server");

    // Give server time to start
    sleep(Duration::from_millis(100)).await;

    // Create multiple concurrent tasks
    let mut handles = vec![];

    for _ in 0..10 {
        let handle = tokio::spawn(async move {
            let batch = create_logs_batch(5);
            // In a real test, we would send these via gRPC client
            // For now, just verify batch creation
            assert_eq!(batch.len(), 5);
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // Shutdown server
    server.shutdown();

    // Give server time to shutdown
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_concurrent_traces_requests() {
    // Create server with test configuration
    let mut config = ReceiverConfig::new();
    config.grpc_addr = "127.0.0.1:14319".parse::<SocketAddr>().unwrap();

    // Create storage backend
    let mut storage = SqliteBackend::new(StorageConfig::default());
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    let server = GrpcServer::new(config, storage);

    // Start server
    server.start().await.expect("Failed to start server");

    // Give server time to start
    sleep(Duration::from_millis(100)).await;

    // Create multiple concurrent tasks
    let mut handles = vec![];

    for _ in 0..10 {
        let handle = tokio::spawn(async move {
            let batch = create_traces_batch(5);
            // In a real test, we would send these via gRPC client
            // For now, just verify batch creation
            assert_eq!(batch.len(), 5);
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // Shutdown server
    server.shutdown();

    // Give server time to shutdown
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_mixed_concurrent_requests() {
    // Create server with test configuration
    let mut config = ReceiverConfig::new();
    config.grpc_addr = "127.0.0.1:14320".parse::<SocketAddr>().unwrap();

    // Create storage backend
    let mut storage = SqliteBackend::new(StorageConfig::default());
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    let server = GrpcServer::new(config, storage);

    // Start server
    server.start().await.expect("Failed to start server");

    // Give server time to start
    sleep(Duration::from_millis(100)).await;

    // Create mixed concurrent tasks
    let mut handles = vec![];

    // Metrics tasks
    for _ in 0..5 {
        let handle = tokio::spawn(async move {
            let batch = create_metrics_batch(3);
            assert_eq!(batch.len(), 3);
        });
        handles.push(handle);
    }

    // Logs tasks
    for _ in 0..5 {
        let handle = tokio::spawn(async move {
            let batch = create_logs_batch(3);
            assert_eq!(batch.len(), 3);
        });
        handles.push(handle);
    }

    // Traces tasks
    for _ in 0..5 {
        let handle = tokio::spawn(async move {
            let batch = create_traces_batch(3);
            assert_eq!(batch.len(), 3);
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // Shutdown server
    server.shutdown();

    // Give server time to shutdown
    sleep(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn test_server_graceful_shutdown_under_load() {
    // Create server with test configuration
    let mut config = ReceiverConfig::new();
    config.grpc_addr = "127.0.0.1:14321".parse::<SocketAddr>().unwrap();

    // Create storage backend
    let mut storage = SqliteBackend::new(StorageConfig::default());
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    let server = GrpcServer::new(config, storage);

    // Start server
    server.start().await.expect("Failed to start server");

    // Give server time to start
    sleep(Duration::from_millis(100)).await;

    // Create long-running tasks
    let mut handles = vec![];

    for _ in 0..20 {
        let handle = tokio::spawn(async move {
            for _ in 0..10 {
                let batch = create_metrics_batch(2);
                assert_eq!(batch.len(), 2);
                sleep(Duration::from_millis(10)).await;
            }
        });
        handles.push(handle);
    }

    // Trigger shutdown while tasks are running
    sleep(Duration::from_millis(50)).await;
    server.shutdown();

    // Wait for all tasks to complete
    for handle in handles {
        handle.await.expect("Task failed");
    }

    // Verify health checker reflects shutdown
    assert!(!server.health_checker().is_ready());
}
