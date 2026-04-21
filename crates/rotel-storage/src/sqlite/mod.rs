//! SQLite backend implementation

use crate::error::{Result, StorageError};
use crate::{PurgeOptions, QueryParams, StorageBackend, StorageConfig, StorageStats};
use async_trait::async_trait;
use chrono::Timelike;
use rotel_core::telemetry::{LogRecord, Metric, Span};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub mod purge;
pub mod reader;
pub mod schema;
pub mod writer;

/// SQLite storage backend
pub struct SqliteBackend {
    config: StorageConfig,
    conn: Arc<Mutex<Option<Connection>>>,
    purge_lock: Arc<purge::PurgeLock>,
    purge_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
}

impl SqliteBackend {
    /// Create a new SQLite backend with the given configuration
    pub fn new(config: StorageConfig) -> Self {
        Self {
            config,
            conn: Arc::new(Mutex::new(None)),
            purge_lock: Arc::new(purge::PurgeLock::new()),
            purge_handle: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the database path or URI to open
    fn db_path(&self) -> PathBuf {
        if self
            .config
            .data_dir
            .to_string_lossy()
            .starts_with(":memory:")
        {
            self.config.data_dir.clone()
        } else {
            self.config.data_dir.join("rotel.db")
        }
    }
}

#[async_trait]
impl StorageBackend for SqliteBackend {
    async fn initialize(&mut self) -> Result<()> {
        let db_path = self.db_path();

        if !db_path.to_string_lossy().starts_with(":memory:") {
            // Create data directory if it doesn't exist
            std::fs::create_dir_all(&self.config.data_dir).map_err(|e| {
                StorageError::InitializationError(format!("Failed to create data directory: {}", e))
            })?;
        }

        // Open database connection
        let conn = Connection::open(&db_path).map_err(|e| {
            StorageError::InitializationError(format!("Failed to open database: {}", e))
        })?;

        // Configure SQLite for better concurrency (WAL mode) and durability
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")
            .map_err(|e| {
                StorageError::InitializationError(format!("Failed to configure SQLite: {}", e))
            })?;

        // Initialize schema
        schema::initialize_schema(&conn)?;

        // Store connection
        *self.conn.lock().unwrap() = Some(conn);

        // Start background purge scheduler if enabled
        if self.config.retention_days > 0 {
            self.start_purge_scheduler();
        }

        Ok(())
    }

    async fn write_log(&self, log: &LogRecord) -> Result<()> {
        let conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::WriteError("Database not initialized".to_string()))?;

        writer::write_log(conn, log)
    }

    async fn write_span(&self, span: &Span) -> Result<()> {
        let conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::WriteError("Database not initialized".to_string()))?;

        writer::write_span(conn, span)
    }

    async fn write_metric(&self, metric: &Metric) -> Result<()> {
        let conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::WriteError("Database not initialized".to_string()))?;

        writer::write_metric(conn, metric)
    }

    async fn query_logs(&self, params: &QueryParams) -> Result<Vec<LogRecord>> {
        let conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::QueryError("Database not initialized".to_string()))?;

        reader::query_logs(conn, params)
    }

    async fn query_spans(&self, params: &QueryParams) -> Result<Vec<Span>> {
        let conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::QueryError("Database not initialized".to_string()))?;

        reader::query_spans(conn, params)
    }

    async fn query_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>> {
        let conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::QueryError("Database not initialized".to_string()))?;

        reader::query_metrics(conn, params)
    }

    async fn query_latest_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>> {
        let conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::QueryError("Database not initialized".to_string()))?;

        reader::query_latest_metrics(conn, params)
    }

    async fn stats(&self) -> Result<StorageStats> {
        let conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::QueryError("Database not initialized".to_string()))?;

        reader::get_stats(conn)
    }

    async fn purge(&self, options: &PurgeOptions) -> Result<u64> {
        // Acquire purge lock
        let _guard = self.purge_lock.try_lock().await?;

        let mut conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_mut()
            .ok_or_else(|| StorageError::WriteError("Database not initialized".to_string()))?;

        // Calculate cutoff timestamp
        let cutoff_timestamp = if let Some(older_than) = options.older_than {
            older_than
        } else {
            // Default to retention period from config
            let cutoff =
                chrono::Utc::now() - chrono::Duration::days(self.config.retention_days as i64);
            cutoff.timestamp_nanos_opt().unwrap_or(0)
        };

        // Perform purge
        let record = purge::purge_old_data(
            conn,
            cutoff_timestamp,
            10000,
            &options.signal_types,
            options.dry_run,
        )?;

        // Run VACUUM to reclaim space (only if not dry run)
        if !options.dry_run {
            purge::vacuum(conn)?;
        }

        let total_deleted = record.logs_deleted + record.spans_deleted + record.metrics_deleted;
        Ok(total_deleted as u64)
    }

    async fn distinct_resource_keys(&self, signal: &str) -> Result<Vec<String>> {
        let conn_guard = self.conn.lock().unwrap();
        let conn = conn_guard
            .as_ref()
            .ok_or_else(|| StorageError::QueryError("Database not initialized".to_string()))?;

        reader::distinct_resource_keys(conn, signal)
    }

    async fn close(&mut self) -> Result<()> {
        // Stop purge scheduler
        if let Some(handle) = self.purge_handle.lock().unwrap().take() {
            handle.abort();
        }

        let mut conn_guard = self.conn.lock().unwrap();
        if let Some(conn) = conn_guard.take() {
            conn.close()
                .map_err(|(_, e)| StorageError::DatabaseError(e))?;
        }
        Ok(())
    }
}

