//! Integration test for data persistence across restarts
//!
//! This test verifies that:
//! - Data written to storage persists after backend is closed
//! - Data can be read back after reopening the database
//! - Multiple write/read cycles work correctly

use rotel_core::telemetry::log::SeverityLevel;
use rotel_core::telemetry::metric::MetricType;
use rotel_core::telemetry::trace::{SpanKind, SpanStatus, StatusCode};
use rotel_core::telemetry::{LogRecord, Metric, Resource, Span};
use rotel_storage::sqlite::SqliteBackend;
use rotel_storage::{QueryParams, StorageBackend, StorageConfig};
use std::collections::HashMap;
use tempfile::TempDir;

#[tokio::test]
async fn test_data_persists_across_restarts() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

    // First session: Write data
    {
        let mut backend = SqliteBackend::new(config.clone());
        backend.initialize().await.unwrap();

        // Write test data
        let log = create_test_log("First log");
        backend.write_log(&log).await.unwrap();

        let span = create_test_span("first-span");
        backend.write_span(&span).await.unwrap();

        let metric = create_test_metric("first.metric", 100.0);
        backend.write_metric(&metric).await.unwrap();

        // Close backend
        backend.close().await.unwrap();
    }

    // Second session: Read data back
    {
        let mut backend = SqliteBackend::new(config.clone());
        backend.initialize().await.unwrap();

        // Query logs
        let logs = backend.query_logs(&QueryParams::default()).await.unwrap();
        assert_eq!(logs.len(), 1, "Should have one log after restart");
        assert_eq!(logs[0].body, "First log");

        // Query spans
        let spans = backend.query_spans(&QueryParams::default()).await.unwrap();
        assert_eq!(spans.len(), 1, "Should have one span after restart");
        assert_eq!(spans[0].name, "first-span");

        // Query metrics
        let metrics = backend
            .query_metrics(&QueryParams::default())
            .await
            .unwrap();
        assert_eq!(metrics.len(), 1, "Should have one metric after restart");
        assert_eq!(metrics[0].name, "first.metric");
        // Check the value inside the MetricType enum
        if let MetricType::Gauge(value) = metrics[0].metric_type {
            assert_eq!(value, 100.0);
        } else {
            panic!("Expected Gauge metric type");
        }

        backend.close().await.unwrap();
    }
}

#[tokio::test]
async fn test_multiple_write_read_cycles() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

    // Cycle 1: Write first batch
    {
        let mut backend = SqliteBackend::new(config.clone());
        backend.initialize().await.unwrap();

        backend.write_log(&create_test_log("Log 1")).await.unwrap();
        backend
            .write_span(&create_test_span("span-1"))
            .await
            .unwrap();
        backend
            .write_metric(&create_test_metric("metric.1", 10.0))
            .await
            .unwrap();

        backend.close().await.unwrap();
    }

    // Cycle 2: Write second batch
    {
        let mut backend = SqliteBackend::new(config.clone());
        backend.initialize().await.unwrap();

        backend.write_log(&create_test_log("Log 2")).await.unwrap();
        backend
            .write_span(&create_test_span("span-2"))
            .await
            .unwrap();
        backend
            .write_metric(&create_test_metric("metric.2", 20.0))
            .await
            .unwrap();

        backend.close().await.unwrap();
    }

    // Cycle 3: Write third batch
    {
        let mut backend = SqliteBackend::new(config.clone());
        backend.initialize().await.unwrap();

        backend.write_log(&create_test_log("Log 3")).await.unwrap();
        backend
            .write_span(&create_test_span("span-3"))
            .await
            .unwrap();
        backend
            .write_metric(&create_test_metric("metric.3", 30.0))
            .await
            .unwrap();

        backend.close().await.unwrap();
    }

    // Final read: Verify all data is present
    {
        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();

        let logs = backend.query_logs(&QueryParams::default()).await.unwrap();
        assert_eq!(logs.len(), 3, "Should have three logs");

        let spans = backend.query_spans(&QueryParams::default()).await.unwrap();
        assert_eq!(spans.len(), 3, "Should have three spans");

        let metrics = backend
            .query_metrics(&QueryParams::default())
            .await
            .unwrap();
        assert_eq!(metrics.len(), 3, "Should have three metrics");

        backend.close().await.unwrap();
    }
}

