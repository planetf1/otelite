//! Write operations for SQLite backend

use crate::error::{Result, StorageError};
use otelite_core::telemetry::metric::MetricType;
use otelite_core::telemetry::{LogRecord, Metric, Span};
use rusqlite::Connection;
use serde_json;
use std::collections::HashMap;

fn scope_json(attributes: &HashMap<String, String>) -> Result<String> {
    let name = attributes.get("otel.scope.name");
    let version = attributes.get("otel.scope.version");
    match (name, version) {
        (None, None) => Ok("{}".to_string()),
        _ => serde_json::to_string(&serde_json::json!({
            "name": name,
            "version": version,
        }))
        .map_err(StorageError::from),
    }
}

/// Classify a SQLite write failure before wrapping it.
///
/// Disk-full and corruption errors keep their dedicated `StorageError`
/// variants (the receiver's health checker flips `/health` unhealthy on
/// sustained failures of exactly those, #256); everything else keeps the
/// contextual `WriteError` the callers always produced.
fn classify_write_error(e: rusqlite::Error, context: &str) -> StorageError {
    match StorageError::from_rusqlite(e) {
        StorageError::DatabaseError(inner) => {
            StorageError::WriteError(format!("{context}: {inner}"))
        },
        classified => classified,
    }
}

/// Write a log record to the database
pub fn write_log(conn: &Connection, log: &LogRecord) -> Result<()> {
    // Serialize complex fields to JSON
    let attributes = serde_json::to_string(&log.attributes)?;
    let resource = serde_json::to_string(&log.resource)?;
    let scope = scope_json(&log.attributes)?;

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
            scope,
        ],
    )
    .map_err(|e| classify_write_error(e, "Failed to write log"))?;

    Ok(())
}

/// Write a span to the database
pub fn write_span(conn: &Connection, span: &Span) -> Result<()> {
    // Serialize complex fields to JSON
    let attributes = serde_json::to_string(&span.attributes)?;
    let events = serde_json::to_string(&span.events)?;
    let resource = serde_json::to_string(&span.resource)?;
    let scope = scope_json(&span.attributes)?;

    // ON CONFLICT DO NOTHING: a span's identity is (trace_id, span_id), and
    // an OTLP exporter retrying a batch whose commit won the race re-sends
    // the same identity — the duplicate is dropped instead of inflating
    // span-based analytics (#254). Other constraint violations still error.
    conn.execute(
        "INSERT INTO spans (
            trace_id, span_id, parent_span_id, name, kind,
            start_time, end_time, attributes, events,
            status_code, status_message, resource, scope
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        ON CONFLICT(trace_id, span_id) DO NOTHING",
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
            resource,
            scope,
        ],
    )
    .map_err(|e| classify_write_error(e, "Failed to write span"))?;

    Ok(())
}

