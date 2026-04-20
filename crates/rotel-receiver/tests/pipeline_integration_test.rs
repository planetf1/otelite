// Pipeline integration test: OTLP to SQLite roundtrip
// Tests the full data flow from OTLP ingestion through storage to query

mod grpc_test_utils;

use grpc_test_utils::{
    create_logs_batch, create_metrics_batch, create_sample_logs_request,
    create_sample_metrics_request, create_sample_traces_request, create_traces_batch,
};
use opentelemetry_proto::tonic::collector::logs::v1::ExportLogsServiceRequest;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use opentelemetry_proto::tonic::common::v1::{any_value, AnyValue, KeyValue};
use opentelemetry_proto::tonic::logs::v1::LogRecord as OtlpLogRecord;
use rotel_core::telemetry::log::SeverityLevel;
use rotel_receiver::signals::{LogsHandler, MetricsHandler, TracesHandler};
use rotel_storage::{sqlite::SqliteBackend, QueryParams, StorageBackend, StorageConfig};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

async fn create_test_storage() -> Arc<SqliteBackend> {
    // Use in-memory database with unique name for test isolation
    let unique_id = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let db_path = format!(":memory:?cache=shared&mode=memory&name=test_{}", unique_id);

    let config = StorageConfig::default().with_data_dir(db_path.into());
    let mut storage = SqliteBackend::new(config);
    storage
        .initialize()
        .await
        .expect("Failed to initialize storage");
    Arc::new(storage)
}

#[tokio::test]
async fn test_logs_pipeline_roundtrip() {
    // Create storage and handler
    let storage = create_test_storage().await;
    let handler = Arc::new(LogsHandler::new(storage.clone() as Arc<dyn StorageBackend>));

    // Create request with known data
    let request = create_sample_logs_request();

    // Extract expected values from request for verification
    let expected_body = "Test log message";
    let expected_severity = SeverityLevel::Info; // severity 9 = INFO
    let expected_attr_key = "log_key";
    let expected_attr_value = "log_value";

    // Process the request
    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Handler should process request successfully"
    );

    // Query storage to verify data was persisted
    let query_result = storage.query_logs(&QueryParams::default()).await;
    assert!(
        query_result.is_ok(),
        "Storage query should succeed: {:?}",
        query_result.err()
    );

    let logs = query_result.unwrap();
    assert!(!logs.is_empty(), "Storage should contain at least one log");

    // Verify the log data matches what was sent
    let log = &logs[0];
    assert_eq!(log.body, expected_body, "Log body should match sent data");
    assert_eq!(log.severity, expected_severity, "Severity should match");

    // Verify attributes
    assert_eq!(
        log.attributes.get(expected_attr_key),
        Some(&expected_attr_value.to_string()),
        "Attributes should match"
    );

    // Verify timestamp is reasonable (within last minute)
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;
    assert!(
        log.timestamp > now - 60_000_000_000,
        "Timestamp should be recent"
    );
}

#[tokio::test]
async fn test_traces_pipeline_roundtrip() {
    // Create storage and handler
    let storage = create_test_storage().await;
    let handler = Arc::new(TracesHandler::new(
        storage.clone() as Arc<dyn StorageBackend>
    ));

    // Create request with known data
    let request = create_sample_traces_request();

    // Extract expected values
    let expected_span_name = "test_span";
    let expected_attr_key = "span_key";
    let expected_attr_value = "span_value";

    // Process the request
    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Handler should process request successfully"
    );

    // Query storage to verify data was persisted
    let query_result = storage.query_spans(&QueryParams::default()).await;
    assert!(
        query_result.is_ok(),
        "Storage query should succeed: {:?}",
        query_result.err()
    );

    let spans = query_result.unwrap();
    assert!(
        !spans.is_empty(),
        "Storage should contain at least one span"
    );

    // Verify the span data matches what was sent
    let span = &spans[0];
    assert_eq!(
        span.name, expected_span_name,
        "Span name should match sent data"
    );

    // Verify trace_id and span_id are present
    assert!(!span.trace_id.is_empty(), "Trace ID should be present");
    assert!(!span.span_id.is_empty(), "Span ID should be present");

    // Verify attributes
    assert_eq!(
        span.attributes.get(expected_attr_key),
        Some(&expected_attr_value.to_string()),
        "Attributes should match"
    );

    // Verify timestamps
    assert!(span.start_time > 0, "Start time should be set");
    assert!(
        span.end_time > span.start_time,
        "End time should be after start time"
    );
}

#[tokio::test]
async fn test_metrics_pipeline_roundtrip() {
    // Create storage and handler
    let storage = create_test_storage().await;
    let handler = Arc::new(MetricsHandler::new(
        storage.clone() as Arc<dyn StorageBackend>
    ));

    // Create request with known data
    let request = create_sample_metrics_request();

    // Extract expected values
    let expected_metric_name = "test_metric";
    let expected_value = 42.0;
    let expected_attr_key = "test_key";
    let expected_attr_value = "test_value";

    // Process the request
    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Handler should process request successfully"
    );

    // Query storage to verify data was persisted
    let query_result = storage.query_metrics(&QueryParams::default()).await;
    assert!(
        query_result.is_ok(),
        "Storage query should succeed: {:?}",
        query_result.err()
    );

    let metrics = query_result.unwrap();
    assert!(
        !metrics.is_empty(),
        "Storage should contain at least one metric"
    );

    // Verify the metric data matches what was sent
    let metric = &metrics[0];
    assert_eq!(
        metric.name, expected_metric_name,
        "Metric name should match sent data"
    );

    // Verify metric value (it's stored as MetricType::Gauge)
    if let rotel_core::telemetry::metric::MetricType::Gauge(value) = metric.metric_type {
        assert_eq!(value, expected_value, "Metric value should match");
    } else {
        panic!("Expected Gauge metric type");
    }

    // Verify attributes
    assert_eq!(
        metric.attributes.get(expected_attr_key),
        Some(&expected_attr_value.to_string()),
        "Attributes should match"
    );

    // Verify timestamp is reasonable
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;
    assert!(
        metric.timestamp > now - 60_000_000_000,
        "Timestamp should be recent"
    );
}