#[tokio::test]
async fn test_concurrent_writes_persist() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

    // Write multiple records in one session
    {
        let mut backend = SqliteBackend::new(config.clone());
        backend.initialize().await.unwrap();

        // Write 10 logs
        for i in 0..10 {
            let log = create_test_log(&format!("Log {}", i));
            backend.write_log(&log).await.unwrap();
        }

        // Write 10 spans
        for i in 0..10 {
            let span = create_test_span(&format!("span-{}", i));
            backend.write_span(&span).await.unwrap();
        }

        // Write 10 metrics
        for i in 0..10 {
            let metric = create_test_metric(&format!("metric.{}", i), i as f64);
            backend.write_metric(&metric).await.unwrap();
        }

        backend.close().await.unwrap();
    }

    // Verify all records persisted
    {
        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();

        let logs = backend.query_logs(&QueryParams::default()).await.unwrap();
        assert_eq!(logs.len(), 10, "Should have 10 logs");

        let spans = backend.query_spans(&QueryParams::default()).await.unwrap();
        assert_eq!(spans.len(), 10, "Should have 10 spans");

        let metrics = backend
            .query_metrics(&QueryParams::default())
            .await
            .unwrap();
        assert_eq!(metrics.len(), 10, "Should have 10 metrics");

        backend.close().await.unwrap();
    }
}

#[tokio::test]
async fn test_data_integrity_after_restart() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

    let original_log = create_test_log_with_attributes();
    let original_span = create_test_span_with_status();
    let original_metric = create_test_metric("complex.metric", 123.45);

    // Write data with complex attributes
    {
        let mut backend = SqliteBackend::new(config.clone());
        backend.initialize().await.unwrap();

        backend.write_log(&original_log).await.unwrap();
        backend.write_span(&original_span).await.unwrap();
        backend.write_metric(&original_metric).await.unwrap();

        backend.close().await.unwrap();
    }

    // Read back and verify all fields match
    {
        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();

        let logs = backend.query_logs(&QueryParams::default()).await.unwrap();
        assert_eq!(logs.len(), 1);
        let retrieved_log = &logs[0];
        assert_eq!(retrieved_log.body, original_log.body);
        assert_eq!(retrieved_log.severity, original_log.severity);
        assert_eq!(
            retrieved_log.attributes.len(),
            original_log.attributes.len()
        );

        let spans = backend.query_spans(&QueryParams::default()).await.unwrap();
        assert_eq!(spans.len(), 1);
        let retrieved_span = &spans[0];
        assert_eq!(retrieved_span.name, original_span.name);
        assert_eq!(retrieved_span.trace_id, original_span.trace_id);
        assert_eq!(retrieved_span.span_id, original_span.span_id);

        let metrics = backend
            .query_metrics(&QueryParams::default())
            .await
            .unwrap();
        assert_eq!(metrics.len(), 1);
        let retrieved_metric = &metrics[0];
        assert_eq!(retrieved_metric.name, original_metric.name);
        assert_eq!(retrieved_metric.metric_type, original_metric.metric_type);

        backend.close().await.unwrap();
    }
}

// Helper functions

fn create_test_log(body: &str) -> LogRecord {
    LogRecord {
        timestamp: 1234567890000000000,
        observed_timestamp: Some(1234567890000000000),
        severity: SeverityLevel::Info,
        severity_text: Some("INFO".to_string()),
        body: body.to_string(),
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
        trace_id: None,
        span_id: None,
    }
}

fn create_test_log_with_attributes() -> LogRecord {
    let mut attributes = HashMap::new();
    attributes.insert("key1".to_string(), "value1".to_string());
    attributes.insert("key2".to_string(), "value2".to_string());

    LogRecord {
        timestamp: 1234567890000000000,
        observed_timestamp: Some(1234567890000000000),
        severity: SeverityLevel::Info,
        severity_text: Some("INFO".to_string()),
        body: "Log with attributes".to_string(),
        attributes,
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
        trace_id: Some("trace123".to_string()),
        span_id: Some("span456".to_string()),
    }
}

fn create_test_span(name: &str) -> Span {
    Span {
        trace_id: format!("{:032x}", name.len()),
        span_id: format!("{:016x}", name.len()),
        parent_span_id: None,
        name: name.to_string(),
        kind: SpanKind::Internal,
        start_time: 1234567890000000000,
        end_time: 1234567891000000000,
        attributes: HashMap::new(),
        events: Vec::new(),
        status: SpanStatus {
            code: StatusCode::Ok,
            message: None,
        },
        resource: None,
    }
}

fn create_test_span_with_status() -> Span {
    Span {
        trace_id: "0123456789abcdef0123456789abcdef".to_string(),
        span_id: "0123456789abcdef".to_string(),
        parent_span_id: Some("fedcba9876543210".to_string()),
        name: "span-with-status".to_string(),
        kind: SpanKind::Server,
        start_time: 1234567890000000000,
        end_time: 1234567891000000000,
        attributes: HashMap::new(),
        events: Vec::new(),
        status: SpanStatus {
            code: StatusCode::Ok,
            message: Some("All good".to_string()),
        },
        resource: None,
    }
}

fn create_test_metric(name: &str, value: f64) -> Metric {
    Metric {
        name: name.to_string(),
        description: Some("Test metric".to_string()),
        unit: Some("count".to_string()),
        metric_type: MetricType::Gauge(value),
        timestamp: 1234567890000000000,
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
    }
}
