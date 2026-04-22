//! SQLite schema definitions and initialization

use crate::error::Result;
use rusqlite::Connection;

/// Initialize the database schema
pub fn initialize_schema(conn: &Connection) -> Result<()> {
    // Enable WAL mode for better concurrency
    conn.execute_batch("PRAGMA journal_mode=WAL;")?;

    // Set synchronous mode to NORMAL for better performance
    conn.execute_batch("PRAGMA synchronous=NORMAL;")?;

    // Create logs table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp INTEGER NOT NULL,
            observed_timestamp INTEGER,
            trace_id TEXT,
            span_id TEXT,
            severity_number INTEGER NOT NULL,
            severity_text TEXT,
            body TEXT NOT NULL,
            attributes TEXT,
            resource TEXT,
            scope TEXT,
            flags INTEGER,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );",
    )?;

    // Create indexes for logs
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_logs_timestamp ON logs(timestamp);
         CREATE INDEX IF NOT EXISTS idx_logs_severity ON logs(severity_number);
         CREATE INDEX IF NOT EXISTS idx_logs_trace_id ON logs(trace_id) WHERE trace_id IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_logs_created_at ON logs(created_at);",
    )?;

    // Create spans table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS spans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            trace_id TEXT NOT NULL,
            span_id TEXT NOT NULL,
            parent_span_id TEXT,
            name TEXT NOT NULL,
            kind INTEGER NOT NULL,
            start_time INTEGER NOT NULL,
            end_time INTEGER NOT NULL,
            attributes TEXT,
            events TEXT,
            links TEXT,
            status_code INTEGER,
            status_message TEXT,
            resource TEXT,
            scope TEXT,
            flags INTEGER,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );",
    )?;

    // Create indexes for spans
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_spans_trace_id ON spans(trace_id);
         CREATE INDEX IF NOT EXISTS idx_spans_span_id ON spans(span_id);
         CREATE INDEX IF NOT EXISTS idx_spans_start_time ON spans(start_time);
         CREATE INDEX IF NOT EXISTS idx_spans_parent_span_id ON spans(parent_span_id) WHERE parent_span_id IS NOT NULL;
         CREATE INDEX IF NOT EXISTS idx_spans_created_at ON spans(created_at);",
    )?;

    // Create metrics table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS metrics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            description TEXT,
            unit TEXT,
            metric_type INTEGER NOT NULL,
            timestamp INTEGER NOT NULL,
            value_int INTEGER,
            value_double REAL,
            value_histogram TEXT,
            value_summary TEXT,
            attributes TEXT,
            resource TEXT,
            scope TEXT,
            flags INTEGER,
            created_at INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
        );",
    )?;

    // Create indexes for metrics
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_metrics_name ON metrics(name);
         CREATE INDEX IF NOT EXISTS idx_metrics_timestamp ON metrics(timestamp);
         CREATE INDEX IF NOT EXISTS idx_metrics_type ON metrics(metric_type);
         CREATE INDEX IF NOT EXISTS idx_metrics_created_at ON metrics(created_at);",
    )?;

    // Create purge_history table
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS purge_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            start_time INTEGER NOT NULL,
            end_time INTEGER NOT NULL,
            logs_deleted INTEGER NOT NULL DEFAULT 0,
            spans_deleted INTEGER NOT NULL DEFAULT 0,
            metrics_deleted INTEGER NOT NULL DEFAULT 0
        );",
    )?;

    // Create index for purge_history
    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_purge_history_start_time ON purge_history(start_time);",
    )?;

    // Create FTS5 full-text search table for logs
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS logs_fts USING fts5(
            body,
            content='logs',
            content_rowid='id'
        );",
    )?;

    // Create triggers to keep FTS5 table in sync
    conn.execute_batch(
        "CREATE TRIGGER IF NOT EXISTS logs_fts_insert AFTER INSERT ON logs BEGIN
            INSERT INTO logs_fts(rowid, body) VALUES (new.id, new.body);
         END;

         CREATE TRIGGER IF NOT EXISTS logs_fts_delete AFTER DELETE ON logs BEGIN
            DELETE FROM logs_fts WHERE rowid = old.id;
         END;

         CREATE TRIGGER IF NOT EXISTS logs_fts_update AFTER UPDATE ON logs BEGIN
            DELETE FROM logs_fts WHERE rowid = old.id;
            INSERT INTO logs_fts(rowid, body) VALUES (new.id, new.body);
         END;",
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_schema_initialization() {
        let conn = Connection::open_in_memory().unwrap();
        let result = initialize_schema(&conn);
        assert!(result.is_ok());
    }

    #[test]
    fn test_tables_created() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Verify logs table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='logs'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);

        // Verify spans table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='spans'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);

        // Verify metrics table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='metrics'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);

        // Verify purge_history table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='purge_history'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);

        // Verify FTS5 table exists
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' AND name='logs_fts'")
            .unwrap();
        let exists = stmt.exists([]).unwrap();
        assert!(exists);
    }

    #[test]
    fn test_indexes_created() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Verify at least one index exists for logs
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND tbl_name='logs'")
            .unwrap();
        let count: i32 = stmt.query_map([], |_| Ok(1)).unwrap().count() as i32;
        assert!(count > 0);
    }

    #[test]
    fn test_fts5_triggers_created() {
        let conn = Connection::open_in_memory().unwrap();
        initialize_schema(&conn).unwrap();

        // Verify FTS5 triggers exist
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type='trigger' AND name LIKE 'logs_fts_%'",
            )
            .unwrap();
        let count: i32 = stmt.query_map([], |_| Ok(1)).unwrap().count() as i32;
        assert_eq!(count, 3); // insert, delete, update triggers
    }
}