#[tokio::test]
async fn test_empty_logs_request() {
    let storage = create_test_storage().await;
    let handler = Arc::new(LogsHandler::new(storage.clone() as Arc<dyn StorageBackend>));

    let request = ExportLogsServiceRequest {
        resource_logs: vec![],
    };

    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Empty logs request should be handled gracefully"
    );

    // Verify no logs were added
    let logs = storage.query_logs(&QueryParams::default()).await.unwrap();
    assert!(
        logs.is_empty(),
        "No logs should be stored from empty request"
    );
}

#[tokio::test]
async fn test_empty_traces_request() {
    let storage = create_test_storage().await;
    let handler = Arc::new(TracesHandler::new(
        storage.clone() as Arc<dyn StorageBackend>
    ));

    let request = ExportTraceServiceRequest {
        resource_spans: vec![],
    };

    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Empty traces request should be handled gracefully"
    );

    // Verify no spans were added
    let spans = storage.query_spans(&QueryParams::default()).await.unwrap();
    assert!(
        spans.is_empty(),
        "No spans should be stored from empty request"
    );
}

#[tokio::test]
async fn test_empty_metrics_request() {
    let storage = create_test_storage().await;
    let handler = Arc::new(MetricsHandler::new(
        storage.clone() as Arc<dyn StorageBackend>
    ));

    let request = ExportMetricsServiceRequest {
        resource_metrics: vec![],
    };

    let result = handler.process(request).await;
    assert!(
        result.is_ok(),
        "Empty metrics request should be handled gracefully"
    );

    // Verify no metrics were added
    let metrics = storage
        .query_metrics(&QueryParams::default())
        .await
        .unwrap();
    assert!(
        metrics.is_empty(),
        "No metrics should be stored from empty request"
    );
}

#[tokio::test]
async fn test_large_logs_batch() {
    let storage = create_test_storage().await;
    let handler = Arc::new(LogsHandler::new(storage.clone() as Arc<dyn StorageBackend>));

    // Create and process 100 log requests
    let batch = create_logs_batch(100);
    for request in batch {
        let result = handler.process(request).await;
        assert!(result.is_ok(), "Handler should process large batch");
    }

    // Verify all logs were stored
    let logs = storage.query_logs(&QueryParams::default()).await.unwrap();
    assert_eq!(logs.len(), 100, "All 100 logs should be stored");
}

#[tokio::test]
async fn test_large_traces_batch() {
    let storage = create_test_storage().await;
    let handler = Arc::new(TracesHandler::new(
        storage.clone() as Arc<dyn StorageBackend>
    ));

    // Create and process 100 trace requests
    let batch = create_traces_batch(100);
    for request in batch {
        let result = handler.process(request).await;
        assert!(result.is_ok(), "Handler should process large batch");
    }

    // Verify all spans were stored
    let spans = storage.query_spans(&QueryParams::default()).await.unwrap();
    assert_eq!(spans.len(), 100, "All 100 spans should be stored");
}

#[tokio::test]
async fn test_large_metrics_batch() {
    let storage = create_test_storage().await;
    let handler = Arc::new(MetricsHandler::new(
        storage.clone() as Arc<dyn StorageBackend>
    ));

    // Create and process 100 metric requests
    let batch = create_metrics_batch(100);
    for request in batch {
        let result = handler.process(request).await;
        assert!(result.is_ok(), "Handler should process large batch");
    }

    // Verify all metrics were stored
    let metrics = storage
        .query_metrics(&QueryParams::default())
        .await
        .unwrap();
    assert_eq!(metrics.len(), 100, "All 100 metrics should be stored");
}

#[tokio::test]
async fn test_logs_with_custom_attributes() {
    let storage = create_test_storage().await;
    let handler = Arc::new(LogsHandler::new(storage.clone() as Arc<dyn StorageBackend>));

    // Create a log with multiple custom attributes
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;

    let log_record = OtlpLogRecord {
        time_unix_nano: timestamp,
        observed_time_unix_nano: timestamp,
        severity_number: 17, // ERROR
        severity_text: "ERROR".to_string(),
        body: Some(AnyValue {
            value: Some(any_value::Value::StringValue(
                "Custom error message".to_string(),
            )),
        }),
        attributes: vec![
            KeyValue {
                key: "error.type".to_string(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::StringValue("DatabaseError".to_string())),
                }),
            },
            KeyValue {
                key: "error.code".to_string(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::IntValue(500)),
                }),
            },
            KeyValue {
                key: "user.id".to_string(),
                value: Some(AnyValue {
                    value: Some(any_value::Value::StringValue("user123".to_string())),
                }),
            },
        ],
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

    // Process and verify
    handler.process(request).await.unwrap();

    let logs = storage.query_logs(&QueryParams::default()).await.unwrap();
    assert!(!logs.is_empty());

    let log = &logs[0];
    assert_eq!(log.body, "Custom error message");
    assert_eq!(log.severity, SeverityLevel::Error);

    // Verify attributes
    assert_eq!(
        log.attributes.get("error.type"),
        Some(&"DatabaseError".to_string())
    );
    assert_eq!(log.attributes.get("error.code"), Some(&"500".to_string()));
    assert_eq!(log.attributes.get("user.id"), Some(&"user123".to_string()));
}
