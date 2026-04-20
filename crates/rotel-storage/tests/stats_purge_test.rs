use rotel_core::telemetry::log::{LogRecord, SeverityLevel};
use rotel_core::telemetry::metric::{Metric, MetricType};
use rotel_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode};
use rotel_core::telemetry::Resource;
use rotel_storage::sqlite::SqliteBackend;
use rotel_storage::{PurgeOptions, SignalType, StorageBackend, StorageConfig};
use std::collections::HashMap;
use tempfile::TempDir;

async fn setup_storage() -> (SqliteBackend, TempDir) {
    let tmp = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(tmp.path().to_path_buf());
    let mut storage = SqliteBackend::new(config);
    storage.initialize().await.unwrap();
    (storage, tmp)
}

fn create_test_log(timestamp: i64) -> LogRecord {
    LogRecord {
        timestamp,
        observed_timestamp: Some(timestamp),
        severity: SeverityLevel::Info,
        severity_text: Some("INFO".to_string()),
        body: "Test log".to_string(),
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
        trace_id: None,
        span_id: None,
    }
}

fn create_test_span(timestamp: i64) -> Span {
    Span {
        trace_id: format!("trace-{}", timestamp),
        span_id: format!("span-{}", timestamp),
        parent_span_id: None,
        name: "test-span".to_string(),
        kind: SpanKind::Internal,
        start_time: timestamp,
        end_time: timestamp + 1000,
        attributes: HashMap::new(),
        status: SpanStatus {
            code: StatusCode::Ok,
            message: None,
        },
        events: Vec::new(),
    }
}

fn create_test_metric(timestamp: i64) -> Metric {
    Metric {
        name: "test.metric".to_string(),
        description: Some("Test metric".to_string()),
        unit: Some("count".to_string()),
        metric_type: MetricType::Gauge(42.0),
        timestamp,
        attributes: HashMap::new(),
        resource: Some(Resource {
            attributes: HashMap::new(),
        }),
    }
}

#[tokio::test]
async fn test_stats_empty_database() {
    let (storage, _tmp) = setup_storage().await;

    let stats = storage.stats().await.unwrap();

    assert_eq!(stats.log_count, 0);
    assert_eq!(stats.span_count, 0);
    assert_eq!(stats.metric_count, 0);
    assert!(stats.oldest_timestamp.is_none());
    assert!(stats.newest_timestamp.is_none());
}

#[tokio::test]
async fn test_stats_after_single_writes() {
    let (storage, _tmp) = setup_storage().await;

    storage.write_log(&create_test_log(1000)).await.unwrap();
    storage.write_span(&create_test_span(2000)).await.unwrap();
    storage
        .write_metric(&create_test_metric(3000))
        .await
        .unwrap();

    let stats = storage.stats().await.unwrap();

    assert_eq!(stats.log_count, 1);
    assert_eq!(stats.span_count, 1);
    assert_eq!(stats.metric_count, 1);
    assert!(stats.oldest_timestamp.is_some());
    assert!(stats.newest_timestamp.is_some());
}

#[tokio::test]
async fn test_stats_after_multiple_writes() {
    let (storage, _tmp) = setup_storage().await;

    // Write 5 logs
    for i in 0..5 {
        storage
            .write_log(&create_test_log(1000 + i * 1000))
            .await
            .unwrap();
    }

    // Write 3 spans
    for i in 0..3 {
        storage
            .write_span(&create_test_span(10000 + i * 1000))
            .await
            .unwrap();
    }

    // Write 7 metrics
    for i in 0..7 {
        storage
            .write_metric(&create_test_metric(20000 + i * 1000))
            .await
            .unwrap();
    }

    let stats = storage.stats().await.unwrap();

    assert_eq!(stats.log_count, 5);
    assert_eq!(stats.span_count, 3);
    assert_eq!(stats.metric_count, 7);
    assert!(stats.oldest_timestamp.is_some());
    assert!(stats.newest_timestamp.is_some());
}

