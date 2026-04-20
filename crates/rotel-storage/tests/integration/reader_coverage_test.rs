//! Comprehensive tests for reader.rs to improve coverage

use rotel_core::telemetry::log::{LogRecord, SeverityLevel};
use rotel_core::telemetry::metric::{HistogramBucket, Metric, MetricType, Quantile};
use rotel_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode};
use rotel_core::telemetry::Resource;
use rotel_storage::sqlite::SqliteBackend;
use rotel_storage::{QueryParams, StorageBackend, StorageConfig};
use std::collections::HashMap;
use tempfile::TempDir;

async fn setup_backend_with_data() -> (SqliteBackend, TempDir) {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs with various severities
    for i in 0..10 {
        let log = LogRecord {
            timestamp: 1000 + i,
            observed_timestamp: Some(1000 + i),
            trace_id: Some(format!("trace-{}", i % 3)),
            span_id: Some(format!("span-{}", i % 3)),
            severity: match i % 4 {
                0 => SeverityLevel::Debug,
                1 => SeverityLevel::Info,
                2 => SeverityLevel::Warn,
                _ => SeverityLevel::Error,
            },
            severity_text: Some("test".to_string()),
            body: format!("Test log message {}", i),
            attributes: HashMap::new(),
            resource: Some(Resource {
                attributes: HashMap::new(),
            }),
        };
        backend.write_log(&log).await.unwrap();
    }

    // Insert test spans
    for i in 0..5 {
        let span = Span {
            trace_id: format!("trace-{}", i % 2),
            span_id: format!("span-{}", i),
            parent_span_id: if i > 0 {
                Some(format!("span-{}", i - 1))
            } else {
                None
            },
            name: format!("test-span-{}", i),
            kind: SpanKind::Internal,
            start_time: 2000 + i,
            end_time: 2100 + i,
            attributes: HashMap::new(),
            events: Vec::new(),
            status: SpanStatus {
                code: StatusCode::Ok,
                message: None,
            },
        };
        backend.write_span(&span).await.unwrap();
    }

    // Insert test metrics
    for i in 0..5 {
        let metric = Metric {
            name: format!("test.metric.{}", i),
            description: Some(format!("Test metric {}", i)),
            unit: Some("count".to_string()),
            metric_type: MetricType::Counter(i as u64 * 10),
            timestamp: 3000 + i,
            attributes: HashMap::new(),
            resource: Some(Resource {
                attributes: HashMap::new(),
            }),
        };
        backend.write_metric(&metric).await.unwrap();
    }

    (backend, temp_dir)
}

#[tokio::test]
async fn test_query_logs_with_time_range() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        start_time: Some(1003),
        end_time: Some(1007),
        ..Default::default()
    };
    let logs = backend.query_logs(&params).await.unwrap();
    assert_eq!(logs.len(), 5);
    assert!(logs
        .iter()
        .all(|l| l.timestamp >= 1003 && l.timestamp <= 1007));
}

#[tokio::test]
async fn test_query_logs_with_trace_id() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        trace_id: Some("trace-1".to_string()),
        ..Default::default()
    };
    let logs = backend.query_logs(&params).await.unwrap();
    assert!(!logs.is_empty());
    assert!(logs
        .iter()
        .all(|l| l.trace_id.as_ref().unwrap() == "trace-1"));
}

#[tokio::test]
async fn test_query_logs_with_span_id() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        span_id: Some("span-1".to_string()),
        ..Default::default()
    };
    let logs = backend.query_logs(&params).await.unwrap();
    assert!(!logs.is_empty());
    assert!(logs.iter().all(|l| l.span_id.as_ref().unwrap() == "span-1"));
}

#[tokio::test]
async fn test_query_logs_with_min_severity() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        min_severity: Some(SeverityLevel::Warn),
        ..Default::default()
    };
    let logs = backend.query_logs(&params).await.unwrap();
    assert!(!logs.is_empty());
    assert!(logs
        .iter()
        .all(|l| l.severity.to_i32() >= SeverityLevel::Warn.to_i32()));
}

#[tokio::test]
async fn test_query_logs_with_limit() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        limit: Some(3),
        ..Default::default()
    };
    let logs = backend.query_logs(&params).await.unwrap();
    assert_eq!(logs.len(), 3);
}

#[tokio::test]
async fn test_query_logs_with_all_filters() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        start_time: Some(1000),
        end_time: Some(1009),
        trace_id: Some("trace-1".to_string()),
        span_id: Some("span-1".to_string()),
        min_severity: Some(SeverityLevel::Debug),
        limit: Some(5),
        search_text: None,
    };
    let logs = backend.query_logs(&params).await.unwrap();
    assert!(logs.len() <= 5);
    assert!(logs.iter().all(|l| {
        l.timestamp >= 1000
            && l.timestamp <= 1009
            && l.trace_id.as_ref().unwrap() == "trace-1"
            && l.span_id.as_ref().unwrap() == "span-1"
    }));
}

