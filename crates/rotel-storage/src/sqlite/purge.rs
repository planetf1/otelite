//! Purge operations for SQLite storage backend.
//!
//! This module handles automatic and manual purging of old telemetry data,
//! including batched deletions, purge history tracking, and VACUUM operations.

use crate::error::StorageError;
use rusqlite::{Connection, Transaction};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Purge history record tracking purge operations
#[derive(Debug, Clone)]
pub struct PurgeRecord {
    pub start_time: i64,
    pub end_time: i64,
    pub logs_deleted: i64,
    pub spans_deleted: i64,
    pub metrics_deleted: i64,
}

/// Purge lock to prevent concurrent purge operations
pub struct PurgeLock {
    locked: Arc<Mutex<bool>>,
}

impl Default for PurgeLock {
    fn default() -> Self {
        Self {
            locked: Arc::new(Mutex::new(false)),
        }
    }
}

impl PurgeLock {
    pub fn new() -> Self {
        Self::default()
    }

    pub async fn try_lock(&self) -> Result<PurgeGuard, StorageError> {
        let mut locked = self.locked.lock().await;
        if *locked {
            return Err(StorageError::WriteError(
                "Purge operation already in progress".to_string(),
            ));
        }
        *locked = true;
        Ok(PurgeGuard {
            locked: self.locked.clone(),
        })
    }
}

/// Guard that releases purge lock when dropped
pub struct PurgeGuard {
    locked: Arc<Mutex<bool>>,
}

impl Drop for PurgeGuard {
    fn drop(&mut self) {
        let locked = self.locked.clone();
        tokio::spawn(async move {
            let mut lock = locked.lock().await;
            *lock = false;
        });
    }
}

/// Purge old data from the database in batches
///
/// Deletes data older than the specified cutoff timestamp in batches
/// to avoid locking the database for extended periods.
pub fn purge_old_data(
    conn: &mut Connection,
    cutoff_timestamp: i64,
    batch_size: usize,
) -> Result<PurgeRecord, StorageError> {
    let start_time = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);

    let mut logs_deleted = 0i64;
    let mut spans_deleted = 0i64;
    let mut metrics_deleted = 0i64;

    // Purge logs in batches
    loop {
        let deleted = delete_batch(conn, "logs", cutoff_timestamp, batch_size)?;
        logs_deleted += deleted;
        if deleted < batch_size as i64 {
            break;
        }
    }

    // Purge spans in batches
    loop {
        let deleted = delete_batch(conn, "spans", cutoff_timestamp, batch_size)?;
        spans_deleted += deleted;
        if deleted < batch_size as i64 {
            break;
        }
    }

    // Purge metrics in batches
    loop {
        let deleted = delete_batch(conn, "metrics", cutoff_timestamp, batch_size)?;
        metrics_deleted += deleted;
        if deleted < batch_size as i64 {
            break;
        }
    }

    let end_time = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);

    // Record purge history
    let record = PurgeRecord {
        start_time,
        end_time,
        logs_deleted,
        spans_deleted,
        metrics_deleted,
    };

    record_purge_history(conn, &record)?;

    Ok(record)
}

/// Delete a batch of records from a table
fn delete_batch(
    conn: &mut Connection,
    table: &str,
    cutoff_timestamp: i64,
    batch_size: usize,
) -> Result<i64, StorageError> {
    let tx = conn
        .transaction()
        .map_err(|e| StorageError::WriteError(format!("Failed to start transaction: {}", e)))?;

    let deleted = delete_batch_in_transaction(&tx, table, cutoff_timestamp, batch_size)?;

    tx.commit()
        .map_err(|e| StorageError::WriteError(format!("Failed to commit transaction: {}", e)))?;

    Ok(deleted)
}

/// Delete a batch of records within a transaction
fn delete_batch_in_transaction(
    tx: &Transaction,
    table: &str,
    cutoff_timestamp: i64,
    batch_size: usize,
) -> Result<i64, StorageError> {
    let sql = format!(
        "DELETE FROM {} WHERE id IN (
            SELECT id FROM {} WHERE timestamp < ? LIMIT ?
        )",
        table, table
    );

    tx.execute(&sql, rusqlite::params![cutoff_timestamp, batch_size])
        .map(|n| n as i64)
        .map_err(|e| StorageError::WriteError(format!("Failed to delete batch: {}", e)))
}

/// Record purge history in the database
fn record_purge_history(conn: &Connection, record: &PurgeRecord) -> Result<(), StorageError> {
    conn.execute(
        "INSERT INTO purge_history (start_time, end_time, logs_deleted, spans_deleted, metrics_deleted)
         VALUES (?, ?, ?, ?, ?)",
        rusqlite::params![
            record.start_time,
            record.end_time,
            record.logs_deleted,
            record.spans_deleted,
            record.metrics_deleted,
        ],
    )
    .map_err(|e| StorageError::WriteError(format!("Failed to record purge history: {}", e)))?;

    Ok(())
}

/// Run VACUUM to reclaim disk space after purge
pub fn vacuum(conn: &mut Connection) -> Result<(), StorageError> {
    conn.execute_batch("VACUUM")
        .map_err(|e| StorageError::WriteError(format!("Failed to vacuum database: {}", e)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_purge_lock() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let lock = PurgeLock::new();

            // First lock should succeed
            let guard1 = lock.try_lock().await;
            assert!(guard1.is_ok());

            // Second lock should fail
            let guard2 = lock.try_lock().await;
            assert!(guard2.is_err());

            // Drop first guard
            drop(guard1);

            // Give tokio time to process the drop
            tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

            // Third lock should succeed
            let guard3 = lock.try_lock().await;
            assert!(guard3.is_ok());
        });
    }

    #[test]
    fn test_delete_batch() {
        let mut conn = Connection::open_in_memory().unwrap();

        // Create test table
        conn.execute(
            "CREATE TABLE logs (
                id INTEGER PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                data TEXT
            )",
            [],
        )
        .unwrap();

        // Insert test data
        for i in 0..15 {
            conn.execute(
                "INSERT INTO logs (timestamp, data) VALUES (?, ?)",
                rusqlite::params![i * 1000, format!("log {}", i)],
            )
            .unwrap();
        }

        // Delete batch of 10 records older than timestamp 10000
        let deleted = delete_batch(&mut conn, "logs", 10000, 10).unwrap();
        assert_eq!(deleted, 10);

        // Verify remaining count
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 5);
    }

    #[test]
    fn test_vacuum() {
        let mut conn = Connection::open_in_memory().unwrap();
        let result = vacuum(&mut conn);
        assert!(result.is_ok());
    }
}

// Made with Bob