#[tokio::test]
async fn test_stats_storage_size_increases() {
    let (storage, _tmp) = setup_storage().await;

    let stats_before = storage.stats().await.unwrap();
    let size_before = stats_before.storage_size_bytes;

    // Write more data to ensure size increase is detectable
    for i in 0..100 {
        storage
            .write_log(&create_test_log(1000 + i * 1000))
            .await
            .unwrap();
    }

    let stats_after = storage.stats().await.unwrap();
    let size_after = stats_after.storage_size_bytes;

    assert!(
        size_after >= size_before,
        "Storage size should not decrease after writes (before: {}, after: {})",
        size_before,
        size_after
    );
}

#[tokio::test]
async fn test_stats_timestamp_ordering() {
    let (storage, _tmp) = setup_storage().await;

    storage.write_log(&create_test_log(5000)).await.unwrap();
    storage.write_log(&create_test_log(1000)).await.unwrap();
    storage.write_log(&create_test_log(3000)).await.unwrap();

    let stats = storage.stats().await.unwrap();

    assert!(stats.oldest_timestamp.is_some());
    assert!(stats.newest_timestamp.is_some());
    assert!(
        stats.oldest_timestamp.unwrap() <= stats.newest_timestamp.unwrap(),
        "Oldest timestamp should be <= newest timestamp"
    );
}

#[tokio::test]
async fn test_purge_dry_run_returns_count_without_deleting() {
    let (storage, _tmp) = setup_storage().await;

    // Write some old data
    storage.write_log(&create_test_log(1000)).await.unwrap();
    storage.write_span(&create_test_span(2000)).await.unwrap();
    storage
        .write_metric(&create_test_metric(3000))
        .await
        .unwrap();

    let stats_before = storage.stats().await.unwrap();

    // Dry run purge with older_than=current time (should match everything)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;

    let options = PurgeOptions {
        older_than: Some(now),
        signal_types: vec![SignalType::Logs, SignalType::Traces, SignalType::Metrics],
        dry_run: true,
    };

    let deleted = storage.purge(&options).await.unwrap();

    assert!(deleted > 0, "Dry run should report records to delete");

    let stats_after = storage.stats().await.unwrap();

    // Verify nothing was actually deleted
    assert_eq!(stats_before.log_count, stats_after.log_count);
    assert_eq!(stats_before.span_count, stats_after.span_count);
    assert_eq!(stats_before.metric_count, stats_after.metric_count);
}

#[tokio::test]
async fn test_purge_deletes_all_with_current_timestamp() {
    let (storage, _tmp) = setup_storage().await;

    // Write some data
    storage.write_log(&create_test_log(1000)).await.unwrap();
    storage.write_span(&create_test_span(2000)).await.unwrap();
    storage
        .write_metric(&create_test_metric(3000))
        .await
        .unwrap();

    let stats_before = storage.stats().await.unwrap();
    assert!(stats_before.log_count > 0);
    assert!(stats_before.span_count > 0);
    assert!(stats_before.metric_count > 0);

    // Purge with older_than=current time (delete everything)
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;

    let options = PurgeOptions {
        older_than: Some(now),
        signal_types: vec![SignalType::Logs, SignalType::Traces, SignalType::Metrics],
        dry_run: false,
    };

    let deleted = storage.purge(&options).await.unwrap();

    assert!(deleted > 0, "Should have deleted records");

    let stats_after = storage.stats().await.unwrap();

    assert_eq!(stats_after.log_count, 0);
    assert_eq!(stats_after.span_count, 0);
    assert_eq!(stats_after.metric_count, 0);
}

