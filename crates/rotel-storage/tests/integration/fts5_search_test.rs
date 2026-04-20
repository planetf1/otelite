//! Integration tests for FTS5 full-text search functionality
//!
//! This test suite verifies:
//! - Basic FTS5 search queries work correctly
//! - Various search patterns (exact, prefix, phrase, boolean)
//! - Search result ranking and relevance
//! - Search performance with large datasets
//! - Edge cases and error handling

use rotel_core::telemetry::log::SeverityLevel;
use rotel_core::telemetry::{LogRecord, Resource};
use rotel_storage::sqlite::SqliteBackend;
use rotel_storage::{QueryParams, StorageBackend, StorageConfig};
use std::collections::HashMap;
use tempfile::TempDir;

/// Helper to create a log with specific body text
fn create_log_with_body(body: &str, timestamp: i64) -> LogRecord {
    LogRecord {
        timestamp,
        observed_timestamp: Some(timestamp),
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

#[tokio::test]
async fn test_fts5_exact_word_match() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs
    backend
        .write_log(&create_log_with_body("error occurred in database", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("warning about memory usage", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("info message about startup", 3000))
        .await
        .unwrap();

    // Search for exact word "error"
    let params = QueryParams {
        search_text: Some("error".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(results.len(), 1, "Should find exactly one log with 'error'");
    assert!(results[0].body.contains("error"));

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_multiple_word_search() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs
    backend
        .write_log(&create_log_with_body("database connection failed", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("database query successful", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("network connection timeout", 3000))
        .await
        .unwrap();

    // Search for "database AND connection" (both words required)
    let params = QueryParams {
        search_text: Some("database AND connection".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        1,
        "Should find one log containing both 'database' and 'connection'"
    );
    assert!(results[0].body.contains("database") && results[0].body.contains("connection"));

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_phrase_search() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs
    backend
        .write_log(&create_log_with_body("connection timeout occurred", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("timeout connection error", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("network issue detected", 3000))
        .await
        .unwrap();

    // Search for exact phrase "connection timeout"
    let params = QueryParams {
        search_text: Some("\"connection timeout\"".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        1,
        "Should find exactly one log with phrase 'connection timeout'"
    );
    assert!(results[0].body.contains("connection timeout"));

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_prefix_search() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs
    backend
        .write_log(&create_log_with_body("processing request", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("processor initialized", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("process completed", 3000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("network error", 4000))
        .await
        .unwrap();

    // Search with prefix "proc*"
    let params = QueryParams {
        search_text: Some("proc*".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        3,
        "Should find three logs starting with 'proc'"
    );
    for log in &results {
        assert!(
            log.body.contains("processing")
                || log.body.contains("processor")
                || log.body.contains("process")
        );
    }

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_boolean_and_search() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs
    backend
        .write_log(&create_log_with_body("error in database connection", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("error in network layer", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("database query successful", 3000))
        .await
        .unwrap();

    // Search for logs with both "error" AND "database"
    let params = QueryParams {
        search_text: Some("error AND database".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        1,
        "Should find one log with both 'error' and 'database'"
    );
    assert!(results[0].body.contains("error") && results[0].body.contains("database"));

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_boolean_or_search() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs
    backend
        .write_log(&create_log_with_body("critical error occurred", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("warning message logged", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("info about startup", 3000))
        .await
        .unwrap();

    // Search for logs with "error" OR "warning"
    let params = QueryParams {
        search_text: Some("error OR warning".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        2,
        "Should find two logs with 'error' or 'warning'"
    );
    for log in &results {
        assert!(log.body.contains("error") || log.body.contains("warning"));
    }

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_boolean_not_search() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs
    backend
        .write_log(&create_log_with_body("database error occurred", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("database query successful", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("network timeout", 3000))
        .await
        .unwrap();

    // Search for logs with "database" but NOT "error"
    let params = QueryParams {
        search_text: Some("database NOT error".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        1,
        "Should find one log with 'database' but not 'error'"
    );
    assert!(results[0].body.contains("database") && !results[0].body.contains("error"));

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_case_insensitive_search() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs with mixed case
    backend
        .write_log(&create_log_with_body("ERROR in system", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("Error detected", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("error occurred", 3000))
        .await
        .unwrap();

    // Search with lowercase
    let params = QueryParams {
        search_text: Some("error".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        3,
        "FTS5 should be case-insensitive by default"
    );

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_no_results() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs
    backend
        .write_log(&create_log_with_body("info message", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("warning alert", 2000))
        .await
        .unwrap();

    // Search for non-existent term
    let params = QueryParams {
        search_text: Some("nonexistent".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(results.len(), 0, "Should return empty results for no match");

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_search_with_time_filter() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert test logs with different timestamps
    backend
        .write_log(&create_log_with_body("error at time 1000", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("error at time 2000", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("error at time 3000", 3000))
        .await
        .unwrap();

    // Search for "error" with time range filter
    let params = QueryParams {
        search_text: Some("error".to_string()),
        start_time: Some(1500),
        end_time: Some(2500),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        1,
        "Should find one log matching search and time range"
    );
    assert_eq!(results[0].timestamp, 2000);

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_search_with_limit() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert multiple matching logs
    for i in 0..10 {
        backend
            .write_log(&create_log_with_body(
                &format!("error number {}", i),
                i * 1000,
            ))
            .await
            .unwrap();
    }

    // Search with limit
    let params = QueryParams {
        search_text: Some("error".to_string()),
        limit: Some(5),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(results.len(), 5, "Should respect limit parameter");

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_search_result_ordering() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert logs with different timestamps
    backend
        .write_log(&create_log_with_body("error message 1", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("error message 2", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("error message 3", 3000))
        .await
        .unwrap();

    // Search should return results ordered by timestamp DESC
    let params = QueryParams {
        search_text: Some("error".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(results.len(), 3);
    assert_eq!(results[0].timestamp, 3000, "Most recent should be first");
    assert_eq!(results[1].timestamp, 2000);
    assert_eq!(results[2].timestamp, 1000, "Oldest should be last");

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_search_performance_large_dataset() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert 1000 logs with various content
    for i in 0..1000 {
        let body = if i % 10 == 0 {
            format!("error message number {}", i)
        } else {
            format!("info message number {}", i)
        };
        backend
            .write_log(&create_log_with_body(&body, i * 1000))
            .await
            .unwrap();
    }

    // Measure search performance
    let start = std::time::Instant::now();
    let params = QueryParams {
        search_text: Some("error".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();
    let duration = start.elapsed();

    assert_eq!(results.len(), 100, "Should find 100 error logs");
    assert!(
        duration.as_millis() < 1000,
        "Search should complete in under 1 second, took {:?}",
        duration
    );

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_special_characters_in_search() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert logs with special characters
    backend
        .write_log(&create_log_with_body("user@example.com logged in", 1000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("path/to/file accessed", 2000))
        .await
        .unwrap();
    backend
        .write_log(&create_log_with_body("value: 123.45", 3000))
        .await
        .unwrap();

    // Search for email (escape special characters for FTS5)
    let params = QueryParams {
        search_text: Some("\"example.com\"".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        1,
        "Should handle special characters in search"
    );
    assert!(results[0].body.contains("example.com"));

    backend.close().await.unwrap();
}

#[tokio::test]
async fn test_fts5_combined_filters() {
    let temp_dir = TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut backend = SqliteBackend::new(config);
    backend.initialize().await.unwrap();

    // Insert logs with trace IDs
    let mut log1 = create_log_with_body("error in database", 1000);
    log1.trace_id = Some("trace-123".to_string());
    backend.write_log(&log1).await.unwrap();

    let mut log2 = create_log_with_body("error in network", 2000);
    log2.trace_id = Some("trace-456".to_string());
    backend.write_log(&log2).await.unwrap();

    let mut log3 = create_log_with_body("error in database", 3000);
    log3.trace_id = Some("trace-123".to_string());
    backend.write_log(&log3).await.unwrap();

    // Search with FTS + trace_id filter
    let params = QueryParams {
        search_text: Some("database".to_string()),
        trace_id: Some("trace-123".to_string()),
        ..Default::default()
    };
    let results = backend.query_logs(&params).await.unwrap();

    assert_eq!(
        results.len(),
        2,
        "Should combine FTS search with trace_id filter"
    );
    for log in &results {
        assert!(log.body.contains("database"));
        assert_eq!(log.trace_id.as_deref(), Some("trace-123"));
    }

    backend.close().await.unwrap();
}

// Made with Bob