impl SqliteBackend {
    /// Start background purge scheduler
    fn start_purge_scheduler(&self) {
        let conn = self.conn.clone();
        let config = self.config.clone();
        let purge_lock = self.purge_lock.clone();

        let handle = tokio::spawn(async move {
            loop {
                // Calculate next purge time (daily at 2 AM)
                let now = chrono::Local::now();
                let next_purge = if now.hour() < 2 {
                    now.date_naive().and_hms_opt(2, 0, 0).unwrap()
                } else {
                    (now.date_naive() + chrono::Duration::days(1))
                        .and_hms_opt(2, 0, 0)
                        .unwrap()
                };
                let next_purge =
                    chrono::TimeZone::from_local_datetime(&chrono::Local, &next_purge).unwrap();
                let duration = (next_purge - now)
                    .to_std()
                    .unwrap_or(std::time::Duration::from_secs(86400));

                // Sleep until next purge time
                tokio::time::sleep(duration).await;

                // Try to acquire purge lock
                if let Ok(_guard) = purge_lock.try_lock().await {
                    // Perform automatic purge
                    let mut conn_guard = conn.lock().unwrap();
                    if let Some(conn_ref) = conn_guard.as_mut() {
                        let cutoff = chrono::Utc::now()
                            - chrono::Duration::days(config.retention_days as i64);
                        let cutoff_timestamp = cutoff.timestamp_nanos_opt().unwrap_or(0);

                        if let Ok(record) = purge::purge_old_data(
                            conn_ref,
                            cutoff_timestamp,
                            10000,
                            &[
                                crate::SignalType::Logs,
                                crate::SignalType::Traces,
                                crate::SignalType::Metrics,
                            ],
                            false,
                        ) {
                            tracing::info!(
                                "Automatic purge completed: {} logs, {} spans, {} metrics deleted",
                                record.logs_deleted,
                                record.spans_deleted,
                                record.metrics_deleted
                            );

                            // Run VACUUM
                            let _ = purge::vacuum(conn_ref);
                        }
                    }
                }
            }
        });

        *self.purge_handle.lock().unwrap() = Some(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_sqlite_backend_creation() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

        let backend = SqliteBackend::new(config);
        assert!(backend.conn.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn test_sqlite_backend_initialization() {
        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

        let mut backend = SqliteBackend::new(config);
        let result = backend.initialize().await;
        assert!(result.is_ok());
        assert!(backend.conn.lock().unwrap().is_some());
    }

    #[tokio::test]
    async fn test_stats_returns_counts() {
        use rotel_core::telemetry::log::SeverityLevel;
        use std::collections::HashMap;

        let temp_dir = TempDir::new().unwrap();
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());

        let mut backend = SqliteBackend::new(config);
        backend.initialize().await.unwrap();

        // Insert a log record
        let log = LogRecord {
            timestamp: 1000,
            observed_timestamp: Some(1000),
            severity: SeverityLevel::Info,
            severity_text: Some("INFO".to_string()),
            body: "test log".to_string(),
            trace_id: None,
            span_id: None,
            attributes: HashMap::new(),
            resource: None,
        };
        backend.write_log(&log).await.unwrap();

        // Get stats
        let stats = backend.stats().await.unwrap();

        // Verify non-zero counts
        assert_eq!(stats.log_count, 1);
        assert_eq!(stats.span_count, 0);
        assert_eq!(stats.metric_count, 0);
        assert!(stats.storage_size_bytes > 0);
    }
}