#[tokio::test]
async fn test_query_logs_no_results() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        trace_id: Some("nonexistent-trace".to_string()),
        ..Default::default()
    };
    let logs = backend.query_logs(&params).await.unwrap();
    assert_eq!(logs.len(), 0);
}

#[tokio::test]
async fn test_query_spans_with_time_range() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        start_time: Some(2001),
        end_time: Some(2105),
        ..Default::default()
    };
    let spans = backend.query_spans(&params).await.unwrap();
    assert!(!spans.is_empty());
    assert!(spans.iter().all(|s| s.start_time >= 2001));
}

#[tokio::test]
async fn test_query_spans_with_trace_id() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        trace_id: Some("trace-0".to_string()),
        ..Default::default()
    };
    let spans = backend.query_spans(&params).await.unwrap();
    assert!(!spans.is_empty());
    assert!(spans.iter().all(|s| s.trace_id == "trace-0"));
}

#[tokio::test]
async fn test_query_spans_with_limit() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        limit: Some(2),
        ..Default::default()
    };
    let spans = backend.query_spans(&params).await.unwrap();
    assert_eq!(spans.len(), 2);
}

#[tokio::test]
async fn test_query_spans_no_results() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        trace_id: Some("nonexistent-trace".to_string()),
        ..Default::default()
    };
    let spans = backend.query_spans(&params).await.unwrap();
    assert_eq!(spans.len(), 0);
}

#[tokio::test]
async fn test_query_metrics_with_time_range() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        start_time: Some(3001),
        end_time: Some(3003),
        ..Default::default()
    };
    let metrics = backend.query_metrics(&params).await.unwrap();
    assert!(!metrics.is_empty());
    assert!(metrics
        .iter()
        .all(|m| m.timestamp >= 3001 && m.timestamp <= 3003));
}

#[tokio::test]
async fn test_query_metrics_with_limit() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        limit: Some(2),
        ..Default::default()
    };
    let metrics = backend.query_metrics(&params).await.unwrap();
    assert_eq!(metrics.len(), 2);
}

#[tokio::test]
async fn test_query_metrics_no_results() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams {
        start_time: Some(9999),
        ..Default::default()
    };
    let metrics = backend.query_metrics(&params).await.unwrap();
    assert_eq!(metrics.len(), 0);
}

#[tokio::test]
async fn test_concurrent_reads() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

    // Create multiple backends pointing to same database
    let mut backends = Vec::new();
    for _ in 0..5 {
        let mut backend = SqliteBackend::new(config.clone());
        backend.initialize().await.unwrap();
        backends.push(backend);
    }

    // Write test data with first backend
    for i in 0..10 {
        let log = LogRecord {
            timestamp: 1000 + i,
            observed_timestamp: Some(1000 + i),
            trace_id: Some(format!("trace-{}", i)),
            span_id: Some(format!("span-{}", i)),
            severity: SeverityLevel::Info,
            severity_text: Some("INFO".to_string()),
            body: format!("Test log {}", i),
            attributes: HashMap::new(),
            resource: Some(Resource {
                attributes: HashMap::new(),
            }),
        };
        backends[0].write_log(&log).await.unwrap();
    }

    // Concurrent reads from all backends
    let handles: Vec<_> = backends
        .into_iter()
        .map(|backend| {
            tokio::spawn(async move {
                let params = QueryParams::default();
                backend.query_logs(&params).await
            })
        })
        .collect();

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 10);
    }
}

#[tokio::test]
async fn test_query_ordering() {
    let (backend, _temp_dir) = setup_backend_with_data().await;
    let params = QueryParams::default();
    let logs = backend.query_logs(&params).await.unwrap();

    // Verify descending order by timestamp
    for i in 1..logs.len() {
        assert!(logs[i - 1].timestamp >= logs[i].timestamp);
    }
}

