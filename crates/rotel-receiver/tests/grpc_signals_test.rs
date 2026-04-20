// Integration tests for gRPC signal handlers

mod grpc_test_utils;

use grpc_test_utils::{
    create_sample_logs_request, create_sample_metrics_request, create_sample_traces_request,
};
use rotel_receiver::signals::{LogsHandler, MetricsHandler, TracesHandler};
use rotel_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
use std::sync::Arc;

async fn create_test_storage() -> Arc<dyn StorageBackend> {
    let mut storage = SqliteBackend::new(StorageConfig::default());
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    Arc::new(storage)
}

#[tokio::test]
async fn test_metrics_handler_with_grpc_request() {
    let storage = create_test_storage().await;
    let handler = Arc::new(MetricsHandler::new(storage));
    let request = create_sample_metrics_request();

    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Metrics handler should process request successfully"
    );
}

#[tokio::test]
async fn test_logs_handler_with_grpc_request() {
    let storage = create_test_storage().await;
    let handler = Arc::new(LogsHandler::new(storage));
    let request = create_sample_logs_request();

    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Logs handler should process request successfully"
    );
}

#[tokio::test]
async fn test_traces_handler_with_grpc_request() {
    let storage = create_test_storage().await;
    let handler = Arc::new(TracesHandler::new(storage));
    let request = create_sample_traces_request();

    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Traces handler should process request successfully"
    );
}

#[tokio::test]
async fn test_metrics_handler_with_empty_request() {
    let storage = create_test_storage().await;
    let handler = Arc::new(MetricsHandler::new(storage));
    let request = opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest {
        resource_metrics: vec![],
    };

    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Empty metrics request should be handled gracefully"
    );
}

#[tokio::test]
async fn test_logs_handler_with_empty_request() {
    let storage = create_test_storage().await;
    let handler = Arc::new(LogsHandler::new(storage));
    let request = opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest {
        resource_logs: vec![],
    };

    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Empty logs request should be handled gracefully"
    );
}

#[tokio::test]
async fn test_traces_handler_with_empty_request() {
    let storage = create_test_storage().await;
    let handler = Arc::new(TracesHandler::new(storage));
    let request = opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest {
        resource_spans: vec![],
    };

    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Empty traces request should be handled gracefully"
    );
}

#[tokio::test]
async fn test_multiple_metrics_requests() {
    let storage = create_test_storage().await;
    let handler = Arc::new(MetricsHandler::new(storage));

    for _ in 0..10 {
        let request = create_sample_metrics_request();
        let result = handler.process(request).await;
        assert!(
            result.is_ok(),
            "Metrics handler should process multiple requests"
        );
    }
}

#[tokio::test]
async fn test_multiple_logs_requests() {
    let storage = create_test_storage().await;
    let handler = Arc::new(LogsHandler::new(storage));

    for _ in 0..10 {
        let request = create_sample_logs_request();
        let result = handler.process(request).await;
        assert!(
            result.is_ok(),
            "Logs handler should process multiple requests"
        );
    }
}

#[tokio::test]
async fn test_multiple_traces_requests() {
    let storage = create_test_storage().await;
    let handler = Arc::new(TracesHandler::new(storage));

    for _ in 0..10 {
        let request = create_sample_traces_request();
        let result = handler.process(request).await;
        assert!(
            result.is_ok(),
            "Traces handler should process multiple requests"
        );
    }
}