/// Write a metric to the database
pub fn write_metric(conn: &Connection, metric: &Metric) -> Result<()> {
    // Serialize complex fields to JSON
    let attributes = serde_json::to_string(&metric.attributes)?;
    let resource = serde_json::to_string(&metric.resource)?;
    let scope = scope_json(&metric.attributes)?;

    // Determine metric type and values based on MetricType
    let (metric_type, value_int, value_double, value_histogram, value_summary) =
        match &metric.metric_type {
            MetricType::Gauge(v) => (0, None, Some(*v), None, None),
            MetricType::Counter(v) => (1, Some(*v as i64), None, None, None),
            MetricType::CounterDouble(v) => (1, None, Some(*v), None, None),
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
            scope,
        ],
    )
    .map_err(|e| classify_write_error(e, "Failed to write metric"))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_core::telemetry::{
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
    fn test_write_log_persists_scope() {
        let conn = setup_test_db();
        let log = LogRecord {
            timestamp: 1234567890,
            observed_timestamp: None,
            severity: SeverityLevel::Info,
            severity_text: None,
            body: "scope".to_string(),
            attributes: HashMap::from([
                ("otel.scope.name".to_string(), "opencode".to_string()),
                ("otel.scope.version".to_string(), "1.18.15".to_string()),
            ]),
            resource: None,
            trace_id: None,
            span_id: None,
        };

        write_log(&conn, &log).unwrap();
        let scope: String = conn
            .query_row("SELECT scope FROM logs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(scope, r#"{"name":"opencode","version":"1.18.15"}"#);
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
            resource: None,
        };

        let result = write_span(&conn, &span);
        assert!(result.is_ok());

        // Verify span was written
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM spans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    /// Exporter retry: re-inserting a span with the same identity must not
    /// duplicate the row (#254).
    #[test]
    fn test_write_span_retry_same_identity_not_duplicated() {
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
            resource: None,
        };

        write_span(&conn, &span).unwrap();
        write_span(&conn, &span).unwrap(); // the retried export

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM spans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "the retried span must be ignored, not duplicated");
    }

    #[test]
    fn test_write_span_persists_scope() {
        let conn = setup_test_db();
        let span = Span {
            trace_id: "trace123".to_string(),
            span_id: "span456".to_string(),
            parent_span_id: None,
            name: "test-span".to_string(),
            kind: SpanKind::Internal,
            start_time: 1234567890,
            end_time: 1234567900,
            attributes: HashMap::from([(
                "otel.scope.name".to_string(),
                "codex_cli_rs".to_string(),
            )]),
            events: Vec::new(),
            status: SpanStatus {
                code: StatusCode::Ok,
                message: None,
            },
            resource: None,
        };

        write_span(&conn, &span).unwrap();
        let scope: String = conn
            .query_row("SELECT scope FROM spans", [], |row| row.get(0))
            .unwrap();
        assert_eq!(scope, r#"{"name":"codex_cli_rs","version":null}"#);
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

    /// Fractional (double-typed OTLP sum) counters round-trip through
    /// `value_double` without integer truncation; integer counters stay in
    /// `value_int` (#252).
    #[test]
    fn test_write_metric_counter_double_roundtrip() {
        use crate::sqlite::reader::query_metrics;
        use otelite_core::storage::QueryParams;

        let conn = setup_test_db();

        let int_metric = Metric {
            name: "test.counter.int".to_string(),
            description: None,
            unit: Some("{token}".to_string()),
            metric_type: MetricType::Counter(42),
            timestamp: 1000,
            attributes: HashMap::new(),
            resource: None,
        };
        let double_metric = Metric {
            name: "test.counter.double".to_string(),
            description: None,
            unit: Some("USD".to_string()),
            metric_type: MetricType::CounterDouble(19.87),
            timestamp: 2000,
            attributes: HashMap::new(),
            resource: None,
        };
        write_metric(&conn, &int_metric).unwrap();
        write_metric(&conn, &double_metric).unwrap();

        // Double counter lands in value_double with value_int NULL.
        let (value_int, value_double): (Option<i64>, Option<f64>) = conn
            .query_row(
                "SELECT value_int, value_double FROM metrics \
                 WHERE name = 'test.counter.double'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(value_int, None);
        assert_eq!(value_double, Some(19.87));

        // Reader maps value_int -> Counter and value_double -> CounterDouble.
        let params = QueryParams {
            start_time: None,
            end_time: None,
            limit: None,
            trace_id: None,
            span_id: None,
            min_severity: None,
            search_text: None,
            predicates: vec![],
        };
        let read = query_metrics(&conn, &params)
            .unwrap()
            .into_iter()
            .filter(|m| m.name.starts_with("test.counter."))
            .collect::<Vec<_>>();
        assert_eq!(read.len(), 2);
        let m = read
            .iter()
            .find(|m| m.name == "test.counter.int")
            .expect("int counter present");
        assert_eq!(m.metric_type, MetricType::Counter(42));
        let m = read
            .iter()
            .find(|m| m.name == "test.counter.double")
            .expect("double counter present");
        assert_eq!(m.metric_type, MetricType::CounterDouble(19.87));
    }

    #[test]
    fn test_write_metric_persists_scope() {
        let conn = setup_test_db();
        let metric = Metric {
            name: "test.metric".to_string(),
            description: None,
            unit: None,
            metric_type: MetricType::Gauge(42.0),
            timestamp: 1234567890,
            attributes: HashMap::from([(
                "otel.scope.name".to_string(),
                "com.anthropic.claude_code.tracing".to_string(),
            )]),
            resource: None,
        };

        write_metric(&conn, &metric).unwrap();
        let scope: String = conn
            .query_row("SELECT scope FROM metrics", [], |row| row.get(0))
            .unwrap();
        assert_eq!(
            scope,
            r#"{"name":"com.anthropic.claude_code.tracing","version":null}"#
        );
    }
}