#[tokio::test]
async fn test_parse_different_metric_types() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Test Gauge
    let gauge = Metric {
        name: "test.gauge".to_string(),
        description: Some("Test gauge".to_string()),
        unit: Some("units".to_string()),
        metric_type: MetricType::Gauge(42.5),
        timestamp: 1000,
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
    };
    backend.write_metric(&gauge).await.unwrap();

    // Test Histogram
    let histogram = Metric {
        name: "test.histogram".to_string(),
        description: Some("Test histogram".to_string()),
        unit: Some("ms".to_string()),
        metric_type: MetricType::Histogram {
            count: 100,
            sum: 1500.0,
            buckets: vec![
                HistogramBucket {
                    upper_bound: 10.0,
                    count: 20,
                },
                HistogramBucket {
                    upper_bound: 50.0,
                    count: 50,
                },
                HistogramBucket {
                    upper_bound: 100.0,
                    count: 30,
                },
            ],
        },
        timestamp: 1001,
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
    };
    backend.write_metric(&histogram).await.unwrap();

    // Test Summary
    let summary = Metric {
        name: "test.summary".to_string(),
        description: Some("Test summary".to_string()),
        unit: Some("bytes".to_string()),
        metric_type: MetricType::Summary {
            count: 50,
            sum: 2500.0,
            quantiles: vec![
                Quantile {
                    quantile: 0.5,
                    value: 45.0,
                },
                Quantile {
                    quantile: 0.95,
                    value: 95.0,
                },
                Quantile {
                    quantile: 0.99,
                    value: 99.0,
                },
            ],
        },
        timestamp: 1002,
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
    };
    backend.write_metric(&summary).await.unwrap();

    let params = QueryParams::default();
    let metrics = backend.query_metrics(&params).await.unwrap();
    assert_eq!(metrics.len(), 3);

    // Verify each type was parsed correctly
    assert!(metrics
        .iter()
        .any(|m| matches!(m.metric_type, MetricType::Gauge(_))));
    assert!(metrics
        .iter()
        .any(|m| matches!(m.metric_type, MetricType::Histogram { .. })));
    assert!(metrics
        .iter()
        .any(|m| matches!(m.metric_type, MetricType::Summary { .. })));
}

#[tokio::test]
async fn test_parse_span_with_all_fields() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    let mut attributes = HashMap::new();
    attributes.insert("key1".to_string(), "value1".to_string());
    attributes.insert("key2".to_string(), "value2".to_string());

    let span = Span {
        trace_id: "trace-123".to_string(),
        span_id: "span-456".to_string(),
        parent_span_id: Some("parent-789".to_string()),
        name: "complex-span".to_string(),
        kind: SpanKind::Server,
        start_time: 5000,
        end_time: 5100,
        attributes,
        events: vec![],
        status: SpanStatus {
            code: StatusCode::Error,
            message: Some("Test error".to_string()),
        },
    };

    backend.write_span(&span).await.unwrap();

    let params = QueryParams::default();
    let spans = backend.query_spans(&params).await.unwrap();
    assert_eq!(spans.len(), 1);

    let retrieved = &spans[0];
    assert_eq!(retrieved.trace_id, "trace-123");
    assert_eq!(retrieved.span_id, "span-456");
    assert_eq!(retrieved.parent_span_id, Some("parent-789".to_string()));
    assert_eq!(retrieved.name, "complex-span");
    assert_eq!(retrieved.kind, SpanKind::Server);
    assert_eq!(retrieved.status.code, StatusCode::Error);
    assert_eq!(retrieved.status.message, Some("Test error".to_string()));
    assert_eq!(retrieved.attributes.len(), 2);
}

#[tokio::test]
async fn test_parse_log_with_all_fields() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    let mut attributes = HashMap::new();
    attributes.insert("user_id".to_string(), "123".to_string());
    attributes.insert("request_id".to_string(), "req-456".to_string());

    let mut resource_attrs = HashMap::new();
    resource_attrs.insert("service.name".to_string(), "test-service".to_string());

    let log = LogRecord {
        timestamp: 6000,
        observed_timestamp: Some(6001),
        trace_id: Some("trace-abc".to_string()),
        span_id: Some("span-def".to_string()),
        severity: SeverityLevel::Error,
        severity_text: Some("ERROR".to_string()),
        body: "Complex log message".to_string(),
        attributes,
        resource: Some(Resource {
            attributes: resource_attrs,
        }),
    };

    backend.write_log(&log).await.unwrap();

    let params = QueryParams::default();
    let logs = backend.query_logs(&params).await.unwrap();
    assert_eq!(logs.len(), 1);

    let retrieved = &logs[0];
    assert_eq!(retrieved.timestamp, 6000);
    assert_eq!(retrieved.observed_timestamp, Some(6001));
    assert_eq!(retrieved.trace_id, Some("trace-abc".to_string()));
    assert_eq!(retrieved.span_id, Some("span-def".to_string()));
    assert_eq!(retrieved.severity, SeverityLevel::Error);
    assert_eq!(retrieved.severity_text, Some("ERROR".to_string()));
    assert_eq!(retrieved.body, "Complex log message");
    assert_eq!(retrieved.attributes.len(), 2);
    assert!(retrieved.resource.is_some());
}
