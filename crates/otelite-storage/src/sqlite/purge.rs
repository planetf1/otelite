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
    signal_types: &[crate::SignalType],
    dry_run: bool,
) -> Result<PurgeRecord, StorageError> {
    let start_time = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);

    let logs_deleted = if signal_types.contains(&crate::SignalType::Logs) {
        purge_table(conn, "logs", cutoff_timestamp, batch_size, dry_run)?
    } else {
        0
    };
    let spans_deleted = if signal_types.contains(&crate::SignalType::Traces) {
        purge_table(conn, "spans", cutoff_timestamp, batch_size, dry_run)?
    } else {
        0
    };
    let metrics_deleted = if signal_types.contains(&crate::SignalType::Metrics) {
        purge_table(conn, "metrics", cutoff_timestamp, batch_size, dry_run)?
    } else {
        0
    };

    let end_time = chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0);

    // Record purge history (only if not dry run)
    let record = PurgeRecord {
        start_time,
        end_time,
        logs_deleted,
        spans_deleted,
        metrics_deleted,
    };

    if !dry_run {
        record_purge_history(conn, &record)?;
    }

    Ok(record)
}

/// Purge one table: a single COUNT in dry-run mode, otherwise
/// repeated batched deletes until fewer than `batch_size` rows match.
fn purge_table(
    conn: &mut Connection,
    table: &str,
    cutoff_timestamp: i64,
    batch_size: usize,
    dry_run: bool,
) -> Result<i64, StorageError> {
    if dry_run {
        return count_all(conn, table, cutoff_timestamp);
    }

    let mut deleted = 0i64;
    loop {
        let n = delete_batch(conn, table, cutoff_timestamp, batch_size)?;
        deleted += n;
        if n < batch_size as i64 {
            break;
        }
    }
    Ok(deleted)
}

/// Count every record that would be deleted (dry-run mode).
///
/// A single COUNT(*): a dry run deletes nothing, so a batch-limited
/// count loop would count the same rows forever and never terminate.
fn count_all(
    conn: &Connection,
    table: &str,
    cutoff_timestamp: i64,
) -> Result<i64, StorageError> {
    // Use correct timestamp column for each table
    let timestamp_col = match table {
        "spans" => "start_time",
        _ => "timestamp", // logs and metrics use 'timestamp'
    };

    let sql = format!("SELECT COUNT(*) FROM {} WHERE {} < ?", table, timestamp_col);

    conn.query_row(&sql, rusqlite::params![cutoff_timestamp], |row| row.get::<_, i64>(0))
        .map_err(|e| StorageError::QueryError(format!("Failed to count rows for dry-run purge: {}", e)))
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
    // Use correct timestamp column for each table
    let timestamp_col = match table {
        "spans" => "start_time",
        _ => "timestamp", // logs and metrics use 'timestamp'
    };

    let sql = format!(
        "DELETE FROM {} WHERE id IN (
            SELECT id FROM {} WHERE {} < ? LIMIT ?
        )",
        table, table, timestamp_col
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

/// Reclaim WAL space after a bulk delete without the exclusive lock
/// that VACUUM requires.
///
/// VACUUM needs exclusive access to the database file and fails with
/// SQLITE_BUSY while the read pool (or any other connection) is open.
/// A passive checkpoint moves committed WAL frames back into the main
/// database file, which is enough to stop the WAL from growing
/// unboundedly after a purge.
pub fn checkpoint(conn: &mut Connection) -> Result<(), StorageError> {
    conn.execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
        .map_err(|e| StorageError::WriteError(format!("Failed to checkpoint WAL after purge: {}", e)))
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

    /// Regression test: a dry run over more matching rows than the
    /// batch size must return the full count and terminate. Before the
    /// fix the dry-run branch reused the batch-limited count, deleted
    /// nothing, and looped forever.
    #[test]
    fn test_dry_run_counts_beyond_batch_size() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE logs (
                id INTEGER PRIMARY KEY,
                timestamp INTEGER NOT NULL,
                data TEXT
            )",
            [],
        )
        .unwrap();

        // 15 rows, all older than the cutoff — more than one batch of
        // 10 would cover.
        for i in 0..15 {
            conn.execute(
                "INSERT INTO logs (timestamp, data) VALUES (?, ?)",
                rusqlite::params![i, format!("log {}{}", i, "pad")],
            )
            .unwrap();
        }

        let record = purge_old_data(&mut conn, 10000, 10, &[crate::SignalType::Logs], true)
            .expect("dry run must terminate");
        assert_eq!(record.logs_deleted, 15, "dry run must count all matching rows");

        // A dry run deletes nothing.
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 15);
    }

    #[test]
    fn test_checkpoint() {
        // In-memory DBs have no WAL; checkpoint is a no-op that must
        // still succeed (the real path runs it on file-backed DBs).
        let mut conn = Connection::open_in_memory().unwrap();
        assert!(checkpoint(&mut conn).is_ok());
    }
}
