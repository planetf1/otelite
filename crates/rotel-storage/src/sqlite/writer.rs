//! Write operations for SQLite backend

use crate::error::{Result, StorageError};
use rotel_core::telemetry::metric::MetricType;
use rotel_core::telemetry::{LogRecord, Metric, Span};
use rusqlite::Connection;
use serde_json;

/// Write a log record to the database
pub fn write_log(conn: &Connection, log: &LogRecord) -> Result<()> {
    // Serialize complex fields to JSON
    let attributes = serde_json::to_string(&log.attributes)?;
    let resource = serde_json::to_string(&log.resource)?;

    conn.execute(
        "INSERT INTO logs (
            timestamp, observed_timestamp, trace_id, span_id,
            severity_number, severity_text, body,
            attributes, resource, scope
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            log.timestamp,
            log.observed_timestamp,
            log.trace_id.as_deref(),
            log.span_id.as_deref(),
            log.severity as i32,
            log.severity_text.as_deref(),
            &log.body,
            attributes,
            resource,
            "{}", // scope placeholder
        ],
    )
    .map_err(|e| StorageError::WriteError(format!("Failed to write log: {}", e)))?;

    Ok(())
}

/// Write a span to the database
pub fn write_span(conn: &Connection, span: &Span) -> Result<()> {
    // Serialize complex fields to JSON
    let attributes = serde_json::to_string(&span.attributes)?;
    let events = serde_json::to_string(&span.events)?;

    conn.execute(
        "INSERT INTO spans (
            trace_id, span_id, parent_span_id, name, kind,
            start_time, end_time, attributes, events,
            status_code, status_message, resource, scope
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        rusqlite::params![
            &span.trace_id,
            &span.span_id,
            span.parent_span_id.as_deref(),
            &span.name,
            span.kind as i32,
            span.start_time,
            span.end_time,
            attributes,
            events,
            span.status.code as i32,
            span.status.message.as_deref(),
            "{}", // resource placeholder
            "{}", // scope placeholder
        ],
    )
    .map_err(|e| StorageError::WriteError(format!("Failed to write span: {}", e)))?;

    Ok(())
}

/// Write a metric to the database
pub fn write_metric(conn: &Connection, metric: &Metric) -> Result<()> {
    // Serialize complex fields to JSON
    let attributes = serde_json::to_string(&metric.attributes)?;
    let resource = serde_json::to_string(&metric.resource)?;

    // Determine metric type and values based on MetricType
    let (metric_type, value_int, value_double, value_histogram, value_summary) =
        match &metric.metric_type {
            MetricType::Gauge(v) => (0, None, Some(*v), None, None),
            MetricType::Counter(v) => (1, Some(*v as i64), None, None, None),
            MetricType::Histogram {
                count,
                sum,
                buckets,
            } => {
                let histogram_json = serde_json::to_string(&(count, sum, buckets))?;
                (2, None, None, Some(histogram_json), None)
            },
            MetricType::Summary {
                count,
                sum,
                quantiles,
            } => {
                let summary_json = serde_json::to_string(&(count, sum, quantiles))?;
                (3, None, None, None, Some(summary_json))
            },
        };

    conn.execute(
        "INSERT INTO metrics (
            name, description, unit, metric_type, timestamp,
            value_int, value_double, value_histogram, value_summary,
            attributes, resource, scope
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
        rusqlite::params![
            &metric.name,
            metric.description.as_deref(),
            metric.unit.as_deref(),
            metric_type,
            metric.timestamp,
            value_int,
            value_double,
            value_histogram,
            value_summary,
            attributes,
            resource,
            "{}", // scope placeholder
        ],
    )
    .map_err(|e| StorageError::WriteError(format!("Failed to write metric: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rotel_core::telemetry::{
        log::SeverityLevel,
        metric::MetricType,
        trace::{SpanKind, SpanStatus, StatusCode},
        LogRecord, Metric, Span,
    };
    use rusqlite::Connection;
    use std::collections::HashMap;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::sqlite::schema::initialize_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_write_log() {
        let conn = setup_test_db();

        let log = LogRecord {
            timestamp: 1234567890,
            observed_timestamp: Some(1234567891),
            trace_id: None,
            span_id: None,
            severity: SeverityLevel::Info,
            severity_text: Some("INFO".to_string()),
            body: "Test log message".to_string(),
            attributes: HashMap::new(),
            resource: None,
        };

        let result = write_log(&conn, &log);
        assert!(result.is_ok());

        // Verify log was written
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_write_span() {
        let conn = setup_test_db();

        let span = Span {
            trace_id: "trace123".to_string(),
            span_id: "span456".to_string(),
            parent_span_id: None,
            name: "test-span".to_string(),
            kind: SpanKind::Internal,
            start_time: 1234567890,
            end_time: 1234567900,
            attributes: HashMap::new(),
            events: Vec::new(),
            status: SpanStatus {
                code: StatusCode::Ok,
                message: None,
            },
        };

        let result = write_span(&conn, &span);
        assert!(result.is_ok());

        // Verify span was written
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM spans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn test_write_metric() {
        let conn = setup_test_db();

        let metric = Metric {
            name: "test.metric".to_string(),
            description: Some("Test metric".to_string()),
            unit: Some("count".to_string()),
            metric_type: MetricType::Gauge(42.0),
            timestamp: 1234567890,
            attributes: HashMap::new(),
            resource: None,
        };

        let result = write_metric(&conn, &metric);
        assert!(result.is_ok());

        // Verify metric was written
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }
}

// Made with Bob
