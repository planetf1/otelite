//! SQLite backend implementation

use crate::error::{Result, StorageError};
use crate::{PurgeOptions, QueryParams, StorageBackend, StorageConfig, StorageStats};
use async_trait::async_trait;
use rotel_core::telemetry::{LogRecord, Metric, Span};
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

pub mod reader;
pub mod schema;
pub mod writer;

/// SQLite storage backend
pub struct SqliteBackend {
    config: StorageConfig,
    conn: Arc<Mutex<Option<Connection>>>,
}

impl SqliteBackend {
    /// Create a new SQLite backend with the given configuration
    pub fn new(config: StorageConfig) -> Self {
        Self {
            config,
            conn: Arc::new(Mutex::new(None)),
        }
    }

    /// Get the database file path
    fn db_path(&self) -> PathBuf {
        self.config.data_dir.join("rotel.db")
    }
}

#[async_trait]
impl StorageBackend for SqliteBackend {
    async fn initialize(&mut self) -> Result<()> {
        // Create data directory if it doesn't exist
        std::fs::create_dir_all(&self.config.data_dir).map_err(|e| {
            StorageError::InitializationError(format!("Failed to create data directory: {}", e))
        })?;

        // Open database connection
        let db_path = self.db_path();
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

    async fn stats(&self) -> Result<StorageStats> {
        // TODO: Implement in Phase 6
        Ok(StorageStats {
            log_count: 0,
            span_count: 0,
            metric_count: 0,
            oldest_timestamp: None,
            newest_timestamp: None,
            storage_size_bytes: 0,
        })
    }

    async fn purge(&self, _options: &PurgeOptions) -> Result<u64> {
        // TODO: Implement in Phase 4/5
        Ok(0)
    }

    async fn close(&mut self) -> Result<()> {
        let mut conn_guard = self.conn.lock().unwrap();
        if let Some(conn) = conn_guard.take() {
            conn.close()
                .map_err(|(_, e)| StorageError::DatabaseError(e))?;
        }
        Ok(())
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
}

// Made with Bob
