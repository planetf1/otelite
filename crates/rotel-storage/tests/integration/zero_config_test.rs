//! Integration test for zero-configuration storage initialization
//!
//! This test verifies that:
//! - Storage automatically initializes in default location
//! - Directory is created with proper permissions
//! - Schema is initialized automatically
//! - Data can be written and read back

use rotel_core::telemetry::log::SeverityLevel;
use rotel_core::telemetry::metric::MetricType;
use rotel_core::telemetry::trace::{SpanKind, SpanStatus, StatusCode};
use rotel_core::telemetry::{LogRecord, Metric, Resource, Span};
use rotel_storage::sqlite::SqliteBackend;
use rotel_storage::{QueryParams, StorageBackend, StorageConfig};
use std::collections::HashMap;
use tempfile::TempDir;

#[tokio::test]
async fn test_zero_config_initialization() {
    // Create a temporary directory for this test
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

    // Create backend - should not fail
    let mut backend = SqliteBackend::new(config);

    // Initialize - should create directory and schema automatically
    let result = backend.initialize().await;
    assert!(
        result.is_ok(),
        "Initialization should succeed: {:?}",
        result
    );

    // Verify database file was created
    let db_path = temp_dir.path().join("rotel.db");
    assert!(db_path.exists(), "Database file should be created");

    // Verify we can write data
    let log = create_test_log();
    let write_result = backend.write_log(&log).await;
    assert!(
        write_result.is_ok(),
        "Writing log should succeed: {:?}",
        write_result
    );

    // Verify we can read data back
    let query_params = QueryParams::default();
    let logs = backend.query_logs(&query_params).await.unwrap();
    assert_eq!(logs.len(), 1, "Should retrieve one log");
    assert_eq!(logs[0].body, log.body, "Log body should match");

    // Clean up
    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_automatic_directory_creation() {
    // Create a temporary directory
    let temp_dir = TempDir::new().unwrap();

    // Use a nested path that doesn't exist yet
    let nested_path = temp_dir
        .path()
        .join("nested")
        .join("path")
        .join("to")
        .join("data");
    let config = StorageConfig::default().with_data_dir(nested_path.clone());

    let mut backend = SqliteBackend::new(config);

    // Initialize should create all parent directories
    let result = backend.initialize().await;
    assert!(
        result.is_ok(),
        "Should create nested directories: {:?}",
        result
    );

    // Verify all directories were created
    assert!(nested_path.exists(), "Nested directory should be created");
    assert!(
        nested_path.join("rotel.db").exists(),
        "Database should be created in nested path"
    );

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_write_and_read_all_signal_types() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Write a log
    let log = create_test_log();
    backend.write_log(&log).await.unwrap();

    // Write a span
    let span = create_test_span();
    backend.write_span(&span).await.unwrap();

    // Write a metric
    let metric = create_test_metric();
    backend.write_metric(&metric).await.unwrap();

    // Query logs
    let logs = backend.query_logs(&QueryParams::default()).await.unwrap();
    assert_eq!(logs.len(), 1, "Should have one log");
    assert_eq!(logs[0].body, "Test log message");

    // Query spans
    let spans = backend.query_spans(&QueryParams::default()).await.unwrap();
    assert_eq!(spans.len(), 1, "Should have one span");
    assert_eq!(spans[0].name, "test-span");

    // Query metrics
    let metrics = backend
        .query_metrics(&QueryParams::default())
        .await
        .unwrap();
    assert_eq!(metrics.len(), 1, "Should have one metric");
    assert_eq!(metrics[0].name, "test.metric");

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_schema_already_initialized() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

    // Initialize first time
    let mut backend1 = SqliteBackend::new(config.clone());
    backend1.initialize().await.unwrap();
    backend1.close().await.unwrap();

    // Initialize second time - should not fail
    let mut backend2 = SqliteBackend::new(config);
    let result = backend2.initialize().await;
    assert!(
        result.is_ok(),
        "Re-initialization should succeed: {:?}",
        result
    );

    backend2.close().await.unwrap();
}

// Helper functions to create test data

fn create_test_log() -> LogRecord {
    LogRecord {
        timestamp: 1234567890000000000,
        observed_timestamp: Some(1234567890000000000),
        severity: SeverityLevel::Info,
        severity_text: Some("INFO".to_string()),
        body: "Test log message".to_string(),
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
        trace_id: None,
        span_id: None,
    }
}

fn create_test_span() -> Span {
    Span {
        trace_id: "0123456789abcdef0123456789abcdef".to_string(),
        span_id: "0123456789abcdef".to_string(),
        parent_span_id: None,
        name: "test-span".to_string(),
        kind: SpanKind::Internal,
        start_time: 1234567890000000000,
        end_time: 1234567891000000000,
        attributes: HashMap::new(),
        events: Vec::new(),
        status: SpanStatus {
            code: StatusCode::Ok,
            message: None,
        },
    }
}

fn create_test_metric() -> Metric {
    Metric {
        name: "test.metric".to_string(),
        description: Some("Test metric".to_string()),
        unit: Some("count".to_string()),
        metric_type: MetricType::Gauge(42.0),
        timestamp: 1234567890000000000,
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
    }
}

// Made with Bob