#[tokio::test]
async fn test_purge_respects_timestamp_threshold() {
    let (storage, _tmp) = setup_storage().await;

    // Current time in nanoseconds
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;

    // Write recent data (now)
    storage.write_log(&create_test_log(now)).await.unwrap();

    // Write old data (1 year ago in nanoseconds)
    let old_timestamp = now - (365 * 24 * 60 * 60 * 1_000_000_000);
    storage
        .write_log(&create_test_log(old_timestamp))
        .await
        .unwrap();

    let stats_before = storage.stats().await.unwrap();
    assert_eq!(stats_before.log_count, 2);

    // Purge data older than 6 months ago
    let six_months_ago = now - (180 * 24 * 60 * 60 * 1_000_000_000);
    let options = PurgeOptions {
        older_than: Some(six_months_ago),
        signal_types: vec![SignalType::Logs],
        dry_run: false,
    };

    storage.purge(&options).await.unwrap();

    let stats_after = storage.stats().await.unwrap();

    // Recent data should survive, old data should be deleted
    assert_eq!(stats_after.log_count, 1);
}

#[tokio::test]
async fn test_purge_with_single_signal_type() {
    let (storage, _tmp) = setup_storage().await;

    // Write data for all signal types
    storage.write_log(&create_test_log(1000)).await.unwrap();
    storage.write_span(&create_test_span(2000)).await.unwrap();
    storage
        .write_metric(&create_test_metric(3000))
        .await
        .unwrap();

    let stats_before = storage.stats().await.unwrap();
    assert_eq!(stats_before.log_count, 1);
    assert_eq!(stats_before.span_count, 1);
    assert_eq!(stats_before.metric_count, 1);

    // Purge only logs
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;

    let options = PurgeOptions {
        older_than: Some(now),
        signal_types: vec![SignalType::Logs],
        dry_run: false,
    };

    storage.purge(&options).await.unwrap();

    let stats_after = storage.stats().await.unwrap();

    // Only logs should be deleted
    assert_eq!(stats_after.log_count, 0);
    assert_eq!(stats_after.span_count, 1);
    assert_eq!(stats_after.metric_count, 1);
}

#[tokio::test]
async fn test_purge_with_multiple_signal_types() {
    let (storage, _tmp) = setup_storage().await;

    // Write data for all signal types
    storage.write_log(&create_test_log(1000)).await.unwrap();
    storage.write_span(&create_test_span(2000)).await.unwrap();
    storage
        .write_metric(&create_test_metric(3000))
        .await
        .unwrap();

    let stats_before = storage.stats().await.unwrap();
    assert_eq!(stats_before.log_count, 1);
    assert_eq!(stats_before.span_count, 1);
    assert_eq!(stats_before.metric_count, 1);

    // Purge traces and metrics
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;

    let options = PurgeOptions {
        older_than: Some(now),
        signal_types: vec![SignalType::Traces, SignalType::Metrics],
        dry_run: false,
    };

    storage.purge(&options).await.unwrap();

    let stats_after = storage.stats().await.unwrap();

    // Traces and metrics should be deleted, logs should remain
    assert_eq!(stats_after.log_count, 1);
    assert_eq!(stats_after.span_count, 0);
    assert_eq!(stats_after.metric_count, 0);
}

#[tokio::test]
async fn test_purge_on_empty_database() {
    let (storage, _tmp) = setup_storage().await;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;

    let options = PurgeOptions {
        older_than: Some(now),
        signal_types: vec![SignalType::Logs, SignalType::Traces, SignalType::Metrics],
        dry_run: false,
    };

    let deleted = storage.purge(&options).await.unwrap();

    assert_eq!(deleted, 0, "Purging empty database should return 0");
}

#[tokio::test]
async fn test_stats_reflect_purge() {
    let (storage, _tmp) = setup_storage().await;

    // Write 10 logs
    for i in 0..10 {
        storage
            .write_log(&create_test_log(1000 + i * 1000))
            .await
            .unwrap();
    }

    let stats_before = storage.stats().await.unwrap();
    assert_eq!(stats_before.log_count, 10);

    // Purge all logs
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64;

    let options = PurgeOptions {
        older_than: Some(now),
        signal_types: vec![SignalType::Logs],
        dry_run: false,
    };

    let deleted = storage.purge(&options).await.unwrap();

    assert_eq!(deleted, 10);

    let stats_after = storage.stats().await.unwrap();
    assert_eq!(stats_after.log_count, 0);
}
