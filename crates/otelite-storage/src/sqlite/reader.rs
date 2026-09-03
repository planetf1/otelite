//! Read operations for SQLite backend

use crate::error::{Result, StorageError};
use crate::{QueryParams, StorageStats};
use otelite_core::filters::GenAiFilters;
use otelite_core::query::{Operator, QueryPredicate, QueryValue};
use otelite_core::semconv;
use otelite_core::telemetry::log::SeverityLevel;
use otelite_core::telemetry::trace::{SpanKind, SpanStatus, StatusCode};
use otelite_core::telemetry::{
    classify_span_capabilities, correlate_codex_usage, is_codex_request_span,
    is_codex_usage_candidate, CorrelationOutcome, GenAiEmitter, GenAiSpanRole, LogRecord, Metric,
    MetricObservation, Span, CODEX_CORRELATION_RULE,
};
use rusqlite::{Connection, Row};
use serde::de::DeserializeOwned;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Query logs from the database
pub fn query_logs(conn: &Connection, params: &QueryParams) -> Result<Vec<LogRecord>> {
    let mut query = String::from("SELECT * FROM logs WHERE 1=1");
    let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // Add time range filter
    if let Some(start) = params.start_time {
        query.push_str(" AND timestamp >= ?");
        sql_params.push(Box::new(start));
    }
    if let Some(end) = params.end_time {
        query.push_str(" AND timestamp <= ?");
        sql_params.push(Box::new(end));
    }

    // Add trace/span filter
    if let Some(ref trace_id) = params.trace_id {
        query.push_str(" AND trace_id = ?");
        sql_params.push(Box::new(trace_id.clone()));
    }
    if let Some(ref span_id) = params.span_id {
        query.push_str(" AND span_id = ?");
        sql_params.push(Box::new(span_id.clone()));
    }

    // Add severity filter
    if let Some(min_severity) = params.min_severity {
        query.push_str(" AND severity_number >= ?");
        sql_params.push(Box::new(min_severity.to_i32()));
    }

    // Add full-text search if provided
    if let Some(ref search) = params.search_text {
        query.push_str(" AND id IN (SELECT rowid FROM logs_fts WHERE body MATCH ?)");
        sql_params.push(Box::new(search.clone()));
    }

    append_predicates("logs", &params.predicates, &mut query, &mut sql_params)?;

    // Add ordering and limit
    query.push_str(" ORDER BY timestamp DESC");
    if let Some(limit) = params.limit {
        query.push_str(" LIMIT ?");
        sql_params.push(Box::new(limit as i64));
    }

    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare query: {}", e)))?;

    let param_refs: Vec<&dyn rusqlite::ToSql> = sql_params.iter().map(|p| p.as_ref()).collect();

    let logs = stmt
        .query_map(param_refs.as_slice(), parse_log_row)
        .map_err(|e| StorageError::QueryError(format!("Failed to execute query: {}", e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse results: {}", e)))?;

    Ok(logs)
}

/// Query spans from the database
pub fn query_spans(conn: &Connection, params: &QueryParams) -> Result<Vec<Span>> {
    let mut query = String::from("SELECT * FROM spans WHERE 1=1");
    let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // Add time range filter
    if let Some(start) = params.start_time {
        query.push_str(" AND start_time >= ?");
        sql_params.push(Box::new(start));
    }
    if let Some(end) = params.end_time {
        query.push_str(" AND end_time <= ?");
        sql_params.push(Box::new(end));
    }

    // Add trace filter
    if let Some(ref trace_id) = params.trace_id {
        query.push_str(" AND trace_id = ?");
        sql_params.push(Box::new(trace_id.clone()));
    }

    append_predicates("spans", &params.predicates, &mut query, &mut sql_params)?;

    // Add ordering and limit
    query.push_str(" ORDER BY start_time DESC");
    if let Some(limit) = params.limit {
        query.push_str(" LIMIT ?");
        sql_params.push(Box::new(limit as i64));
    }

    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare query: {}", e)))?;

    let param_refs: Vec<&dyn rusqlite::ToSql> = sql_params.iter().map(|p| p.as_ref()).collect();

    let spans = stmt
        .query_map(param_refs.as_slice(), parse_span_row)
        .map_err(|e| StorageError::QueryError(format!("Failed to execute query: {}", e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse results: {}", e)))?;

    Ok(spans)
}

/// Query all spans belonging to the N most-recent traces matching the filters.
/// Avoids the "big trace eats the span budget" problem in list_traces.
pub fn query_spans_for_trace_list(
    conn: &Connection,
    params: &QueryParams,
    trace_limit: usize,
) -> Result<Vec<Span>> {
    // Phase 1: find the trace IDs of the N most-recent traces.
    //
    // Scanning spans by start_time DESC, a trace_id's FIRST encounter is
    // exactly its MAX(start_time) — any span with a later start_time would
    // have been seen earlier in the scan. So the first N distinct trace IDs
    // encountered are precisely the N traces with the largest MAX(start_time)
    // (the old GROUP BY + ORDER BY MAX result), but the scan can stop at the
    // Nth distinct value instead of reading the whole time window: on a
    // one-day window that is a few hundred rows instead of 2M+ (90s -> ms).
    let (trace_ids, mut outer_params): (Vec<String>, Vec<Box<dyn rusqlite::ToSql>>) =
        if let Some(ref trace_id) = params.trace_id {
            // A specific trace may be old; seeking it directly via the
            // trace_id index beats scanning backwards from the newest. The
            // window still has to match the old semantics: the trace only
            // qualifies if it has at least one span inside it.
            let mut check_sql = String::from("SELECT 1 FROM spans WHERE trace_id = ?");
            let mut check_params: Vec<Box<dyn rusqlite::ToSql>> =
                vec![Box::new(trace_id.clone()) as Box<dyn rusqlite::ToSql>];
            if let Some(start) = params.start_time {
                check_sql.push_str(" AND start_time >= ?");
                check_params.push(Box::new(start));
            }
            if let Some(end) = params.end_time {
                check_sql.push_str(" AND end_time <= ?");
                check_params.push(Box::new(end));
            }
            // The trace only qualifies if a span inside the window also
            // matches the structured predicates, mirroring the span-list
            // query's WHERE clause.
            append_predicates(
                "spans",
                &params.predicates,
                &mut check_sql,
                &mut check_params,
            )?;
            check_sql.push_str(" LIMIT 1");
            let mut stmt = conn
                .prepare(&check_sql)
                .map_err(|e| StorageError::QueryError(format!("Failed to prepare query: {}", e)))?;
            let refs: Vec<&dyn rusqlite::ToSql> = check_params.iter().map(|p| p.as_ref()).collect();
            match stmt.query_row(refs.as_slice(), |row| row.get::<_, i64>(0)) {
                Ok(_) => {},
                // No span of this trace inside the window: the old query
                // returned an empty list in that case.
                Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(Vec::new()),
                Err(e) => {
                    return Err(StorageError::QueryError(format!(
                        "Failed to check trace window: {}",
                        e
                    )))
                },
            }
            (vec![trace_id.clone()], Vec::new())
        } else if trace_limit == 0 {
            (Vec::new(), Vec::new())
        } else {
            let mut sql = String::from("SELECT trace_id FROM spans WHERE 1=1");
            let mut scan_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            if let Some(start) = params.start_time {
                sql.push_str(" AND start_time >= ?");
                scan_params.push(Box::new(start));
            }
            if let Some(end) = params.end_time {
                sql.push_str(" AND end_time <= ?");
                scan_params.push(Box::new(end));
            }
            // Structured predicates (session.id / gen_ai.* / attributes)
            // must constrain trace SELECTION exactly as they constrain
            // the span list — otherwise a trace filtered out of /spans
            // would still be picked here and its spans returned.
            append_predicates("spans", &params.predicates, &mut sql, &mut scan_params)?;
            sql.push_str(" ORDER BY start_time DESC");

            let mut stmt = conn
                .prepare(&sql)
                .map_err(|e| StorageError::QueryError(format!("Failed to prepare query: {}", e)))?;
            let refs: Vec<&dyn rusqlite::ToSql> = scan_params.iter().map(|p| p.as_ref()).collect();
            let rows = stmt
                .query_map(refs.as_slice(), |row| row.get::<_, String>(0))
                .map_err(|e| StorageError::QueryError(format!("Failed to execute query: {}", e)))?;

            let mut seen: Vec<String> = Vec::new();
            let mut seen_set: std::collections::HashSet<String> =
                std::collections::HashSet::with_capacity(trace_limit);
            for row in rows {
                let tid = row.map_err(|e| {
                    StorageError::QueryError(format!("Failed to parse results: {}", e))
                })?;
                if seen_set.insert(tid.clone()) {
                    seen.push(tid);
                    if seen.len() >= trace_limit {
                        break;
                    }
                }
            }
            (seen, Vec::new())
        };

    if trace_ids.is_empty() {
        return Ok(Vec::new());
    }

    // Phase 2: fetch all spans of those traces (the old query also returned
    // spans outside the window for selected traces, so no window here).
    let placeholders = vec!["?"; trace_ids.len()].join(", ");
    let query = format!(
        "SELECT * FROM spans WHERE trace_id IN ({}) ORDER BY start_time DESC",
        placeholders
    );
    outer_params.extend(
        trace_ids
            .iter()
            .map(|t| Box::new(t.clone()) as Box<dyn rusqlite::ToSql>),
    );

    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare query: {}", e)))?;

    let param_refs: Vec<&dyn rusqlite::ToSql> = outer_params.iter().map(|p| p.as_ref()).collect();

    let spans = stmt
        .query_map(param_refs.as_slice(), parse_span_row)
        .map_err(|e| StorageError::QueryError(format!("Failed to execute query: {}", e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse results: {}", e)))?;

    Ok(spans)
}

/// Query metrics from the database
pub fn query_metrics(conn: &Connection, params: &QueryParams) -> Result<Vec<Metric>> {
    let mut query = String::from("SELECT * FROM metrics WHERE 1=1");
    let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // Add time range filter
    if let Some(start) = params.start_time {
        query.push_str(" AND timestamp >= ?");
        sql_params.push(Box::new(start));
    }
    if let Some(end) = params.end_time {
        query.push_str(" AND timestamp <= ?");
        sql_params.push(Box::new(end));
    }

    append_predicates("metrics", &params.predicates, &mut query, &mut sql_params)?;

    // Add ordering and limit
    query.push_str(" ORDER BY timestamp DESC");
    if let Some(limit) = params.limit {
        query.push_str(" LIMIT ?");
        sql_params.push(Box::new(limit as i64));
    }

    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare query: {}", e)))?;

    let param_refs: Vec<&dyn rusqlite::ToSql> = sql_params.iter().map(|p| p.as_ref()).collect();

    let metrics = stmt
        .query_map(param_refs.as_slice(), parse_metric_row)
        .map_err(|e| StorageError::QueryError(format!("Failed to execute query: {}", e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse results: {}", e)))?;

    Ok(metrics)
}

/// Query metrics returning only the most-recent data point per unique metric name.
///
/// Prevents high-frequency counters from crowding out less-frequent gauges and
/// histograms when the caller only needs the current value for each metric (e.g.,
/// the metrics list sidebar). The inner subquery computes MAX(timestamp) per name
/// before any time-range filtering; the outer query then applies the window and
/// predicate filters. Ties at the maximum timestamp all come back (the same
/// rows the previous `HAVING timestamp = MAX(timestamp)` form returned).
///
/// The inner aggregation is a covering scan of `idx_metrics_name_ts` and the
/// join is an index seek per name, so this is O(index size) instead of a
/// full-table GROUP BY. Subquery columns are aliased (`g_name`, `g_ts`) so
/// unqualified predicate columns (`name`, `timestamp`, …) stay unambiguous.
pub fn query_latest_metrics(conn: &Connection, params: &QueryParams) -> Result<Vec<Metric>> {
    // Outer query adds optional time/predicate filters on top of the dedup subquery.
    let mut query = String::from(
        "SELECT m.* FROM metrics m \
         JOIN (SELECT name AS g_name, MAX(timestamp) AS g_ts FROM metrics GROUP BY name) g \
               ON g.g_name = m.name AND g.g_ts = m.timestamp \
         WHERE 1=1",
    );
    let mut sql_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = params.start_time {
        query.push_str(" AND timestamp >= ?");
        sql_params.push(Box::new(start));
    }
    if let Some(end) = params.end_time {
        query.push_str(" AND timestamp <= ?");
        sql_params.push(Box::new(end));
    }

    append_predicates("metrics", &params.predicates, &mut query, &mut sql_params)?;

    query.push_str(" ORDER BY name ASC");
    if let Some(limit) = params.limit {
        query.push_str(" LIMIT ?");
        sql_params.push(Box::new(limit as i64));
    }

    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare query: {}", e)))?;

    let param_refs: Vec<&dyn rusqlite::ToSql> = sql_params.iter().map(|p| p.as_ref()).collect();

    let metrics = stmt
        .query_map(param_refs.as_slice(), parse_metric_row)
        .map_err(|e| StorageError::QueryError(format!("Failed to execute query: {}", e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse results: {}", e)))?;

    Ok(metrics)
}

/// Distinct metric names, sorted ascending.
///
/// Uses the name prefix of `idx_metrics_name_ts` (a covering index scan that
/// emits only on key change), so this never touches the table rows — the
/// previous implementation loaded the entire metrics table into memory and
/// deduplicated in Rust.
pub fn query_distinct_metric_names(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn
        .prepare("SELECT DISTINCT name FROM metrics ORDER BY name")
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare query: {}", e)))?;

    let names = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| StorageError::QueryError(format!("Failed to execute query: {}", e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse results: {}", e)))?;

    Ok(names)
}

/// Get storage statistics
pub fn get_stats(conn: &Connection) -> Result<StorageStats> {
    // Count records
    let log_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM logs", [], |row| row.get(0))
        .map_err(|e| StorageError::QueryError(format!("Failed to count logs: {}", e)))?;

    let span_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM spans", [], |row| row.get(0))
        .map_err(|e| StorageError::QueryError(format!("Failed to count spans: {}", e)))?;

    let metric_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM metrics", [], |row| row.get(0))
        .map_err(|e| StorageError::QueryError(format!("Failed to count metrics: {}", e)))?;

    // Get time ranges. One MIN/MAX per table: each is a single index seek
    // (idx_logs_timestamp, idx_spans_start_time, idx_metrics_timestamp,
    // idx_spans_end_time). The equivalent MIN/MAX over a UNION ALL of all
    // three tables forces full covering-index scans of every table.
    let scalar_min = |sql: &str| -> Option<i64> {
        conn.query_row(sql, [], |row| row.get::<_, Option<i64>>(0))
            .ok()
            .flatten()
    };
    let oldest_timestamp = [
        "SELECT MIN(timestamp) FROM logs",
        "SELECT MIN(start_time) FROM spans",
        "SELECT MIN(timestamp) FROM metrics",
    ]
    .iter()
    .filter_map(|sql| scalar_min(sql))
    .min();
    let newest_timestamp = [
        "SELECT MAX(timestamp) FROM logs",
        "SELECT MAX(end_time) FROM spans",
        "SELECT MAX(timestamp) FROM metrics",
    ]
    .iter()
    .filter_map(|sql| scalar_min(sql))
    .max();

    // Get database size (page_count * page_size)
    let page_count: i64 = conn
        .query_row("PRAGMA page_count", [], |row| row.get(0))
        .unwrap_or(0);
    let page_size: i64 = conn
        .query_row("PRAGMA page_size", [], |row| row.get(0))
        .unwrap_or(4096);
    let total_size_bytes = page_count * page_size;

    Ok(StorageStats {
        log_count: log_count as u64,
        span_count: span_count as u64,
        metric_count: metric_count as u64,
        oldest_timestamp,
        newest_timestamp,
        storage_size_bytes: total_size_bytes as u64,
    })
}

fn append_predicates(
    signal_type: &str,
    predicates: &[QueryPredicate],
    query: &mut String,
    sql_params: &mut Vec<Box<dyn rusqlite::ToSql>>,
) -> Result<()> {
    for predicate in predicates {
        let clause = predicate_to_sql(signal_type, predicate, sql_params)?;
        query.push_str(" AND ");
        query.push_str(&clause);
    }

    Ok(())
}

fn predicate_to_sql(
    signal_type: &str,
    predicate: &QueryPredicate,
    sql_params: &mut Vec<Box<dyn rusqlite::ToSql>>,
) -> Result<String> {
    let lhs = field_to_sql(signal_type, &predicate.field)?;
    let operator = sql_operator(&predicate.operator);

    let clause = match (&predicate.field[..], &predicate.operator, &predicate.value) {
        ("duration", op, QueryValue::Duration(value)) if signal_type == "spans" => {
            sql_params.push(Box::new(*value as i64));
            Ok(format!("((end_time - start_time) {} ?)", sql_operator(op)))
        },
        ("duration", _, _) if signal_type == "spans" => Err(StorageError::QueryError(
            "Structured query field 'duration' for spans requires a duration value like 500ms"
                .to_string(),
        )),
        (_, Operator::Contains, QueryValue::String(value)) => {
            sql_params.push(Box::new(format!("%{}%", value)));
            Ok(format!("{} LIKE ?", lhs))
        },
        (_, Operator::Contains, _) => Err(StorageError::QueryError(format!(
            "Structured query operator 'contains' for field '{}' requires a quoted string value",
            predicate.field
        ))),
        (_, _, QueryValue::String(value)) => {
            sql_params.push(Box::new(value.clone()));
            Ok(format!("{} {} ?", lhs, operator))
        },
        (_, _, QueryValue::Number(value)) => {
            sql_params.push(Box::new(*value));
            Ok(format!("{} {} ?", lhs, operator))
        },
        (_, _, QueryValue::Duration(value)) => {
            sql_params.push(Box::new(*value as i64));
            Ok(format!("{} {} ?", lhs, operator))
        },
    }?;

    // Session-id predicates run against idx_spans_session_id, a partial
    // index: SQLite only considers a partial index when the query carries
    // the index predicate as conjuncts. The equality already implies it, so
    // appending it changes no results — it only makes the index usable.
    if predicate.field == otelite_core::semconv::SESSION_ID_KEY
        || predicate
            .field
            .strip_prefix("attributes.")
            .is_some_and(|f| f == otelite_core::semconv::SESSION_ID_KEY)
    {
        Ok(format!(
            "{clause} AND {}",
            otelite_core::semconv::session_id_index_predicate("attributes")
        ))
    } else {
        Ok(clause)
    }
}

fn field_to_sql(signal_type: &str, field: &str) -> Result<String> {
    let direct_column = match (signal_type, field) {
        ("logs", "timestamp") => Some("timestamp"),
        ("logs", "trace_id") => Some("trace_id"),
        ("logs", "span_id") => Some("span_id"),
        ("logs", "severity") | ("logs", "severity_number") => Some("severity_number"),
        ("logs", "body") => Some("body"),
        ("spans", "trace_id") => Some("trace_id"),
        ("spans", "span_id") => Some("span_id"),
        ("spans", "parent_span_id") => Some("parent_span_id"),
        ("spans", "name") => Some("name"),
        ("spans", "kind") => Some("kind"),
        ("spans", "start_time") => Some("start_time"),
        ("spans", "end_time") => Some("end_time"),
        ("metrics", "name") => Some("name"),
        ("metrics", "description") => Some("description"),
        ("metrics", "unit") => Some("unit"),
        ("metrics", "timestamp") => Some("timestamp"),
        _ => None,
    };

    if let Some(column) = direct_column {
        return Ok(column.to_string());
    }

    if let Some(attribute_field) = field.strip_prefix("attributes.") {
        if attribute_field == otelite_core::semconv::SESSION_ID_KEY {
            return Ok(otelite_core::semconv::session_id_expr("attributes"));
        }
        return Ok(format!(
            "json_extract(attributes, '{}')",
            json_path_for_key(attribute_field)
        ));
    }

    if let Some(resource_field) = field.strip_prefix("resource.") {
        return Ok(format!(
            "json_extract(resource, '$.attributes{}')",
            json_key_accessor(resource_field)
        ));
    }

    if field == otelite_core::semconv::SESSION_ID_KEY {
        return Ok(otelite_core::semconv::session_id_expr("attributes"));
    }

    Ok(format!(
        "json_extract(attributes, '{}')",
        json_path_for_key(field)
    ))
}

fn json_path_for_key(field: &str) -> String {
    format!("$.\"{}\"", field)
}

fn json_key_accessor(field: &str) -> String {
    format!(".\"{}\"", field)
}

fn sql_operator(operator: &Operator) -> &'static str {
    match operator {
        Operator::Equal => "=",
        Operator::NotEqual => "!=",
        Operator::GreaterThan => ">",
        Operator::LessThan => "<",
        Operator::GreaterThanOrEqual => ">=",
        Operator::LessThanOrEqual => "<=",
        Operator::Contains => "LIKE",
    }
}

// Helper functions to parse rows into telemetry types

fn parse_json_or_default<T>(json: &str, field: &str, record_type: &'static str) -> T
where
    T: DeserializeOwned + Default,
{
    serde_json::from_str(json).unwrap_or_else(|error| {
        tracing::warn!(
            field,
            record_type,
            %error,
            "Malformed JSON in stored telemetry field; using default value"
        );
        T::default()
    })
}

fn parse_json_or_none<T>(json: &str, field: &str, record_type: &'static str) -> Option<T>
where
    T: DeserializeOwned,
{
    serde_json::from_str::<Option<T>>(json)
        .map_err(|error| {
            tracing::warn!(
                field,
                record_type,
                %error,
                "Malformed JSON in stored telemetry field; omitting value"
            );
        })
        .ok()
        .flatten()
}

fn parse_log_row(row: &Row) -> rusqlite::Result<LogRecord> {
    let timestamp: i64 = row.get("timestamp")?;
    let trace_id: Option<String> = row.get("trace_id")?;
    let span_id: Option<String> = row.get("span_id")?;
    let attributes_json: String = row.get("attributes")?;
    let attributes = parse_json_or_default(&attributes_json, "attributes", "log record");

    let resource_json: String = row.get("resource")?;
    let resource = parse_json_or_none(&resource_json, "resource", "log record");

    let severity_num: i32 = row.get("severity_number")?;
    let severity = SeverityLevel::from_i32(severity_num).unwrap_or(SeverityLevel::Info);

    Ok(LogRecord {
        timestamp,
        observed_timestamp: row.get("observed_timestamp")?,
        trace_id,
        span_id,
        severity,
        severity_text: row.get("severity_text")?,
        body: row.get("body")?,
        attributes,
        resource,
    })
}

fn parse_span_row(row: &Row) -> rusqlite::Result<Span> {
    let trace_id: String = row.get("trace_id")?;
    let span_id: String = row.get("span_id")?;
    let name: String = row.get("name")?;
    let attributes_json: String = row.get("attributes")?;
    let attributes = parse_json_or_default(&attributes_json, "attributes", "span record");

    let events_json: String = row.get("events")?;
    let events = parse_json_or_default(&events_json, "events", "span record");

    let resource_json: String = row.get("resource")?;
    let resource = parse_json_or_none(&resource_json, "resource", "span record");

    let kind_num: i32 = row.get("kind")?;
    let kind = SpanKind::from_i32(kind_num).unwrap_or(SpanKind::Internal);

    let status_code_num: i32 = row.get("status_code")?;
    let status_code = StatusCode::from_i32(status_code_num).unwrap_or(StatusCode::Unset);

    let status = SpanStatus {
        code: status_code,
        message: row.get("status_message")?,
    };

    Ok(Span {
        trace_id,
        span_id,
        parent_span_id: row.get("parent_span_id")?,
        name,
        kind,
        start_time: row.get("start_time")?,
        end_time: row.get("end_time")?,
        attributes,
        events,
        status,
        resource,
    })
}

fn parse_metric_row(row: &Row) -> rusqlite::Result<Metric> {
    use otelite_core::telemetry::metric::MetricType;

    let name: String = row.get("name")?;
    let timestamp: i64 = row.get("timestamp")?;
    let attributes_json: String = row.get("attributes")?;
    let attributes = parse_json_or_default(&attributes_json, "attributes", "metric record");

    let resource_json: String = row.get("resource")?;
    let resource = parse_json_or_none(&resource_json, "resource", "metric record");

    let metric_type_int: i32 = row.get("metric_type")?;
    let metric_type = match metric_type_int {
        0 => {
            let value: f64 = row.get("value_double")?;
            MetricType::Gauge(value)
        },
        1 => {
            let value: i64 = row.get("value_int")?;
            MetricType::Counter(value as u64)
        },
        2 => {
            let histogram_json: String = row.get("value_histogram")?;
            let (count, sum, buckets) =
                parse_json_or_default(&histogram_json, "value_histogram", "metric record");
            MetricType::Histogram {
                count,
                sum,
                buckets,
            }
        },
        3 => {
            let summary_json: String = row.get("value_summary")?;
            let (count, sum, quantiles) =
                parse_json_or_default(&summary_json, "value_summary", "metric record");
            MetricType::Summary {
                count,
                sum,
                quantiles,
            }
        },
        _ => MetricType::Gauge(0.0),
    };

    Ok(Metric {
        name,
        description: row.get("description")?,
        unit: row.get("unit")?,
        metric_type,
        timestamp,
        attributes,
        resource,
    })
}

/// SQL expressions for extracting token / model / system values from a span's
/// `attributes` JSON column, shared by all GenAI analytics queries.
///
/// The attribute vocabulary lives in [`otelite_core::semconv`]. This struct
/// projects those lists into SQL COALESCE fragments once per query.
struct TokenExprs {
    input: String,
    output: String,
    cache_creation: String,
    cache_read: String,
    /// Model label across common spellings (request model preferred, response
    /// model only as a fallback for spans that carry no request model at all).
    model: String,
    /// Model identity for grouping: `provider/model` when a provider is
    /// recorded, bare model otherwise. Never built from a response model when
    /// a request model exists (#143: no silent rerouting merge).
    identity: String,
    /// Response model that actually served the call (aliases coalesced).
    response_model: String,
    /// Request model across aliases (no response-model fallback) — used to
    /// detect rerouted responses.
    request_model: String,
    system: String,
    /// Parenthesised OR-chain identifying LLM spans (also includes the
    /// OpenInference `openinference.span.kind` clause).
    llm_span_guard: String,
    /// LLM calls with reliable request-count and duration semantics. Includes
    /// Codex's completed sampling spans, but not token, cost, or outcome
    /// analytics because Codex does not emit those attributes.
    request_span_guard: String,
}

fn token_exprs() -> TokenExprs {
    use otelite_core::semconv;
    let mut exprs = TokenExprs {
        input: semconv::coalesce_extract_cast("attributes", semconv::INPUT_TOKEN_KEYS, "INTEGER"),
        output: semconv::coalesce_extract_cast("attributes", semconv::OUTPUT_TOKEN_KEYS, "INTEGER"),
        cache_creation: semconv::coalesce_extract_cast(
            "attributes",
            semconv::CACHE_CREATION_TOKEN_KEYS,
            "INTEGER",
        ),
        cache_read: semconv::coalesce_extract_cast(
            "attributes",
            semconv::CACHE_READ_TOKEN_KEYS,
            "INTEGER",
        ),
        model: semconv::coalesce_extract("attributes", semconv::MODEL_KEYS),
        // Filled below once `model` and `system` exist.
        identity: String::new(),
        system: semconv::coalesce_extract("attributes", semconv::SYSTEM_KEYS),
        response_model: semconv::coalesce_extract("attributes", semconv::RESPONSE_MODEL_KEYS),
        request_model: semconv::coalesce_extract("attributes", semconv::REQUEST_MODEL_KEYS),
        llm_span_guard: semconv::llm_span_guard("attributes"),
        request_span_guard: semconv::request_span_guard("attributes"),
    };
    let model = exprs.model.clone();
    let system = exprs.system.clone();
    exprs.identity = format!(
        "CASE WHEN {} IS NOT NULL AND {} IS NOT NULL THEN ({} || '/' || {}) ELSE {} END",
        system, model, system, model, model
    );
    exprs
}

/// Append a GenAI filter scope (predicate fragment + bind params) to a
/// WHERE clause under construction. No-op when the scope is `None`.
fn push_scope(
    where_clause: &mut String,
    params: &mut Vec<Box<dyn rusqlite::ToSql>>,
    scope: Option<(String, Vec<String>)>,
) {
    if let Some((frag, fp)) = scope {
        where_clause.push_str(&format!(" AND {frag}"));
        params.extend(
            fp.into_iter()
                .map(|p| Box::new(p) as Box<dyn rusqlite::ToSql>),
        );
    }
}

const CAPABILITY_QUERY_LIMIT: usize = 10_000;

#[derive(Default)]
struct CapabilityMetricAccum {
    eligible_count: usize,
    observed_count: usize,
    valid_count: usize,
    invalid_count: usize,
    degenerate_count: usize,
    /// Correlated observations that were recorded because the request span
    /// carried no native attribute for this metric.
    correlated_observed_count: usize,
    source_attributes: HashMap<String, usize>,
}

impl CapabilityMetricAccum {
    fn record(
        &mut self,
        observation: MetricObservation,
        source_attribute: Option<&str>,
        degenerate: bool,
    ) {
        self.eligible_count += 1;
        if let Some(attribute) = source_attribute {
            *self
                .source_attributes
                .entry(attribute.to_string())
                .or_default() += 1;
            self.observed_count += 1;
        }
        match observation {
            MetricObservation::Valid => {
                self.valid_count += 1;
                if degenerate {
                    self.degenerate_count += 1;
                }
            },
            MetricObservation::Invalid => self.invalid_count += 1,
            MetricObservation::Absent => {},
        }
    }

    /// Record a value that a correlation rule associated with a request whose
    /// native observation for this metric was absent. Only valid/invalid
    /// states reach this path; absent correlated counters are skipped by the
    /// caller (they carry no evidence to record).
    fn record_correlated(&mut self, observation: MetricObservation, source_attribute: &str) {
        *self
            .source_attributes
            .entry(source_attribute.to_string())
            .or_default() += 1;
        self.correlated_observed_count += 1;
        match observation {
            MetricObservation::Valid => self.valid_count += 1,
            MetricObservation::Invalid => self.invalid_count += 1,
            MetricObservation::Absent => {},
        }
    }

    fn report(&self, ttft: bool) -> otelite_core::api::GenAiMetricCapability {
        let availability = if self.valid_count == 0 {
            "absent"
        } else if self.valid_count == self.eligible_count {
            "available"
        } else {
            "sparse"
        };
        let quality = if ttft
            && self.valid_count >= TTFT_DEGENERATE_MIN_SAMPLES
            && self.degenerate_count * 100 >= self.valid_count * 90
        {
            "degenerate"
        } else if self.invalid_count > 0 {
            "invalid"
        } else if self.valid_count > 0 {
            "reliable"
        } else {
            "not_assessed"
        };
        let derivation = if self.observed_count > 0 {
            "native"
        } else if self.correlated_observed_count > 0 {
            "correlated"
        } else {
            "unavailable"
        };
        otelite_core::api::GenAiMetricCapability {
            eligible_count: self.eligible_count,
            observed_count: self.observed_count,
            valid_count: self.valid_count,
            invalid_count: self.invalid_count,
            availability: availability.to_string(),
            quality: quality.to_string(),
            derivation: derivation.to_string(),
            source_attributes: self.source_attributes.clone(),
        }
    }
}

#[derive(Default)]
struct CapabilityAccum {
    request_count: usize,
    input_tokens: CapabilityMetricAccum,
    output_tokens: CapabilityMetricAccum,
    cache_creation_tokens: CapabilityMetricAccum,
    cache_read_tokens: CapabilityMetricAccum,
    ttft: CapabilityMetricAccum,
}

type CapabilityGroupKey = (Option<String>, Option<String>, String, String, String);

fn first_semconv_attribute<'a>(
    attrs: &'a HashMap<String, String>,
    keys: &[&str],
) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| attrs.get(*key).map(String::as_str))
}

fn emitter_name(emitter: GenAiEmitter) -> &'static str {
    match emitter {
        GenAiEmitter::ClaudeCode => "claude_code",
        GenAiEmitter::Codex => "codex",
        GenAiEmitter::OpenCode => "opencode",
        GenAiEmitter::StandardOtel => "standard_otel",
        GenAiEmitter::Unknown => "unknown",
        GenAiEmitter::Ambiguous => "ambiguous",
    }
}

fn capability_fingerprint(
    adapter_rule: &str,
    service_name: Option<&str>,
    scope_name: Option<&str>,
    scope_version: Option<&str>,
) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for part in [
        adapter_rule,
        service_name.unwrap_or(""),
        scope_name.unwrap_or(""),
        scope_version.unwrap_or(""),
    ] {
        for byte in part.bytes().chain([0_u8]) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x100000001b3);
        }
    }
    format!("genai-v1-{hash:016x}")
}

/// Bounded most-recent physical-span sample for model-performance
/// comparisons. The rolling baseline can span long horizons, so the sample
/// must be capped by construction; statistics are computed over the sample
/// and `truncated` reports that older spans were not examined.
const MODEL_PERFORMANCE_QUERY_LIMIT: usize = 100_000;

/// Model-performance comparison of a current interval against an
/// equal-length preceding interval and an optional rolling historical
/// baseline (#121/#151).
///
/// Canonical population: verified request spans (the capability
/// classification's `RequestTiming` role), deduplicated across duplicate
/// OTLP deliveries, tagged by the window their `start_time` falls in.
/// Response model is kept as a separate observation so routing changes are
/// never merged into one identity. Per-metric eligibility is independent:
/// a request lacking output tokens or duration stays in the population but
/// is ineligible for throughput — duration-only emitters are visible, not
/// zero.
///
/// Windows are half-open `[start, end)` on `start_time`. The rolling
/// baseline must exclude the current and preceding windows (validated).
/// Percentiles use the shared rounded-rank estimator (#119). Deltas carry
/// an explicit percentage-unavailable state (`relative: None`) when the
/// baseline is zero or has no eligible samples.
pub fn query_model_performance(
    conn: &Connection,
    query: &otelite_core::api::ModelPerformanceQuery,
) -> Result<otelite_core::api::ModelPerformanceResponse> {
    use otelite_core::api::{
        ModelPerformanceCounts, ModelPerformanceDelta, ModelPerformanceErrorRate,
        ModelPerformanceErrorValue, ModelPerformanceIdentity, ModelPerformanceMetric,
        ModelPerformancePercentile, ModelPerformanceResponse, ModelPerformanceSample,
    };
    use otelite_core::telemetry::{classify_ttft_value, TtftValueQuality};

    let current = query.current;
    if current.end_time <= current.start_time {
        return Err(StorageError::QueryError(format!(
            "model-performance current window is empty or inverted \
             ([{} ns, {} ns)); pass a non-empty half-open interval",
            current.start_time, current.end_time
        )));
    }
    let current_len = current.end_time - current.start_time;
    let preceding = otelite_core::api::ModelPerformanceWindow {
        start_time: current.start_time - current_len,
        end_time: current.start_time,
    };
    let rolling = query.rolling;
    if let Some(r) = rolling {
        if r.end_time <= r.start_time {
            return Err(StorageError::QueryError(format!(
                "model-performance rolling baseline window is empty or inverted \
                 ([{} ns, {} ns)); pass a non-empty half-open interval or omit it",
                r.start_time, r.end_time
            )));
        }
        if r.end_time > preceding.start_time {
            return Err(StorageError::QueryError(format!(
                "model-performance rolling baseline [{} ns, {} ns) overlaps the current or \
                 preceding comparison window (preceding starts at {} ns); the baseline must \
                 exclude both — shift it earlier",
                r.start_time, r.end_time, preceding.start_time
            )));
        }
    }

    let global_start = rolling
        .map(|r| r.start_time)
        .unwrap_or(preceding.start_time)
        .min(preceding.start_time);
    let global_end = current.end_time;

    let mut where_clause = format!("WHERE {}", semconv::request_span_guard("attributes"));
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    where_clause.push_str(" AND start_time >= ?");
    params.push(Box::new(global_start));
    where_clause.push_str(" AND start_time < ?");
    params.push(Box::new(global_end));
    if let Some(model) = &query.model {
        let model_expr = semconv::coalesce_extract("attributes", semconv::REQUEST_MODEL_KEYS);
        where_clause.push_str(&format!(" AND {} = ?", model_expr));
        params.push(Box::new(model.clone()));
    }
    if let Some(provider) = &query.provider {
        let system_expr = semconv::coalesce_extract("attributes", semconv::SYSTEM_KEYS);
        where_clause.push_str(&format!(" AND {} = ?", system_expr));
        params.push(Box::new(provider.clone()));
    }

    let sql = format!(
        "WITH recent_spans AS (
            SELECT
                trace_id, span_id, parent_span_id, name, kind, start_time, end_time,
                COALESCE(attributes, '{{}}') AS attributes,
                COALESCE(events, '[]') AS events,
                COALESCE(status_code, 0) AS status_code,
                status_message,
                COALESCE(resource, 'null') AS resource,
                created_at,
                id
            FROM spans {where_clause}
            ORDER BY start_time DESC, id DESC
            LIMIT {limit}
         ),
         ranked_spans AS (
            SELECT
                trace_id, span_id, parent_span_id, name, kind, start_time, end_time,
                attributes, events, status_code, status_message, resource,
                ROW_NUMBER() OVER (
                    PARTITION BY trace_id, span_id
                    ORDER BY created_at ASC, id ASC
                ) AS delivery_rank
            FROM recent_spans
         )
         SELECT
            trace_id, span_id, parent_span_id, name, kind, start_time, end_time,
            attributes, events, status_code, status_message, resource
         FROM ranked_spans
         WHERE delivery_rank = 1
         ORDER BY start_time ASC, trace_id ASC, span_id ASC",
        limit = MODEL_PERFORMANCE_QUERY_LIMIT + 1,
    );

    let mut stmt = conn.prepare(&sql).map_err(|error| {
        StorageError::QueryError(format!(
            "Failed to prepare model-performance query: {error}"
        ))
    })?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut spans: Vec<Span> = stmt
        .query_map(param_refs.as_slice(), parse_span_row)
        .map_err(|error| {
            StorageError::QueryError(format!(
                "Failed to execute model-performance query: {error}"
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| {
            StorageError::QueryError(format!("Failed to parse model-performance rows: {error}"))
        })?;
    let truncated = spans.len() > MODEL_PERFORMANCE_QUERY_LIMIT;
    if truncated {
        spans.truncate(MODEL_PERFORMANCE_QUERY_LIMIT);
    }

    // One canonical population per (identity, window): verified request
    // spans only. Usage spans (e.g. Codex handle_responses) are not requests.
    #[derive(Default)]
    struct WindowAgg<'a> {
        requests: Vec<&'a Span>,
        errors: usize,
        durations_ms: Vec<i64>,
        throughputs: Vec<f64>,
        ttft_ms: Vec<i64>,
        input_tokens: Vec<i64>,
        cache_creation: Vec<i64>,
        cache_read: Vec<i64>,
        output_tokens: Vec<i64>,
        response_models: BTreeSet<String>,
    }
    #[derive(Hash, Eq, PartialEq, Clone, PartialOrd, Ord)]
    struct IdentityKey {
        provider: Option<String>,
        model: Option<String>,
        fingerprint: String,
    }
    #[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
    enum WindowSlot {
        Preceding,
        Current,
        Rolling,
    }
    let window_of = |start_time: i64| -> Option<WindowSlot> {
        if start_time >= current.start_time && start_time < current.end_time {
            Some(WindowSlot::Current)
        } else if start_time >= preceding.start_time && start_time < preceding.end_time {
            Some(WindowSlot::Preceding)
        } else {
            rolling
                .filter(|r| start_time >= r.start_time && start_time < r.end_time)
                .map(|_| WindowSlot::Rolling)
        }
    };

    let mut groups: BTreeMap<(IdentityKey, WindowSlot), WindowAgg> = BTreeMap::new();
    for span in &spans {
        let Some(window) = window_of(span.start_time) else {
            continue;
        };
        let capabilities = classify_span_capabilities(span);
        if capabilities.role != GenAiSpanRole::RequestTiming {
            continue;
        }
        let attrs = &span.attributes;
        let provider = first_semconv_attribute(attrs, semconv::SYSTEM_KEYS).map(str::to_string);
        let model = first_semconv_attribute(attrs, semconv::REQUEST_MODEL_KEYS).map(str::to_string);
        let fingerprint = capability_fingerprint(
            capabilities.fingerprint.adapter_rule,
            capabilities.fingerprint.service_name.as_deref(),
            capabilities.fingerprint.scope_name.as_deref(),
            capabilities.fingerprint.scope_version.as_deref(),
        );
        let key = IdentityKey {
            provider,
            model,
            fingerprint,
        };
        let agg = groups.entry((key, window)).or_default();
        agg.requests.push(span);
        if span.status.code == StatusCode::Error {
            agg.errors += 1;
        }
        let duration_ms = span.end_time.saturating_sub(span.start_time) / 1_000_000;
        if duration_ms > 0 {
            agg.durations_ms.push(duration_ms);
        }
        if let Some(rate) = throughput_rate_tok_s(
            span.end_time.saturating_sub(span.start_time),
            attrs
                .iter()
                .find_map(|(k, v)| {
                    semconv::OUTPUT_TOKEN_KEYS
                        .iter()
                        .find(|key| **key == *k)
                        .and_then(|_| v.parse::<i64>().ok().filter(|t| *t > 0))
                })
                .map(|t| t as f64),
        ) {
            agg.throughputs.push(rate);
        }
        if let Some(Ok(ttft_secs)) = normalized_ttft_secs(
            attrs
                .get("gen_ai.server.time_to_first_token")
                .map(String::as_str),
            attrs.get("llm.time_to_first_token").map(String::as_str),
            attrs.get("ttft_ms").map(String::as_str),
        ) {
            if classify_ttft_value(Some(ttft_secs), duration_ms as f64 / 1000.0)
                == TtftValueQuality::Valid
            {
                agg.ttft_ms.push((ttft_secs * 1000.0).round() as i64);
            }
        }
        let token_of = |keys: &[&str]| -> Option<i64> {
            first_semconv_attribute(attrs, keys).and_then(|v| v.parse::<i64>().ok())
        };
        if let Some(v) = token_of(semconv::INPUT_TOKEN_KEYS) {
            agg.input_tokens.push(v);
        }
        if let Some(v) = token_of(semconv::CACHE_CREATION_TOKEN_KEYS) {
            agg.cache_creation.push(v);
        }
        if let Some(v) = token_of(semconv::CACHE_READ_TOKEN_KEYS) {
            agg.cache_read.push(v);
        }
        if let Some(v) = token_of(semconv::OUTPUT_TOKEN_KEYS) {
            agg.output_tokens.push(v);
        }
        if window == WindowSlot::Current {
            if let Some(response_model) =
                first_semconv_attribute(attrs, semconv::RESPONSE_MODEL_KEYS)
            {
                agg.response_models.insert(response_model.to_string());
            }
        }
    }

    // Assemble identities (stable order: BTreeMap over the identity keys).
    let identity_keys: BTreeSet<IdentityKey> = groups.keys().map(|(key, _)| key.clone()).collect();
    let identity_of = |key: &IdentityKey, slot: WindowSlot| -> Option<&WindowAgg> {
        groups.get(&(key.clone(), slot))
    };
    let two_percentile = |values: &[i64]| -> Vec<ModelPerformancePercentile> {
        vec![
            ModelPerformancePercentile {
                percentile: 50,
                value: percentile(values, 0.5) as f64,
                delta_vs_preceding: None,
                delta_vs_rolling: None,
            },
            ModelPerformancePercentile {
                percentile: 95,
                value: percentile(values, 0.95) as f64,
                delta_vs_preceding: None,
                delta_vs_rolling: None,
            },
        ]
    };

    let mut identities = Vec::new();
    for key in &identity_keys {
        let current_agg = identity_of(key, WindowSlot::Current);
        let preceding_agg = identity_of(key, WindowSlot::Preceding);
        let rolling_agg = rolling
            .is_some()
            .then(|| identity_of(key, WindowSlot::Rolling))
            .flatten();

        let delta = |current: f64, baseline: f64| ModelPerformanceDelta {
            absolute: current - baseline,
            relative: if baseline != 0.0 {
                Some((current - baseline) / baseline)
            } else {
                None
            },
        };

        let build_metric = |current_values: Vec<i64>,
                            preceding_values: Vec<i64>,
                            rolling_values: Option<Vec<i64>>,
                            percentiles: fn(&[i64]) -> Vec<ModelPerformancePercentile>|
         -> ModelPerformanceMetric {
            let to_sample = |values: Vec<i64>| -> Option<ModelPerformanceSample> {
                if values.is_empty() {
                    return None;
                }
                let mut sorted = values;
                sorted.sort_unstable();
                Some(ModelPerformanceSample {
                    eligible_count: sorted.len(),
                    percentiles: percentiles(&sorted),
                })
            };
            let mut metric = ModelPerformanceMetric {
                current: to_sample(current_values),
                preceding: to_sample(preceding_values),
                rolling: rolling_values.and_then(to_sample),
            };
            if let (Some(current), Some(baseline)) =
                (metric.current.as_mut(), metric.preceding.as_ref())
            {
                for (p, b) in current
                    .percentiles
                    .iter_mut()
                    .zip(baseline.percentiles.iter())
                {
                    if p.percentile == b.percentile {
                        p.delta_vs_preceding = Some(delta(p.value, b.value));
                    }
                }
            }
            if let (Some(current), Some(baseline)) =
                (metric.current.as_mut(), metric.rolling.as_ref())
            {
                for (p, b) in current
                    .percentiles
                    .iter_mut()
                    .zip(baseline.percentiles.iter())
                {
                    if p.percentile == b.percentile {
                        p.delta_vs_rolling = Some(delta(p.value, b.value));
                    }
                }
            }
            metric
        };

        let current_durations = current_agg
            .map(|a| a.durations_ms.clone())
            .unwrap_or_default();
        let preceding_durations = preceding_agg
            .map(|a| a.durations_ms.clone())
            .unwrap_or_default();
        let rolling_durations = rolling_agg.map(|a| a.durations_ms.clone());
        let duration = build_metric(
            current_durations,
            preceding_durations,
            rolling_durations,
            two_percentile,
        );

        let p10_p50 = |values: &[f64]| -> Vec<ModelPerformancePercentile> {
            vec![
                ModelPerformancePercentile {
                    percentile: 10,
                    value: percentile_f64(values, 0.1),
                    delta_vs_preceding: None,
                    delta_vs_rolling: None,
                },
                ModelPerformancePercentile {
                    percentile: 50,
                    value: percentile_f64(values, 0.5),
                    delta_vs_preceding: None,
                    delta_vs_rolling: None,
                },
            ]
        };
        let throughput = {
            let to_sample_f64 = |values: Vec<f64>| -> Option<ModelPerformanceSample> {
                if values.is_empty() {
                    return None;
                }
                let mut sorted = values;
                sorted.sort_unstable_by(f64::total_cmp);
                Some(ModelPerformanceSample {
                    eligible_count: sorted.len(),
                    percentiles: p10_p50(&sorted),
                })
            };
            let current_values = current_agg
                .map(|a| a.throughputs.clone())
                .unwrap_or_default();
            let preceding_values = preceding_agg
                .map(|a| a.throughputs.clone())
                .unwrap_or_default();
            let rolling_values = rolling_agg.map(|a| a.throughputs.clone());
            let mut metric = ModelPerformanceMetric {
                current: to_sample_f64(current_values),
                preceding: to_sample_f64(preceding_values),
                rolling: rolling_values.and_then(to_sample_f64),
            };
            if let (Some(current), Some(baseline)) =
                (metric.current.as_mut(), metric.preceding.as_ref())
            {
                for (p, b) in current
                    .percentiles
                    .iter_mut()
                    .zip(baseline.percentiles.iter())
                {
                    if p.percentile == b.percentile {
                        p.delta_vs_preceding = Some(delta(p.value, b.value));
                    }
                }
            }
            if let (Some(current), Some(baseline)) =
                (metric.current.as_mut(), metric.rolling.as_ref())
            {
                for (p, b) in current
                    .percentiles
                    .iter_mut()
                    .zip(baseline.percentiles.iter())
                {
                    if p.percentile == b.percentile {
                        p.delta_vs_rolling = Some(delta(p.value, b.value));
                    }
                }
            }
            metric
        };

        let p50_i64 = |values: &[i64]| -> Vec<ModelPerformancePercentile> {
            vec![ModelPerformancePercentile {
                percentile: 50,
                value: percentile(values, 0.5) as f64,
                delta_vs_preceding: None,
                delta_vs_rolling: None,
            }]
        };
        let token_metric =
            |pick: &dyn for<'a> Fn(&'a WindowAgg<'a>) -> &'a Vec<i64>| -> ModelPerformanceMetric {
                build_metric(
                    current_agg.map(|a| pick(a).clone()).unwrap_or_default(),
                    preceding_agg.map(|a| pick(a).clone()).unwrap_or_default(),
                    rolling_agg.map(|a| pick(a).clone()),
                    p50_i64,
                )
            };
        let ttft = build_metric(
            current_agg.map(|a| a.ttft_ms.clone()).unwrap_or_default(),
            preceding_agg.map(|a| a.ttft_ms.clone()).unwrap_or_default(),
            rolling_agg.map(|a| a.ttft_ms.clone()),
            p50_i64,
        );
        let input_tokens = token_metric(&|a: &WindowAgg| &a.input_tokens);
        let cache_creation_tokens = token_metric(&|a: &WindowAgg| &a.cache_creation);
        let cache_read_tokens = token_metric(&|a: &WindowAgg| &a.cache_read);
        let output_tokens = token_metric(&|a: &WindowAgg| &a.output_tokens);

        let error_value = |agg: Option<&WindowAgg>| -> Option<ModelPerformanceErrorValue> {
            let agg = agg?;
            let requests = agg.requests.len();
            if requests == 0 {
                return None;
            }
            Some(ModelPerformanceErrorValue {
                requests,
                errors: agg.errors,
                rate: agg.errors as f64 / requests as f64,
                delta_vs_preceding: None,
                delta_vs_rolling: None,
            })
        };
        let mut error_rate = ModelPerformanceErrorRate {
            current: error_value(current_agg),
            preceding: error_value(preceding_agg),
            rolling: rolling
                .is_some()
                .then(|| error_value(rolling_agg))
                .flatten(),
        };
        if let Some(current_value) = error_rate.current.as_mut() {
            if let Some(baseline) = error_rate.preceding.as_ref() {
                current_value.delta_vs_preceding = Some(delta(current_value.rate, baseline.rate));
            }
            if let Some(baseline) = error_rate.rolling.as_ref() {
                current_value.delta_vs_rolling = Some(delta(current_value.rate, baseline.rate));
            }
        }

        identities.push(ModelPerformanceIdentity {
            provider: key.provider.clone(),
            model: key.model.clone(),
            emitter_fingerprint: key.fingerprint.clone(),
            response_models: current_agg
                .map(|a| a.response_models.iter().cloned().collect())
                .unwrap_or_default(),
            request_counts: ModelPerformanceCounts {
                current: current_agg.map(|a| a.requests.len()).unwrap_or_default(),
                preceding: preceding_agg.map(|a| a.requests.len()).unwrap_or_default(),
                rolling: rolling_agg.map(|a| a.requests.len()).unwrap_or_default(),
            },
            duration,
            throughput,
            ttft,
            input_tokens,
            cache_creation_tokens,
            cache_read_tokens,
            output_tokens,
            error_rate,
        });
    }

    Ok(ModelPerformanceResponse {
        current_window: current,
        preceding_window: preceding,
        rolling_window: rolling,
        identities,
        truncated,
    })
}

/// Query GenAI capability coverage.
///
/// Native observations come from the verified request spans themselves.
/// For Codex, token counters that live on separate usage spans are joined to
/// their enclosing request span with the `codex-one-to-one-v1` rule: same
/// trace, same stored parent chain (the usage span must be a descendant of
/// the request span), a non-error request status, no conflicting model, and
/// exactly one candidate — anything else is reported in the group's
/// correlation provenance rather than guessed.
///
/// The report is calculated from the most recent bounded physical-span
/// sample, so the join is bounded by construction. Duplicate delivery is
/// canonicalised within that sample. `truncated` means older physical spans
/// were not examined. No correlation identifiers are exposed: provenance
/// carries counts and the rule name only.
pub fn query_genai_capabilities(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<otelite_core::api::GenAiCapabilityResponse> {
    let mut where_clause = String::from("WHERE 1=1");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());
    let sql = format!(
        "WITH recent_spans AS (
            SELECT
                trace_id, span_id, parent_span_id, name, kind, start_time, end_time,
                COALESCE(attributes, '{{}}') AS attributes,
                COALESCE(events, '[]') AS events,
                COALESCE(status_code, 0) AS status_code,
                status_message,
                COALESCE(resource, 'null') AS resource,
                created_at,
                id
            FROM spans {where_clause}
            ORDER BY start_time DESC, id DESC
            LIMIT {}
         ),
         ranked_spans AS (
            SELECT
                trace_id, span_id, parent_span_id, name, kind, start_time, end_time,
                COALESCE(attributes, '{{}}') AS attributes,
                COALESCE(events, '[]') AS events,
                COALESCE(status_code, 0) AS status_code,
                status_message,
                COALESCE(resource, 'null') AS resource,
                created_at,
                id,
                ROW_NUMBER() OVER (
                    PARTITION BY trace_id, span_id
                    ORDER BY created_at ASC, id ASC
                ) AS delivery_rank,
                COUNT(*) OVER (PARTITION BY trace_id, span_id) - 1 AS duplicate_deliveries
            FROM recent_spans
         )
         SELECT
            trace_id, span_id, parent_span_id, name, kind, start_time, end_time,
            attributes, events, status_code, status_message, resource,
            duplicate_deliveries
         FROM ranked_spans
         WHERE delivery_rank = 1
         ORDER BY start_time DESC, id DESC",
        CAPABILITY_QUERY_LIMIT + 1
    );
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|param| param.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|error| {
        StorageError::QueryError(format!("Failed to prepare GenAI capability query: {error}"))
    })?;
    let mut rows: Vec<(Span, usize)> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok((parse_span_row(row)?, row.get::<_, usize>(12)?))
        })
        .map_err(|error| {
            StorageError::QueryError(format!("Failed to execute GenAI capability query: {error}"))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|error| {
            StorageError::QueryError(format!("Failed to parse GenAI capability rows: {error}"))
        })?;
    let truncated = rows.len() == CAPABILITY_QUERY_LIMIT + 1;
    if truncated {
        rows.pop();
    }

    let mut canonical_request_span_count = 0;
    let mut duplicate_span_count = 0;
    // LLM-ish spans no verified signature matched, bucketed by the sorted
    // list of attribute names a verified signature would still require
    // (#149). Names and counts only — no values, span or trace identifiers.
    let mut unidentified: BTreeMap<Vec<String>, usize> = BTreeMap::new();
    let mut groups: BTreeMap<CapabilityGroupKey, CapabilityAccum> = BTreeMap::new();
    // Codex request spans seen in the sample: row index, their group key,
    // whether the span completed, its request model, and which metrics had a
    // native observation (the join only fills metrics the request span did
    // not carry itself).
    #[derive(Clone, Copy)]
    struct CodexRequestRef {
        row: usize,
        completed: bool,
        native_present: [bool; 4],
    }
    let mut codex_requests: Vec<(CodexRequestRef, CapabilityGroupKey)> = Vec::new();

    for (row_index, (span, duplicate_deliveries)) in rows.iter().enumerate() {
        let capabilities = classify_span_capabilities(span);
        if capabilities.emitter == GenAiEmitter::Unknown {
            // LLM-ish per the query guard but unidentifiable: record what a
            // verified signature would still require (names only, #149).
            *unidentified
                .entry(otelite_core::telemetry::unidentified_required_attributes(
                    span,
                ))
                .or_default() += 1;
            continue;
        }
        if capabilities.role != GenAiSpanRole::RequestTiming {
            continue;
        }
        let request_model =
            first_semconv_attribute(&span.attributes, otelite_core::semconv::REQUEST_MODEL_KEYS);
        if filters
            .model
            .as_deref()
            .is_some_and(|model| request_model != Some(model))
        {
            continue;
        }
        canonical_request_span_count += 1;
        duplicate_span_count += duplicate_deliveries;
        let provider =
            first_semconv_attribute(&span.attributes, otelite_core::semconv::SYSTEM_KEYS)
                .map(str::to_string);
        let model = request_model.map(str::to_string);
        let fingerprint = capability_fingerprint(
            capabilities.fingerprint.adapter_rule,
            capabilities.fingerprint.service_name.as_deref(),
            capabilities.fingerprint.scope_name.as_deref(),
            capabilities.fingerprint.scope_version.as_deref(),
        );
        let key = (
            provider,
            model,
            fingerprint,
            emitter_name(capabilities.emitter).to_string(),
            capabilities.fingerprint.adapter_rule.to_string(),
        );
        let entry = groups.entry(key.clone()).or_default();
        entry.request_count += 1;
        entry.input_tokens.record(
            capabilities.input_tokens.observation,
            capabilities.input_tokens.source_attribute,
            false,
        );
        entry.output_tokens.record(
            capabilities.output_tokens.observation,
            capabilities.output_tokens.source_attribute,
            false,
        );
        entry.cache_creation_tokens.record(
            capabilities.cache_creation_tokens.observation,
            capabilities.cache_creation_tokens.source_attribute,
            false,
        );
        entry.cache_read_tokens.record(
            capabilities.cache_read_tokens.observation,
            capabilities.cache_read_tokens.source_attribute,
            false,
        );
        let duration_secs =
            (span.end_time.saturating_sub(span.start_time)) as f64 / 1_000_000_000.0;
        let degenerate = capabilities.ttft.seconds.is_some_and(|seconds| {
            duration_secs > 0.0 && seconds / duration_secs >= TTFT_DEGENERATE_RATIO
        });
        entry.ttft.record(
            capabilities.ttft.observation,
            capabilities.ttft.source_attribute,
            degenerate,
        );
        if capabilities.emitter == GenAiEmitter::Codex {
            codex_requests.push((
                CodexRequestRef {
                    row: row_index,
                    completed: span.status.code != StatusCode::Error,
                    native_present: [
                        capabilities.input_tokens.observation != MetricObservation::Absent,
                        capabilities.output_tokens.observation != MetricObservation::Absent,
                        capabilities.cache_creation_tokens.observation != MetricObservation::Absent,
                        capabilities.cache_read_tokens.observation != MetricObservation::Absent,
                    ],
                },
                key,
            ));
        }
    }

    // Correlation pass (codex-one-to-one-v1): join usage-bearing
    // `handle_responses` spans to their enclosing `run_sampling_request`
    // span through the stored parent chain. Operates only on the bounded
    // sample above, so the join is bounded by construction; the parent walk
    // is depth-capped.
    let mut provenance: BTreeMap<
        CapabilityGroupKey,
        otelite_core::api::GenAiCorrelationProvenance,
    > = BTreeMap::new();
    if !codex_requests.is_empty() {
        let key_to_row: HashMap<(String, String), usize> = rows
            .iter()
            .enumerate()
            .map(|(i, (span, _))| ((span.trace_id.clone(), span.span_id.clone()), i))
            .collect();
        let mut candidates_by_request: HashMap<usize, Vec<usize>> = HashMap::new();
        for (row_index, (span, _)) in rows.iter().enumerate() {
            if !is_codex_usage_candidate(span) {
                continue;
            }
            let mut current = span.parent_span_id.clone();
            let mut found: Option<usize> = None;
            // Depth cap: a request is at most a handful of levels above its
            // usage spans; anything deeper is a corrupted or foreign chain.
            for _ in 0..64 {
                let Some(parent_id) = current.clone() else {
                    break;
                };
                let Some(&parent_row) = key_to_row.get(&(span.trace_id.clone(), parent_id)) else {
                    break; // chain leaves the bounded sample
                };
                if is_codex_request_span(&rows[parent_row].0) {
                    found = Some(parent_row);
                    break;
                }
                current = rows[parent_row].0.parent_span_id.clone();
            }
            if let Some(request_row) = found {
                candidates_by_request
                    .entry(request_row)
                    .or_default()
                    .push(row_index);
            }
            // A candidate whose chain never reaches a Codex request span is
            // outside the request-level population (turn-level or orphaned
            // delivery); it is not attributable to any (provider, model)
            // group and is not counted in group provenance.
        }
        for (request, key) in &codex_requests {
            let prov = provenance.entry(key.clone()).or_insert_with(|| {
                otelite_core::api::GenAiCorrelationProvenance {
                    rule: CODEX_CORRELATION_RULE.to_string(),
                    matched_count: 0,
                    unmatched_count: 0,
                    rejected_count: 0,
                    ambiguous_count: 0,
                }
            });
            let request_span = &rows[request.row].0;
            let model = first_semconv_attribute(
                &request_span.attributes,
                otelite_core::semconv::REQUEST_MODEL_KEYS,
            )
            .map(str::to_string);
            let candidates: Vec<&Span> = candidates_by_request
                .get(&request.row)
                .map(|list| list.iter().map(|i| &rows[*i].0).collect())
                .unwrap_or_default();
            match correlate_codex_usage(request.completed, model.as_deref(), &candidates) {
                CorrelationOutcome::Matched {
                    input_tokens,
                    output_tokens,
                    cache_creation_tokens,
                    cache_read_tokens,
                } => {
                    prov.matched_count += 1;
                    let evidence = [
                        (input_tokens, request.native_present[0]),
                        (output_tokens, request.native_present[1]),
                        (cache_creation_tokens, request.native_present[2]),
                        (cache_read_tokens, request.native_present[3]),
                    ];
                    if let Some(accum) = groups.get_mut(key) {
                        for (metric_index, (evidence, native)) in evidence.iter().enumerate() {
                            if *native {
                                continue; // native data wins; no silent override
                            }
                            let Some(attribute) = evidence.source_attribute else {
                                continue; // absent on the candidate: no evidence
                            };
                            match metric_index {
                                0 => accum
                                    .input_tokens
                                    .record_correlated(evidence.observation, attribute),
                                1 => accum
                                    .output_tokens
                                    .record_correlated(evidence.observation, attribute),
                                2 => accum
                                    .cache_creation_tokens
                                    .record_correlated(evidence.observation, attribute),
                                _ => accum
                                    .cache_read_tokens
                                    .record_correlated(evidence.observation, attribute),
                            }
                        }
                    }
                },
                CorrelationOutcome::Unmatched => {
                    // Request-level gap: visible through the metric cells
                    // (absent/sparse availability) as well as this count.
                    prov.unmatched_count += 1;
                },
                CorrelationOutcome::Rejected(_) => prov.rejected_count += 1,
                CorrelationOutcome::Ambiguous(count) => prov.ambiguous_count += count,
            }
        }
    }

    let reports = groups
        .into_iter()
        .map(
            |((provider, model, emitter_fingerprint, emitter, adapter_rule), accum)| {
                let correlation = provenance
                    .remove(&(
                        provider.clone(),
                        model.clone(),
                        emitter_fingerprint.clone(),
                        emitter.clone(),
                        adapter_rule.clone(),
                    ))
                    .unwrap_or(otelite_core::api::GenAiCorrelationProvenance {
                        rule: "none".to_string(),
                        matched_count: 0,
                        unmatched_count: 0,
                        rejected_count: 0,
                        ambiguous_count: 0,
                    });
                otelite_core::api::GenAiCapabilityReport {
                    provider,
                    model,
                    emitter_fingerprint,
                    emitter,
                    adapter_rule,
                    request_count: accum.request_count,
                    input_tokens: accum.input_tokens.report(false),
                    output_tokens: accum.output_tokens.report(false),
                    cache_creation_tokens: accum.cache_creation_tokens.report(false),
                    cache_read_tokens: accum.cache_read_tokens.report(false),
                    ttft: accum.ttft.report(true),
                    correlation,
                }
            },
        )
        .collect();
    Ok(otelite_core::api::GenAiCapabilityResponse {
        reports,
        canonical_span_count: canonical_request_span_count,
        duplicate_span_count,
        truncated,
        filters_applied: Vec::new(),
        unidentified: unidentified
            .into_iter()
            .map(|(required_attributes, span_count)| {
                otelite_core::api::GenAiUnidentifiedSignature {
                    required_attributes,
                    span_count,
                }
            })
            .collect(),
    })
}

/// Query token usage statistics for GenAI/LLM spans
///
/// Returns aggregated token usage grouped by model and system (provider).
pub fn query_token_usage(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<(
    otelite_core::api::TokenUsageSummary,
    Vec<otelite_core::api::ModelUsage>,
    Vec<otelite_core::api::SystemUsage>,
)> {
    let exprs = token_exprs();
    // Build WHERE clause for time filtering.
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let input_expr = exprs.input;
    let output_expr = exprs.output;
    let cache_creation_expr = exprs.cache_creation;
    let cache_read_expr = exprs.cache_read;

    // Query overall summary
    let summary_query = format!(
        "SELECT
            COALESCE(SUM({input_expr}), 0) as total_input,
            COALESCE(SUM({output_expr}), 0) as total_output,
            COUNT(*) as total_requests,
            COALESCE(SUM({cache_creation_expr}), 0) as cache_creation,
            COALESCE(SUM({cache_read_expr}), 0) as cache_read
        FROM spans
        {where_clause}"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let summary = conn
        .query_row(&summary_query, param_refs.as_slice(), |row| {
            Ok(otelite_core::api::TokenUsageSummary {
                total_input_tokens: row.get::<_, i64>(0)? as u64,
                total_output_tokens: row.get::<_, i64>(1)? as u64,
                total_requests: row.get::<_, i64>(2)? as usize,
                total_cache_creation_tokens: row.get::<_, i64>(3)? as u64,
                total_cache_read_tokens: row.get::<_, i64>(4)? as u64,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to query token summary: {}", e)))?;

    // Query by model identity (`provider/model`, bare model when no provider
    // is recorded). Rerouted calls stay in the request-model identity and are
    // counted in `rerouted_count` (#143).
    let model_expr = exprs.identity.clone();
    let request_model_expr = exprs.request_model.clone();
    let response_model_expr = exprs.response_model.clone();
    let model_query = format!(
        "SELECT
            {model_expr} as model,
            COALESCE(SUM({input_expr}), 0) as input_tokens,
            COALESCE(SUM({output_expr}), 0) as output_tokens,
            COUNT(*) as requests,
            SUM(CASE WHEN {request_model_expr} IS NOT NULL
                     AND {response_model_expr} IS NOT NULL
                     AND {request_model_expr} != {response_model_expr}
                THEN 1 ELSE 0 END) as rerouted
        FROM spans
        {where_clause}
        GROUP BY model
        HAVING model IS NOT NULL
        ORDER BY input_tokens + output_tokens DESC"
    );

    let mut stmt = conn
        .prepare(&model_query)
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare model query: {}", e)))?;

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut by_model = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::ModelUsage {
                model: row.get(0)?,
                input_tokens: row.get::<_, i64>(1)? as u64,
                output_tokens: row.get::<_, i64>(2)? as u64,
                requests: row.get::<_, i64>(3)? as usize,
                response_model: None,
                rerouted_count: row.get::<_, i64>(4)? as usize,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to execute model query: {}", e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse model results: {}", e)))?;

    // Dominant differing response model per identity (rerouting analysis).
    // Ordered so the first row per identity is the mode (ties: lexicographic).
    let response_query = format!(
        "SELECT
            {model_expr} as model,
            {response_model_expr} as response_model,
            COUNT(*) as n
        FROM spans
        {where_clause}
        GROUP BY model, response_model
        HAVING response_model IS NOT NULL AND response_model != {request_model_expr}
        ORDER BY model, n DESC, response_model ASC"
    );
    let mut stmt = conn.prepare(&response_query).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare response-model query: {}", e))
    })?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut dominant: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute response-model query: {}", e))
        })?;
    for row in rows {
        let (m, rm) = row.map_err(|e| {
            StorageError::QueryError(format!("Failed to parse response-model row: {}", e))
        })?;
        dominant.entry(m).or_insert(rm);
    }
    for usage in by_model.iter_mut() {
        if let Some(rm) = dominant.remove(&usage.model) {
            usage.response_model = Some(rm);
        }
    }

    // Query by system/provider — accept the OTel-standard names plus llm.* variants.
    let system_expr = exprs.system;
    let system_query = format!(
        "SELECT
            {system_expr} as system,
            COALESCE(SUM({input_expr}), 0) as input_tokens,
            COALESCE(SUM({output_expr}), 0) as output_tokens,
            COUNT(*) as requests
        FROM spans
        {where_clause}
        GROUP BY system
        HAVING system IS NOT NULL
        ORDER BY input_tokens + output_tokens DESC"
    );

    let mut stmt = conn
        .prepare(&system_query)
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare system query: {}", e)))?;

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let by_system = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::SystemUsage {
                system: row.get(0)?,
                input_tokens: row.get::<_, i64>(1)? as u64,
                output_tokens: row.get::<_, i64>(2)? as u64,
                requests: row.get::<_, i64>(3)? as usize,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to execute system query: {}", e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse system results: {}", e)))?;

    Ok((summary, by_model, by_system))
}

/// Time-bucketed token usage grouped by model.
///
/// Bucket assignment uses SQLite integer division (floor): `bucket = (start_time / bucket_ns) * bucket_ns`.
pub fn query_cost_series(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    bucket_ns: i64,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::CostSeriesPoint>> {
    if bucket_ns <= 0 {
        return Err(StorageError::QueryError(format!(
            "bucket_ns must be positive, got {}",
            bucket_ns
        )));
    }

    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            (start_time / ?) * ? as bucket,
            {model} as model,
            COALESCE(SUM({input}), 0),
            COALESCE(SUM({output}), 0),
            COALESCE(SUM({cache_creation}), 0),
            COALESCE(SUM({cache_read}), 0),
            COUNT(*) as requests
        FROM spans
        {where_clause}
        GROUP BY bucket, model
        ORDER BY bucket ASC",
        model = exprs.identity,
        input = exprs.input,
        output = exprs.output,
        cache_creation = exprs.cache_creation,
        cache_read = exprs.cache_read,
    );

    // bucket_ns parameters (two occurrences) must come first to match the `?` order in SQL.
    let mut all_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(params.len() + 2);
    all_params.push(Box::new(bucket_ns));
    all_params.push(Box::new(bucket_ns));
    all_params.extend(params);

    let param_refs: Vec<&dyn rusqlite::ToSql> = all_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare cost_series query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::CostSeriesPoint {
                timestamp: row.get::<_, i64>(0)?,
                model: row.get::<_, Option<String>>(1)?,
                input_tokens: row.get::<_, i64>(2)? as u64,
                output_tokens: row.get::<_, i64>(3)? as u64,
                cache_creation_tokens: row.get::<_, i64>(4)? as u64,
                cache_read_tokens: row.get::<_, i64>(5)? as u64,
                requests: row.get::<_, i64>(6)? as usize,
                // Cost enrichment happens in the API layer.
                cost: None,
                cost_source: None,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute cost_series query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse cost_series results: {}", e))
        })?;

    Ok(rows)
}

/// Top-N most expensive LLM spans by total tokens.
#[allow(clippy::too_many_arguments)]
pub fn query_top_spans(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
    limit: usize,
    sort_by: otelite_core::api::TopSpanSort,
    truncated_only: bool,
) -> Result<Vec<otelite_core::api::TopSpan>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());
    if truncated_only {
        where_clause.push_str(
            " AND (json_extract(attributes, '$.\"gen_ai.response.finish_reason\"') IN ('max_tokens','length')\
             OR (json_type(attributes, '$.\"gen_ai.response.finish_reasons\"') = 'array'\
                 AND json_extract(json_extract(attributes, '$.\"gen_ai.response.finish_reasons\"'), '$[0]') IN ('max_tokens','length')))",
        );
    }

    use otelite_core::api::TopSpanSort;
    let order_by = match sort_by {
        TopSpanSort::TotalTokens => "total_tokens DESC".to_string(),
        TopSpanSort::Duration => "(end_time - start_time) DESC".to_string(),
        TopSpanSort::OutputInputRatio => {
            "CAST(COALESCE(output_tokens_raw, 0) AS FLOAT) / NULLIF(COALESCE(input_tokens_raw, 0) + COALESCE(cache_creation_tokens_raw, 0) + COALESCE(cache_read_tokens_raw, 0), 0) DESC".to_string()
        }
        TopSpanSort::CacheEfficiency => {
            "CAST(COALESCE(cache_read_tokens_raw, 0) AS FLOAT) / NULLIF(COALESCE(input_tokens_raw, 0) + COALESCE(cache_read_tokens_raw, 0), 0) ASC".to_string()
        }
    };

    let sql = format!(
        "SELECT
            trace_id,
            span_id,
            start_time,
            (end_time - start_time) as duration,
            {model} as model,
            {system} as system,
            json_extract(attributes, '$.\"session.id\"') as session_id,
            json_extract(attributes, '$.\"prompt.id\"') as prompt_id,
            COALESCE({input}, 0) as input_tokens,
            COALESCE({output}, 0) as output_tokens,
            COALESCE({cache_creation}, 0) as cache_creation_tokens,
            COALESCE({cache_read}, 0) as cache_read_tokens,
            COALESCE({input}, 0) + COALESCE({output}, 0) + COALESCE({cache_creation}, 0) + COALESCE({cache_read}, 0) as total_tokens,
            COALESCE(
                json_extract(attributes, '$.\"gen_ai.response.finish_reason\"'),
                CASE WHEN json_type(attributes, '$.\"gen_ai.response.finish_reasons\"') = 'array'
                     THEN json_extract(json_extract(attributes, '$.\"gen_ai.response.finish_reasons\"'), '$[0]')
                     ELSE NULL END
            ) as finish_reason,
            json_extract(attributes, '$.\"gen_ai.conversation.id\"') as conversation_id,
            {input} as input_tokens_raw,
            {output} as output_tokens_raw,
            {cache_creation} as cache_creation_tokens_raw,
            {cache_read} as cache_read_tokens_raw
        FROM spans
        {where_clause}
        ORDER BY {order_by}
        LIMIT ?",
        model = exprs.identity,
        system = exprs.system,
        input = exprs.input,
        output = exprs.output,
        cache_creation = exprs.cache_creation,
        cache_read = exprs.cache_read,
        order_by = order_by,
    );

    params.push(Box::new(limit as i64));
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare top_spans query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::TopSpan {
                trace_id: row.get(0)?,
                span_id: row.get(1)?,
                start_time: row.get::<_, i64>(2)?,
                duration: row.get::<_, i64>(3)?,
                model: row.get::<_, Option<String>>(4)?,
                system: row.get::<_, Option<String>>(5)?,
                session_id: row.get::<_, Option<String>>(6)?,
                prompt_id: row.get::<_, Option<String>>(7)?,
                input_tokens: row.get::<_, i64>(8)? as u64,
                output_tokens: row.get::<_, i64>(9)? as u64,
                cache_creation_tokens: row.get::<_, i64>(10)? as u64,
                cache_read_tokens: row.get::<_, i64>(11)? as u64,
                total_tokens: row.get::<_, i64>(12)? as u64,
                finish_reason: row.get::<_, Option<String>>(13)?,
                conversation_id: row.get::<_, Option<String>>(14)?,
                // Cost and derived fields computed in the API layer.
                cost: None,
                cost_source: None,
                cost_reason: None,
                derived_output_tokens_per_sec: None,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to execute top_spans query: {}", e)))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse top_spans results: {}", e))
        })?;

    Ok(rows)
}

pub fn query_top_sessions(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
    limit: usize,
) -> Result<Vec<otelite_core::api::SessionCostRow>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {} AND session_id IS NOT NULL", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            json_extract(attributes, '$.\"session.id\"') as session_id,
            COUNT(*) as request_count,
            SUM(COALESCE({input}, 0)) as input_tokens,
            SUM(COALESCE({output}, 0)) as output_tokens,
            SUM(COALESCE({input}, 0) + COALESCE({output}, 0) + COALESCE({cache_creation}, 0) + COALESCE({cache_read}, 0)) as total_tokens
        FROM spans
        {where_clause}
        GROUP BY session_id
        ORDER BY total_tokens DESC
        LIMIT ?",
        input = exprs.input,
        output = exprs.output,
        cache_creation = exprs.cache_creation,
        cache_read = exprs.cache_read,
    );

    params.push(Box::new(limit as i64));
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare top_sessions query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::SessionCostRow {
                session_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                request_count: row.get::<_, i64>(1)? as u64,
                input_tokens: row.get::<_, i64>(2)? as u64,
                output_tokens: row.get::<_, i64>(3)? as u64,
                total_tokens: row.get::<_, i64>(4)? as u64,
                cost: None,
                cost_source: None,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute top_sessions query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse top_sessions results: {}", e))
        })?;

    Ok(rows)
}

pub fn query_top_conversations(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
    limit: usize,
) -> Result<Vec<otelite_core::api::ConversationCostRow>> {
    let exprs = token_exprs();
    let conversation_id_expr = "json_extract(attributes, '$.\"gen_ai.conversation.id\"')";
    let mut where_clause = format!(
        "WHERE {} AND {} IS NOT NULL",
        exprs.llm_span_guard, conversation_id_expr
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            {conv_id} as conversation_id,
            COUNT(*) as request_count,
            SUM(COALESCE({input}, 0)) as input_tokens,
            SUM(COALESCE({output}, 0)) as output_tokens,
            SUM(COALESCE({input}, 0) + COALESCE({output}, 0) + COALESCE({cache_creation}, 0) + COALESCE({cache_read}, 0)) as total_tokens
        FROM spans
        {where_clause}
        GROUP BY conversation_id
        ORDER BY total_tokens DESC
        LIMIT ?",
        conv_id = conversation_id_expr,
        input = exprs.input,
        output = exprs.output,
        cache_creation = exprs.cache_creation,
        cache_read = exprs.cache_read,
    );

    params.push(Box::new(limit as i64));
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare top_conversations query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::ConversationCostRow {
                conversation_id: row.get::<_, Option<String>>(0)?.unwrap_or_default(),
                request_count: row.get::<_, i64>(1)? as u64,
                input_tokens: row.get::<_, i64>(2)? as u64,
                output_tokens: row.get::<_, i64>(3)? as u64,
                total_tokens: row.get::<_, i64>(4)? as u64,
                cost: None,
                cost_source: None,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute top_conversations query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse top_conversations results: {}", e))
        })?;

    Ok(rows)
}

/// Finish-reason distribution across LLM spans and Claude Code api_response_body logs.
///
/// Unions three sources:
/// 1. OTel plural `gen_ai.response.finish_reasons` (array attribute, unpacked via json_each).
/// 2. OTel singular `gen_ai.response.finish_reason` (scalar attribute).
/// 3. Claude Code `stop_reason` embedded in `claude_code.api_response_body` log bodies.
pub fn query_finish_reasons(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::FinishReasonCount>> {
    // Time/filter scopes are applied per sub-query. We build fragments so each UNION
    // branch only references its own table's columns (spans.start_time / logs.timestamp).
    let mut spans_time_filter = String::new();
    let mut logs_time_filter = String::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        spans_time_filter.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        spans_time_filter.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    // The span filter scope applies to both spans branches (they share the
    // fragment), so its params are bound twice — once per branch.
    if let Some((frag, fp)) = filters.span_scope() {
        spans_time_filter.push_str(&format!(" AND {frag}"));
        params.extend(
            fp.iter()
                .map(|p| Box::new(p.clone()) as Box<dyn rusqlite::ToSql>),
        );
    }
    // The plural (json_each) branch re-uses the same spans time/filter scope, so bind again.
    if let Some(start) = start_time {
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        params.push(Box::new(end));
    }
    if let Some((_, fp)) = filters.span_scope() {
        params.extend(
            fp.into_iter()
                .map(|p| Box::new(p) as Box<dyn rusqlite::ToSql>),
        );
    }
    if let Some(start) = start_time {
        logs_time_filter.push_str(" AND timestamp >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        logs_time_filter.push_str(" AND timestamp <= ?");
        params.push(Box::new(end));
    }
    if let Some((frag, fp)) = filters.log_scope() {
        logs_time_filter.push_str(&format!(" AND {frag}"));
        params.extend(
            fp.into_iter()
                .map(|p| Box::new(p) as Box<dyn rusqlite::ToSql>),
        );
    }

    // Both spans branches carry the finish-reason guard verbatim so the
    // planner can answer them from idx_spans_finish_reason instead of
    // scanning the whole window. The branch-specific IS NOT NULL /
    // json_type conditions remain, so each branch still returns exactly
    // the rows it always did.
    let singular = format!(
        "json_extract(attributes, '$.\"{}\"')",
        semconv::FINISH_REASON_KEY
    );
    let plural = format!(
        "json_extract(attributes, '$.\"{}\"')",
        semconv::FINISH_REASONS_KEY
    );
    let sql = format!(
        "WITH reasons AS (
            SELECT {singular} AS reason
            FROM spans
            WHERE {fr_guard}
              AND {singular} IS NOT NULL
            {spans_time_filter}

            UNION ALL

            SELECT je.value AS reason
            FROM (
                SELECT {plural} AS arr
                FROM spans
                WHERE {fr_guard}
                  AND {plural} IS NOT NULL
                  -- json_valid on the extracted value (not just the
                  -- attributes document): an extracted JSON string such as
                  -- stop is not itself a JSON document, and json_type
                  -- raises on it. json_valid never raises and short-
                  -- circuits the row before json_type is evaluated.
                  AND json_valid({plural})
                  AND json_type({plural}) = 'array'
                {spans_time_filter}
            ) s, json_each(s.arr) je

            UNION ALL

            SELECT json_extract(body_json, '$.stop_reason') AS reason
            FROM (
                SELECT json_extract(attributes, '$.body') AS body_json
                FROM logs
                WHERE body = '{api_body}'
                  AND json_extract(attributes, '$.body') IS NOT NULL
                  AND json_valid(json_extract(attributes, '$.body'))
                  {logs_time_filter}
            ) l
            WHERE json_extract(body_json, '$.stop_reason') IS NOT NULL
        )
        SELECT reason, COUNT(*) as cnt
        FROM reasons
        WHERE reason IS NOT NULL
        GROUP BY reason
        ORDER BY cnt DESC",
        singular = singular,
        plural = plural,
        fr_guard = semconv::finish_reason_guard("attributes"),
        api_body = semconv::API_RESPONSE_BODY_LOG_BODY
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare finish_reasons query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::FinishReasonCount {
                reason: row.get::<_, String>(0)?,
                count: row.get::<_, i64>(1)? as usize,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute finish_reasons query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse finish_reasons results: {}", e))
        })?;

    Ok(rows)
}

const TTFT_DEGENERATE_RATIO: f64 = 0.9;
const TTFT_DEGENERATE_MIN_SAMPLES: usize = 10;

#[derive(Default)]
struct TtftAccum {
    values_ms: Vec<i64>,
    invalid_count: usize,
    degenerate_count: usize,
}

impl TtftAccum {
    /// Record one normalised TTFT observation (exact seconds from the
    /// canonical core normaliser) against its span duration. Quality is
    /// classified on the exact value — rounding to integer milliseconds
    /// first could flip borderline `ttft > duration` verdicts.
    fn record(&mut self, duration_ms: i64, ttft_secs: Option<std::result::Result<f64, ()>>) {
        let Some(ttft_secs) = ttft_secs else {
            return;
        };
        let Ok(ttft_secs) = ttft_secs else {
            self.invalid_count += 1;
            return;
        };
        let quality = otelite_core::telemetry::classify_ttft_value(
            Some(ttft_secs),
            duration_ms as f64 / 1000.0,
        );
        if quality != otelite_core::telemetry::TtftValueQuality::Valid {
            self.invalid_count += 1;
            return;
        }
        let duration_secs = duration_ms as f64 / 1000.0;
        if duration_secs > 0.0 && ttft_secs / duration_secs >= TTFT_DEGENERATE_RATIO {
            self.degenerate_count += 1;
        }
        self.values_ms.push((ttft_secs * 1000.0).round() as i64);
    }

    fn is_degenerate(&self) -> bool {
        self.values_ms.len() >= TTFT_DEGENERATE_MIN_SAMPLES
            && self.degenerate_count * 100 >= self.values_ms.len() * 90
    }
}

/// Latency / TTFT percentile statistics per model for LLM spans.
#[derive(Default)]
struct LatencyAccum {
    durations_ms: Vec<i64>,
    ttft: TtftAccum,
    token_rates: Vec<f64>,
    input_tokens: Vec<i64>,
    output_input_ratios: Vec<f64>,
}

/// Canonical TTFT normalisation shared by every latency path: the key
/// priority, unit conversion and rejection rules live in
/// `otelite_core::telemetry::normalise_span_ttft_secs`; this only adapts
/// the separately-fetched columns to an attribute map and maps rejection
/// reasons to `Err(())` for the accumulators.
fn normalized_ttft_secs(
    otel_ttft_secs: Option<&str>,
    llm_ttft_secs: Option<&str>,
    custom_ttft_ms: Option<&str>,
) -> Option<std::result::Result<f64, ()>> {
    let mut attrs = std::collections::HashMap::new();
    if let Some(value) = otel_ttft_secs {
        attrs.insert(
            "gen_ai.server.time_to_first_token".to_string(),
            value.to_string(),
        );
    }
    if let Some(value) = llm_ttft_secs {
        attrs.insert("llm.time_to_first_token".to_string(), value.to_string());
    }
    if let Some(value) = custom_ttft_ms {
        attrs.insert("ttft_ms".to_string(), value.to_string());
    }
    if attrs.is_empty() {
        return None;
    }
    match otelite_core::telemetry::normalise_span_ttft_secs(&attrs) {
        Some(Ok(secs)) => Some(Ok(secs)),
        Some(Err(_)) => Some(Err(())),
        None => None,
    }
}

///
/// SQLite has no native percentile, so we fetch raw durations per model into memory
/// and compute percentiles in Rust after sorting.
pub fn query_latency_stats(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::LatencyStats>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.request_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            {model} AS model,
            (end_time - start_time) / 1000000 AS duration_ms,
            json_extract(attributes, '$.\"gen_ai.server.time_to_first_token\"') AS otel_ttft_secs,
            json_extract(attributes, '$.\"llm.time_to_first_token\"') AS llm_ttft_secs,
            json_extract(attributes, '$.\"ttft_ms\"') AS custom_ttft_ms,
            {output} AS output_tokens,
            {input} AS input_tokens,
            {cache_creation} AS cache_creation_tokens,
            {cache_read} AS cache_read_tokens,
            (end_time - start_time) AS duration_ns
        FROM spans
        {where_clause}",
        model = exprs.identity,
        output = exprs.output,
        input = exprs.input,
        cache_creation = exprs.cache_creation,
        cache_read = exprs.cache_read,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare latency_stats query: {}", e))
    })?;

    struct Row {
        model: Option<String>,
        duration_ms: i64,
        otel_ttft_secs: Option<String>,
        llm_ttft_secs: Option<String>,
        custom_ttft_ms: Option<String>,
        output_tokens: Option<i64>,
        input_tokens: Option<i64>,
        cache_creation_tokens: Option<i64>,
        cache_read_tokens: Option<i64>,
        duration_ns: i64,
    }

    let rows: Vec<Row> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(Row {
                model: row.get::<_, Option<String>>(0)?,
                duration_ms: row.get::<_, i64>(1)?,
                otel_ttft_secs: row.get::<_, Option<String>>(2)?,
                llm_ttft_secs: row.get::<_, Option<String>>(3)?,
                custom_ttft_ms: row.get::<_, Option<String>>(4)?,
                output_tokens: row.get::<_, Option<i64>>(5)?,
                input_tokens: row.get::<_, Option<i64>>(6)?,
                cache_creation_tokens: row.get::<_, Option<i64>>(7)?,
                cache_read_tokens: row.get::<_, Option<i64>>(8)?,
                duration_ns: row.get::<_, i64>(9)?,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute latency_stats query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse latency_stats results: {}", e))
        })?;

    let mut groups: std::collections::BTreeMap<Option<String>, LatencyAccum> =
        std::collections::BTreeMap::new();
    for r in rows {
        let entry = groups.entry(r.model).or_default();
        entry.durations_ms.push(r.duration_ms);
        entry.ttft.record(
            r.duration_ms,
            normalized_ttft_secs(
                r.otel_ttft_secs.as_deref(),
                r.llm_ttft_secs.as_deref(),
                r.custom_ttft_ms.as_deref(),
            ),
        );
        // Per-call throughput from the raw nanosecond duration (#119);
        // integer-ms division truncated sub-millisecond calls.
        if let Some(rate) = throughput_rate_tok_s(r.duration_ns, r.output_tokens.map(|t| t as f64))
        {
            entry.token_rates.push(rate);
        }
        if let Some(input_tokens) = r.input_tokens {
            entry.input_tokens.push(input_tokens);
        }
        let input_context_tokens = r.input_tokens.unwrap_or_default()
            + r.cache_creation_tokens.unwrap_or_default()
            + r.cache_read_tokens.unwrap_or_default();
        if input_context_tokens > 0 {
            entry
                .output_input_ratios
                .push(r.output_tokens.unwrap_or_default() as f64 / input_context_tokens as f64);
        }
    }

    let mut out = Vec::with_capacity(groups.len());
    for (model, accum) in groups {
        let mut durations = accum.durations_ms;
        let ttft_degenerate = accum.ttft.is_degenerate();
        let TtftAccum {
            values_ms: mut ttfts,
            invalid_count: invalid_ttfts,
            degenerate_count: degenerate_ttfts,
        } = accum.ttft;
        let mut token_rates = accum.token_rates;
        let mut input_tkns = accum.input_tokens;
        let mut ratios = accum.output_input_ratios;
        durations.sort_unstable();
        ttfts.sort_unstable();
        token_rates.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        input_tkns.sort_unstable();
        ratios.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let count = durations.len();
        let avg_ms = if count == 0 {
            0.0
        } else {
            durations.iter().sum::<i64>() as f64 / count as f64
        };

        let ttft_count = ttfts.len();
        let (ttft_p50, ttft_p95, ttft_p99) = if ttft_count == 0 {
            (None, None, None)
        } else {
            (
                Some(percentile(&ttfts, 0.50)),
                Some(percentile(&ttfts, 0.95)),
                Some(percentile(&ttfts, 0.99)),
            )
        };

        let throughput_sample_count = token_rates.len();
        let (tok_p10, tok_p50, tok_p90, tok_p95, tok_p99) = if token_rates.is_empty() {
            (None, None, None, None, None)
        } else {
            (
                Some(percentile_f64(&token_rates, 0.10)),
                Some(percentile_f64(&token_rates, 0.50)),
                Some(percentile_f64(&token_rates, 0.90)),
                Some(percentile_f64(&token_rates, 0.95)),
                Some(percentile_f64(&token_rates, 0.99)),
            )
        };

        let (inp_p50, inp_p95, inp_p99) = if input_tkns.is_empty() {
            (None, None, None)
        } else {
            (
                Some(percentile(&input_tkns, 0.50)),
                Some(percentile(&input_tkns, 0.95)),
                Some(percentile(&input_tkns, 0.99)),
            )
        };

        let (rat_p50, rat_p95, rat_p99) = if ratios.is_empty() {
            (None, None, None)
        } else {
            (
                Some(percentile_f64(&ratios, 0.50)),
                Some(percentile_f64(&ratios, 0.95)),
                Some(percentile_f64(&ratios, 0.99)),
            )
        };

        out.push(otelite_core::api::LatencyStats {
            model,
            count,
            avg_ms,
            p50_ms: percentile(&durations, 0.50),
            p95_ms: percentile(&durations, 0.95),
            p99_ms: percentile(&durations, 0.99),
            ttft_count,
            ttft_invalid_count: invalid_ttfts,
            ttft_degenerate_count: degenerate_ttfts,
            ttft_degenerate,
            ttft_p50_ms: ttft_p50,
            ttft_p95_ms: ttft_p95,
            ttft_p99_ms: ttft_p99,
            derived_tokens_per_sec_p10: tok_p10,
            derived_tokens_per_sec_p50: tok_p50,
            derived_tokens_per_sec_p90: tok_p90,
            derived_tokens_per_sec_p95: tok_p95,
            derived_tokens_per_sec_p99: tok_p99,
            throughput_sample_count,
            input_tokens_p50: inp_p50,
            input_tokens_p95: inp_p95,
            input_tokens_p99: inp_p99,
            output_input_ratio_p50: rat_p50,
            output_input_ratio_p95: rat_p95,
            output_input_ratio_p99: rat_p99,
        });
    }

    out.sort_by_key(|r| std::cmp::Reverse(r.count));
    Ok(out)
}

fn percentile(sorted: &[i64], p: f64) -> i64 {
    if sorted.is_empty() {
        return 0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Percentile estimator used by every latency/throughput endpoint
/// (documented in issue #119 so a change is an explicit API decision, not
/// an accidental behaviour change): rounded-rank on the sorted values,
/// `idx = round((n - 1) * p)` clamped to `[0, n - 1]`, returning the value
/// at that index (`.round()` is half-away-from-zero). Worked examples:
/// n=10, p=0.10 -> idx=1 (second smallest); n=10, p=0.50 -> idx=5;
/// n=11, p=0.90 -> idx=9. Empty input returns 0.0 (callers guard emptiness
/// before publishing, so 0.0 never reaches a response as a measured value).
fn percentile_f64(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() as f64 - 1.0) * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// One LLM request span's latency values (issue #132/#133 cohort).
#[derive(Debug, Clone)]
struct LlmRequestValue {
    model: Option<String>,
    start_ns: i64,
    /// Span duration in ms; negative when `end_time < start_time`.
    duration_ms: f64,
    /// Span duration in raw nanoseconds; the throughput rate divisor
    /// (#119) — integer-ms division would truncate sub-millisecond calls.
    duration_ns: i64,
    /// Validated normalized TTFT in ms, when the span carried a usable one.
    ttft_ms: Option<f64>,
    /// Requested output tokens, when the span carried a usable count.
    output_tokens: Option<f64>,
}

/// Per-call derived output throughput in tokens/second, or `None` when the
/// call is not throughput-eligible (#119 default cohort: completed calls
/// with positive output and duration — in practice output > 0 excludes
/// failed/cancelled/truncated calls, which carry no usable output count).
/// The rate divides by the raw nanosecond duration, never an integer-millisecond
/// approximation.
fn throughput_rate_tok_s(duration_ns: i64, output_tokens: Option<f64>) -> Option<f64> {
    let tokens = output_tokens?;
    if duration_ns <= 0 || tokens <= 0.0 || !tokens.is_finite() {
        return None;
    }
    Some(tokens * 1e9 / duration_ns as f64)
}

/// Collect LLM request spans (all harnesses, `request_span_guard`) with
/// duration, validated TTFT and output tokens for the window.
fn collect_llm_request_values(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<LlmRequestValue>> {
    use otelite_core::telemetry::{classify_ttft_value, TtftValueQuality};

    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.request_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            (end_time - start_time) / 1000000 AS duration_ms,
            {model} AS model,
            start_time,
            json_extract(attributes, '$.\"gen_ai.server.time_to_first_token\"') AS otel_ttft_secs,
            json_extract(attributes, '$.\"llm.time_to_first_token\"') AS llm_ttft_secs,
            json_extract(attributes, '$.\"ttft_ms\"') AS custom_ttft_ms,
            {output} AS output_tokens,
            (end_time - start_time) AS duration_ns
         FROM spans
         {where_clause}",
        model = exprs.identity,
        output = exprs.output,
    );
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare llm request values query: {e}"))
    })?;
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<i64>>(6)?,
                row.get::<_, i64>(7)?,
            ))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute llm request values query: {e}"))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse llm request value rows: {e}"))
        })?;

    let mut out = Vec::with_capacity(rows.len());
    for (duration_ms, model, start_ns, otel, llm, custom, output_tokens, duration_ns) in rows {
        let mut ttft_ms = None;
        if let Some(Ok(ttft_secs)) =
            normalized_ttft_secs(otel.as_deref(), llm.as_deref(), custom.as_deref())
        {
            if classify_ttft_value(Some(ttft_secs), duration_ms as f64 / 1000.0)
                == TtftValueQuality::Valid
            {
                ttft_ms = Some(ttft_secs * 1000.0);
            }
        }
        out.push(LlmRequestValue {
            model,
            start_ns,
            duration_ms: duration_ms as f64,
            duration_ns,
            ttft_ms,
            output_tokens: output_tokens.map(|t| t as f64),
        });
    }
    Ok(out)
}

/// Collect codex turn TTFT histogram observations (issue #132/#133 cohort).
/// Codex request spans carry no TTFT attribute, so this cohort is disjoint
/// from the span TTFT values. `count == 1` rows are exact; larger rows
/// expand each bucket's observations at the bucket midpoint.
fn collect_codex_ttft_values(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<(Option<String>, i64, f64)>> {
    use otelite_core::semconv::metric_names as mnames;

    let mut where_clause =
        String::from("WHERE name = ? AND json_valid(attributes) AND json_valid(value_histogram)");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(mnames::CODEX_TURN_TTFT.to_string())];
    if let Some(start) = start_time {
        where_clause.push_str(" AND timestamp >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND timestamp <= ?");
        params.push(Box::new(end));
    }
    // Codex metrics carry no session/project/model labels, so only session-scoped
    // rows are addressable here; the scope fragment degrades to None otherwise.
    if let Some((frag, fp)) = filters.metric_scope() {
        where_clause.push_str(&format!(" AND {frag}"));
        params.extend(
            fp.into_iter()
                .map(|p| Box::new(p) as Box<dyn rusqlite::ToSql>),
        );
    }
    let sql = format!(
        "SELECT json_extract(attributes, '$.model'), timestamp, value_histogram          FROM metrics {where_clause}",
    );
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare codex ttft histogram query: {e}"))
    })?;
    let hist_rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute codex ttft histogram query: {e}"))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse codex ttft histogram rows: {e}"))
        })?;

    let mut out: Vec<(Option<String>, i64, f64)> = Vec::new();
    for (model, ts, hist_json) in hist_rows {
        let Some(values) = expand_histogram_midpoints(&hist_json) else {
            continue;
        };
        for v in values {
            out.push((model.clone(), ts, v));
        }
    }
    Ok(out)
}

/// Calendar-day bucket boundaries in an IANA timezone.
///
/// Each bucket covers `[start, end)` of one local day; DST days are 23 or
/// 25 hours because boundaries are local midnights mapped to instants, not
/// a fixed 86 400 s step. Buckets are clipped to the query window so the
/// first and last (possibly partial) days tile `[start_ns, end_ns)`
/// exactly, and calls are attributed by start time — a boundary-crossing
/// call appears exactly once, in the day it started.
fn calendar_day_buckets(start_ns: i64, end_ns: i64, tz: &chrono_tz::Tz) -> Result<Vec<(i64, i64)>> {
    use chrono::{DateTime, TimeDelta, Utc};

    if end_ns <= start_ns {
        return Err(StorageError::QueryError(
            "end_time must be after start_time".to_string(),
        ));
    }
    // i64 nanosecond instants only reach ~±292 000 years; anything beyond
    // is invalid input, not an instant to map.
    let day_of = |ns: i64, what: &str| -> Result<chrono::NaiveDate> {
        let secs = ns.div_euclid(1_000_000_000);
        let nsecs = ns.rem_euclid(1_000_000_000) as u32;
        DateTime::<Utc>::from_timestamp(secs, nsecs)
            .ok_or_else(|| StorageError::QueryError(format!("{what} out of range")))
            .map(|dt| dt.with_timezone(tz).date_naive())
    };
    let first_day = day_of(start_ns, "start_time")?;
    let last_day = day_of(end_ns - 1, "end_time")?;

    let mut buckets = Vec::new();
    let mut day = first_day;
    while day <= last_day {
        let next_day = day + TimeDelta::days(1);
        // Local midnight can be ambiguous (DST ends at midnight) or
        // nonexistent (DST starts at midnight) in some zones; `.earliest()`
        // is the deterministic chronological mapping in both cases.
        let day_start = day
            .and_hms_opt(0, 0, 0)
            .and_then(|t| {
                t.and_local_timezone(*tz)
                    .earliest()
                    .and_then(|dt| dt.timestamp_nanos_opt())
            })
            .ok_or_else(|| {
                StorageError::QueryError(format!(
                    "timezone {tz}: cannot resolve local midnight for {day}"
                ))
            })?;
        let day_end = next_day
            .and_hms_opt(0, 0, 0)
            .and_then(|t| {
                t.and_local_timezone(*tz)
                    .earliest()
                    .and_then(|dt| dt.timestamp_nanos_opt())
            })
            .ok_or_else(|| {
                StorageError::QueryError(format!(
                    "timezone {tz}: cannot resolve local midnight for {next_day}"
                ))
            })?;
        let clipped_start = day_start.max(start_ns);
        let clipped_end = day_end.min(end_ns);
        if clipped_start < clipped_end {
            buckets.push((clipped_start, clipped_end));
        }
        day = next_day;
    }
    Ok(buckets)
}

/// Bucketed latency percentiles (issue #132).
///
/// Cohorts, matching `query_latency_stats` so the two endpoints never
/// disagree about what a "request" is:
/// - duration: per-request span duration via `request_span_guard` (all
///   harnesses), `(end_time - start_time)` in ms.
/// - ttft: the same spans' normalized TTFT attribute (validated with the
///   same `classify_ttft_value` rules), UNION the codex
///   `codex.turn.ttft.duration_ms` histogram — codex request spans carry no
///   TTFT attribute, so the cohorts are disjoint. Histogram rows with
///   count > 1 expand each bucket's observations at the bucket midpoint;
///   count == 1 rows contribute their exact `sum`.
///
/// Buckets are either a fixed `bucket_secs` grid from the epoch (rolling
/// mode, non-empty buckets only) or calendar days in `timezone`
/// (DST-aware, empty days included, #141).
#[allow(clippy::too_many_arguments)] // filter bar (#135) and calendar mode (#141) pushed us past 5
pub fn query_latency_percentiles(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    bucket_secs: u64,
    metrics: &[&str],
    filters: &GenAiFilters,
    timezone: Option<&str>,
) -> Result<otelite_core::api::LatencyPercentilesResponse> {
    use otelite_core::api::{
        LatencyPercentilePoint, LatencyPercentileSeries, LatencyPercentilesResponse,
    };

    let want_duration = metrics.contains(&"duration");
    let want_ttft = metrics.contains(&"ttft");
    if !want_duration && !want_ttft {
        return Err(StorageError::QueryError(
            "metrics must include \"duration\" and/or \"ttft\"".to_string(),
        ));
    }

    // Bucket mode. Rolling: fixed-width grid from the epoch, only
    // non-empty buckets are emitted (pre-#141 behaviour). Calendar: explicit
    // IANA timezone, one bucket per local day, empty days emitted with
    // count 0 and null percentiles; requires an explicit window.
    let calendar_buckets: Option<Vec<(i64, i64)>> = match timezone {
        Some(tz_name) => {
            let (start_ns, end_ns) = match (start_time, end_time) {
                (Some(s), Some(e)) => (s, e),
                _ => {
                    return Err(StorageError::QueryError(
                        "calendar-day mode requires explicit start_time and end_time".to_string(),
                    ))
                },
            };
            let tz = std::str::FromStr::from_str(tz_name).map_err(|e| {
                StorageError::QueryError(format!("unknown IANA timezone '{tz_name}': {e}"))
            })?;
            Some(calendar_day_buckets(start_ns, end_ns, &tz)?)
        },
        None => {
            if bucket_secs == 0 {
                return Err(StorageError::QueryError(
                    "bucket_secs must be positive".to_string(),
                ));
            }
            None
        },
    };
    let bucket_ns = bucket_secs as i64 * 1_000_000_000;

    let span_values = collect_llm_request_values(conn, start_time, end_time, filters)?;
    let codex_ttft = if want_ttft {
        collect_codex_ttft_values(conn, start_time, end_time, filters)?
    } else {
        Vec::new()
    };

    // Attribute a timestamp to its bucket start, or None when it falls
    // outside the bucket grid (defensive: window-filtered values cannot
    // miss, calendar windows tile the query range exactly).
    let assign_bucket = |ts: i64| -> Option<i64> {
        match &calendar_buckets {
            Some(buckets) => {
                let starts: Vec<i64> = buckets.iter().map(|(s, _)| *s).collect();
                let idx = starts.partition_point(|s| *s <= ts).checked_sub(1)?;
                let (s, e) = buckets[idx];
                (ts < e).then_some(s)
            },
            None => Some((ts / bucket_ns) * bucket_ns),
        }
    };
    let bucket_end = |bucket_start: i64| -> i64 {
        match &calendar_buckets {
            Some(buckets) => buckets
                .iter()
                .find(|(s, _)| *s == bucket_start)
                .map(|(_, e)| *e)
                .unwrap_or(bucket_start + bucket_ns),
            None => bucket_start + bucket_ns,
        }
    };

    // (metric, model|None, bucket start) -> values in ms
    let mut groups: std::collections::BTreeMap<(&'static str, Option<String>, i64), Vec<f64>> =
        std::collections::BTreeMap::new();
    let push =
        |g: &mut std::collections::BTreeMap<(&'static str, Option<String>, i64), Vec<f64>>,
         metric: &'static str,
         model: Option<String>,
         ts: i64,
         value_ms: f64| {
            let Some(bucket) = assign_bucket(ts) else {
                return;
            };
            match model {
                Some(m) => {
                    g.entry((metric, Some(m), bucket))
                        .or_default()
                        .push(value_ms);
                    g.entry((metric, None, bucket)).or_default().push(value_ms);
                },
                None => {
                    g.entry((metric, None, bucket)).or_default().push(value_ms);
                },
            }
        };

    for v in &span_values {
        if v.duration_ms < 0.0 {
            continue;
        }
        if want_duration {
            push(
                &mut groups,
                "duration",
                v.model.clone(),
                v.start_ns,
                v.duration_ms,
            );
        }
        if want_ttft {
            if let Some(ttft) = v.ttft_ms {
                push(&mut groups, "ttft", v.model.clone(), v.start_ns, ttft);
            }
        }
    }
    for (model, ts, v) in codex_ttft {
        push(&mut groups, "ttft", model, ts, v);
    }

    // Per-call throughput rates for the same buckets (#119): output_tokens
    // divided by the raw nanosecond duration, never aggregate tokens over
    // aggregate duration. Collected over every request value (not just the
    // requested metrics) so each bucket point carries the triple.
    let mut rates: std::collections::BTreeMap<(Option<String>, i64), Vec<f64>> =
        std::collections::BTreeMap::new();
    for v in &span_values {
        let Some(rate) = throughput_rate_tok_s(v.duration_ns, v.output_tokens) else {
            continue;
        };
        let Some(bucket) = assign_bucket(v.start_ns) else {
            continue;
        };
        match &v.model {
            Some(m) => {
                rates
                    .entry((Some(m.clone()), bucket))
                    .or_default()
                    .push(rate);
                rates.entry((None, bucket)).or_default().push(rate);
            },
            None => {
                rates.entry((None, bucket)).or_default().push(rate);
            },
        }
    }

    // Models that have data for at least one metric — the per-model grid of
    // calendar mode spans these plus the "all models" scope.
    let mut model_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for key in groups.keys() {
        if let Some(m) = &key.1 {
            model_set.insert(m.clone());
        }
    }

    let percentile_point =
        |ts: i64, model: &Option<String>, values: Option<&Vec<f64>>| -> LatencyPercentilePoint {
            let (p10_ms, p50_ms, p90_ms, p95_ms, p99_ms, count) = match values {
                Some(values) if !values.is_empty() => {
                    let mut values = values.clone();
                    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    (
                        Some(percentile_f64(&values, 0.10)),
                        Some(percentile_f64(&values, 0.50)),
                        Some(percentile_f64(&values, 0.90)),
                        Some(percentile_f64(&values, 0.95)),
                        Some(percentile_f64(&values, 0.99)),
                        values.len() as u64,
                    )
                },
                _ => (None, None, None, None, None, 0),
            };
            let rate_key = (model.clone(), ts);
            let (
                throughput_p10_tok_s,
                throughput_p50_tok_s,
                throughput_p90_tok_s,
                throughput_sample_count,
            ) = match rates.get(&rate_key) {
                Some(bucket_rates) if !bucket_rates.is_empty() => {
                    let mut bucket_rates = bucket_rates.clone();
                    bucket_rates
                        .sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                    (
                        Some(percentile_f64(&bucket_rates, 0.10)),
                        Some(percentile_f64(&bucket_rates, 0.50)),
                        Some(percentile_f64(&bucket_rates, 0.90)),
                        bucket_rates.len() as u64,
                    )
                },
                _ => (None, None, None, 0),
            };
            LatencyPercentilePoint {
                ts,
                end_ts: bucket_end(ts),
                p10_ms,
                p50_ms,
                p90_ms,
                p95_ms,
                p99_ms,
                count,
                throughput_p10_tok_s,
                throughput_p50_tok_s,
                throughput_p90_tok_s,
                throughput_sample_count,
            }
        };

    let mut out = LatencyPercentilesResponse::default();
    match &calendar_buckets {
        // Rolling mode: only non-empty buckets, pre-#141 behaviour.
        None => {
            for ((metric, model, bucket), values) in &groups {
                let point = percentile_point(*bucket, model, Some(values));
                let series = out
                    .metrics
                    .entry(metric.to_string())
                    .or_insert_with(LatencyPercentileSeries::default);
                match model {
                    Some(m) => series.models.entry(m.clone()).or_default().push(point),
                    None => series.all.push(point),
                }
            }
        },
        // Calendar mode: full grid — every day × every model (and the
        // "all models" scope) × every requested metric, empty buckets
        // included with count 0 and null percentiles.
        Some(buckets) => {
            let mut model_scopes: Vec<Option<String>> = vec![None];
            model_scopes.extend(model_set.iter().cloned().map(Some));
            for metric in metrics {
                for model in &model_scopes {
                    for (bucket_start, _) in buckets {
                        let values = groups.get(&(metric, model.clone(), *bucket_start));
                        let point = percentile_point(*bucket_start, model, values);
                        let series = out
                            .metrics
                            .entry(metric.to_string())
                            .or_insert_with(LatencyPercentileSeries::default);
                        match model {
                            Some(m) => series.models.entry(m.clone()).or_default().push(point),
                            None => series.all.push(point),
                        }
                    }
                }
            }
        },
    }
    for series in out.metrics.values_mut() {
        series.all.sort_by_key(|p| p.ts);
        for v in series.models.values_mut() {
            v.sort_by_key(|p| p.ts);
        }
    }
    Ok(out)
}

/// Generic distribution over a named metric cohort (issue #133).
///
/// Resolvers:
/// - tool_duration: tool-span `(end_time - start_time)` in ms, all
///   harnesses (`tool_span_guard`, same cohort as the tool-usage view).
/// - llm_duration / ttft / output_tokens: the #132 LLM cohorts (span
///   values UNION the codex TTFT histogram for ttft).
/// - session_cost: requires pricing (the API layer prices claude sessions
///   from tokens), so it is resolved there, not here.
#[allow(clippy::too_many_arguments)]
pub fn query_distribution(
    conn: &Connection,
    metric: &str,
    start_time: Option<i64>,
    end_time: Option<i64>,
    buckets: usize,
    scale: &str,
    filters: &GenAiFilters,
) -> Result<otelite_core::api::DistributionResponse> {
    use otelite_core::distribution;

    if scale != "linear" && scale != "log" {
        return Err(StorageError::QueryError(format!(
            "unknown scale '{scale}' — expected \"linear\" or \"log\""
        )));
    }

    let values: Vec<f64> = match metric {
        "tool_duration" => {
            let mut where_clause =
                format!("WHERE {}", otelite_core::semconv::tool_span_guard("attributes"));
            let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
            if let Some(start) = start_time {
                where_clause.push_str(" AND start_time >= ?");
                params.push(Box::new(start));
            }
            if let Some(end) = end_time {
                where_clause.push_str(" AND end_time <= ?");
                params.push(Box::new(end));
            }
            push_scope(&mut where_clause, &mut params, filters.span_scope());
            let sql = format!(
                "SELECT (end_time - start_time) / 1000000 FROM spans {where_clause}"
            );
            let param_refs: Vec<&dyn rusqlite::ToSql> =
                params.iter().map(|p| p.as_ref()).collect();
            let mut stmt = conn.prepare(&sql).map_err(|e| {
                StorageError::QueryError(format!("Failed to prepare tool_duration query: {e}"))
            })?;
            let rows = stmt
                .query_map(param_refs.as_slice(), |row| row.get::<_, i64>(0))
                .map_err(|e| {
                    StorageError::QueryError(format!("Failed to execute tool_duration query: {e}"))
                })?
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| {
                    StorageError::QueryError(format!("Failed to parse tool_duration rows: {e}"))
                })?;
            rows.into_iter().filter(|v| *v >= 0).map(|v| v as f64).collect()
        },
        "llm_duration" | "ttft" | "output_tokens" => {
            let reqs = collect_llm_request_values(conn, start_time, end_time, filters)?;
            match metric {
                "llm_duration" => reqs
                    .iter()
                    .filter(|v| v.duration_ms >= 0.0)
                    .map(|v| v.duration_ms)
                    .collect(),
                "ttft" => {
                    let mut vals: Vec<f64> =
                        reqs.iter().filter_map(|v| v.ttft_ms).collect();
                    for (_, _, v) in collect_codex_ttft_values(conn, start_time, end_time, filters)? {
                        vals.push(v);
                    }
                    vals
                },
                _ => reqs.iter().filter_map(|v| v.output_tokens).collect(),
            }
        },
        "session_cost" => {
            return Err(StorageError::QueryError(
                "session_cost is priced in the API layer (claude sessions are priced from \
                 tokens); use the distributions endpoint"
                    .to_string(),
            ))
        },
        other => {
            return Err(StorageError::QueryError(format!(
                "unknown metric '{other}' — expected session_cost | tool_duration | llm_duration | ttft | output_tokens"
            )))
        },
    };

    let unit = match metric {
        "tool_duration" | "llm_duration" | "ttft" => "ms",
        _ => "tokens",
    };
    Ok(distribution::build(metric, unit, scale, buckets, values))
}

/// Expand one OTel histogram data point into per-observation values in ms.
/// `count == 1` rows contribute their exact `sum`; rows with multiple
/// observations contribute each bucket's count at the bucket midpoint
/// (the topmost bucket uses its upper bound as a lower bound).
fn expand_histogram_midpoints(hist_json: &str) -> Option<Vec<f64>> {
    let parsed: serde_json::Value = serde_json::from_str(hist_json).ok()?;
    let arr = parsed.as_array()?;
    let count = arr.first()?.as_u64()? as usize;
    let sum = arr.get(1)?.as_f64()?;
    let mut out: Vec<f64> = Vec::with_capacity(count);
    if count == 1 {
        out.push(sum);
        return Some(out);
    }
    let buckets = arr.get(2)?.as_array()?;
    let mut prev: f64 = 0.0;
    let mut emitted = 0usize;
    for b in buckets {
        let bound = b.get("upper_bound")?.as_f64()?;
        let k = b.get("count")?.as_u64()? as usize;
        for _ in 0..k {
            out.push((prev + bound) / 2.0);
        }
        emitted += k;
        prev = bound;
    }
    // Observations beyond the final bounded bucket (open tail) fall back to
    // the running mean of the remaining mass — never a fabricated zero.
    if emitted < count {
        let remaining_sum = sum - out.iter().sum::<f64>();
        let v = if count > emitted {
            remaining_sum / (count - emitted) as f64
        } else {
            prev
        };
        out.resize(count, v.max(0.0));
    }
    Some(out)
}

/// Error rate per model across LLM spans.
///
/// The spans table stores status as `status_code INTEGER` (0 = Unset, 1 = Ok, 2 = Error);
/// any row with status_code = 2 counts as an error.
pub fn query_error_rate(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::ErrorRateByModel>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            {model} AS model,
            SUM(CASE WHEN status_code = 2 THEN 1 ELSE 0 END) AS errors,
            COUNT(*) AS total
        FROM spans
        {where_clause}
        GROUP BY model
        HAVING model IS NOT NULL
        ORDER BY errors DESC, total DESC",
        model = exprs.identity,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare error_rate query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let model: Option<String> = row.get(0)?;
            let errors: i64 = row.get(1)?;
            let total: i64 = row.get(2)?;
            let error_rate = if total > 0 {
                errors as f64 / total as f64
            } else {
                0.0
            };
            Ok(otelite_core::api::ErrorRateByModel {
                model,
                total: total as usize,
                errors: errors as usize,
                error_rate,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute error_rate query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse error_rate results: {}", e))
        })?;

    Ok(rows)
}

/// Aggregated per-tool usage from tool-execution spans.
pub fn query_tool_usage(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
    limit: usize,
) -> Result<Vec<otelite_core::api::ToolUsage>> {
    // The tool-span guard (verbatim conjunct) scopes the scan to
    // idx_spans_tool instead of the whole window; it is exactly the
    // condition for the COALESCE below to be non-NULL, so results are
    // unchanged.
    let mut where_clause = format!("WHERE {}", semconv::tool_span_guard("attributes"));
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            COALESCE(
                {},
                CASE WHEN name LIKE '{prefix}%' AND name != '{prefix}' THEN name ELSE NULL END
            ) AS tool_name,
            COUNT(*) AS cnt,
            SUM(CASE WHEN status_code = 2 THEN 1 ELSE 0 END) AS errors,
            SUM(CASE WHEN status_code = 1 OR status_code = 0 THEN 1 ELSE 0 END) AS ok_cnt,
            COALESCE(SUM(end_time - start_time), 0) AS total_duration_ns
        FROM spans
        {where_clause}
        GROUP BY tool_name
        HAVING tool_name IS NOT NULL
        ORDER BY cnt DESC
        LIMIT ?",
        semconv::coalesce_extract("attributes", semconv::TOOL_NAME_KEYS),
        prefix = semconv::TOOL_SPAN_NAME_PREFIX
    );

    params.push(Box::new(limit as i64));
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare tool_usage query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let tool_name: String = row.get(0)?;
            let count: i64 = row.get(1)?;
            let errors: i64 = row.get(2)?;
            let ok_cnt: i64 = row.get(3)?;
            let total_ns: i64 = row.get(4)?;
            let total_ms = total_ns / 1_000_000;
            let avg_ms = if count > 0 {
                (total_ns as f64 / count as f64) / 1_000_000.0
            } else {
                0.0
            };
            Ok(otelite_core::api::ToolUsage {
                tool_name,
                count: count as usize,
                success_count: ok_cnt as usize,
                error_count: errors as usize,
                avg_duration_ms: avg_ms,
                total_duration_ms: total_ms,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute tool_usage query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse tool_usage results: {}", e))
        })?;

    Ok(rows)
}

/// Retry statistics across LLM spans.
pub fn query_retry_stats(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<otelite_core::api::RetryStats> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            COALESCE(
                CAST(json_extract(attributes, '$.\"attempt\"') AS INTEGER),
                CAST(json_extract(attributes, '$.\"retry_count\"') AS INTEGER),
                CAST(json_extract(attributes, '$.\"gen_ai.request.attempt\"') AS INTEGER),
                1
            ) AS attempt
        FROM spans
        {where_clause}"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare retry_stats query: {}", e))
    })?;

    let attempts: Vec<i64> = stmt
        .query_map(param_refs.as_slice(), |row| row.get::<_, i64>(0))
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute retry_stats query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse retry_stats results: {}", e))
        })?;

    let total_llm_calls = attempts.len();
    let mut retried_calls = 0usize;
    let mut extra_attempts = 0i64;
    for a in &attempts {
        let attempt = (*a).max(1);
        if attempt > 1 {
            retried_calls += 1;
            extra_attempts += attempt - 1;
        }
    }
    let retry_rate = if total_llm_calls > 0 {
        retried_calls as f64 / total_llm_calls as f64
    } else {
        0.0
    };

    Ok(otelite_core::api::RetryStats {
        total_llm_calls,
        retried_calls,
        extra_attempts: extra_attempts as usize,
        retry_rate,
        filters_applied: Vec::new(),
    })
}

/// Aggregated retrieval / RAG statistics across retriever spans.
///
/// Retriever spans are identified by either:
/// - `openinference.span.kind = 'RETRIEVER'`, or
/// - presence of a `retrieval.query` attribute (fallback for non-OpenInference instrumentations).
///
/// OpenInference stores retrieved documents under `retrieval.documents` as a JSON
/// array of `{document.id, document.score, document.content, document.metadata}`.
/// Document count is taken from `json_array_length`, and the per-span top-1 score
/// is the `document.score` of the first element.
pub fn query_retrieval_stats(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
    top_queries_limit: usize,
) -> Result<otelite_core::api::RetrievalStats> {
    let mut time_filter = String::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        time_filter.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        time_filter.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    if let Some((frag, fp)) = filters.span_scope() {
        time_filter.push_str(&format!(" AND {frag}"));
        params.extend(
            fp.into_iter()
                .map(|p| Box::new(p) as Box<dyn rusqlite::ToSql>),
        );
    }

    // CTE: per-retrieval-span query, document count, and top-1 score.
    // Reused by both the summary and top-queries aggregations. The
    // retrieval guard (verbatim conjunct) scopes the scan to
    // idx_spans_retrieval; it is the same condition the old inline
    // OR used, plus the json_valid gate that makes it total.
    let cte = format!(
        "WITH retrieval_spans AS (
            SELECT
                CAST(json_extract(attributes, '$.\"retrieval.query\"') AS TEXT) AS query,
                COALESCE(
                    json_array_length(json_extract(attributes, '$.\"retrieval.documents\"')),
                    0
                ) AS doc_count,
                CAST(json_extract(attributes, '$.\"retrieval.documents\"[0].\"document.score\"') AS REAL) AS top_score
            FROM spans
            WHERE {guard}
            {time_filter}
        )",
        guard = semconv::retrieval_span_guard("attributes")
    );

    // Summary query: totals plus averages. AVG(top_score) auto-ignores NULLs.
    let summary_sql = format!(
        "{cte}
         SELECT
             COUNT(*) AS total,
             COALESCE(AVG(CAST(doc_count AS REAL)), 0.0) AS avg_docs,
             AVG(top_score) AS avg_top_score
         FROM retrieval_spans"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let (total_retrievals, avg_documents_per_query, avg_top_document_score) = conn
        .query_row(&summary_sql, param_refs.as_slice(), |row| {
            let total: i64 = row.get(0)?;
            let avg_docs: f64 = row.get(1)?;
            let avg_top_score: Option<f64> = row.get(2)?;
            Ok((total as usize, avg_docs, avg_top_score))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to query retrieval summary: {}", e))
        })?;

    if total_retrievals == 0 {
        return Ok(otelite_core::api::RetrievalStats {
            total_retrievals: 0,
            avg_documents_per_query: 0.0,
            avg_top_document_score: None,
            top_queries: Vec::new(),
            filters_applied: Vec::new(),
        });
    }

    // Top queries: group by query text, ordered by count desc.
    // The same time-filter params are bound a second time for this query.
    let top_sql = format!(
        "{cte}
         SELECT
             query,
             COUNT(*) AS cnt,
             COALESCE(AVG(CAST(doc_count AS REAL)), 0.0) AS avg_docs,
             AVG(top_score) AS avg_top_score
         FROM retrieval_spans
         WHERE query IS NOT NULL
         GROUP BY query
         ORDER BY cnt DESC
         LIMIT ?"
    );

    let mut top_params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(params.len() + 1);
    if let Some(start) = start_time {
        top_params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        top_params.push(Box::new(end));
    }
    top_params.push(Box::new(top_queries_limit as i64));

    let top_param_refs: Vec<&dyn rusqlite::ToSql> = top_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&top_sql).map_err(|e| {
        StorageError::QueryError(format!(
            "Failed to prepare retrieval top_queries query: {}",
            e
        ))
    })?;

    let top_queries = stmt
        .query_map(top_param_refs.as_slice(), |row| {
            Ok(otelite_core::api::TopRetrievalQuery {
                query: row.get::<_, String>(0)?,
                count: row.get::<_, i64>(1)? as usize,
                avg_documents: row.get::<_, f64>(2)?,
                avg_top_score: row.get::<_, Option<f64>>(3)?,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!(
                "Failed to execute retrieval top_queries query: {}",
                e
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!(
                "Failed to parse retrieval top_queries results: {}",
                e
            ))
        })?;

    Ok(otelite_core::api::RetrievalStats {
        total_retrievals,
        avg_documents_per_query,
        avg_top_document_score,
        top_queries,
        filters_applied: Vec::new(),
    })
}

/// Return up to 50 distinct resource attribute keys for the given signal table.
/// `signal` must be one of "logs", "spans", or "metrics".
pub fn distinct_resource_keys(conn: &Connection, signal: &str) -> Result<Vec<String>> {
    // (table, recency column used to pick the most recent sample rows)
    let (table, ts) = match signal {
        "logs" => ("logs", "timestamp"),
        "spans" => ("spans", "start_time"),
        "metrics" => ("metrics", "timestamp"),
        other => {
            return Err(StorageError::QueryError(format!(
                "Unknown signal type: {}",
                other
            )));
        },
    };

    // The key space of resource attributes is stable per service, and this
    // feeds a typeahead datalist — so sample the most recent rows instead of
    // scanning and JSON-parsing the whole table (65s on an 18M-span DB for a
    // list of a few dozen keys). `json_valid` makes the parse total: a
    // malformed resource JSON contributes no keys instead of failing the query.
    const SAMPLE_ROWS: i64 = 50_000;
    let sql = format!(
        "SELECT je.key FROM ( \
           SELECT {table}.resource AS resource FROM {table} \
           ORDER BY {ts} DESC LIMIT {SAMPLE_ROWS} \
         ) r, json_each(CASE WHEN json_valid(r.resource) \
                              THEN json_extract(r.resource, '$.attributes') END) je \
         GROUP BY je.key \
         LIMIT 50"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare query: {}", e)))?;

    let keys = stmt
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|e| StorageError::QueryError(format!("Failed to execute query: {}", e)))?
        .filter_map(|r| r.ok())
        .collect();

    Ok(keys)
}

/// Truncation rate (finish_reason = max_tokens / length) per model.
pub fn query_truncation_rate(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::TruncationRateByModel>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            {model} AS model,
            COUNT(*) AS total,
            SUM(CASE
                WHEN COALESCE(
                    json_extract(attributes, '$.\"gen_ai.response.finish_reason\"'),
                    CASE WHEN json_type(attributes, '$.\"gen_ai.response.finish_reasons\"') = 'array'
                         THEN json_extract(json_extract(attributes, '$.\"gen_ai.response.finish_reasons\"'), '$[0]')
                         ELSE NULL END
                ) IN ('max_tokens', 'length') THEN 1 ELSE 0 END) AS truncated
        FROM spans
        {where_clause}
        GROUP BY {model}
        ORDER BY total DESC",
        model = exprs.identity,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare truncation_rate query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let total = row.get::<_, i64>(1)? as usize;
            let truncated = row.get::<_, i64>(2)? as usize;
            let rate = if total > 0 {
                truncated as f64 / total as f64
            } else {
                0.0
            };
            Ok(otelite_core::api::TruncationRateByModel {
                model: row.get::<_, Option<String>>(0)?,
                total,
                truncated,
                rate,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute truncation_rate query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse truncation_rate results: {}", e))
        })?;

    Ok(rows)
}

/// Cache token hit rate per model.
pub fn query_cache_hit_rate(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::CacheHitRateByModel>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            {model} AS model,
            SUM(COALESCE({input}, 0)) AS input_tokens,
            SUM(COALESCE({cache_read}, 0)) AS cache_read_tokens,
            SUM(COALESCE({cache_creation}, 0)) AS cache_creation_tokens
        FROM spans
        {where_clause}
        GROUP BY {model}
        ORDER BY cache_read_tokens DESC",
        model = exprs.identity,
        input = exprs.input,
        cache_read = exprs.cache_read,
        cache_creation = exprs.cache_creation,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare cache_hit_rate query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let input = row.get::<_, i64>(1)? as u64;
            let cache_read = row.get::<_, i64>(2)? as u64;
            let cache_creation = row.get::<_, i64>(3)? as u64;
            let denominator = cache_read + input;
            let hit_rate = if denominator > 0 {
                Some(cache_read as f64 / denominator as f64)
            } else {
                None
            };
            Ok(otelite_core::api::CacheHitRateByModel {
                model: row.get::<_, Option<String>>(0)?,
                total_input_tokens: input,
                total_cache_read_tokens: cache_read,
                total_cache_creation_tokens: cache_creation,
                hit_rate,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute cache_hit_rate query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse cache_hit_rate results: {}", e))
        })?;

    Ok(rows)
}

/// One cumulative-counter series and its windowed delta.
#[derive(Debug, Clone)]
pub(crate) struct CounterWindowDelta {
    /// Extracted label values, in the order of `label_paths`.
    pub labels: Vec<Option<String>>,
    /// Usage within the window: last value at or before `end_time` minus the
    /// last value before `start_time` (0 when the series did not exist before
    /// the window). A series whose in-window last value is below its baseline
    /// (counter reset, e.g. app restart) is treated as restarting from zero.
    pub delta: f64,
}
/// Scalar counter value (value_int, falling back to value_double).
pub(crate) const COUNTER_SCALAR_VALUE_SQL: &str =
    "COALESCE(value_int, CAST(value_double AS INTEGER))";
/// Cumulative-histogram observation count (`value_histogram[0]`).
pub(crate) const HISTOGRAM_COUNT_VALUE_SQL: &str = "CASE WHEN json_valid(value_histogram) \
                                                    THEN CAST(json_extract(value_histogram, '$[0]') AS REAL) END";
/// Cumulative-histogram value sum (`value_histogram[1]`). Kept as REAL (no
/// integer cast) so sub-cent cost counters do not round away.
pub(crate) const HISTOGRAM_SUM_VALUE_SQL: &str = "CASE WHEN json_valid(value_histogram) \
                                                  THEN CAST(json_extract(value_histogram, '$[1]') AS REAL) END";

/// Compute windowed usage for a cumulative counter metric.
///
/// Agent telemetry (opencode, claude_code) emits cumulative counters keyed
/// by the full label set, so summing window rows would overcount. The
/// per-series delta is "last value at or before `end_time`" minus "last
/// value before `start_time`"; rows sharing the maximum timestamp resolve to
/// the max value (duplicate flushes at one tick).
///
/// The per-series baseline seeks rely on the covering indexes defined in
/// `schema.rs` (e.g. `idx_metrics_opencode_token_usage`): the expressions
/// below use the index's expression columns verbatim, and a metric added to
/// counter queries without its covering index degrades every baseline seek
/// to a full table scan.
pub(crate) fn counter_window_deltas(
    conn: &Connection,
    metric_name: &str,
    label_paths: &[&str],
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<Vec<CounterWindowDelta>> {
    counter_window_deltas_value(
        conn,
        metric_name,
        label_paths,
        COUNTER_SCALAR_VALUE_SQL,
        start_time,
        end_time,
    )
}

/// [`counter_window_deltas`] for a counter whose value lives in an
/// expression (e.g. a `value_histogram` field) rather than `value_int`.
///
/// `value_sql` must be total: it may return NULL (missing or malformed
/// value) but must never raise, so a corrupt row degrades to a zero
/// contribution instead of breaking the query. NULL values sort after all
/// real values in the baseline's `DESC` order, so a baseline seek skips
/// corrupt rows and settles on the newest valid one.
#[allow(clippy::too_many_arguments)]
pub(crate) fn counter_window_deltas_value(
    conn: &Connection,
    metric_name: &str,
    label_paths: &[&str],
    value_sql: &str,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<Vec<CounterWindowDelta>> {
    let label_exprs: Vec<String> = label_paths
        .iter()
        .map(|p| {
            format!("CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{p}') END")
        })
        .collect();

    let mut where_clause = String::from("WHERE name = ?1");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(metric_name.to_string())];
    if let Some(start) = start_time {
        where_clause.push_str(" AND timestamp >= ?2");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
        params.push(Box::new(end));
    }

    let select_cols = if label_exprs.is_empty() {
        format!("timestamp, {value_sql}")
    } else {
        format!("{}, timestamp, {value_sql}", label_exprs.join(", "))
    };
    let sql = format!("SELECT {select_cols} FROM metrics {where_clause}");
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!(
            "Failed to prepare counter window query for {metric_name}: {e}"
        ))
    })?;
    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let labels = (0..label_paths.len())
                .map(|i| row.get::<_, Option<String>>(i))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let ts = row.get::<_, i64>(label_paths.len())?;
            let value = row
                .get::<_, Option<f64>>(label_paths.len() + 1)?
                .unwrap_or(0.0);
            Ok((labels, ts, value))
        })
        .map_err(|e| {
            StorageError::QueryError(format!(
                "Failed to execute counter window query for {metric_name}: {e}"
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!(
                "Failed to parse counter window results for {metric_name}: {e}"
            ))
        })?;

    // Group by label tuple -> (last timestamp, max value at that timestamp).
    let mut last_values: HashMap<Vec<Option<String>>, (i64, f64)> = HashMap::new();
    for (labels, ts, value) in rows {
        match last_values.get_mut(&labels) {
            Some(entry) => {
                if ts > entry.0 {
                    *entry = (ts, value);
                } else if ts == entry.0 && value > entry.1 {
                    entry.1 = value;
                }
            },
            None => {
                last_values.insert(labels, (ts, value));
            },
        }
    }

    // Baseline per series: last value strictly before the window start.
    let mut baselines: HashMap<Vec<Option<String>>, f64> = HashMap::new();
    if let Some(start) = start_time {
        let mut predicate = String::new();
        for (i, expr) in label_exprs.iter().enumerate() {
            predicate.push_str(&format!(" AND {expr} IS ?{}", 3 + i));
        }

        let baseline_sql = format!(
            "SELECT {value_sql} FROM metrics \
             WHERE name = ?1 AND timestamp < ?2{predicate} \
             ORDER BY timestamp DESC, {value_sql} DESC \
             LIMIT 1"
        );
        let mut baseline_stmt = conn.prepare(&baseline_sql).map_err(|e| {
            StorageError::QueryError(format!(
                "Failed to prepare counter baseline query for {metric_name}: {e}"
            ))
        })?;
        for labels in last_values.keys() {
            let mut binds: Vec<Box<dyn rusqlite::ToSql>> =
                vec![Box::new(metric_name.to_string()), Box::new(start)];
            binds.extend(
                labels
                    .iter()
                    .map(|l| Box::new(l.clone()) as Box<dyn rusqlite::ToSql>),
            );
            let refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
            match baseline_stmt.query_row(refs.as_slice(), |row| row.get::<_, Option<f64>>(0)) {
                Ok(Some(v)) => {
                    baselines.insert(labels.clone(), v);
                },
                Ok(None) | Err(rusqlite::Error::QueryReturnedNoRows) => {},
                Err(e) => {
                    return Err(StorageError::QueryError(format!(
                        "Failed to execute counter baseline query for {metric_name}: {e}"
                    )))
                },
            }
        }
    }

    let mut out = Vec::with_capacity(last_values.len());
    for (labels, (_ts, last)) in last_values {
        let baseline = baselines.get(&labels).copied().unwrap_or(0.0);
        let delta = if last < baseline {
            last
        } else {
            last - baseline
        };
        if delta > 0.0 {
            out.push(CounterWindowDelta { labels, delta });
        }
    }
    Ok(out)
}

/// Cache economics per model and per time bucket.
///
/// Combines the three harness sources, one per harness so nothing is
/// double-counted:
/// - opencode: windowed per-row deltas of the cumulative
///   `opencode.token.usage` counter (reset-safe: a value below the previous
///   one restarts that series' running total, same semantics as
///   [`counter_window_deltas`]);
/// - codex: per-turn sums of the `codex.turn.token_usage` histogram
///   (`value_histogram[1]`); the `total` category is the sum of the parts
///   and is never counted;
/// - claude_code: token sums on `claude_code.llm_request` spans (per-request
///   events, no counter semantics). The `claude_code.token.usage` metric is
///   deliberately NOT a fourth source: its counter does not line up with the
///   span totals (verified on the live DB, 2026-08-27) and adding it would
///   miscount.
///
/// `hit_rate` is `cache_read / (cache_read + input)` everywhere (same
/// definition as `query_cache_hit_rate`). Savings are enriched by the API
/// layer.
/// One opencode `token.usage` counter row: its full stable label set
/// (agent, model, type, session.id — the counter's series key), its
/// timestamp, and its value.
type OpencodeCounterRow = (Vec<Option<String>>, i64, i64);

/// Fetch all opencode `token.usage` rows in the time window, in timestamp
/// order (the `(name, timestamp)` index range).
fn opencode_usage_rows(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<OpencodeCounterRow>> {
    use otelite_core::semconv::metric_labels as lbl;
    use otelite_core::semconv::metric_names as mnames;

    let label_paths = [lbl::AGENT, lbl::MODEL, lbl::TYPE, lbl::SESSION_ID];
    let label_exprs: Vec<String> = label_paths
        .iter()
        .map(|p| {
            format!("CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{p}') END")
        })
        .collect();

    let mut where_clause = String::from("WHERE name = ?");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(mnames::OPENCODE_TOKEN_USAGE.to_string())];
    if let Some(start) = start_time {
        where_clause.push_str(" AND timestamp >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND timestamp <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.metric_scope());

    let sql = format!(
        "SELECT {}, timestamp, \
         COALESCE(value_int, CAST(value_double AS INTEGER)) FROM metrics {}",
        label_exprs.join(", "),
        where_clause
    );
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare opencode usage query: {e}"))
    })?;
    let mapped = stmt
        .query_map(param_refs.as_slice(), |row| {
            let labels = (0..label_paths.len())
                .map(|i| row.get::<_, Option<String>>(i))
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let ts = row.get::<_, i64>(label_paths.len())?;
            let value = row
                .get::<_, Option<i64>>(label_paths.len() + 1)?
                .unwrap_or(0);
            Ok((labels, ts, value))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute opencode usage query: {e}"))
        })?;
    mapped
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse opencode usage results: {e}"))
        })
}

/// Per-series baseline: last value strictly before the window start, for
/// every series present in `rows`. Uses the covering-index pattern from
/// `counter_window_deltas` — the predicate must use the index's expression
/// columns verbatim.
fn opencode_usage_baselines(
    conn: &Connection,
    rows: &[OpencodeCounterRow],
    start_time: Option<i64>,
) -> Result<HashMap<Vec<Option<String>>, i64>> {
    use otelite_core::semconv::metric_labels as lbl;
    use otelite_core::semconv::metric_names as mnames;

    let mut baselines: HashMap<Vec<Option<String>>, i64> = HashMap::new();
    let Some(start) = start_time else {
        return Ok(baselines);
    };
    let label_paths = [lbl::AGENT, lbl::MODEL, lbl::TYPE, lbl::SESSION_ID];
    let label_exprs: Vec<String> = label_paths
        .iter()
        .map(|p| {
            format!("CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{p}') END")
        })
        .collect();
    let mut predicate = String::new();
    for (i, expr) in label_exprs.iter().enumerate() {
        predicate.push_str(&format!(" AND {expr} IS ?{}", 3 + i));
    }
    let baseline_sql = format!(
        "SELECT COALESCE(value_int, CAST(value_double AS INTEGER)) FROM metrics \
         WHERE name = ?1 AND timestamp < ?2{predicate} \
         ORDER BY timestamp DESC, \
           COALESCE(value_int, CAST(value_double AS INTEGER)) DESC \
         LIMIT 1"
    );
    let known_series: Vec<Vec<Option<String>>> = rows.iter().map(|(l, _, _)| l.clone()).collect();
    let mut seen: std::collections::HashSet<Vec<Option<String>>> = std::collections::HashSet::new();
    let mut baseline_stmt = conn.prepare(&baseline_sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare opencode baseline query: {e}"))
    })?;
    for labels in known_series.into_iter().filter(|l| seen.insert(l.clone())) {
        let mut binds: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(mnames::OPENCODE_TOKEN_USAGE.to_string()),
            Box::new(start),
        ];
        binds.extend(
            labels
                .iter()
                .map(|l| Box::new(l.clone()) as Box<dyn rusqlite::ToSql>),
        );
        let refs: Vec<&dyn rusqlite::ToSql> = binds.iter().map(|b| b.as_ref()).collect();
        match baseline_stmt.query_row(refs.as_slice(), |row| row.get::<_, Option<i64>>(0)) {
            Ok(Some(v)) => {
                baselines.insert(labels, v);
            },
            Ok(None) | Err(rusqlite::Error::QueryReturnedNoRows) => {},
            Err(e) => {
                return Err(StorageError::QueryError(format!(
                    "Failed to execute opencode baseline query: {e}"
                )))
            },
        }
    }
    Ok(baselines)
}

/// Clamp each counter row's delta to its series' running value: a value
/// below the previous one means the counter restarted, so that row's full
/// value counts. An equal value is NOT a reset — it counts as zero (flat
/// counters, e.g. opencode re-flushing resumed-session state, must not
/// contribute their own value on every equal row).
fn opencode_counter_deltas(
    rows: Vec<OpencodeCounterRow>,
    baselines: &HashMap<Vec<Option<String>>, i64>,
) -> Vec<OpencodeCounterRow> {
    let mut last_by_series: HashMap<Vec<Option<String>>, i64> = HashMap::new();
    let mut out = Vec::with_capacity(rows.len());
    for (labels, ts, value) in rows {
        let delta = match last_by_series.get(&labels) {
            None => {
                // First in-window row of this series.
                match baselines.get(&labels) {
                    Some(base) if value < *base => value, // reset: counts from zero
                    Some(base) => value - base,
                    None => value, // series did not exist before the window
                }
            },
            Some(prev) if value < *prev => value, // in-window reset
            Some(prev) => value - prev,
        };
        last_by_series.insert(labels.clone(), value);
        if delta > 0 {
            out.push((labels, ts, delta));
        }
    }
    out
}

pub fn query_cache_economics(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    bucket_ns: i64,
) -> Result<otelite_core::api::CacheEconomicsResponse> {
    if bucket_ns <= 0 {
        return Err(StorageError::QueryError(format!(
            "bucket_ns must be positive, got {bucket_ns}"
        )));
    }

    use otelite_core::semconv::codex_token_types as ctt;
    use otelite_core::semconv::metric_labels as lbl;
    use otelite_core::semconv::metric_names as mnames;
    use otelite_core::semconv::opencode_token_types as otypes;

    #[derive(Default)]
    struct CacheAcc {
        input: u64,
        cache_read: u64,
        cache_write: u64,
    }

    const UNKNOWN_MODEL: &str = "(unknown)";
    let mut models: HashMap<String, CacheAcc> = HashMap::new();
    let mut buckets: HashMap<i64, CacheAcc> = HashMap::new();

    let add_model =
        |m: &mut HashMap<String, CacheAcc>, model: Option<&str>, input: u64, cr: u64, cw: u64| {
            let acc = m
                .entry(model.unwrap_or(UNKNOWN_MODEL).to_string())
                .or_default();
            acc.input += input;
            acc.cache_read += cr;
            acc.cache_write += cw;
        };
    let add_bucket =
        |b: &mut HashMap<i64, CacheAcc>, ts: i64, bucket_ns: i64, input: u64, cr: u64, cw: u64| {
            let acc = b.entry((ts / bucket_ns) * bucket_ns).or_default();
            acc.input += input;
            acc.cache_read += cr;
            acc.cache_write += cw;
        };
    // ── opencode: cumulative counter, window fetch + per-series baseline ──
    {
        let rows = opencode_usage_rows(conn, start_time, end_time, &GenAiFilters::default())?;
        let baselines = opencode_usage_baselines(conn, &rows, start_time)?;
        for (labels, ts, delta) in opencode_counter_deltas(rows, &baselines) {
            let model = labels.get(1).and_then(|m| m.clone());
            let kind = labels.get(2).and_then(|k| k.clone());
            let d = delta as u64;
            match kind.as_deref() {
                Some(k) if k == otypes::INPUT => {
                    add_model(&mut models, model.as_deref(), d, 0, 0);
                    add_bucket(&mut buckets, ts, bucket_ns, d, 0, 0);
                },
                Some(k) if k == otypes::CACHE_READ => {
                    add_model(&mut models, model.as_deref(), 0, d, 0);
                    add_bucket(&mut buckets, ts, bucket_ns, 0, d, 0);
                },
                Some(k) if k == otypes::CACHE_WRITE => {
                    add_model(&mut models, model.as_deref(), 0, 0, d);
                    add_bucket(&mut buckets, ts, bucket_ns, 0, 0, d);
                },
                _ => {}, // output/reasoning/unknown are not cache economics
            }
        }
    }

    // ── codex: per-turn histogram sums, bucketed in SQL ──
    {
        let model_expr = format!(
            "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END",
            lbl::MODEL
        );
        let type_expr = format!(
            "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END",
            lbl::TOKEN_TYPE
        );
        let mut where_clause = String::from(&format!(
            "WHERE name = ?3 AND json_valid(attributes) \
             AND {model_expr} IS NOT NULL AND {type_expr} IS NOT NULL"
        ));
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(bucket_ns),
            Box::new(bucket_ns),
            Box::new(mnames::CODEX_TURN_TOKEN_USAGE.to_string()),
        ];
        if let Some(start) = start_time {
            where_clause.push_str(&format!(" AND timestamp >= ?{}", params.len() + 1));
            params.push(Box::new(start));
        }
        if let Some(end) = end_time {
            where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
            params.push(Box::new(end));
        }
        let sql = format!(
            "SELECT (timestamp / ?1) * ?1 AS bucket, {model_expr} AS model, \
             {type_expr} AS token_type, \
             SUM(CASE WHEN json_valid(value_histogram) \
                 THEN json_extract(value_histogram, '$[1]') ELSE 0 END) AS sum_tokens \
             FROM metrics {where_clause} GROUP BY bucket, model, token_type"
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!(
                "Failed to prepare cache economics codex query: {e}"
            ))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })
            .map_err(|e| {
                StorageError::QueryError(format!(
                    "Failed to execute cache economics codex query: {e}"
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::QueryError(format!(
                    "Failed to parse cache economics codex results: {e}"
                ))
            })?;
        for (bucket, model, token_type, sum) in rows {
            let tokens = sum.round().max(0.0) as u64;
            if tokens == 0 {
                continue;
            }
            let (input, cr, cw) = match token_type.as_str() {
                t if t == ctt::INPUT => (tokens, 0, 0),
                t if t == ctt::CACHE_READ => (0, tokens, 0),
                t if t == ctt::CACHE_WRITE => (0, 0, tokens),
                _ => continue, // output/reasoning_output/total are not cache economics
            };
            add_model(&mut models, model.as_deref(), input, cr, cw);
            add_bucket(&mut buckets, bucket, bucket_ns, input, cr, cw);
        }
    }

    // ── claude_code: llm_request span token sums, bucketed in SQL ──
    {
        let exprs = token_exprs();
        let mut where_clause = format!(
            "WHERE name = '{}' AND json_valid(attributes)",
            otelite_core::semconv::LLM_REQUEST_SPAN_NAME
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(bucket_ns), Box::new(bucket_ns)];
        if let Some(start) = start_time {
            where_clause.push_str(&format!(" AND start_time >= ?{}", params.len() + 1));
            params.push(Box::new(start));
        }
        if let Some(end) = end_time {
            where_clause.push_str(&format!(" AND end_time <= ?{}", params.len() + 1));
            params.push(Box::new(end));
        }
        let sql = format!(
            "SELECT (start_time / ?1) * ?1 AS bucket, {model} AS model, \
             COALESCE(SUM({input}), 0) AS input_tokens, \
             COALESCE(SUM({cache_creation}), 0) AS cache_creation, \
             COALESCE(SUM({cache_read}), 0) AS cache_read \
             FROM spans {where_clause} GROUP BY bucket, model",
            model = exprs.identity,
            input = exprs.input,
            cache_creation = exprs.cache_creation,
            cache_read = exprs.cache_read,
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!(
                "Failed to prepare cache economics claude query: {e}"
            ))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                ))
            })
            .map_err(|e| {
                StorageError::QueryError(format!(
                    "Failed to execute cache economics claude query: {e}"
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::QueryError(format!(
                    "Failed to parse cache economics claude results: {e}"
                ))
            })?;
        for (bucket, model, input, cache_creation, cache_read) in rows {
            add_model(
                &mut models,
                model.as_deref(),
                input.max(0) as u64,
                cache_read.max(0) as u64,
                cache_creation.max(0) as u64,
            );
            add_bucket(
                &mut buckets,
                bucket,
                bucket_ns,
                input.max(0) as u64,
                cache_read.max(0) as u64,
                cache_creation.max(0) as u64,
            );
        }
    }

    // ── assembly ──
    let mut model_entries: Vec<otelite_core::api::CacheEconModelEntry> = models
        .iter()
        .map(|(model, acc)| {
            let hit_rate = cache_hit_rate(acc.cache_read, acc.input);
            let read_write_ratio = cache_read_write_ratio(acc.cache_read, acc.cache_write);
            otelite_core::api::CacheEconModelEntry {
                model: model.clone(),
                input_tokens: acc.input,
                cache_read_tokens: acc.cache_read,
                cache_write_tokens: acc.cache_write,
                hit_rate,
                read_write_ratio,
                est_savings_usd: None,
                savings_known: false,
            }
        })
        .collect();
    model_entries.sort_by(|a, b| {
        b.cache_read_tokens
            .cmp(&a.cache_read_tokens)
            .then_with(|| a.model.cmp(&b.model))
    });

    let mut series_points: Vec<otelite_core::api::CacheEconSeriesPoint> = buckets
        .iter()
        .map(|(ts, acc)| {
            let hit_rate = cache_hit_rate(acc.cache_read, acc.input);
            otelite_core::api::CacheEconSeriesPoint {
                timestamp: *ts,
                input: acc.input,
                cache_read: acc.cache_read,
                cache_write: acc.cache_write,
                hit_rate,
            }
        })
        .collect();
    series_points.sort_by_key(|p| p.timestamp);

    Ok(otelite_core::api::CacheEconomicsResponse {
        series: series_points,
        models: model_entries,
    })
}

/// Reasoning-token share per model plus a global per-effort breakdown
/// (issue #131).
///
/// Sources (one per harness, no double counting):
/// - opencode `token.usage` cumulative counters, types `reasoning` and
///   `output` — windowed per-row clamped deltas (reset-safe; a flat counter
///   contributes zero, see `opencode_counter_deltas`);
/// - codex `turn.token_usage` per-turn histograms, token types
///   `reasoning_output` and `output` (the `total` category is the sum of the
///   parts and is never counted);
/// - claude_code is deliberately **absent**: its `llm_request` spans carry
///   no thinking-token attributes (verified on the live DB), so nothing
///   would be real to report rather than fabricate.
///
/// `cost_usd` is left `None` for the API layer, which prices the reasoning
/// tokens at the model's output rate.
pub fn query_reasoning_share(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::ReasoningShareResponse> {
    use otelite_core::semconv::codex_token_types as ctt;
    use otelite_core::semconv::metric_labels as lbl;
    use otelite_core::semconv::metric_names as mnames;
    use otelite_core::semconv::opencode_token_types as otypes;
    use otelite_core::semconv::{
        CODEX_HANDLE_RESPONSES_SPAN_NAME, CODEX_REASONING_EFFORT_KEY,
        CODEX_REASONING_OUTPUT_TOKENS_KEY,
    };

    const UNKNOWN_MODEL: &str = "(unknown)";
    #[derive(Default)]
    struct ReasonAcc {
        reasoning: u64,
        output: u64,
    }
    let mut models: HashMap<String, ReasonAcc> = HashMap::new();

    // ── opencode: cumulative counters, types reasoning + output ──
    {
        let rows = opencode_usage_rows(conn, start_time, end_time, &GenAiFilters::default())?;
        let baselines = opencode_usage_baselines(conn, &rows, start_time)?;
        for (labels, _ts, delta) in opencode_counter_deltas(rows, &baselines) {
            let model = labels.get(1).and_then(|m| m.clone());
            let kind = labels.get(2).and_then(|k| k.clone());
            let acc = models
                .entry(model.unwrap_or_else(|| UNKNOWN_MODEL.to_string()))
                .or_default();
            match kind.as_deref() {
                Some(k) if k == otypes::REASONING => acc.reasoning += delta as u64,
                Some(k) if k == otypes::OUTPUT => acc.output += delta as u64,
                _ => {},
            }
        }
    }

    // ── codex: per-turn histogram sums, reasoning_output + output ──
    {
        let model_expr = format!(
            "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END",
            lbl::MODEL
        );
        let type_expr = format!(
            "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END",
            lbl::TOKEN_TYPE
        );
        let mut where_clause = format!(
            "WHERE name = ?1 AND json_valid(attributes) \
             AND {model_expr} IS NOT NULL AND {type_expr} IS NOT NULL"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(mnames::CODEX_TURN_TOKEN_USAGE.to_string())];
        if let Some(start) = start_time {
            where_clause.push_str(&format!(" AND timestamp >= ?{}", params.len() + 1));
            params.push(Box::new(start));
        }
        if let Some(end) = end_time {
            where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
            params.push(Box::new(end));
        }
        let sql = format!(
            "SELECT {model_expr} AS model, {type_expr} AS token_type, \
             SUM(CASE WHEN json_valid(value_histogram) \
                  THEN json_extract(value_histogram, '$[1]') ELSE 0 END) AS sum_tokens \
             FROM metrics {where_clause} GROUP BY model, token_type"
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!(
                "Failed to prepare reasoning share codex query: {e}"
            ))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(|e| {
                StorageError::QueryError(format!(
                    "Failed to execute reasoning share codex query: {e}"
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::QueryError(format!(
                    "Failed to parse reasoning share codex results: {e}"
                ))
            })?;
        for (model, token_type, sum) in rows {
            let tokens = sum.round().max(0.0) as u64;
            if tokens == 0 {
                continue;
            }
            let acc = models
                .entry(model.as_deref().unwrap_or(UNKNOWN_MODEL).to_string())
                .or_default();
            match token_type.as_str() {
                t if t == ctt::REASONING => acc.reasoning += tokens,
                t if t == ctt::OUTPUT => acc.output += tokens,
                _ => continue, // input/cached_input/cache_write_input/total
            }
        }
    }

    // ── span-level gen_ai.usage.reasoning_tokens (opencode plugin, pi, etc.) ──
    // These spans carry the attribute directly; aggregate by model.
    {
        let reasoning_key = otelite_core::semconv::REASONING_TOKEN_KEYS[0];
        let model_expr = "COALESCE(json_extract(attributes,'$.\"gen_ai.request.model\"'), \
             json_extract(attributes,'$.\"model\"'), '(unknown)')".to_string();
        let rtok_expr = format!(
            "CAST(json_extract(attributes,'$.\"{reasoning_key}\"') AS INTEGER)"
        );
        let output_expr = "CAST(COALESCE(json_extract(attributes,'$.\"gen_ai.usage.output_tokens\"'), \
             json_extract(attributes,'$.\"output_tokens\"')) AS INTEGER)".to_string();
        let mut where_clause = format!(
            "WHERE json_valid(attributes) \
             AND json_extract(attributes,'$.\"{reasoning_key}\"') IS NOT NULL \
             AND CAST(json_extract(attributes,'$.\"{reasoning_key}\"') AS INTEGER) > 0"
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(start) = start_time {
            where_clause.push_str(&format!(" AND start_time >= ?{}", params.len() + 1));
            params.push(Box::new(start));
        }
        if let Some(end) = end_time {
            where_clause.push_str(&format!(" AND end_time <= ?{}", params.len() + 1));
            params.push(Box::new(end));
        }
        let sql = format!(
            "SELECT {model_expr} AS model, \
             COALESCE(SUM({rtok_expr}), 0) AS reasoning_sum, \
             COALESCE(SUM({output_expr}), 0) AS output_sum \
             FROM spans {where_clause} GROUP BY model"
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        if let Ok(mut stmt) = conn.prepare(&sql) {
            if let Ok(rows) = stmt
                .query_map(param_refs.as_slice(), |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                })
                .and_then(|r| r.collect::<std::result::Result<Vec<_>, _>>())
            {
                for (model, reasoning, output) in rows {
                    let acc = models.entry(model).or_default();
                    acc.reasoning += reasoning.max(0) as u64;
                    acc.output += output.max(0) as u64;
                }
            }
        }
    }

    // ── codex effort breakdown: handle_responses spans (no model attr) ──
    let effort_expr = format!(
        "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"{key}\"') END",
        key = CODEX_REASONING_EFFORT_KEY
    );
    let rtok_expr = format!(
        "CASE WHEN json_valid(attributes) THEN CAST(json_extract(attributes, '$.\"{key}\"') AS INTEGER) END",
        key = CODEX_REASONING_OUTPUT_TOKENS_KEY
    );
    let mut where_clause = format!(
        "WHERE name = '{name}' AND json_valid(attributes) AND {effort_expr} IS NOT NULL",
        name = CODEX_HANDLE_RESPONSES_SPAN_NAME
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(start) = start_time {
        where_clause.push_str(&format!(" AND start_time >= ?{}", params.len() + 1));
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(&format!(" AND end_time <= ?{}", params.len() + 1));
        params.push(Box::new(end));
    }
    let effort_sql = format!(
        "SELECT {effort_expr} AS effort, COUNT(*) AS calls, \
         COALESCE(SUM({rtok_expr}), 0) AS reasoning_tokens \
         FROM spans {where_clause} GROUP BY effort"
    );
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&effort_sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare reasoning effort query: {e}"))
    })?;
    let effort_rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute reasoning effort query: {e}"))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse reasoning effort results: {e}"))
        })?;
    let mut effort: Vec<otelite_core::api::ReasoningEffortEntry> = effort_rows
        .into_iter()
        .map(
            |(effort, calls, tokens)| otelite_core::api::ReasoningEffortEntry {
                effort,
                calls: calls.max(0) as u64,
                reasoning_tokens: tokens.max(0) as u64,
            },
        )
        .collect();
    effort.sort_by(|a, b| b.calls.cmp(&a.calls).then_with(|| a.effort.cmp(&b.effort)));

    let mut model_entries: Vec<otelite_core::api::ReasoningShareByModel> = models
        .into_iter()
        .map(|(model, acc)| {
            let share_pct = reasoning_share_pct(acc.reasoning, acc.output);
            otelite_core::api::ReasoningShareByModel {
                model,
                reasoning_tokens: acc.reasoning,
                output_tokens: acc.output,
                share_pct,
                cost_usd: None, // enriched by the API layer
            }
        })
        .collect();
    model_entries.sort_by(|a, b| {
        b.reasoning_tokens
            .cmp(&a.reasoning_tokens)
            .then_with(|| a.model.cmp(&b.model))
    });

    Ok(otelite_core::api::ReasoningShareResponse {
        models: model_entries,
        effort,
        filters_applied: Vec::new(),
    })
}

/// Per-harness rollup: sessions, tokens (per model), tool calls and retries
/// for opencode, codex and claude, plus per-bucket series data for the
/// chart. One source per harness, so nothing is double-counted:
///
/// - opencode: `token.usage` counter deltas (types input/output/reasoning/
///   cacheRead/cacheCreation), `session.cost.total` histogram-sum deltas
///   (actual USD spend), `tool.duration` histogram-count deltas,
///   `retry.count` counter deltas, `session.count` distinct sessions
///   (top-level only: `is_subagent != 'true'`).
/// - codex: `turn.token_usage` per-turn histogram sums (input/output/
///   reasoning_output/cached_input/cache_write_input; `total` never
///   counted), `thread.started` cli thread starts as sessions, `tool.call`
///   rows as tool calls, `api_request` success='false' rows as retries.
/// - claude: `token.usage` per-event token sums, `session.count` distinct
///   sessions, `claude_code.tool.execution` span count as tool calls, no
///   retry telemetry (`retries: None`).
///
/// Cost is `None` here in every case: opencode's counter delta is carried
/// in `counter_cost_usd` (actual spend), and codex/claude are priced by the
/// API layer from the per-model token totals. Claude's own `cost.usage`
/// counter is deliberately ignored — it under-reports against the token
/// volumes by ~30x on the live DB (2026-08-27), so an estimate is more
/// honest than a truncated actual.
pub fn query_agent_rollup(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    bucket_secs: u64,
) -> Result<Vec<otelite_core::api::AgentRollupStorage>> {
    use otelite_core::api::AgentTokenUsage;
    use otelite_core::semconv::agent_names as anames;
    use otelite_core::semconv::metric_labels as lbl;
    use otelite_core::semconv::metric_names as mnames;
    use otelite_core::semconv::opencode_token_types as otypes;

    if bucket_secs == 0 {
        return Err(StorageError::QueryError(format!(
            "bucket_secs must be positive, got {bucket_secs}"
        )));
    }
    let bucket_ns = bucket_secs as i64 * 1_000_000_000;
    let bucket_of = |ts: i64| (ts / bucket_ns) * bucket_ns;

    const UNKNOWN_MODEL: &str = "(unknown)";
    let mut out: Vec<otelite_core::api::AgentRollupStorage> = Vec::new();

    // ── opencode ─────────────────────────────────────────────────────────
    {
        // token.usage: one fetch, one baseline pass, per-row clamped deltas.
        let rows = opencode_usage_rows(conn, start_time, end_time, &GenAiFilters::default())?;
        let baselines = opencode_usage_baselines(conn, &rows, start_time)?;
        let mut models: HashMap<String, AgentTokenUsage> = HashMap::new();
        let mut series: HashMap<i64, HashMap<String, AgentTokenUsage>> = HashMap::new();
        for (labels, ts, delta) in opencode_counter_deltas(rows, &baselines) {
            let model = labels
                .get(1)
                .and_then(|m| m.as_deref())
                .unwrap_or(UNKNOWN_MODEL)
                .to_string();
            let kind = labels.get(2).and_then(|k| k.clone());
            let tokens = delta as u64;
            let add = |acc: &mut AgentTokenUsage| match kind.as_deref() {
                Some(k) if k == otypes::INPUT => acc.input += tokens,
                Some(k) if k == otypes::OUTPUT => acc.output += tokens,
                Some(k) if k == otypes::REASONING => acc.reasoning += tokens,
                Some(k) if k == otypes::CACHE_READ => acc.cache_read += tokens,
                Some(k) if k == otypes::CACHE_WRITE => acc.cache_write += tokens,
                _ => {},
            };
            add(models.entry(model.clone()).or_default());
            let bkt = series.entry(bucket_of(ts)).or_default();
            add(bkt.entry(model).or_default());
        }

        // Sessions: distinct top-level session ids in the window.
        let sessions = distinct_session_ids(
            conn,
            mnames::OPENCODE_SESSION_COUNT,
            start_time,
            end_time,
            Some(lbl::IS_SUBAGENT),
        )?;

        // Cost: per-session cumulative histogram sum, windowed delta.
        let cost_deltas = counter_window_deltas_value(
            conn,
            mnames::OPENCODE_SESSION_COST_TOTAL,
            &[lbl::SESSION_ID],
            HISTOGRAM_SUM_VALUE_SQL,
            start_time,
            end_time,
        )?;
        let counter_cost: f64 = cost_deltas.iter().map(|d| d.delta).sum();

        // Tool calls: per-(session, tool) cumulative histogram count.
        let tool_deltas = counter_window_deltas_value(
            conn,
            mnames::OPENCODE_TOOL_DURATION,
            &[lbl::SESSION_ID, lbl::TOOL_NAME],
            HISTOGRAM_COUNT_VALUE_SQL,
            start_time,
            end_time,
        )?;
        let tool_calls = tool_deltas
            .iter()
            .map(|d| d.delta.round().max(0.0))
            .sum::<f64>()
            .round()
            .max(0.0) as u64;

        // Retries: per-session cumulative counter.
        let retry_deltas = counter_window_deltas(
            conn,
            mnames::OPENCODE_RETRY_COUNT,
            &[lbl::SESSION_ID],
            start_time,
            end_time,
        )?;
        let retries = retry_deltas
            .iter()
            .map(|d| d.delta.round().max(0.0))
            .sum::<f64>()
            .round()
            .max(0.0) as u64;

        push_agent(
            &mut out,
            anames::OPENCODE,
            sessions,
            Some(counter_cost),
            models,
            series,
            tool_calls,
            Some(retries),
        );
    }

    // ── codex ────────────────────────────────────────────────────────────
    {
        // Per-turn histogram sums, bucketed in SQL (per-event metric: a
        // plain windowed SUM is correct — no counter semantics).
        let mut where_clause = String::from("WHERE name = ?1");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(mnames::CODEX_TURN_TOKEN_USAGE.to_string()),
            Box::new(bucket_ns),
        ];
        if let Some(start) = start_time {
            where_clause.push_str(&format!(" AND timestamp >= ?{}", params.len() + 1));
            params.push(Box::new(start));
        }
        if let Some(end) = end_time {
            where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
            params.push(Box::new(end));
        }
        let sql = format!(
            "SELECT CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END AS model, \
                    CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END AS token_type, \
                    (timestamp / ?2) * ?2 AS bucket, \
                    SUM(CASE WHEN json_valid(value_histogram) \
                         THEN json_extract(value_histogram, '$[1]') ELSE 0 END) AS sum_tokens \
             FROM metrics {where_clause} \
             GROUP BY model, token_type, bucket",
            lbl::MODEL, lbl::TOKEN_TYPE
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!("Failed to prepare codex turn token query: {e}"))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, f64>(3)?,
                ))
            })
            .map_err(|e| {
                StorageError::QueryError(format!("Failed to execute codex turn token query: {e}"))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::QueryError(format!("Failed to parse codex turn tokens: {e}"))
            })?;
        let mut models: HashMap<String, AgentTokenUsage> = HashMap::new();
        let mut series: HashMap<i64, HashMap<String, AgentTokenUsage>> = HashMap::new();
        for (model, token_type, bucket, sum) in rows {
            add_turn_tokens(
                &mut models,
                &mut series,
                model,
                token_type.as_deref(),
                bucket,
                sum,
            );
        }

        let (sessions, tool_calls, retries) = codex_event_totals(conn, start_time, end_time)?;

        push_agent(
            &mut out,
            anames::CODEX,
            sessions,
            None,
            models,
            series,
            tool_calls,
            Some(retries),
        );
    }

    // ── claude ───────────────────────────────────────────────────────────
    {
        // Per-event token sums, bucketed in SQL.
        let mut where_clause = String::from("WHERE name = ?1");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![
            Box::new(mnames::CLAUDE_CODE_TOKEN_USAGE.to_string()),
            Box::new(bucket_ns),
        ];
        if let Some(start) = start_time {
            where_clause.push_str(&format!(" AND timestamp >= ?{}", params.len() + 1));
            params.push(Box::new(start));
        }
        if let Some(end) = end_time {
            where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
            params.push(Box::new(end));
        }
        let sql = format!(
            "SELECT CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END AS model, \
                    CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END AS token_type, \
                    (timestamp / ?2) * ?2 AS bucket, \
                    COALESCE(SUM(COALESCE(value_int, CAST(value_double AS INTEGER))), 0) AS total \
             FROM metrics {where_clause} \
             GROUP BY model, token_type, bucket",
            lbl::MODEL, lbl::TYPE
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!("Failed to prepare claude token query: {e}"))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| {
                StorageError::QueryError(format!("Failed to execute claude token query: {e}"))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| StorageError::QueryError(format!("Failed to parse claude tokens: {e}")))?;
        let mut models: HashMap<String, AgentTokenUsage> = HashMap::new();
        let mut series: HashMap<i64, HashMap<String, AgentTokenUsage>> = HashMap::new();
        for (model, token_type, bucket, total) in rows {
            add_turn_tokens(
                &mut models,
                &mut series,
                model,
                token_type.as_deref(),
                bucket,
                total as f64,
            );
        }

        let sessions = distinct_session_ids(
            conn,
            mnames::CLAUDE_CODE_SESSION_COUNT,
            start_time,
            end_time,
            None,
        )?;

        // Tool calls: claude_code.tool.execution span count (covered by
        // idx_spans_tool_exec).
        let tool_calls: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM spans \
                 WHERE name = 'claude_code.tool.execution' \
                 AND start_time >= ?1 AND start_time <= ?2",
                rusqlite::params![start_time.unwrap_or(i64::MIN), end_time.unwrap_or(i64::MAX)],
                |r| r.get(0),
            )
            .map_err(|e| {
                StorageError::QueryError(format!("Failed to count claude tool spans: {e}"))
            })?;

        push_agent(
            &mut out,
            anames::CLAUDE,
            sessions,
            None,
            models,
            series,
            tool_calls.max(0) as u64,
            None,
        );
    }

    Ok(out)
}

/// Per-project rollup: opencode sessions attributed by their `project.id`
/// label, plus one `"unattributed"` row for codex/claude (no project label
/// today) and any label-less opencode activity.
///
/// Reuses the #125 machinery verbatim where the series keys already
/// include the full counter key: `opencode_usage_rows`/`baselines`/
/// `counter_deltas` for token deltas (series key = agent/model/type/
/// session.id), and `counter_window_deltas_value` over `[session.id]` for
/// the per-session cost counter (one series per session). No new index:
/// both ride `idx_metrics_name_ts` and the existing covering indexes.
pub fn query_project_rollup(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<Vec<otelite_core::api::ProjectRollupStorage>> {
    use otelite_core::api::{AgentTokenUsage, ProjectRollupStorage};
    use otelite_core::semconv::agent_names as anames;
    use otelite_core::semconv::metric_labels as lbl;
    use otelite_core::semconv::metric_names as mnames;
    use otelite_core::semconv::opencode_token_types as otypes;

    const UNATTRIBUTED: &str = "unattributed";
    const UNKNOWN_MODEL: &str = "(unknown)";

    struct ProjectAcc {
        sessions: u64,
        counter_cost: f64,
        has_counter: bool,
        counter_disjoint: bool,
        models: HashMap<String, AgentTokenUsage>,
    }

    fn acc<'a>(m: &'a mut HashMap<String, ProjectAcc>, p: &str) -> &'a mut ProjectAcc {
        m.entry(p.to_string()).or_insert_with(|| ProjectAcc {
            sessions: 0,
            counter_cost: 0.0,
            has_counter: false,
            counter_disjoint: false,
            models: HashMap::new(),
        })
    }

    let mut projects: HashMap<String, ProjectAcc> = HashMap::new();

    // ── opencode: token deltas attributed via session → project ─────────
    // Session → project map: one windowed pass over `session.count` (the
    // marker every opencode session emits, with the project label).
    let mut where_clause = String::from(
        "WHERE name = ?1 AND json_valid(attributes) \
         AND json_extract(attributes, '$.\"session.id\"') IS NOT NULL",
    );
    let n_time_bounds = start_time.is_some() as usize + end_time.is_some() as usize;
    if start_time.is_some() {
        where_clause.push_str(" AND timestamp >= ?2");
    }
    if end_time.is_some() {
        where_clause.push_str(&format!(
            " AND timestamp <= ?{}",
            2 + start_time.is_some() as usize
        ));
    }
    // Sentinel placeholder sits after name + any time bounds.
    let sentinel_idx = 2 + n_time_bounds;
    // NULL project labels fold into the sentinel in SQL so a session's
    // attribution is deterministic.
    let make_params = || {
        let mut p: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(mnames::OPENCODE_SESSION_COUNT.to_string())];
        if let Some(start) = start_time {
            p.push(Box::new(start));
        }
        if let Some(end) = end_time {
            p.push(Box::new(end));
        }
        p.push(Box::new(UNATTRIBUTED.to_string()));
        p
    };
    let map_sql = format!(
        "SELECT DISTINCT \
            json_extract(attributes, '$.\"session.id\"'), \
            COALESCE(json_extract(attributes, '{lbl_project}'), ?{sentinel_idx}) \
         FROM metrics {where_clause}",
        lbl_project = lbl::PROJECT_ID,
    );
    let map_params = make_params();
    let param_refs: Vec<&dyn rusqlite::ToSql> = map_params.iter().map(|p| p.as_ref()).collect();
    let mut map_stmt = conn
        .prepare(&map_sql)
        .map_err(|e| StorageError::QueryError(format!("Failed to prepare project map: {e}")))?;
    let mut session_project: HashMap<String, String> = HashMap::new();
    let map_rows = map_stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to query project map: {e}")))?;
    for row in map_rows {
        let (sid, project) =
            row.map_err(|e| StorageError::QueryError(format!("Failed to parse project map: {e}")))?;
        session_project.insert(sid, project);
    }

    // Session counts per project (top-level sessions only), same marker.
    let mut sess_where = where_clause.clone();
    sess_where.push_str(&format!(
        " AND (COALESCE(json_extract(attributes, '{lbl_subagent}'), 'false') != 'true')",
        lbl_subagent = lbl::IS_SUBAGENT
    ));
    let sess_sql = format!(
        "SELECT COALESCE(json_extract(attributes, '{lbl_project}'), ?{sentinel_idx}), \
                COUNT(DISTINCT json_extract(attributes, '$.\"session.id\"')) \
         FROM metrics {sess_where} \
         GROUP BY 1",
        lbl_project = lbl::PROJECT_ID,
    );
    let sess_params = make_params();
    let sess_refs: Vec<&dyn rusqlite::ToSql> = sess_params.iter().map(|p| p.as_ref()).collect();
    let mut sess_stmt = conn.prepare(&sess_sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare project sessions: {e}"))
    })?;
    let sess_rows = sess_stmt
        .query_map(sess_refs.as_slice(), |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to query project sessions: {e}")))?;
    for row in sess_rows {
        let (project, n) = row.map_err(|e| {
            StorageError::QueryError(format!("Failed to parse project sessions: {e}"))
        })?;
        acc(&mut projects, &project).sessions += n.max(0) as u64;
    }

    // Token deltas: the #125 per-row clamped pass (series key includes
    // session.id, so per-project attribution cannot cross sessions).
    let rows = opencode_usage_rows(conn, start_time, end_time, &GenAiFilters::default())?;
    let baselines = opencode_usage_baselines(conn, &rows, start_time)?;
    for (labels, _ts, delta) in opencode_counter_deltas(rows, &baselines) {
        let session = labels.get(3).and_then(|s| s.as_deref());
        let project = session
            .and_then(|s| session_project.get(s))
            .map(|p| p.as_str())
            .unwrap_or(UNATTRIBUTED);
        let model = labels
            .get(1)
            .and_then(|m| m.as_deref())
            .unwrap_or(UNKNOWN_MODEL);
        let kind = labels.get(2).and_then(|k| k.as_deref());
        let tokens = delta as u64;
        let p = acc(&mut projects, project);
        let apply = |t: &mut AgentTokenUsage| match kind {
            Some(k) if k == otypes::INPUT => t.input += tokens,
            Some(k) if k == otypes::OUTPUT => t.output += tokens,
            Some(k) if k == otypes::REASONING => t.reasoning += tokens,
            Some(k) if k == otypes::CACHE_READ => t.cache_read += tokens,
            Some(k) if k == otypes::CACHE_WRITE => t.cache_write += tokens,
            _ => {},
        };
        apply(p.models.entry(model.to_string()).or_default());
    }

    // Cost: per-session cumulative counter, attributed via the same map.
    // The unattributed bucket's counter covers only label-less opencode
    // sessions — disjoint from the codex/claude tokens it will also hold.
    let cost_deltas = counter_window_deltas_value(
        conn,
        mnames::OPENCODE_SESSION_COST_TOTAL,
        &[lbl::SESSION_ID],
        HISTOGRAM_SUM_VALUE_SQL,
        start_time,
        end_time,
    )?;
    for d in &cost_deltas {
        let sid = d.labels.first().and_then(|s| s.as_deref());
        let project = sid
            .and_then(|s| session_project.get(s))
            .map(|p| p.as_str())
            .unwrap_or(UNATTRIBUTED);
        let delta = d.delta.max(0.0);
        let p = acc(&mut projects, project);
        p.has_counter = true;
        p.counter_cost += delta;
    }

    // ── unattributed: fold in codex + claude (no project label) ─────────
    // Reuse the agent rollup's verified codex/claude passes; discard the
    // per-bucket series (this endpoint has none).
    let unattr_agent_rollups = query_agent_rollup(conn, start_time, end_time, 1)?;
    let mut unattr_models: HashMap<String, AgentTokenUsage> = HashMap::new();
    let mut unattr_sessions: u64 = 0;
    for a in unattr_agent_rollups {
        if a.agent != anames::CODEX && a.agent != anames::CLAUDE {
            continue;
        }
        unattr_sessions += a.sessions;
        for (model, tokens) in a.models {
            unattr_models.entry(model).or_default().fold_tokens(tokens);
        }
    }
    if !unattr_models.is_empty() || unattr_sessions > 0 {
        let p = acc(&mut projects, UNATTRIBUTED);
        p.sessions += unattr_sessions;
        p.counter_disjoint = true;
        for (model, tokens) in unattr_models {
            let m = p.models.entry(model).or_default();
            m.fold_tokens(tokens);
        }
    }

    let out: Vec<ProjectRollupStorage> = projects
        .into_iter()
        .map(|(project_id, p)| ProjectRollupStorage {
            counter_cost_usd: p.has_counter.then_some(p.counter_cost),
            counter_disjoint_from_tokens: p.counter_disjoint,
            project_id,
            sessions: p.sessions,
            models: p.models.into_iter().collect(),
        })
        .collect();
    Ok(out)
}

/// Fold one bucketed (model, token_type) total into per-model and
/// per-bucket-per-model token accumulators.
#[allow(clippy::too_many_arguments)]
fn add_turn_tokens(
    models: &mut HashMap<String, otelite_core::api::AgentTokenUsage>,
    series: &mut HashMap<i64, HashMap<String, otelite_core::api::AgentTokenUsage>>,
    model: Option<String>,
    token_type: Option<&str>,
    bucket: i64,
    sum: f64,
) {
    use otelite_core::api::AgentTokenUsage;
    use otelite_core::semconv::codex_token_types as ctt;
    use otelite_core::semconv::opencode_token_types as otypes;

    let tokens = sum.round().max(0.0) as u64;
    if tokens == 0 {
        return;
    }
    // Codex and the opencode/claude harnesses use different vocabularies for
    // the same five categories; match both. codex `total` matches neither
    // set and is deliberately skipped (double-counting).
    let apply = |acc: &mut AgentTokenUsage| match token_type {
        Some(t) if t == otypes::INPUT || t == ctt::INPUT => acc.input += tokens,
        Some(t) if t == otypes::OUTPUT || t == ctt::OUTPUT => acc.output += tokens,
        Some(t) if t == otypes::REASONING || t == ctt::REASONING => acc.reasoning += tokens,
        Some(t) if t == otypes::CACHE_READ || t == ctt::CACHE_READ => acc.cache_read += tokens,
        Some(t) if t == otypes::CACHE_WRITE || t == ctt::CACHE_WRITE => acc.cache_write += tokens,
        _ => {},
    };
    let model = model.unwrap_or_else(|| "(unknown)".to_string());
    apply(models.entry(model.clone()).or_default());
    apply(series.entry(bucket).or_default().entry(model).or_default());
}

/// Distinct `session.id` values in the window for one marker metric. When
/// `subagent_label` is given, rows whose label is the string "true" are
/// excluded (opencode sub-agent sessions have their own ids and are not
/// user sessions).
fn distinct_session_ids(
    conn: &Connection,
    metric_name: &str,
    start_time: Option<i64>,
    end_time: Option<i64>,
    subagent_label: Option<&str>,
) -> Result<u64> {
    let sid_expr =
        "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END";
    let mut where_clause = format!("WHERE name = ?1 AND {sid_expr} IS NOT NULL");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(metric_name.to_string())];
    if let Some(sa) = subagent_label {
        let sa_expr =
            format!("CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{sa}') END");
        where_clause.push_str(&format!(" AND (COALESCE({sa_expr}, 'false') != 'true')"));
    }
    if let Some(start) = start_time {
        where_clause.push_str(&format!(" AND timestamp >= ?{}", params.len() + 1));
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
        params.push(Box::new(end));
    }
    let sql = format!("SELECT COUNT(DISTINCT {sid_expr}) FROM metrics {where_clause}");
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    conn.query_row(&sql, refs.as_slice(), |r| r.get::<_, i64>(0))
        .map(|n| n.max(0) as u64)
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to count {metric_name} sessions: {e}"))
        })
}

/// Codex per-event totals: cli thread starts (sessions), tool calls and
/// failed API requests (retries), one windowed pass each.
fn codex_event_totals(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<(u64, u64, u64)> {
    use otelite_core::semconv::metric_labels as lbl;
    use otelite_core::semconv::metric_names as mnames;

    let (start, end) = match (start_time, end_time) {
        (Some(s), Some(e)) => (s, e),
        _ => {
            return Err(StorageError::QueryError(
                "codex rollup requires both start_time and end_time".to_string(),
            ))
        },
    };

    let ss_expr = format!(
        "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END",
        lbl::SESSION_SOURCE
    );
    let sessions: i64 = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM(COALESCE(value_int, CAST(value_double AS INTEGER))), 0) \
                 FROM metrics WHERE name = ?1 AND timestamp >= ?2 AND timestamp <= ?3 \
                 AND {ss_expr} = 'cli'"
            ),
            rusqlite::params![mnames::CODEX_THREAD_STARTED.to_string(), start, end],
            |r| r.get(0),
        )
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to count codex thread starts: {e}"))
        })?;

    let tool_calls: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(COALESCE(value_int, CAST(value_double AS INTEGER))), 0) \
             FROM metrics WHERE name = ?1 AND timestamp >= ?2 AND timestamp <= ?3",
            rusqlite::params![mnames::CODEX_TOOL_CALL.to_string(), start, end],
            |r| r.get(0),
        )
        .map_err(|e| StorageError::QueryError(format!("Failed to count codex tool calls: {e}")))?;

    let ok_expr = format!(
        "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{}') END",
        lbl::SUCCESS
    );
    let retries: i64 = conn
        .query_row(
            &format!(
                "SELECT COALESCE(SUM(COALESCE(value_int, CAST(value_double AS INTEGER))), 0) \
                 FROM metrics WHERE name = ?1 AND timestamp >= ?2 AND timestamp <= ?3 \
                 AND {ok_expr} = 'false'"
            ),
            rusqlite::params![mnames::CODEX_API_REQUEST.to_string(), start, end],
            |r| r.get(0),
        )
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to count codex request failures: {e}"))
        })?;

    Ok((
        sessions.max(0) as u64,
        tool_calls.max(0) as u64,
        retries.max(0) as u64,
    ))
}

/// Append one agent's rollup when it has anything to report (an agent with
/// zero sessions, tokens, calls and retries in the window is omitted rather
/// than listed as a zero row).
#[allow(clippy::too_many_arguments)]
fn push_agent(
    out: &mut Vec<otelite_core::api::AgentRollupStorage>,
    agent: &str,
    sessions: u64,
    counter_cost_usd: Option<f64>,
    models: HashMap<String, otelite_core::api::AgentTokenUsage>,
    series: HashMap<i64, HashMap<String, otelite_core::api::AgentTokenUsage>>,
    tool_calls: u64,
    retries: Option<u64>,
) {
    let total_tokens: u64 = models.values().map(|t| t.total()).sum();
    if sessions == 0 && total_tokens == 0 && tool_calls == 0 && retries.is_none_or(|r| r == 0) {
        return;
    }
    let series: Vec<(i64, Vec<(String, otelite_core::api::AgentTokenUsage)>)> = series
        .into_iter()
        .map(|(ts, per_model)| (ts, per_model.into_iter().collect()))
        .collect();
    out.push(otelite_core::api::AgentRollupStorage {
        agent: agent.to_string(),
        sessions,
        tool_calls,
        retries,
        counter_cost_usd,
        models: models.into_iter().collect(),
        series,
    });
}

/// Per-session costs (opencode + claude) over the window. Codex is
/// deliberately absent: its metrics carry no per-session identifier.
pub fn query_session_costs(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<Vec<otelite_core::api::SessionCostStorage>> {
    use otelite_core::api::SessionCostStorage;

    let mut out: Vec<SessionCostStorage> = Vec::new();
    out.extend(opencode_session_costs(conn, start_time, end_time)?);
    out.extend(claude_session_costs(conn, start_time, end_time)?);
    Ok(out)
}

/// Opencode per-session values from its three cumulative session metrics:
/// each row re-emits the running session total, so a session's value is the
/// last row for that session in the window. Row volumes are small on the
/// live DB (~10k rows/name/day), so a name-indexed window fetch plus an
/// in-memory merge suffices — no per-session index needed.
fn opencode_session_costs(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<Vec<otelite_core::api::SessionCostStorage>> {
    use otelite_core::api::SessionCostStorage;
    use otelite_core::semconv::agent_names as anames;
    use otelite_core::semconv::metric_names as mnames;

    let last_cost = last_hist_sum_per_session(
        conn,
        mnames::OPENCODE_SESSION_COST_TOTAL,
        start_time,
        end_time,
    )?;
    let last_duration_ms = last_hist_sum_per_session(
        conn,
        mnames::OPENCODE_SESSION_DURATION,
        start_time,
        end_time,
    )?;
    let last_tokens = last_hist_sum_per_session(
        conn,
        mnames::OPENCODE_SESSION_TOKEN_TOTAL,
        start_time,
        end_time,
    )?;
    // `project.id` is a flat attribute key, so the JSON path needs the
    // quoted form.
    let last_project = last_attr_per_session(
        conn,
        mnames::OPENCODE_SESSION_COST_TOTAL,
        "$.\"project.id\"",
        start_time,
        end_time,
    )?;

    let mut session_ids: BTreeSet<String> = BTreeSet::new();
    for map in [&last_cost, &last_duration_ms, &last_tokens] {
        session_ids.extend(map.keys().cloned());
    }

    Ok(session_ids
        .into_iter()
        .map(|sid| {
            let tokens = last_tokens
                .get(&sid)
                .map(|v| v.round().max(0.0) as u64)
                .unwrap_or(0);
            SessionCostStorage {
                agent: anames::OPENCODE.to_string(),
                session_id: sid.clone(),
                project_id: last_project.get(&sid).cloned(),
                counter_cost_usd: last_cost.get(&sid).copied(),
                tokens,
                // Priced from the counter, not from tokens: the session
                // metrics carry no per-model split.
                models: Vec::new(),
                duration_secs: last_duration_ms.get(&sid).map(|ms| ms / 1000.0),
            }
        })
        .collect())
}

/// Last histogram sum ($[1]) per `session.id` in the window for one metric.
/// Rows whose session has no in-window row are absent; a malformed
/// histogram value never overrides an earlier valid one.
fn last_hist_sum_per_session(
    conn: &Connection,
    metric_name: &str,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<HashMap<String, f64>> {
    let (sql, params) =
        session_window_fetch(metric_name, start_time, end_time, HISTOGRAM_SUM_FIELD_SQL)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to read {metric_name} sessions: {e}"))
    })?;
    let rows = stmt
        .query_map(refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<f64>>(2)?,
            ))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to read {metric_name} sessions: {e}"))
        })?;
    let mut out: HashMap<String, (i64, f64)> = HashMap::new();
    for row in rows {
        let (sid, ts, value) = row.map_err(|e| {
            StorageError::QueryError(format!("Failed to read {metric_name} sessions: {e}"))
        })?;
        if let Some(v) = value {
            let entry = out.entry(sid).or_insert((i64::MIN, 0.0));
            if ts >= entry.0 {
                *entry = (ts, v);
            }
        }
    }
    Ok(out.into_iter().map(|(k, (_, v))| (k, v)).collect())
}

/// Last non-null attribute value per `session.id` in the window (used for
/// the optional `project.id` label).
fn last_attr_per_session(
    conn: &Connection,
    metric_name: &str,
    attr_path: &str,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<HashMap<String, String>> {
    let val_expr = format!(
        "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '{attr_path}') END"
    );
    let (sql, params) = session_window_fetch(metric_name, start_time, end_time, &val_expr)?;
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to read {metric_name} sessions: {e}"))
    })?;
    let rows = stmt
        .query_map(refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to read {metric_name} sessions: {e}"))
        })?;
    let mut out: HashMap<String, (i64, String)> = HashMap::new();
    for row in rows {
        let (sid, ts, value) = row.map_err(|e| {
            StorageError::QueryError(format!("Failed to read {metric_name} sessions: {e}"))
        })?;
        if let Some(v) = value {
            let entry = out.entry(sid).or_insert((i64::MIN, String::new()));
            if ts >= entry.0 {
                *entry = (ts, v);
            }
        }
    }
    Ok(out.into_iter().map(|(k, (_, v))| (k, v)).collect())
}

const HISTOGRAM_SUM_FIELD_SQL: &str =
    "CASE WHEN json_valid(value_histogram) THEN CAST(json_extract(value_histogram, '$[1]') AS REAL) END";

/// WHERE clause + params for a name-indexed metrics window fetch keyed on
/// `session.id`, shared by the per-session last-value helpers.
fn session_window_fetch(
    metric_name: &str,
    start_time: Option<i64>,
    end_time: Option<i64>,
    value_sql: &str,
) -> Result<(String, Vec<Box<dyn rusqlite::ToSql>>)> {
    let sid_expr =
        "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END";
    let mut where_clause = format!("WHERE name = ?1 AND {sid_expr} IS NOT NULL");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(metric_name.to_string())];
    if let Some(start) = start_time {
        where_clause.push_str(&format!(" AND timestamp >= ?{}", params.len() + 1));
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
        params.push(Box::new(end));
    }
    let sql = format!("SELECT {sid_expr}, timestamp, {value_sql} FROM metrics {where_clause}");
    Ok((sql, params))
}

/// Claude per-session costs from `claude_code.llm_request` span attributes:
/// token counts summed per (session, model), duration as first-to-last
/// request span time. The token data comes from the spans rather than the
/// `claude_code.cost.usage` counter, which under-reports on the live data
/// (~$19 counter vs ~$570 tokens×pricing over 7 days). Spans carry no
/// reasoning tokens, so claude session token totals exclude thinking
/// tokens (the agent rollup includes them via the token.usage metric).
fn claude_session_costs(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<Vec<otelite_core::api::SessionCostStorage>> {
    use otelite_core::api::{AgentTokenUsage, SessionCostStorage};
    use otelite_core::semconv::agent_names as anames;
    use otelite_core::semconv::LLM_REQUEST_SPAN_NAME;

    let sid_expr =
        "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END";
    // `gen_ai.request.model` is a flat attribute key → quoted JSON path.
    let model_expr = "CASE WHEN json_valid(attributes) THEN COALESCE(\
        json_extract(attributes, '$.model'), \
        json_extract(attributes, '$.\"gen_ai.request.model\"')) END";
    let tok =
        |key: &str| format!("COALESCE(CAST(json_extract(attributes, '$.{key}') AS INTEGER), 0)");

    let mut where_clause = format!("WHERE name = ?1 AND {sid_expr} IS NOT NULL");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(LLM_REQUEST_SPAN_NAME.to_string())];
    if let Some(start) = start_time {
        where_clause.push_str(&format!(" AND start_time >= ?{}", params.len() + 1));
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(&format!(" AND start_time <= ?{}", params.len() + 1));
        params.push(Box::new(end));
    }
    let sql = format!(
        "SELECT {sid_expr}, {model_expr}, {}, {}, {}, {}, start_time \
         FROM spans {where_clause}",
        tok("input_tokens"),
        tok("output_tokens"),
        tok("cache_read_tokens"),
        tok("cache_creation_tokens"),
    );
    let refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to read claude session spans: {e}"))
    })?;
    let rows = stmt
        .query_map(refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, i64>(5)?,
                r.get::<_, i64>(6)?,
            ))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to read claude session spans: {e}"))
        })?;

    struct Acc {
        models: BTreeMap<String, AgentTokenUsage>,
        min_ts: i64,
        max_ts: i64,
    }
    let mut sessions: BTreeMap<String, Acc> = BTreeMap::new();
    for row in rows {
        let (sid, model, input, output, cache_read, cache_write, ts) = row.map_err(|e| {
            StorageError::QueryError(format!("Failed to read claude session spans: {e}"))
        })?;
        let acc = sessions.entry(sid).or_insert_with(|| Acc {
            models: BTreeMap::new(),
            min_ts: i64::MAX,
            max_ts: i64::MIN,
        });
        acc.min_ts = acc.min_ts.min(ts);
        acc.max_ts = acc.max_ts.max(ts);
        let m = acc
            .models
            .entry(model.unwrap_or_else(|| "(unknown)".to_string()))
            .or_default();
        m.input += input as u64;
        m.output += output as u64;
        m.cache_read += cache_read as u64;
        m.cache_write += cache_write as u64;
    }

    Ok(sessions
        .into_iter()
        .map(|(sid, acc)| {
            let tokens: u64 = acc.models.values().map(|t| t.total()).sum::<u64>();
            let duration_secs = (acc.max_ts > acc.min_ts)
                .then_some((acc.max_ts - acc.min_ts) as f64 / 1_000_000_000.0);
            SessionCostStorage {
                agent: anames::CLAUDE.to_string(),
                session_id: sid,
                project_id: None,
                counter_cost_usd: None,
                tokens,
                models: acc.models.into_iter().collect(),
                duration_secs,
            }
        })
        .collect())
}

/// Cache hit rate: `cache_read / (cache_read + input)`, `None` when the
/// denominator is 0 (no prompt tokens at all in the window).
fn cache_hit_rate(cache_read: u64, input: u64) -> Option<f64> {
    let denom = cache_read + input;
    if denom == 0 {
        None
    } else {
        Some(cache_read as f64 / denom as f64)
    }
}

/// Reasoning share of output tokens, in percent: `reasoning / output × 100`,
/// `None` when there were no output tokens (the share is undefined, not 0).
fn reasoning_share_pct(reasoning: u64, output: u64) -> Option<f64> {
    if output == 0 {
        None
    } else {
        Some(reasoning as f64 / output as f64 * 100.0)
    }
}

/// Read:write ratio: `cache_read / cache_write`, `None` when there were no
/// cache writes (an infinite ratio is not a useful number to surface).
fn cache_read_write_ratio(cache_read: u64, cache_write: u64) -> Option<f64> {
    if cache_write == 0 {
        None
    } else {
        Some(cache_read as f64 / cache_write as f64)
    }
}

/// Add `v` tokens of category `kind` (an `opencode.token.usage` `type`
/// label) to a token-usage accumulator. Unknown categories are ignored, not
/// misfiled.
fn add_opencode_tokens(t: &mut otelite_core::api::RoleTokenUsage, kind: Option<&str>, v: u64) {
    use otelite_core::semconv::opencode_token_types as ttypes;
    match kind {
        Some(k) if k == ttypes::INPUT => t.input += v,
        Some(k) if k == ttypes::OUTPUT => t.output += v,
        Some(k) if k == ttypes::CACHE_READ => t.cache_read += v,
        Some(k) if k == ttypes::CACHE_WRITE => t.cache_write += v,
        Some(k) if k == ttypes::REASONING => t.reasoning += v,
        _ => {}, // unknown token types are ignored, not misfiled
    }
}

/// Split a `total` across weighted buckets using largest-remainder
/// apportionment so the parts sum exactly to `total`. Buckets keep their
/// input order; a zero total weight yields all-zero buckets.
fn largest_remainder_split(total: u64, weights: &[(String, u64)]) -> Vec<(String, u64)> {
    if weights.is_empty() {
        return Vec::new();
    }
    let total_weight: u64 = weights.iter().map(|(_, w)| *w).sum();
    if total_weight == 0 {
        return weights.iter().map(|(p, _)| (p.clone(), 0)).collect();
    }
    let exact: Vec<f64> = weights
        .iter()
        .map(|(_, w)| total as f64 * (*w as f64) / (total_weight as f64))
        .collect();
    let mut out: Vec<(String, u64)> = weights
        .iter()
        .zip(exact.iter())
        .map(|((p, _), e)| (p.clone(), e.floor() as u64))
        .collect();
    let mut remainder = total - out.iter().map(|(_, v)| *v).sum::<u64>();
    // Rank buckets by fractional part, largest first, for the leftover units.
    let mut order: Vec<usize> = (0..exact.len()).collect();
    order.sort_by(|&a, &b| {
        let fa = exact[a] - exact[a].floor();
        let fb = exact[b] - exact[b].floor();
        fb.partial_cmp(&fa).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut i = 0;
    while remainder > 0 && !order.is_empty() {
        out[order[i % order.len()]].1 += 1;
        remainder -= 1;
        i += 1;
    }
    out
}

/// Attribute one model's token usage and (optional) cost to its providers by
/// weight (telemetry-row counts). A single provider keeps everything
/// ("direct"); several providers split proportionally ("token-share-split").
fn attribute_model_to_providers(
    tokens: otelite_core::api::RoleTokenUsage,
    cost: Option<f64>,
    providers: &[(String, u64)],
) -> Vec<(String, otelite_core::api::RoleTokenUsage, Option<f64>)> {
    use otelite_core::api::RoleTokenUsage;
    if providers.is_empty() {
        return Vec::new();
    }
    let split_each =
        |pick: fn(&RoleTokenUsage) -> u64| largest_remainder_split(pick(&tokens), providers);
    let inputs = split_each(|t| t.input);
    let outputs = split_each(|t| t.output);
    let cache_reads = split_each(|t| t.cache_read);
    let cache_writes = split_each(|t| t.cache_write);
    let reasonings = split_each(|t| t.reasoning);
    let total_weight: u64 = providers.iter().map(|(_, w)| *w).sum();
    providers
        .iter()
        .enumerate()
        .map(|(i, (provider, w))| {
            let t = RoleTokenUsage {
                input: inputs[i].1,
                output: outputs[i].1,
                cache_read: cache_reads[i].1,
                cache_write: cache_writes[i].1,
                reasoning: reasonings[i].1,
            };
            let c = cost.map(|c| {
                if total_weight == 0 {
                    0.0
                } else {
                    c * (*w as f64) / (total_weight as f64)
                }
            });
            (provider.clone(), t, c)
        })
        .collect()
}

/// Session context (issue #134): spans, logs and aggregated metrics for one
/// session id over the window.
///
/// Session addressing per harness (verified against the live DB):
/// - claude: `session.id` on all `claude_code.*` spans and on the
///   `com.anthropic.claude_code.events` logs; claude emits no metrics.
/// - opencode: `session.id` on llm/tool spans, `com.opencode` logs (100%)
///   and all `opencode.*` metrics.
/// - codex: `session.id` on `mcp.tools.call` spans only; `conversation.id`
///   on `codex_otel.log_only` logs (100%); codex metrics carry
///   `session_source` (cli/exec/subagent_*), which is NOT a session id —
///   codex has no per-session metrics.
///
/// Spans and logs are truncated to `limit` (true counts in `*_total`);
/// metrics are aggregated per name in SQL (count/sum/min/max, first/last
/// ts) — never raw-dumped, since an opencode session carries hundreds of
/// thousands of points. Returns `None` when the session has no data in any
/// of the three stores (the API maps that to 404).
#[allow(clippy::too_many_arguments)]
pub fn query_session_context(
    conn: &Connection,
    session_id: &str,
    start_time: Option<i64>,
    end_time: Option<i64>,
    limit: u64,
) -> Result<Option<otelite_core::api::SessionContextResponse>> {
    use otelite_core::api::{
        SessionContextLog, SessionContextMetric, SessionContextResponse, SessionContextSession,
        SessionContextSpan, SessionContextTimelineEvent,
    };

    let sess_pred = format!(
        "json_extract(attributes, '$.\"{key}\"') = ?1",
        key = semconv::SESSION_ID_KEY
    );
    // Codex logs address the session via conversation.id (verified: 100% of
    // codex_otel.log_only logs carry it; session.id is absent there).
    // Both predicates share the single ?1 binding below.
    let conv_pred = "json_extract(attributes, '$.\"conversation.id\"') = ?1".to_string();
    let window = |col: &str, q: &mut String, p: &mut Vec<Box<dyn rusqlite::ToSql>>| {
        if let Some(start) = start_time {
            q.push_str(&format!(" AND {col} >= ?"));
            p.push(Box::new(start));
        }
        if let Some(end) = end_time {
            q.push_str(&format!(" AND {col} <= ?"));
            p.push(Box::new(end));
        }
    };

    // ── spans (seek idx_spans_session_id; the partial-index predicate is a
    // required conjunct for the planner, semantically implied by the eq) ──
    let sess_expr = semconv::session_id_expr("attributes");
    let mut q = format!(
        "SELECT trace_id, span_id, name, start_time, end_time, \
         CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"model\"') END \
         FROM spans WHERE {expr} = ?1 AND {pred} AND json_valid(attributes)",
        expr = sess_expr,
        pred = semconv::session_id_index_predicate("attributes")
    );
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string())];
    window("start_time", &mut q, &mut p);
    q.push_str(" ORDER BY start_time LIMIT ?");
    p.push(Box::new(limit + 1));
    let mut stmt = conn
        .prepare(&q)
        .map_err(|e| StorageError::QueryError(format!("Failed to query session spans: {e}")))?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
    let spans: Vec<(String, String, String, i64, i64, Option<String>)> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)?,
                r.get::<_, Option<String>>(5)?,
            ))
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to read session spans: {e}")))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse session spans: {e}")))?;
    // Totals count the queried scope: the window, when one is given
    // (bounded + index-friendly), else the whole session. A windowed
    // request must not pay for a full-history count.
    let mut q = format!(
        "SELECT COUNT(*) FROM spans WHERE {expr} = ?1 AND {pred} AND json_valid(attributes)",
        expr = sess_expr,
        pred = semconv::session_id_index_predicate("attributes")
    );
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string())];
    window("start_time", &mut q, &mut p);
    let param_refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
    let spans_total: i64 = conn
        .query_row(&q, param_refs.as_slice(), |r| r.get(0))
        .map_err(|e| StorageError::QueryError(format!("Failed to count session spans: {e}")))?;

    // ── logs (window-bounded scan; no session index on logs) ──
    let mut q = format!(
        "SELECT timestamp, severity_number, body FROM logs \
         WHERE json_valid(attributes) AND ({sess} OR {conv})",
        sess = sess_pred,
        conv = conv_pred
    );
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string())];
    window("timestamp", &mut q, &mut p);
    q.push_str(" ORDER BY timestamp LIMIT ?");
    p.push(Box::new(limit + 1));
    let mut stmt = conn
        .prepare(&q)
        .map_err(|e| StorageError::QueryError(format!("Failed to query session logs: {e}")))?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
    let logs: Vec<(i64, Option<i32>, String)> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<i32>>(1)?,
                r.get::<_, String>(2)?,
            ))
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to read session logs: {e}")))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse session logs: {e}")))?;
    let mut q = format!(
        "SELECT COUNT(*) FROM logs WHERE json_valid(attributes) AND ({sess} OR {conv})",
        sess = sess_pred,
        conv = conv_pred
    );
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string())];
    window("timestamp", &mut q, &mut p);
    let param_refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
    let logs_total: i64 = conn
        .query_row(&q, param_refs.as_slice(), |r| r.get(0))
        .map_err(|e| StorageError::QueryError(format!("Failed to count session logs: {e}")))?;

    // ── metrics (aggregated in SQL; session.id addressing) ──
    // CASE-wrapped equality, matching the span index expression so the
    // predicate reads the same as `session_id_expr` elsewhere. (A bare
    // `json_extract(...) = ?` is equivalent — `NULL = ?` is never true —
    // but the shared form keeps the two stores' session addressing
    // visually identical; the regression test pins that unlabelled
    // metric rows stay out.)
    // project.id rides along as the 10th aggregate column: it is a
    // per-session constant on opencode metrics, so MAX() over the rows
    // already scanned costs nothing — a separate lookup would mean a
    // second full-table scan of the metrics table.
    let mut q = format!(
        "SELECT name, unit, metric_type AS mtype, COUNT(*), \
         SUM(COALESCE(value_double, CAST(value_int AS REAL))), \
         MIN(COALESCE(value_double, CAST(value_int AS REAL))), \
         MAX(COALESCE(value_double, CAST(value_int AS REAL))), \
         MIN(timestamp), MAX(timestamp), \
         MAX(CASE WHEN json_extract(attributes, '$.\"project.id\"') IS NOT NULL \
                 THEN json_extract(attributes, '$.\"project.id\"') END) \
         FROM metrics WHERE {expr} IS NOT NULL AND {expr} = ?",
        expr = semconv::session_id_expr("attributes")
    );
    let mut p: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(session_id.to_string())];
    window("timestamp", &mut q, &mut p);
    q.push_str(" GROUP BY name, unit, metric_type ORDER BY name");
    let mut stmt = conn
        .prepare(&q)
        .map_err(|e| StorageError::QueryError(format!("Failed to query session metrics: {e}")))?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = p.iter().map(|b| b.as_ref()).collect();
    // (name, unit, metric_type, count, sum, min, max, first_ts, last_ts)
    #[allow(clippy::type_complexity)]
    type MetricAggRow = (
        String,
        Option<String>,
        i64,
        i64,
        Option<f64>,
        Option<f64>,
        Option<f64>,
        i64,
        i64,
        Option<String>,
    );
    let metrics: Vec<MetricAggRow> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Option<f64>>(4)?,
                r.get::<_, Option<f64>>(5)?,
                r.get::<_, Option<f64>>(6)?,
                r.get::<_, i64>(7)?,
                r.get::<_, i64>(8)?,
                r.get::<_, Option<String>>(9)?,
            ))
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to read session metrics: {e}")))?
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse session metrics: {e}")))?;

    if spans.is_empty() && logs.is_empty() && metrics.is_empty() {
        return Ok(None);
    }

    // ── agent detection + span coverage ──
    let has_claude = spans.iter().any(|s| s.2.starts_with("claude_code."))
        || logs.iter().any(|l| l.2.starts_with("claude_code."));
    let has_opencode = spans.iter().any(|s| s.2.starts_with("opencode."))
        || metrics.iter().any(|m| m.0.starts_with("opencode."));
    let has_codex = logs.iter().any(|l| l.2.starts_with("codex_otel"))
        || metrics.iter().any(|m| m.0.starts_with("codex."));
    // NOTE: claude event logs put the event name in the body
    // ("claude_code.api_request"); opencode log bodies are free text, so
    // claude detection also checks span names (done above).
    let agent = if has_claude {
        Some("claude".to_string())
    } else if has_opencode {
        Some("opencode".to_string())
    } else if has_codex {
        Some("codex".to_string())
    } else {
        None
    };
    // Verified coverage: claude labels its whole span set ("full");
    // opencode labels only llm/tool spans and codex only mcp.tools.call
    // ("partial" for both).
    let span_coverage = match agent.as_deref() {
        Some("claude") => "full".to_string(),
        _ => "partial".to_string(),
    };
    // opencode carries project.id on its metrics; claude/codex don't.
    // (Scoped to opencode.* names so a foreign metric row for the same
    // session id can't leak another project label in.)
    let project_id = metrics
        .iter()
        .find(|m| m.0.starts_with("opencode."))
        .and_then(|m| m.9.clone());

    let spans_out: Vec<SessionContextSpan> = spans
        .iter()
        .take(limit as usize)
        .map(
            |(trace_id, span_id, name, start, end, model)| SessionContextSpan {
                trace_id: trace_id.clone(),
                span_id: span_id.clone(),
                name: name.clone(),
                start_time: *start,
                duration_ns: *end - *start,
                model: model.clone(),
            },
        )
        .collect();
    let logs_out: Vec<SessionContextLog> = logs
        .iter()
        .take(limit as usize)
        .map(|(ts, sev, body)| SessionContextLog {
            timestamp: *ts,
            severity: sev
                .and_then(SeverityLevel::from_i32)
                .map(|s| s.as_str().to_string()),
            body: body.chars().take(512).collect(),
        })
        .collect();
    let metrics_out: Vec<SessionContextMetric> = metrics
        .iter()
        .map(
            |(name, unit, mtype, count, sum, min, max, first_ts, last_ts, _project)| {
                SessionContextMetric {
                    name: name.clone(),
                    unit: unit.clone(),
                    metric_type: *mtype as u8,
                    count: *count as u64,
                    sum: *sum,
                    min: *min,
                    max: *max,
                    first_ts: *first_ts,
                    last_ts: *last_ts,
                }
            },
        )
        .collect();

    // ── timeline: spans + logs merged, ascending, capped at limit ──
    let mut events: Vec<(i64, u8, String)> = Vec::with_capacity(spans.len() + logs.len());
    for s in &spans_out {
        let label = match &s.model {
            Some(m) if !m.is_empty() => format!("{} {}", s.name, m),
            _ => s.name.clone(),
        };
        events.push((s.start_time, 0, label));
    }
    for l in &logs_out {
        let severity = l.severity.clone().unwrap_or_default();
        let label = if severity.is_empty() {
            l.body.chars().take(80).collect()
        } else {
            format!(
                "{} {}",
                l.body.chars().take(80).collect::<String>(),
                severity
            )
        };
        events.push((l.timestamp, 1, label));
    }
    events.sort_by_key(|(ts, ..)| *ts);
    let timeline: Vec<SessionContextTimelineEvent> = events
        .into_iter()
        .take(limit as usize)
        .map(|(ts, kind, label)| SessionContextTimelineEvent {
            ts,
            kind: if kind == 0 {
                "span".to_string()
            } else {
                "log".to_string()
            },
            label,
        })
        .collect();

    Ok(Some(SessionContextResponse {
        session: SessionContextSession {
            id: session_id.to_string(),
            agent,
            project_id,
            span_coverage,
        },
        spans: spans_out,
        spans_total: spans_total as u64,
        logs: logs_out,
        logs_total: logs_total as u64,
        metrics: metrics_out,
        timeline,
        // Per-session endpoint: the global filter bar is not applicable.
        filters_applied: Vec::new(),
    }))
}

/// Sub-agent role attribution (opencode `agent` label).
///
/// Tokens come from windowed deltas of `opencode.token.usage` (a cumulative
/// counter). Session and model presence come from `opencode.model.usage`
/// window rows (no counter math needed for presence). Cost is enriched by
/// the API layer from the pricing table: opencode's own `cost.usage`
/// counter is zero-valued in the wire data, so deriving cost from tokens is
/// the only source.
pub fn query_agent_roles(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::AgentRolesResponse> {
    use otelite_core::api::{
        AgentRoleBreakdown, AgentRolesResponse, RoleModelBreakdown, RoleTokenUsage,
    };
    use otelite_core::semconv::{metric_labels as lbl, metric_names as mnames};

    const ROLE_UNKNOWN: &str = "unknown";

    struct RoleAgg {
        tokens: RoleTokenUsage,
        sessions: std::collections::HashSet<String>,
        models: std::collections::HashMap<String, RoleTokenUsage>,
    }

    let token_deltas = counter_window_deltas(
        conn,
        mnames::OPENCODE_TOKEN_USAGE,
        &[lbl::AGENT, lbl::MODEL, lbl::TYPE, lbl::SESSION_ID],
        start_time,
        end_time,
    )?;

    let mut roles: HashMap<String, RoleAgg> = HashMap::new();
    for d in token_deltas {
        let role = d
            .labels
            .first()
            .and_then(|l| l.clone())
            .unwrap_or_else(|| ROLE_UNKNOWN.to_string());
        let model = d
            .labels
            .get(1)
            .and_then(|l| l.clone())
            .unwrap_or_else(|| ROLE_UNKNOWN.to_string());
        let kind = d.labels.get(2).and_then(|l| l.as_deref());
        let agg = roles.entry(role).or_insert_with(|| RoleAgg {
            tokens: RoleTokenUsage::default(),
            sessions: std::collections::HashSet::new(),
            models: std::collections::HashMap::new(),
        });
        add_opencode_tokens(&mut agg.tokens, kind, d.delta as u64);
        add_opencode_tokens(agg.models.entry(model).or_default(), kind, d.delta as u64);
    }

    // Session and model presence from opencode.model.usage window rows.
    // Presence only needs the name-indexed seek; labels are extracted from
    // the fetched rows.
    let mut where_clause = String::from("WHERE name = ?1");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(mnames::OPENCODE_MODEL_USAGE.to_string())];
    if let Some(start) = start_time {
        where_clause.push_str(" AND timestamp >= ?2");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
        params.push(Box::new(end));
    }
    // json_valid-gated (total) so a malformed attributes value can only
    // yield NULL, never an error, for a corrupted metrics row.
    let presence_sql = format!(
        "SELECT CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.agent') END, \
                 CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.model') END, \
                 CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END \
         FROM metrics {where_clause}",
        where_clause = where_clause
    );
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&presence_sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare agent_roles presence query: {e}"))
    })?;
    let presence_rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok((
                row.get::<_, Option<String>>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute agent_roles presence query: {e}"))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse agent_roles presence rows: {e}"))
        })?;
    for (role, model, sid) in presence_rows {
        let role = role.unwrap_or_else(|| ROLE_UNKNOWN.to_string());
        let agg = roles.entry(role).or_insert_with(|| RoleAgg {
            tokens: RoleTokenUsage::default(),
            sessions: std::collections::HashSet::new(),
            models: std::collections::HashMap::new(),
        });
        if let Some(sid) = sid {
            agg.sessions.insert(sid);
        }
        if let Some(model) = model {
            agg.models.entry(model).or_default();
        }
    }

    let total_tokens: u64 = roles.values().map(|a| a.tokens.total()).sum();
    let mut role_rows: Vec<AgentRoleBreakdown> = roles
        .into_iter()
        .map(|(role, agg)| {
            let mut models: Vec<(String, RoleTokenUsage)> = agg.models.into_iter().collect();
            models.sort_by_key(|a| std::cmp::Reverse(a.1.total()));
            let top_models = models
                .into_iter()
                .take(5)
                .map(|(model, tokens)| RoleModelBreakdown {
                    model,
                    tokens,
                    cost: None,
                    cost_source: None,
                    cost_reason: None,
                })
                .collect();
            let share_pct = if total_tokens > 0 {
                Some(agg.tokens.total() as f64 / total_tokens as f64 * 100.0)
            } else {
                None
            };
            AgentRoleBreakdown {
                role,
                tokens: agg.tokens,
                sessions: agg.sessions.len() as u64,
                cost: None,
                share_pct,
                top_models,
            }
        })
        .collect();
    role_rows.sort_by_key(|a| std::cmp::Reverse(a.tokens.total()));

    let unknown_share_pct = role_rows
        .iter()
        .find(|r| r.role == ROLE_UNKNOWN)
        .and_then(|r| r.share_pct);

    Ok(AgentRolesResponse {
        roles: role_rows,
        unknown_share_pct,
        agents_covered: vec!["opencode".to_string()],
        filters_applied: Vec::new(),
    })
}

/// Provider × model mix (tokens, sessions, estimated cost) over the window,
/// across the three agent harnesses:
///
/// - **opencode**: per-model windowed deltas of the cumulative
///   `opencode.token.usage` counter (same series + covering index as
///   [`query_agent_roles`]); provider and weight from `opencode.model.usage`
///   rows (a `model → provider` mapping that is 1:1 in practice).
/// - **codex**: per-turn sums of the `codex.turn.token_usage` histogram
///   (`value_histogram[1]`); the `total` category is the sum of the parts
///   and is never counted. Codex emits no provider attribute, so its models
///   are reported under "(unknown)" — never guessed.
/// - **claude_code**: `claude_code.llm_request` spans; provider is the
///   `gen_ai.system` attribute (again, only where it says so).
///
/// Each harness contributes through exactly one source, so no model is
/// counted twice. Cost is enriched by the API layer from the pricing table
/// (opencode's own `cost.usage` counter is zero-valued in the wire data).
/// A model's tokens/cost are attributed to each provider by that provider's
/// share of the model's telemetry rows ("direct" when one provider,
/// "token-share-split" when several).
pub fn query_provider_mix(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::ProviderMixResponse> {
    use otelite_core::api::{
        ProviderMixEntry, ProviderMixResponse, ProviderModelEntry, RoleTokenUsage,
    };
    use otelite_core::semconv::{
        codex_token_types as ctt, metric_labels as lbl, metric_names as mnames,
    };

    const PROVIDER_UNKNOWN: &str = "(unknown)";

    let mut model_tokens: HashMap<String, RoleTokenUsage> = HashMap::new();
    let mut model_sessions: HashMap<String, std::collections::HashSet<String>> = HashMap::new();
    // model -> [(provider, weight)] where weight = telemetry-row count.
    let mut model_providers: HashMap<String, Vec<(String, u64)>> = HashMap::new();

    // ── opencode: counter deltas per (agent, model, type, session.id) ──────
    let token_deltas = counter_window_deltas(
        conn,
        mnames::OPENCODE_TOKEN_USAGE,
        &[lbl::AGENT, lbl::MODEL, lbl::TYPE, lbl::SESSION_ID],
        start_time,
        end_time,
    )?;
    for d in token_deltas {
        let model = d
            .labels
            .get(1)
            .and_then(|l| l.clone())
            .unwrap_or_else(|| PROVIDER_UNKNOWN.to_string());
        let kind = d.labels.get(2).and_then(|l| l.as_deref());
        add_opencode_tokens(
            model_tokens.entry(model.clone()).or_default(),
            kind,
            d.delta as u64,
        );
        if let Some(sid) = d.labels.get(3).and_then(|l| l.clone()) {
            model_sessions.entry(model).or_default().insert(sid);
        }
    }

    // opencode: provider + weight from model.usage window rows.
    {
        let mut where_clause = String::from("WHERE name = ?1");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(mnames::OPENCODE_MODEL_USAGE.to_string())];
        if let Some(start) = start_time {
            where_clause.push_str(" AND timestamp >= ?2");
            params.push(Box::new(start));
        }
        if let Some(end) = end_time {
            where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
            params.push(Box::new(end));
        }
        let sql = format!(
            "SELECT CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.provider') END, \
                     CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.model') END \
             FROM metrics {where_clause}",
            where_clause = where_clause
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!(
                "Failed to prepare provider_mix opencode query: {e}"
            ))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                ))
            })
            .map_err(|e| {
                StorageError::QueryError(format!(
                    "Failed to execute provider_mix opencode query: {e}"
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::QueryError(format!("Failed to parse provider_mix opencode rows: {e}"))
            })?;
        for (provider, model) in rows {
            if let (Some(p), Some(m)) = (provider, model) {
                let entry = model_providers.entry(m).or_default();
                match entry.iter_mut().find(|(p2, _)| *p2 == p) {
                    Some((_, w)) => *w += 1,
                    None => entry.push((p, 1)),
                }
            }
        }
    }

    // ── codex: per-turn histogram sums per (model, token_type) ──────────────
    {
        let mut where_clause = String::from("WHERE name = ?1");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> =
            vec![Box::new(mnames::CODEX_TURN_TOKEN_USAGE.to_string())];
        if let Some(start) = start_time {
            where_clause.push_str(" AND timestamp >= ?2");
            params.push(Box::new(start));
        }
        if let Some(end) = end_time {
            where_clause.push_str(&format!(" AND timestamp <= ?{}", params.len() + 1));
            params.push(Box::new(end));
        }
        // json_valid-gated on both columns so a corrupt row yields NULL,
        // never an error.
        let sql = format!(
            "SELECT CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.model') END, \
                     CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.token_type') END, \
                     COALESCE(CASE WHEN json_valid(value_histogram) THEN json_extract(value_histogram, '$[1]') END, 0.0) \
             FROM metrics {where_clause}",
            where_clause = where_clause
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!("Failed to prepare provider_mix codex query: {e}"))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, f64>(2)?,
                ))
            })
            .map_err(|e| {
                StorageError::QueryError(format!("Failed to execute provider_mix codex query: {e}"))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::QueryError(format!("Failed to parse provider_mix codex rows: {e}"))
            })?;
        for (model, ttype, sum) in rows {
            let v = sum as u64;
            let Some(model) = model else {
                continue;
            };
            let acc = model_tokens.entry(model).or_default();
            // "total" is the sum of the other categories: skip it.
            match ttype.as_deref() {
                Some(t) if t == ctt::INPUT => acc.input += v,
                Some(t) if t == ctt::OUTPUT => acc.output += v,
                Some(t) if t == ctt::REASONING => acc.reasoning += v,
                Some(t) if t == ctt::CACHE_READ => acc.cache_read += v,
                Some(t) if t == ctt::CACHE_WRITE => acc.cache_write += v,
                _ => {},
            }
        }
    }

    // ── claude_code: llm_request spans per (model, system) ──────────────────
    {
        let exprs = token_exprs();
        // json_valid conjunct: the token/model/system expressions below are
        // plain json_extract (not total), so a corrupt attributes value must
        // be excluded here rather than raise mid-query. Such rows carry no
        // readable model/system/tokens anyway.
        let mut where_clause = format!(
            "WHERE name = '{}' AND json_valid(attributes)",
            otelite_core::semconv::LLM_REQUEST_SPAN_NAME
        );
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(s) = start_time {
            where_clause.push_str(" AND start_time >= ?");
            params.push(Box::new(s));
        }
        if let Some(e) = end_time {
            where_clause.push_str(" AND end_time <= ?");
            params.push(Box::new(e));
        }
        let sql = format!(
            "SELECT {model} AS model, {system} AS system, \
                     {session_id} AS session_id, \
                     COALESCE(SUM({input}), 0)  AS input_tokens, \
                     COALESCE(SUM({output}), 0) AS output_tokens, \
                     COALESCE(SUM({cache_creation}), 0) AS cache_creation_tokens, \
                     COALESCE(SUM({cache_read}), 0) AS cache_read_tokens, \
                     COUNT(*) AS calls \
             FROM spans \
             {where_clause} \
             GROUP BY model, system, session_id",
            // Inner label stays the raw model: the provider is the outer
            // dimension of this view, so the composite identity would be
            // redundant here.
            model = exprs.model,
            system = exprs.system,
            session_id = semconv::session_id_expr("attributes"),
            input = exprs.input,
            output = exprs.output,
            cache_creation = exprs.cache_creation,
            cache_read = exprs.cache_read,
            where_clause = where_clause,
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!("Failed to prepare provider_mix claude query: {e}"))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, i64>(3)? as u64,
                    row.get::<_, i64>(4)? as u64,
                    row.get::<_, i64>(5)? as u64,
                    row.get::<_, i64>(6)? as u64,
                    row.get::<_, i64>(7)? as u64,
                ))
            })
            .map_err(|e| {
                StorageError::QueryError(format!(
                    "Failed to execute provider_mix claude query: {e}"
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                StorageError::QueryError(format!("Failed to parse provider_mix claude rows: {e}"))
            })?;
        for (model, system, sid, input, output, cache_creation, cache_read, calls) in rows {
            let Some(model) = model else {
                continue;
            };
            let acc = model_tokens.entry(model.clone()).or_default();
            acc.input += input;
            acc.output += output;
            acc.cache_write += cache_creation;
            acc.cache_read += cache_read;
            if let Some(system) = system {
                let entry = model_providers.entry(model.clone()).or_default();
                match entry.iter_mut().find(|(p, _)| *p == system) {
                    Some((_, w)) => *w += calls,
                    None => entry.push((system, calls)),
                }
            }
            if let Some(sid) = sid {
                model_sessions.entry(model).or_default().insert(sid);
            }
        }
    }

    // ── assemble provider × model rows ──────────────────────────────────────
    // A model's tokens are attributed to its providers by weight. Cost is
    // linear in tokens (tokens × pricing), so computing cost per attributed
    // (provider, model) row in the API layer is exactly the cost split — no
    // separate cost split step is needed here.
    let mut any_split = false;
    // (provider, model) -> accumulated tokens.
    let mut provider_models: HashMap<(String, String), RoleTokenUsage> = HashMap::new();
    let sessions_of: HashMap<String, u64> = model_sessions
        .iter()
        .map(|(m, s)| (m.clone(), s.len() as u64))
        .collect();

    for (model, tokens) in &model_tokens {
        let providers = model_providers.get(model).cloned().unwrap_or_default();
        let attributed: Vec<(String, RoleTokenUsage)> = if providers.is_empty() {
            // No provider signal for this model (e.g. codex): attribute the
            // whole thing to "(unknown)" — never guessed.
            vec![(PROVIDER_UNKNOWN.to_string(), *tokens)]
        } else {
            if providers.len() > 1 {
                any_split = true;
            }
            attribute_model_to_providers(*tokens, None, &providers)
                .into_iter()
                .map(|(p, t, _c)| (p, t))
                .collect()
        };
        for (provider, attributed_tokens) in attributed {
            let entry = provider_models
                .entry((provider, model.clone()))
                .or_default();
            entry.input += attributed_tokens.input;
            entry.output += attributed_tokens.output;
            entry.cache_read += attributed_tokens.cache_read;
            entry.cache_write += attributed_tokens.cache_write;
            entry.reasoning += attributed_tokens.reasoning;
        }
    }

    let total_tokens: u64 = model_tokens.values().map(|t| t.total()).sum();

    // Group (provider, model) by provider.
    let mut by_provider: HashMap<String, Vec<(String, RoleTokenUsage)>> = HashMap::new();
    for ((provider, model), tokens) in provider_models {
        by_provider
            .entry(provider)
            .or_default()
            .push((model, tokens));
    }

    let mut providers: Vec<ProviderMixEntry> = by_provider
        .into_iter()
        .map(|(provider, mut models)| {
            models.sort_by_key(|a| std::cmp::Reverse(a.1.total()));
            let model_entries: Vec<ProviderModelEntry> = models
                .into_iter()
                .map(|(model, tokens)| ProviderModelEntry {
                    sessions: sessions_of.get(&model).copied().unwrap_or(0),
                    cost_usd: None,
                    cost_source: None,
                    model,
                    tokens,
                })
                .collect();
            let provider_tokens: u64 = model_entries.iter().map(|m| m.tokens.total()).sum();
            let share_pct = if total_tokens > 0 {
                Some(provider_tokens as f64 / total_tokens as f64 * 100.0)
            } else {
                None
            };
            ProviderMixEntry {
                provider,
                cost_usd: None,
                share_pct,
                models: model_entries,
            }
        })
        .collect();
    providers
        .sort_by_key(|a| std::cmp::Reverse(a.models.iter().map(|m| m.tokens.total()).sum::<u64>()));

    let method = if any_split {
        "token-share-split".to_string()
    } else {
        "direct".to_string()
    };

    Ok(ProviderMixResponse {
        method,
        providers,
        total_tokens,
        filters_applied: Vec::new(),
    })
}

/// Distribution of request parameter settings (temperature, max_tokens).
pub fn query_request_param_profile(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<otelite_core::api::RequestParamProfile> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    // Temperature distribution
    let temp_sql = format!(
        "SELECT
            ROUND(CAST(json_extract(attributes, '$.\"gen_ai.request.temperature\"') AS REAL), 2) AS temperature,
            COUNT(*) AS cnt
        FROM spans
        {where_clause}
        GROUP BY temperature
        ORDER BY cnt DESC",
    );
    let mut temp_stmt = conn.prepare(&temp_sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare temperature query: {}", e))
    })?;
    let temperature_buckets = temp_stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::TemperatureBucket {
                temperature: row.get::<_, Option<f64>>(0)?,
                count: row.get::<_, i64>(1)? as usize,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute temperature query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse temperature results: {}", e))
        })?;

    // max_tokens distribution
    let max_sql = format!(
        "SELECT
            CAST(json_extract(attributes, '$.\"gen_ai.request.max_tokens\"') AS INTEGER) AS max_tokens,
            COUNT(*) AS cnt
        FROM spans
        {where_clause}
        GROUP BY max_tokens
        ORDER BY cnt DESC",
    );
    let mut max_stmt = conn.prepare(&max_sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare max_tokens query: {}", e))
    })?;
    let max_tokens_buckets = max_stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::MaxTokensBucket {
                max_tokens: row.get::<_, Option<i64>>(0)?,
                count: row.get::<_, i64>(1)? as usize,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute max_tokens query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse max_tokens results: {}", e))
        })?;

    Ok(otelite_core::api::RequestParamProfile {
        temperature_buckets,
        max_tokens_buckets,
        filters_applied: Vec::new(),
    })
}

/// Turn-count distribution across conversations with a known conversation_id.
pub fn query_conversation_depth(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<otelite_core::api::ConversationDepthStats> {
    let exprs = token_exprs();
    let conv_id = "json_extract(attributes, '$.\"gen_ai.conversation.id\"')";
    let mut where_clause = format!("WHERE {} AND {} IS NOT NULL", exprs.llm_span_guard, conv_id);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT COUNT(*) AS turns
        FROM spans
        {where_clause}
        GROUP BY {conv_id}",
        conv_id = conv_id,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare conversation_depth query: {}", e))
    })?;

    let mut turn_counts: Vec<i64> = stmt
        .query_map(param_refs.as_slice(), |row| row.get::<_, i64>(0))
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute conversation_depth query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse conversation_depth results: {}", e))
        })?;

    if turn_counts.is_empty() {
        return Ok(otelite_core::api::ConversationDepthStats {
            total_conversations: 0,
            avg_turns: 0.0,
            p50_turns: 0,
            p95_turns: 0,
            p99_turns: 0,
            filters_applied: Vec::new(),
        });
    }

    turn_counts.sort_unstable();
    let n = turn_counts.len();
    let avg = turn_counts.iter().sum::<i64>() as f64 / n as f64;

    Ok(otelite_core::api::ConversationDepthStats {
        total_conversations: n,
        avg_turns: avg,
        p50_turns: percentile(&turn_counts, 0.50),
        p95_turns: percentile(&turn_counts, 0.95),
        p99_turns: percentile(&turn_counts, 0.99),
        filters_applied: Vec::new(),
    })
}

/// LLM span latency per time bucket grouped by model.
///
/// Fetches raw (bucket, model, duration_ms, ttft_ms, is_error) rows then aggregates in Rust
/// so that p95 can be computed without a SQLite percentile extension.
#[allow(clippy::too_many_arguments)] // filter bar (#135) and calendar mode (#141/#142) pushed us past 5
pub fn query_latency_series(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    bucket_secs: u64,
    filters: &GenAiFilters,
    all_spans: bool,
    timezone: Option<&str>,
) -> Result<Vec<otelite_core::api::LatencySeriesPoint>> {
    let exprs = token_exprs();
    let bucket_ns = bucket_secs as i64 * 1_000_000_000;

    // Calendar-day mode: precompute local-midnight boundaries so every row
    // is assigned to its day in Rust (SQL cannot express DST-aware
    // boundaries). Requires an explicit window, as in the percentiles
    // query (#141).
    let calendar_bounds: Option<Vec<i64>> = match timezone {
        Some(tz_name) => {
            let (start_ns, end_ns) = match (start_time, end_time) {
                (Some(s), Some(e)) if s < e => (s, e),
                _ => {
                    return Err(StorageError::QueryError(
                        "calendar-day mode requires explicit start_time and end_time".to_string(),
                    ))
                },
            };
            let tz = std::str::FromStr::from_str(tz_name).map_err(|e| {
                StorageError::QueryError(format!("unknown IANA timezone '{tz_name}': {e}"))
            })?;
            Some(
                calendar_day_buckets(start_ns, end_ns, &tz)?
                    .into_iter()
                    .map(|(s, _)| s)
                    .collect(),
            )
        },
        None => None,
    };
    // In all_spans mode group by span name; otherwise group by model.
    let group_col = if all_spans {
        "name".to_string()
    } else {
        exprs.identity.clone()
    };
    let mut where_clause = if all_spans {
        "WHERE 1=1".to_string()
    } else {
        format!("WHERE {}", exprs.request_span_guard)
    };
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            (start_time / {bucket_ns}) * {bucket_ns} AS bucket,
            {group_col} AS group_label,
            (end_time - start_time) / 1000000 AS duration_ms,
            json_extract(attributes, '$.\"gen_ai.server.time_to_first_token\"') AS otel_ttft_secs,
            json_extract(attributes, '$.\"llm.time_to_first_token\"') AS llm_ttft_secs,
            json_extract(attributes, '$.\"ttft_ms\"') AS custom_ttft_ms,
            CASE WHEN status_code = 2 THEN 1 ELSE 0 END AS is_error,
            start_time AS start_ns,
            (end_time - start_time) AS duration_ns,
            {output_tokens} AS output_tokens
        FROM spans
        {where_clause}
        ORDER BY bucket ASC",
        bucket_ns = bucket_ns,
        group_col = group_col,
        output_tokens = exprs.output,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare latency_series query: {}", e))
    })?;

    struct RawRow {
        bucket: i64,
        label: Option<String>,
        duration_ms: i64,
        otel_ttft_secs: Option<String>,
        llm_ttft_secs: Option<String>,
        custom_ttft_ms: Option<String>,
        is_error: bool,
        start_ns: i64,
        duration_ns: i64,
        output_tokens: Option<i64>,
    }

    let raw: Vec<RawRow> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(RawRow {
                bucket: row.get::<_, i64>(0)?,
                label: row.get::<_, Option<String>>(1)?,
                duration_ms: row.get::<_, i64>(2)?,
                otel_ttft_secs: row.get::<_, Option<String>>(3)?,
                llm_ttft_secs: row.get::<_, Option<String>>(4)?,
                custom_ttft_ms: row.get::<_, Option<String>>(5)?,
                is_error: row.get::<_, i64>(6)? != 0,
                start_ns: row.get::<_, i64>(7)?,
                duration_ns: row.get::<_, i64>(8)?,
                output_tokens: row.get::<_, Option<i64>>(9)?,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute latency_series query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse latency_series results: {}", e))
        })?;

    type BucketKey = (i64, Option<String>);
    type BucketAccum = (Vec<i64>, TtftAccum, usize, Vec<f64>);

    let mut groups: std::collections::BTreeMap<BucketKey, BucketAccum> =
        std::collections::BTreeMap::new();
    for r in raw {
        // Calendar mode re-buckets by local day from the call start time;
        // rolling mode keeps the SQL epoch-grid bucket.
        let bucket = match &calendar_bounds {
            Some(bounds) => match bounds.partition_point(|&b| b <= r.start_ns) {
                0 => continue,
                i => bounds[i - 1],
            },
            None => r.bucket,
        };
        let entry = groups.entry((bucket, r.label)).or_default();
        entry.0.push(r.duration_ms);
        entry.1.record(
            r.duration_ms,
            normalized_ttft_secs(
                r.otel_ttft_secs.as_deref(),
                r.llm_ttft_secs.as_deref(),
                r.custom_ttft_ms.as_deref(),
            ),
        );
        if r.is_error {
            entry.2 += 1;
        }
        if let Some(rate) = throughput_rate_tok_s(r.duration_ns, r.output_tokens.map(|t| t as f64))
        {
            entry.3.push(rate);
        }
    }

    let mut out = Vec::with_capacity(groups.len());
    for ((bucket, label), (mut durations, ttft, error_count, mut rates)) in groups {
        if durations.is_empty() {
            continue;
        }
        let ttft_degenerate = ttft.is_degenerate();
        let TtftAccum {
            values_ms: mut ttfts,
            invalid_count: ttft_invalid_count,
            degenerate_count: ttft_degenerate_count,
        } = ttft;
        durations.sort_unstable();
        ttfts.sort_unstable();

        let count = durations.len();
        let min_ms = durations[0];
        let max_ms = durations[count - 1];
        let avg_ms = durations.iter().sum::<i64>() as f64 / count as f64;
        let p95_ms = percentile(&durations, 0.95);

        let (avg_ttft_ms, p95_ttft_ms) = if ttfts.is_empty() {
            (None, None)
        } else {
            let avg = ttfts.iter().sum::<i64>() as f64 / ttfts.len() as f64;
            let p95 = percentile(&ttfts, 0.95);
            (Some(avg), Some(p95))
        };

        let (model, name) = if all_spans {
            (None, label)
        } else {
            (label, None)
        };

        rates.sort_by(|a, b| a.total_cmp(b));
        let (throughput_p10_tok_s, throughput_p50_tok_s, throughput_p90_tok_s) = if rates.is_empty()
        {
            (None, None, None)
        } else {
            (
                Some(percentile_f64(&rates, 0.10)),
                Some(percentile_f64(&rates, 0.50)),
                Some(percentile_f64(&rates, 0.90)),
            )
        };

        out.push(otelite_core::api::LatencySeriesPoint {
            timestamp: bucket,
            model,
            name,
            count,
            error_count,
            min_ms,
            avg_ms,
            p95_ms,
            max_ms,
            avg_ttft_ms,
            p95_ttft_ms,
            ttft_count: ttfts.len(),
            ttft_invalid_count,
            ttft_degenerate_count,
            ttft_degenerate,
            throughput_p10_tok_s,
            throughput_p50_tok_s,
            throughput_p90_tok_s,
            throughput_sample_count: rates.len(),
        });
    }

    Ok(out)
}

/// Call volume per time bucket grouped by model (LLM mode) or span name (all-spans mode).
#[allow(clippy::too_many_arguments)] // filter bar (#135) pushed us past 5
pub fn query_calls_series(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
    bucket_secs: u64,
    all_spans: bool,
) -> Result<Vec<otelite_core::api::CallsSeriesPoint>> {
    let exprs = token_exprs();
    let bucket_ns = bucket_secs as i64 * 1_000_000_000;
    let group_col = if all_spans {
        "name".to_string()
    } else {
        exprs.identity.clone()
    };
    let mut where_clause = if all_spans {
        "WHERE 1=1".to_string()
    } else {
        format!("WHERE {}", exprs.request_span_guard)
    };
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            (start_time / {bucket_ns}) * {bucket_ns} AS bucket,
            {group_col} AS label,
            COUNT(*) AS requests
        FROM spans
        {where_clause}
        GROUP BY bucket, {group_col}
        ORDER BY bucket ASC",
        bucket_ns = bucket_ns,
        group_col = group_col,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare calls_series query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let label: Option<String> = row.get(1)?;
            let requests = row.get::<_, i64>(2)? as usize;
            let (model, name) = if all_spans {
                (None, label)
            } else {
                (label, None)
            };
            Ok(otelite_core::api::CallsSeriesPoint {
                timestamp: row.get::<_, i64>(0)?,
                model,
                name,
                requests,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute calls_series query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse calls_series results: {}", e))
        })?;

    Ok(rows)
}

/// LLM latency broken down by input-token context size bin × model.
///
/// Bins: 0–1K, 1K–10K, 10K–50K, 50K–100K, 100K+
/// p95 is computed in Rust over raw rows per bin.
pub fn query_latency_by_context(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::LatencyByContextBin>> {
    let exprs = token_exprs();
    let mut where_clause = format!(
        "WHERE {} AND ({}) IS NOT NULL AND ({}) > 0",
        exprs.llm_span_guard, exprs.input, exprs.input
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            {model} AS model,
            COALESCE({input}, 0) AS input_tokens,
            (end_time - start_time) / 1000000 AS duration_ms,
            json_extract(attributes, '$.\"gen_ai.server.time_to_first_token\"') AS otel_ttft_secs,
            json_extract(attributes, '$.\"llm.time_to_first_token\"') AS llm_ttft_secs,
            json_extract(attributes, '$.\"ttft_ms\"') AS custom_ttft_ms
        FROM spans
        {where_clause}",
        model = exprs.identity,
        input = exprs.input,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare latency_by_context query: {}", e))
    })?;

    struct RawRow {
        model: Option<String>,
        input_tokens: i64,
        duration_ms: i64,
        otel_ttft_secs: Option<String>,
        llm_ttft_secs: Option<String>,
        custom_ttft_ms: Option<String>,
    }

    let raw: Vec<RawRow> = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(RawRow {
                model: row.get(0)?,
                input_tokens: row.get::<_, i64>(1)?,
                duration_ms: row.get::<_, i64>(2)?,
                otel_ttft_secs: row.get::<_, Option<String>>(3)?,
                llm_ttft_secs: row.get::<_, Option<String>>(4)?,
                custom_ttft_ms: row.get::<_, Option<String>>(5)?,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute latency_by_context query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse latency_by_context results: {}", e))
        })?;

    const BINS: &[(u64, u64, &str)] = &[
        (0, 1_000, "0–1K"),
        (1_000, 10_000, "1K–10K"),
        (10_000, 50_000, "10K–50K"),
        (50_000, 100_000, "50K–100K"),
        (100_000, u64::MAX, "100K+"),
    ];

    type BinKey = (usize, Option<String>); // (bin_index, model)
    type BinAccum = (Vec<i64>, TtftAccum); // (durations, ttfts)

    let mut groups: std::collections::BTreeMap<BinKey, BinAccum> =
        std::collections::BTreeMap::new();

    for r in raw {
        let bin_idx = BINS
            .iter()
            .position(|(lo, hi, _)| {
                let t = r.input_tokens as u64;
                t >= *lo && t < *hi
            })
            .unwrap_or(BINS.len() - 1);
        let entry = groups.entry((bin_idx, r.model)).or_default();
        entry.0.push(r.duration_ms);
        entry.1.record(
            r.duration_ms,
            normalized_ttft_secs(
                r.otel_ttft_secs.as_deref(),
                r.llm_ttft_secs.as_deref(),
                r.custom_ttft_ms.as_deref(),
            ),
        );
    }

    let mut out = Vec::with_capacity(groups.len());
    for ((bin_idx, model), (mut durations, ttft)) in groups {
        if durations.is_empty() {
            continue;
        }
        let ttft_degenerate = ttft.is_degenerate();
        let TtftAccum {
            values_ms: mut ttfts,
            invalid_count: ttft_invalid_count,
            degenerate_count: ttft_degenerate_count,
        } = ttft;
        durations.sort_unstable();
        ttfts.sort_unstable();

        let (lo, hi, label) = BINS[bin_idx];
        let count = durations.len();
        let avg_ms = durations.iter().sum::<i64>() as f64 / count as f64;
        let p95_ms = percentile(&durations, 0.95);
        let max_ms = durations[count - 1];
        let avg_ttft_ms = if ttfts.is_empty() {
            None
        } else {
            Some(ttfts.iter().sum::<i64>() as f64 / ttfts.len() as f64)
        };

        out.push(otelite_core::api::LatencyByContextBin {
            bin: label.to_string(),
            min_tokens: lo,
            max_tokens: hi,
            model,
            count,
            avg_ms,
            p95_ms,
            max_ms,
            avg_ttft_ms,
            ttft_count: ttfts.len(),
            ttft_invalid_count,
            ttft_degenerate_count,
            ttft_degenerate,
        });
    }

    // Sort by bin index so output is always 0–1K → 1K–10K → …
    out.sort_by_key(|r| {
        BINS.iter()
            .position(|(_, _, lbl)| lbl == &r.bin.as_str())
            .unwrap_or(usize::MAX)
    });

    Ok(out)
}

/// Per-(model, error_type) breakdown of error spans, with bucketing into actionable categories.
///
/// Spans are errors when `status_code = 2`. The error-type label is derived by COALESCE:
///   1. `error.type` (OTel standard)
///   2. `exception.type`
///   3. `http.response.status_code`
///   4. `http.status_code` (legacy)
///   5. literal "unknown"
///
/// Bucketing is heuristic — different SDKs use different labels. Raw `error_type` is also
/// returned so callers can inspect unparsed values.
pub fn query_error_types(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::ErrorTypeBreakdown>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {} AND status_code = 2", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "WITH error_spans AS (
            SELECT
                {model} AS model,
                COALESCE(
                    json_extract(attributes, '$.\"error.type\"'),
                    json_extract(attributes, '$.\"exception.type\"'),
                    CAST(json_extract(attributes, '$.\"http.response.status_code\"') AS TEXT),
                    CAST(json_extract(attributes, '$.\"http.status_code\"') AS TEXT),
                    'unknown'
                ) AS error_type
            FROM spans
            {where_clause}
        )
        SELECT model, error_type,
            CASE
                WHEN LOWER(error_type) LIKE '%rate%limit%'
                  OR error_type LIKE '%429%'
                  OR LOWER(error_type) LIKE '%throttl%'         THEN 'rate_limit'
                WHEN LOWER(error_type) LIKE '%timeout%'
                  OR error_type IN ('408', '504')
                  OR LOWER(error_type) LIKE '%deadline%'        THEN 'timeout'
                WHEN LOWER(error_type) LIKE '%context%length%'
                  OR LOWER(error_type) LIKE '%context%window%'
                  OR LOWER(error_type) LIKE '%max%token%'
                  OR LOWER(error_type) LIKE '%too%long%'        THEN 'context_length'
                WHEN LOWER(error_type) LIKE '%content_filter%'
                  OR LOWER(error_type) LIKE '%moderation%'
                  OR LOWER(error_type) LIKE '%content_policy%'
                  OR LOWER(error_type) LIKE '%safety%'          THEN 'content_filter'
                WHEN error_type IN ('401', '403')
                  OR LOWER(error_type) LIKE '%unauthor%'
                  OR LOWER(error_type) LIKE '%forbid%'
                  OR LOWER(error_type) LIKE '%invalid%api%key%' THEN 'auth'
                WHEN CAST(error_type AS INTEGER) BETWEEN 500 AND 599 THEN 'server_error'
                ELSE 'unknown'
            END AS bucket,
            COUNT(*) AS count
        FROM error_spans
        GROUP BY model, error_type, bucket
        ORDER BY count DESC",
        model = exprs.identity,
        where_clause = where_clause,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare error_types query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            Ok(otelite_core::api::ErrorTypeBreakdown {
                model: row.get::<_, Option<String>>(0)?,
                error_type: row.get::<_, String>(1)?,
                bucket: row.get::<_, String>(2)?,
                count: row.get::<_, i64>(3)? as usize,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute error_types query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse error_types results: {}", e))
        })?;

    Ok(rows)
}

/// All observed (request_model, response_model) pairs with a `differs` flag.
///
/// Returns ALL pairs including matching ones — callers filter if they only want drifted pairs.
/// `differs` is true when both fields are non-null and differ, indicating silent provider rerouting.
pub fn query_model_drift(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::ModelDriftPair>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(start) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(start));
    }
    if let Some(end) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(end));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    // Alias-aware key sets (llm.* spellings included) so drift is detected
    // the same way model identity is built elsewhere.
    let request_model_expr = exprs.request_model.clone();
    let response_model_expr = exprs.response_model.clone();
    let sql = format!(
        "SELECT
            {request_model_expr} AS request_model,
            {response_model_expr} AS response_model,
            COUNT(*) AS count
        FROM spans
        {where_clause}
        GROUP BY request_model, response_model
        HAVING request_model IS NOT NULL OR response_model IS NOT NULL
        ORDER BY count DESC",
        where_clause = where_clause,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare model_drift query: {}", e))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |row| {
            let request_model: Option<String> = row.get(0)?;
            let response_model: Option<String> = row.get(1)?;
            let count: i64 = row.get(2)?;
            let differs = request_model.is_some()
                && response_model.is_some()
                && request_model != response_model;
            Ok(otelite_core::api::ModelDriftPair {
                request_model,
                response_model,
                count: count as usize,
                differs,
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute model_drift query: {}", e))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse model_drift results: {}", e))
        })?;

    Ok(rows)
}

// ── New analytics queries ─────────────────────────────────────────────────────

/// Approval/rejection summary for claude_code.tool.blocked_on_user spans.
pub fn query_tool_approvals(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<otelite_core::api::ToolApprovalStats> {
    // name = TOOL_APPROVAL_SPAN_NAME scopes the scan to
    // idx_spans_tool_approval.
    let mut where_clause = format!("WHERE name = '{}'", semconv::TOOL_APPROVAL_SPAN_NAME);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(e));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            json_extract(attributes, '$.decision') AS decision,
            json_extract(attributes, '$.source')   AS source,
            json_extract(attributes, '$.tool_name') AS tool_name,
            COUNT(*) AS cnt
         FROM spans
         {where_clause}
         GROUP BY decision, source, tool_name
         ORDER BY cnt DESC"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare tool_approvals query: {e}"))
    })?;

    struct Row {
        decision: Option<String>,
        source: Option<String>,
        tool_name: Option<String>,
        cnt: usize,
    }
    let rows: Vec<Row> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(Row {
                decision: r.get(0)?,
                source: r.get(1)?,
                tool_name: r.get(2)?,
                cnt: r.get::<_, i64>(3)? as usize,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to execute tool_approvals: {e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse tool_approvals: {e}")))?;

    let mut stats = otelite_core::api::ToolApprovalStats::default();
    let mut rejected_map: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for row in rows {
        let decision = row.decision.as_deref().unwrap_or("unknown");
        let source = row.source.as_deref().unwrap_or("unknown");
        match decision {
            "accept" if source == "config" => stats.auto_accepted += row.cnt,
            "accept" => stats.user_accepted += row.cnt,
            "reject" => {
                stats.rejected += row.cnt;
                if let Some(t) = &row.tool_name {
                    *rejected_map.entry(t.clone()).or_default() += row.cnt;
                }
            },
            _ => stats.unknown += row.cnt,
        }
        stats.total += row.cnt;
    }

    let mut top: Vec<_> = rejected_map
        .into_iter()
        .map(|(tool_name, count)| otelite_core::api::ToolApprovalEntry { tool_name, count })
        .collect();
    top.sort_by_key(|a| std::cmp::Reverse(a.count));
    top.truncate(10);
    stats.top_rejected = top;
    Ok(stats)
}

/// Distribution of stop_reason values across LLM spans.
pub fn query_stop_reasons(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::StopReasonCount>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(e));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            COALESCE(
                json_extract(attributes, '$.stop_reason'),
                json_extract(attributes, '$.\"gen_ai.response.finish_reason\"'),
                '(none)'
            ) AS reason,
            COUNT(*) AS cnt
         FROM spans
         {where_clause}
         GROUP BY reason
         ORDER BY cnt DESC"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare stop_reasons query: {e}"))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(otelite_core::api::StopReasonCount {
                reason: r.get(0)?,
                count: r.get::<_, i64>(1)? as usize,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to execute stop_reasons: {e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse stop_reasons: {e}")))?;
    Ok(rows)
}

/// Token usage broken down by llm_request.context attribute.
pub fn query_context_type_split(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::ContextTypeSplit>> {
    let exprs = token_exprs();
    let mut where_clause = format!("WHERE {}", exprs.llm_span_guard);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(e));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    let sql = format!(
        "SELECT
            COALESCE(json_extract(attributes, '$.\"llm_request.context\"'), '(unknown)') AS context,
            COUNT(*) AS calls,
            COALESCE(SUM({input}), 0)  AS input_tokens,
            COALESCE(SUM({output}), 0) AS output_tokens,
            AVG((end_time - start_time) / 1000000.0) AS avg_ms
         FROM spans
         {where_clause}
         GROUP BY context
         ORDER BY calls DESC",
        input = exprs.input,
        output = exprs.output,
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare context_type_split query: {e}"))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(otelite_core::api::ContextTypeSplit {
                context: r.get(0)?,
                calls: r.get::<_, i64>(1)? as usize,
                input_tokens: r.get::<_, i64>(2)? as u64,
                output_tokens: r.get::<_, i64>(3)? as u64,
                avg_ms: r.get::<_, f64>(4).unwrap_or(0.0),
            })
        })
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to execute context_type_split: {e}"))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            StorageError::QueryError(format!("Failed to parse context_type_split: {e}"))
        })?;
    Ok(rows)
}

/// Top error messages from failed tool executions.
pub fn query_tool_errors(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
    limit: usize,
) -> Result<Vec<otelite_core::api::ToolErrorEntry>> {
    // name = TOOL_EXECUTION_SPAN_NAME scopes the scan to
    // idx_spans_tool_exec; the json_valid gate keeps corrupt rows from
    // raising in the json_extract filters (no-op for valid rows).
    let mut where_clause = format!(
        "WHERE name = '{}'
           AND json_valid(attributes)
           AND json_extract(attributes, '$.success') = 'false'
           AND json_extract(attributes, '$.error') IS NOT NULL",
        semconv::TOOL_EXECUTION_SPAN_NAME
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(e));
    }
    push_scope(&mut where_clause, &mut params, filters.span_scope());

    // Truncate long error messages at 120 chars for grouping
    let sql = format!(
        "SELECT
            COALESCE(json_extract(attributes, '$.tool_name'), '(unknown)') AS tool_name,
            SUBSTR(json_extract(attributes, '$.error'), 1, 120)           AS error_msg,
            COUNT(*) AS cnt
         FROM spans
         {where_clause}
         GROUP BY tool_name, error_msg
         ORDER BY cnt DESC
         LIMIT ?"
    );

    params.push(Box::new(limit as i64));
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare tool_errors query: {e}"))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(otelite_core::api::ToolErrorEntry {
                tool_name: r.get(0)?,
                error_message: r.get(1)?,
                count: r.get::<_, i64>(2)? as usize,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("Failed to execute tool_errors: {e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("Failed to parse tool_errors: {e}")))?;
    Ok(rows)
}

/// Hour-of-day activity buckets (0–23, UTC).
pub fn query_hour_of_day(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
    filters: &GenAiFilters,
) -> Result<Vec<otelite_core::api::HourOfDayBucket>> {
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    let mut time_filter = String::new();
    if let Some(s) = start_time {
        time_filter.push_str(" AND start_time >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        time_filter.push_str(" AND end_time <= ?");
        params.push(Box::new(e));
    }
    // Shared by both sub-queries; each statement binds its own copy of params.
    if let Some((frag, fp)) = filters.span_scope() {
        time_filter.push_str(&format!(" AND {frag}"));
        params.extend(
            fp.into_iter()
                .map(|p| Box::new(p) as Box<dyn rusqlite::ToSql>),
        );
    }

    // Duplicate params for the two sub-queries (SQLite doesn't support named params easily here)
    // Each name-equality filter matches a partial index
    // (idx_spans_llm_request_name / idx_spans_tool_exec), so both scans
    // are index-only instead of full-window scans.
    let llm_filter = format!(
        "WHERE name = '{}'{}",
        semconv::LLM_REQUEST_SPAN_NAME,
        time_filter
    );
    let tool_filter = format!(
        "WHERE name = '{}'{}",
        semconv::TOOL_EXECUTION_SPAN_NAME,
        time_filter
    );

    // Build hour table by merging two separate queries in Rust — simpler than a FULL OUTER JOIN
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let llm_sql = format!(
        "SELECT CAST(strftime('%H', start_time/1000000000, 'unixepoch') AS INTEGER) AS h, COUNT(*) AS cnt
         FROM spans {llm_filter} GROUP BY h"
    );
    let tool_sql = format!(
        "SELECT CAST(strftime('%H', start_time/1000000000, 'unixepoch') AS INTEGER) AS h, COUNT(*) AS cnt
         FROM spans {tool_filter} GROUP BY h"
    );

    let mut llm_by_hour = [0usize; 24];
    let mut tool_by_hour = [0usize; 24];

    {
        let mut stmt = conn.prepare(&llm_sql).map_err(|e| {
            StorageError::QueryError(format!("Failed to prepare hour_of_day llm query: {e}"))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| StorageError::QueryError(format!("{e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| StorageError::QueryError(format!("{e}")))?;
        for (h, cnt) in rows {
            if (0..24).contains(&(h as usize)) {
                llm_by_hour[h as usize] = cnt as usize;
            }
        }
    }
    {
        let mut stmt = conn.prepare(&tool_sql).map_err(|e| {
            StorageError::QueryError(format!("Failed to prepare hour_of_day tool query: {e}"))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| StorageError::QueryError(format!("{e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| StorageError::QueryError(format!("{e}")))?;
        for (h, cnt) in rows {
            if (0..24).contains(&(h as usize)) {
                tool_by_hour[h as usize] = cnt as usize;
            }
        }
    }

    Ok((0u8..24u8)
        .map(|h| otelite_core::api::HourOfDayBucket {
            hour: h,
            llm_calls: llm_by_hour[h as usize],
            tool_calls: tool_by_hour[h as usize],
        })
        .collect())
}

// ── New insight queries (issues #157–#164) ────────────────────────────────

/// Claude Code effort-level × model × token-type breakdown. (#157)
///
/// Uses the `idx_metrics_claude_code_token_effort` covering index.
pub fn query_effort_breakdown(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::EffortBreakdownResponse> {
    use otelite_core::api::{EffortBreakdownResponse, EffortBreakdownRow};
    use otelite_core::semconv::metric_names as mnames;

    let mut where_clause = String::from("WHERE name = ?");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(mnames::CLAUDE_CODE_TOKEN_USAGE.to_string())];
    if let Some(s) = start_time {
        where_clause.push_str(" AND timestamp >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND timestamp <= ?");
        params.push(Box::new(e));
    }

    // Effort breakdown uses the claude_code.token.usage metric which stores
    // cumulative counters per (effort, model, type, session_id) label key.
    // We use counter_window_deltas to avoid overcounting cumulative resets.
    let deltas = counter_window_deltas(
        conn,
        mnames::CLAUDE_CODE_TOKEN_USAGE,
        &["$.effort", "$.model", "$.type"],
        start_time,
        end_time,
    )?;

    // Accumulate: (effort, model, token_type) -> total
    let mut acc: HashMap<(String, String, String), u64> = HashMap::new();
    for d in deltas {
        let effort = d
            .labels
            .first()
            .and_then(|l| l.clone())
            .unwrap_or_else(|| "(none)".to_string());
        let model = d
            .labels
            .get(1)
            .and_then(|l| l.clone())
            .unwrap_or_else(|| "(unknown)".to_string());
        let token_type = d
            .labels
            .get(2)
            .and_then(|l| l.clone())
            .unwrap_or_else(|| "(unknown)".to_string());
        *acc.entry((effort, model, token_type)).or_default() += d.delta as u64;
    }

    let mut rows: Vec<EffortBreakdownRow> = acc
        .into_iter()
        .map(|((effort, model, token_type), tokens)| EffortBreakdownRow {
            effort,
            model,
            token_type,
            tokens,
            cost_usd: None,
        })
        .collect();
    rows.sort_by(|a, b| {
        a.effort
            .cmp(&b.effort)
            .then(a.model.cmp(&b.model))
            .then(a.token_type.cmp(&b.token_type))
    });

    Ok(EffortBreakdownResponse {
        rows,
        filters_applied: Vec::new(),
    })
}

/// Cross-agent tokens-per-commit and tokens-per-LOC efficiency stats. (#158)
///
/// Aggregates token usage from claude_code and opencode cumulative counters
/// alongside commit counts and lines-of-code counters.
pub fn query_efficiency_stats(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::EfficiencyStats> {
    use otelite_core::api::{AgentEfficiencyRow, EfficiencyStats};
    use otelite_core::semconv::metric_names as mnames;

    // ── Claude Code tokens (cumulative counter) ──────────────────────────
    let cc_token_deltas = counter_window_deltas(
        conn,
        mnames::CLAUDE_CODE_TOKEN_USAGE,
        &["$.type"],
        start_time,
        end_time,
    )?;
    let mut cc_tokens: u64 = 0;
    for d in cc_token_deltas {
        let t = d.labels.first().and_then(|l| l.as_deref()).unwrap_or("");
        // Include input + output (skip cache_read to avoid double-count)
        if matches!(t, "input" | "output") {
            cc_tokens += d.delta as u64;
        }
    }

    // ── Claude Code commits (cumulative counter) ─────────────────────────
    let cc_commit_deltas = counter_window_deltas(
        conn,
        mnames::CLAUDE_CODE_COMMIT_COUNT,
        &[],
        start_time,
        end_time,
    )?;
    let cc_commits: u64 = cc_commit_deltas.iter().map(|d| d.delta as u64).sum();

    // ── Claude Code LOC (cumulative counter) ─────────────────────────────
    let cc_loc_deltas = counter_window_deltas(
        conn,
        mnames::CLAUDE_CODE_LINES_OF_CODE,
        &["$.type"],
        start_time,
        end_time,
    )?;
    let cc_loc_added: i64 = cc_loc_deltas
        .iter()
        .filter(|d| d.labels.first().and_then(|l| l.as_deref()) == Some("added"))
        .map(|d| d.delta as i64)
        .sum();
    let cc_loc_removed: i64 = cc_loc_deltas
        .iter()
        .filter(|d| d.labels.first().and_then(|l| l.as_deref()) == Some("removed"))
        .map(|d| d.delta as i64)
        .sum();

    // ── opencode tokens (cumulative counter) ─────────────────────────────
    let oc_token_deltas = counter_window_deltas(
        conn,
        mnames::OPENCODE_TOKEN_USAGE,
        &["$.type"],
        start_time,
        end_time,
    )?;
    let mut oc_tokens: u64 = 0;
    for d in oc_token_deltas {
        let t = d.labels.first().and_then(|l| l.as_deref()).unwrap_or("");
        if matches!(t, "input" | "output") {
            oc_tokens += d.delta as u64;
        }
    }

    // ── opencode LOC (cumulative counter) ────────────────────────────────
    let oc_loc_deltas = counter_window_deltas(
        conn,
        mnames::OPENCODE_LINES_OF_CODE,
        &["$.type"],
        start_time,
        end_time,
    )?;
    let oc_loc_added: i64 = oc_loc_deltas
        .iter()
        .filter(|d| d.labels.first().and_then(|l| l.as_deref()) == Some("added"))
        .map(|d| d.delta as i64)
        .sum();
    let oc_loc_removed: i64 = oc_loc_deltas
        .iter()
        .filter(|d| d.labels.first().and_then(|l| l.as_deref()) == Some("removed"))
        .map(|d| d.delta as i64)
        .sum();

    let total_tokens = cc_tokens + oc_tokens;
    let total_commits = cc_commits; // opencode doesn't emit commit metrics
    let net_lines_added: i64 = (cc_loc_added + oc_loc_added) - (cc_loc_removed + oc_loc_removed);

    let tokens_per_commit = if total_commits > 0 {
        Some(total_tokens as f64 / total_commits as f64)
    } else {
        None
    };
    let tokens_per_loc = if net_lines_added > 0 {
        Some(total_tokens as f64 / net_lines_added as f64)
    } else {
        None
    };

    let by_agent = vec![
        AgentEfficiencyRow {
            agent: "claude_code".to_string(),
            tokens: cc_tokens,
            commits: cc_commits,
            lines_added: cc_loc_added,
            lines_removed: cc_loc_removed,
        },
        AgentEfficiencyRow {
            agent: "opencode".to_string(),
            tokens: oc_tokens,
            commits: 0,
            lines_added: oc_loc_added,
            lines_removed: oc_loc_removed,
        },
    ];

    Ok(EfficiencyStats {
        total_tokens,
        total_commits,
        total_prs: 0,
        net_lines_added,
        tokens_per_commit,
        tokens_per_loc,
        by_agent,
        filters_applied: Vec::new(),
    })
}

/// Codex turn TTFT histogram: p50/p90/p99 per model. (#159)
///
/// Reuses the existing `collect_codex_ttft_values` helper to expand buckets
/// then computes percentiles in Rust.
pub fn query_codex_ttft(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::CodexTtftResponse> {
    use otelite_core::api::{CodexTtftByModel, CodexTtftResponse};

    let filters = otelite_core::filters::GenAiFilters::default();
    let values = collect_codex_ttft_values(conn, start_time, end_time, &filters)?;

    // Group by model
    let mut by_model: HashMap<String, Vec<f64>> = HashMap::new();
    for (model, _ts, v) in values {
        let key = model.unwrap_or_else(|| "(unknown)".to_string());
        by_model.entry(key).or_default().push(v);
    }

    fn percentile(sorted: &[f64], p: f64) -> f64 {
        if sorted.is_empty() {
            return 0.0;
        }
        let idx = ((sorted.len() as f64 - 1.0) * p).floor() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    let mut models: Vec<CodexTtftByModel> = by_model
        .into_iter()
        .map(|(model, mut vals)| {
            vals.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let count = vals.len() as u64;
            CodexTtftByModel {
                model,
                count,
                p50_ms: if vals.is_empty() {
                    None
                } else {
                    Some(percentile(&vals, 0.50))
                },
                p90_ms: if vals.is_empty() {
                    None
                } else {
                    Some(percentile(&vals, 0.90))
                },
                p95_ms: if vals.is_empty() {
                    None
                } else {
                    Some(percentile(&vals, 0.95))
                },
            }
        })
        .collect();
    models.sort_by(|a, b| a.model.cmp(&b.model));

    Ok(CodexTtftResponse {
        models,
        filters_applied: Vec::new(),
    })
}

/// Per-project token + commit rollup across all agents. (#160)
///
/// Projects are identified from the `cwd` label on Codex `run_sampling_request`
/// spans (basename of the path) and from opencode `project.id` metric labels.
/// Claude Code is attributed by the `cwd` label carried on opencode sessions
/// when detected — otherwise falls back to "(unattributed)".
pub fn query_agent_project_rollup(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::AgentProjectRollupResponse> {
    use otelite_core::api::{AgentProjectRollupResponse, ProjectRollupEntry};
    use otelite_core::semconv::metric_names as mnames;

    // project -> (agent -> tokens)
    let mut project_tokens: HashMap<String, HashMap<String, u64>> = HashMap::new();

    // ── Codex: extract project from cwd basename on run_sampling_request spans ──
    {
        let mut where_clause = String::from("WHERE name = 'run_sampling_request'");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        if let Some(s) = start_time {
            where_clause.push_str(" AND start_time >= ?");
            params.push(Box::new(s));
        }
        if let Some(e) = end_time {
            where_clause.push_str(" AND end_time <= ?");
            params.push(Box::new(e));
        }

        // Extract basename: last segment after the final '/'.
        // rtrim(path, replace(path,'/','')) strips all non-slash chars from the
        // right, leaving everything up to and including the last slash.
        // Adding 1 to its length gives the start position of the basename.
        let sql = format!(
            "SELECT
                CASE
                    WHEN json_valid(attributes) AND instr(json_extract(attributes,'$.cwd'),'/') > 0
                    THEN substr(
                        json_extract(attributes,'$.cwd'),
                        1 + length(rtrim(
                            json_extract(attributes,'$.cwd'),
                            replace(json_extract(attributes,'$.cwd'), '/', '')
                        ))
                    )
                    WHEN json_valid(attributes)
                    THEN json_extract(attributes,'$.cwd')
                    ELSE NULL
                END AS project,
                COUNT(*) AS turns
             FROM spans
             {where_clause}
             GROUP BY project
             HAVING project IS NOT NULL"
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!("Failed to prepare codex project rollup query: {e}"))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
            })
            .map_err(|e| StorageError::QueryError(format!("{e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| StorageError::QueryError(format!("{e}")))?;

        for (project, turns) in rows {
            // Codex spans don't carry token counts — use turn count as a proxy
            *project_tokens
                .entry(project)
                .or_default()
                .entry("codex".to_string())
                .or_default() += turns as u64;
        }
    }

    // ── opencode: project.id from token usage metric labels ──────────────
    let oc_deltas = counter_window_deltas(
        conn,
        mnames::OPENCODE_TOKEN_USAGE,
        &["$.project.id", "$.type"],
        start_time,
        end_time,
    )?;
    for d in oc_deltas {
        let project = d
            .labels
            .first()
            .and_then(|l| l.clone())
            .unwrap_or_else(|| "(unattributed)".to_string());
        let token_type = d.labels.get(1).and_then(|l| l.as_deref()).unwrap_or("");
        if matches!(token_type, "input" | "output") {
            *project_tokens
                .entry(project)
                .or_default()
                .entry("opencode".to_string())
                .or_default() += d.delta as u64;
        }
    }

    // ── Claude Code: no cwd label on metrics; use a single "(unattributed)" bucket ──
    let cc_deltas = counter_window_deltas(
        conn,
        mnames::CLAUDE_CODE_TOKEN_USAGE,
        &["$.type"],
        start_time,
        end_time,
    )?;
    let cc_total: u64 = cc_deltas
        .iter()
        .filter(|d| {
            matches!(
                d.labels.first().and_then(|l| l.as_deref()),
                Some("input") | Some("output")
            )
        })
        .map(|d| d.delta as u64)
        .sum();
    if cc_total > 0 {
        *project_tokens
            .entry("(claude_code-unattributed)".to_string())
            .or_default()
            .entry("claude_code".to_string())
            .or_default() += cc_total;
    }

    let mut entries: Vec<ProjectRollupEntry> = project_tokens
        .into_iter()
        .map(|(project, agent_map)| {
            let total_tokens: u64 = agent_map.values().sum();
            let requests: u64 = agent_map.values().sum(); // same as tokens for codex (turn proxy)
            let agents: Vec<String> = {
                let mut v: Vec<String> = agent_map.into_keys().collect();
                v.sort();
                v
            };
            ProjectRollupEntry {
                project,
                agents,
                total_tokens,
                requests,
                cost_usd: None,
            }
        })
        .collect();
    entries.sort_by_key(|a| std::cmp::Reverse(a.total_tokens));

    Ok(AgentProjectRollupResponse {
        projects: entries,
        filters_applied: Vec::new(),
    })
}

/// MCP call health: success/error rates per server+tool. (#161)
///
/// Queries the `codex.mcp.call` event counter, grouping by server, tool, and status.
pub fn query_mcp_health(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::McpHealthResponse> {
    use otelite_core::api::{McpHealthEntry, McpHealthResponse};
    use otelite_core::semconv::metric_names as mnames;

    let mut where_clause = String::from("WHERE name = ? AND json_valid(attributes)");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(mnames::CODEX_MCP_CALL.to_string())];
    if let Some(s) = start_time {
        where_clause.push_str(" AND timestamp >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND timestamp <= ?");
        params.push(Box::new(e));
    }

    let sql = format!(
        "SELECT
            COALESCE(json_extract(attributes,'$.mcp_server'), '(unknown)') AS server,
            COALESCE(json_extract(attributes,'$.mcp_tool'),   '(unknown)') AS tool,
            COALESCE(json_extract(attributes,'$.status'),     '(unknown)') AS status,
            SUM(COALESCE(value_int, 1)) AS cnt
         FROM metrics
         {where_clause}
         GROUP BY server, tool, status
         ORDER BY server, tool, cnt DESC"
    );
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare mcp_health query: {e}"))
    })?;
    let raw = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| StorageError::QueryError(format!("{e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("{e}")))?;

    // Pivot: (server, tool) -> (ok_count, error_count)
    let mut pivot: HashMap<(String, String), (u64, u64)> = HashMap::new();
    for (server, tool, status, cnt) in raw {
        let entry = pivot.entry((server, tool)).or_default();
        let is_error = status.to_lowercase().contains("error")
            || status.to_lowercase().contains("fail")
            || status == "false";
        if is_error {
            entry.1 += cnt as u64;
        } else {
            entry.0 += cnt as u64;
        }
    }

    let mut entries: Vec<McpHealthEntry> = pivot
        .into_iter()
        .map(|((server, tool), (ok, errors))| {
            let total_calls = ok + errors;
            let error_rate = if total_calls > 0 {
                errors as f64 / total_calls as f64
            } else {
                0.0
            };
            McpHealthEntry {
                server,
                tool,
                ok_calls: ok,
                error_calls: errors,
                total_calls,
                error_rate,
            }
        })
        .collect();
    entries.sort_by(|a, b| {
        b.error_rate
            .partial_cmp(&a.error_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.total_calls.cmp(&a.total_calls))
    });

    Ok(McpHealthResponse {
        entries,
        filters_applied: Vec::new(),
    })
}

/// Codex Guardian review summary by risk level and action. (#162)
pub fn query_guardian_stats(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::GuardianStatsResponse> {
    use otelite_core::api::{GuardianActionEntry, GuardianRiskLevelEntry, GuardianStatsResponse};
    use otelite_core::semconv::metric_names as mnames;

    let mut where_clause = String::from("WHERE name = ? AND json_valid(attributes)");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(mnames::CODEX_GUARDIAN_REVIEW.to_string())];
    if let Some(s) = start_time {
        where_clause.push_str(" AND timestamp >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND timestamp <= ?");
        params.push(Box::new(e));
    }

    let risk_sql = format!(
        "SELECT
            COALESCE(json_extract(attributes,'$.risk_level'), '(unknown)') AS risk,
            SUM(COALESCE(value_int, 1)) AS cnt
         FROM metrics
         {where_clause}
         GROUP BY risk
         ORDER BY cnt DESC"
    );
    let action_sql = format!(
        "SELECT
            COALESCE(json_extract(attributes,'$.decision'), '(unknown)') AS decision,
            COALESCE(json_extract(attributes,'$.action'),   '(unknown)') AS action,
            SUM(COALESCE(value_int, 1)) AS cnt
         FROM metrics
         {where_clause}
         GROUP BY decision, action
         ORDER BY cnt DESC"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&risk_sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare guardian risk query: {e}"))
    })?;
    let by_risk: Vec<GuardianRiskLevelEntry> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(GuardianRiskLevelEntry {
                risk_level: r.get(0)?,
                count: r.get::<_, i64>(1)? as u64,
                approval_rate: 0.0, // enriched below
            })
        })
        .map_err(|e| StorageError::QueryError(format!("{e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("{e}")))?;

    let total: u64 = by_risk.iter().map(|r| r.count).sum();

    let mut stmt2 = conn.prepare(&action_sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare guardian action query: {e}"))
    })?;
    // action_sql selects: decision (col0), action (col1), cnt (col2)
    // GuardianActionEntry has: action, count, denial_rate
    let by_action: Vec<GuardianActionEntry> = stmt2
        .query_map(param_refs.as_slice(), |r| {
            let _decision: String = r.get(0)?; // decision label — not a struct field
            let action: String = r.get(1)?;
            let count: u64 = r.get::<_, i64>(2)? as u64;
            Ok(GuardianActionEntry {
                action,
                count,
                denial_rate: 0.0, // enriched below
            })
        })
        .map_err(|e| StorageError::QueryError(format!("{e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("{e}")))?;

    // Approximate approval rate: reviews whose action was not a denial/block.
    let denied: u64 = by_action
        .iter()
        .filter(|a| {
            a.action.to_lowercase().contains("den") || a.action.to_lowercase().contains("block")
        })
        .map(|a| a.count)
        .sum();
    let approval_rate = if total > 0 {
        1.0 - denied as f64 / total as f64
    } else {
        0.0
    };

    Ok(GuardianStatsResponse {
        total_reviews: total,
        approval_rate,
        by_risk_level: by_risk,
        by_action,
        filters_applied: Vec::new(),
    })
}

/// Codex multi-agent spawn/resume topology by role. (#163)
pub fn query_multi_agent_stats(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::MultiAgentStatsResponse> {
    use otelite_core::api::{MultiAgentRoleEntry, MultiAgentStatsResponse};
    use otelite_core::semconv::metric_names as mnames;

    fn metric_by_role(
        conn: &Connection,
        metric_name: &str,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<Vec<(String, u64)>> {
        let mut where_clause = String::from("WHERE name = ? AND json_valid(attributes)");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(metric_name.to_string())];
        if let Some(s) = start_time {
            where_clause.push_str(" AND timestamp >= ?");
            params.push(Box::new(s));
        }
        if let Some(e) = end_time {
            where_clause.push_str(" AND timestamp <= ?");
            params.push(Box::new(e));
        }
        let sql = format!(
            "SELECT COALESCE(json_extract(attributes,'$.role'), '(unknown)') AS role,
                    SUM(COALESCE(value_int, 1)) AS cnt
             FROM metrics {where_clause} GROUP BY role ORDER BY cnt DESC"
        );
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql).map_err(|e| {
            StorageError::QueryError(format!("Failed to prepare multi_agent query: {e}"))
        })?;
        let rows = stmt
            .query_map(param_refs.as_slice(), |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as u64))
            })
            .map_err(|e| StorageError::QueryError(format!("{e}")))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| StorageError::QueryError(format!("{e}")))?;
        Ok(rows)
    }

    let spawn_by_role =
        metric_by_role(conn, mnames::CODEX_MULTI_AGENT_SPAWN, start_time, end_time)?;
    let resume_by_role =
        metric_by_role(conn, mnames::CODEX_MULTI_AGENT_RESUME, start_time, end_time)?;

    // Merge into MultiAgentRoleEntry
    let mut role_map: HashMap<String, (u64, u64)> = HashMap::new();
    for (role, cnt) in spawn_by_role {
        role_map.entry(role).or_default().0 += cnt;
    }
    for (role, cnt) in resume_by_role {
        role_map.entry(role).or_default().1 += cnt;
    }

    let total_spawns: u64 = role_map.values().map(|v| v.0).sum();
    let total_resumes: u64 = role_map.values().map(|v| v.1).sum();

    let total_events = total_spawns + total_resumes;
    let mut roles: Vec<MultiAgentRoleEntry> = role_map
        .into_iter()
        .map(|(role, (spawns, resumes))| {
            let share_pct = if total_events > 0 {
                (spawns + resumes) as f64 / total_events as f64 * 100.0
            } else {
                0.0
            };
            MultiAgentRoleEntry {
                role,
                spawns,
                resumes,
                share_pct,
            }
        })
        .collect();
    roles.sort_by_key(|a| std::cmp::Reverse(a.spawns + a.resumes));

    Ok(MultiAgentStatsResponse {
        total_spawns,
        total_resumes,
        roles,
        filters_applied: Vec::new(),
    })
}

/// Codex turn busy vs idle breakdown per model and project. (#164)
///
/// Uses `run_sampling_request` spans which carry `busy_ns`, `idle_ns`, and
/// `cwd` attributes. Project is the basename of `cwd`.
pub fn query_codex_turn_breakdown(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::CodexTurnBreakdownResponse> {
    use otelite_core::api::{CodexTurnBreakdownResponse, CodexTurnBreakdownRow};

    let mut where_clause =
        String::from("WHERE name = 'run_sampling_request' AND json_valid(attributes)");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(e));
    }

    // Extract basename from cwd using SQLite string functions:
    // last segment after the final '/' character.
    let sql = format!(
        "SELECT
            COALESCE(json_extract(attributes,'$.model'), '(unknown)') AS model,
            CASE
                WHEN json_extract(attributes,'$.cwd') IS NOT NULL
                     AND instr(json_extract(attributes,'$.cwd'), '/') > 0
                THEN substr(
                    json_extract(attributes,'$.cwd'),
                    1 + length(rtrim(
                        json_extract(attributes,'$.cwd'),
                        replace(json_extract(attributes,'$.cwd'), '/', '')
                    ))
                )
                ELSE COALESCE(json_extract(attributes,'$.cwd'), '(unknown)')
            END AS project,
            COUNT(*) AS turns,
            AVG((end_time - start_time) / 1000000.0) AS avg_turn_ms,
            AVG(
                CASE WHEN json_extract(attributes,'$.busy_ns') IS NOT NULL
                     THEN CAST(json_extract(attributes,'$.busy_ns') AS REAL) / 1000000.0
                     ELSE NULL END
            ) AS avg_busy_ms,
            AVG(
                CASE WHEN json_extract(attributes,'$.idle_ns') IS NOT NULL
                     THEN CAST(json_extract(attributes,'$.idle_ns') AS REAL) / 1000000.0
                     ELSE NULL END
            ) AS avg_idle_ms
         FROM spans
         {where_clause}
         GROUP BY model, project
         ORDER BY turns DESC"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare codex_turn_breakdown query: {e}"))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |r| {
            let turn_count: u64 = r.get::<_, i64>(2)? as u64;
            let avg_duration_ms: f64 = r.get::<_, f64>(3).unwrap_or(0.0);
            let avg_busy_ms: f64 = r.get::<_, Option<f64>>(4)?.unwrap_or(0.0);
            let avg_idle_ms: f64 = r.get::<_, Option<f64>>(5)?.unwrap_or(0.0);
            let busy_ratio = if avg_duration_ms > 0.0 {
                avg_busy_ms / avg_duration_ms
            } else {
                0.0
            };
            Ok(CodexTurnBreakdownRow {
                model: r.get(0)?,
                project: r.get(1)?,
                turn_count,
                avg_duration_ms,
                avg_busy_ms,
                avg_idle_ms,
                busy_ratio,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("{e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("{e}")))?;

    Ok(CodexTurnBreakdownResponse {
        rows,
        filters_applied: Vec::new(),
    })
}

/// Session × model cross-tab: tokens and estimated cost per (session_id, model) pair. (#115)
///
/// Returns rows sorted by requests descending. Spans that carry no session.id
/// are grouped under "(no session)". Cost is unenriched here — the API layer
/// prices each row from the model field after this function returns.
pub fn query_session_model_breakdown(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::SessionModelBreakdown> {
    use otelite_core::{
        api::{SessionModelBreakdown, SessionModelRow},
        semconv,
    };

    let session_expr = semconv::session_id_expr("attributes");
    let model_expr = semconv::coalesce_extract("attributes", semconv::MODEL_KEYS);
    let input_expr =
        semconv::coalesce_extract_cast("attributes", semconv::INPUT_TOKEN_KEYS, "INTEGER");
    let output_expr =
        semconv::coalesce_extract_cast("attributes", semconv::OUTPUT_TOKEN_KEYS, "INTEGER");
    let llm_guard = semconv::llm_span_guard("attributes");

    let mut where_clause = format!(
        "WHERE ({llm_guard} OR name = '{llm_req}')",
        llm_req = semconv::LLM_REQUEST_SPAN_NAME
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(e));
    }

    let sql = format!(
        "SELECT
            COALESCE({session_expr}, '(no session)') AS session_id,
            COALESCE({model_expr}, '(unknown)') AS model,
            COUNT(*) AS requests,
            COALESCE(SUM({input_expr}), 0) AS input_tokens,
            COALESCE(SUM({output_expr}), 0) AS output_tokens
         FROM spans
         {where_clause}
         GROUP BY session_id, model
         ORDER BY requests DESC"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!(
            "Failed to prepare session_model_breakdown query: {e}"
        ))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(SessionModelRow {
                session_id: r.get::<_, String>(0)?,
                model: r.get::<_, String>(1)?,
                requests: r.get::<_, i64>(2)? as u64,
                input_tokens: r.get::<_, i64>(3)? as u64,
                output_tokens: r.get::<_, i64>(4)? as u64,
                cost: None, // priced by the API layer
            })
        })
        .map_err(|e| StorageError::QueryError(format!("{e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("{e}")))?;

    Ok(SessionModelBreakdown {
        rows,
        filters_applied: Vec::new(),
    })
}

/// Speed/effort attribute distribution across Claude Code LLM spans. (#114)
///
/// Groups by (speed, model) and returns counts + token sums. Rows where
/// `speed` is absent are included with `speed = None` so callers can see
/// the split between instrumented and un-instrumented spans.
pub fn query_speed_distribution(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::SpeedDistribution> {
    use otelite_core::{
        api::{SpeedBucket, SpeedDistribution},
        semconv,
    };

    let model_expr = semconv::coalesce_extract("attributes", semconv::MODEL_KEYS);
    let input_expr =
        semconv::coalesce_extract_cast("attributes", semconv::INPUT_TOKEN_KEYS, "INTEGER");
    let output_expr =
        semconv::coalesce_extract_cast("attributes", semconv::OUTPUT_TOKEN_KEYS, "INTEGER");
    let llm_guard = semconv::llm_span_guard("attributes");

    // Include only spans that originate from Claude Code (the only instrumentation
    // known to emit `speed`). Using the full LLM guard keeps the result general
    // in case other frameworks adopt the attribute later.
    let mut where_clause = format!(
        "WHERE ({llm_guard} OR name = '{llm_req}')",
        llm_req = semconv::LLM_REQUEST_SPAN_NAME
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    if let Some(s) = start_time {
        where_clause.push_str(" AND start_time >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND end_time <= ?");
        params.push(Box::new(e));
    }

    let speed_key = semconv::SPEED_KEY;
    let sql = format!(
        "SELECT
            CASE WHEN json_valid(attributes)
                 THEN json_extract(attributes, '$.\"{speed_key}\"')
                 ELSE NULL END AS speed,
            COALESCE({model_expr}, '(unknown)') AS model,
            COUNT(*) AS requests,
            COALESCE(SUM({input_expr}), 0) AS input_tokens,
            COALESCE(SUM({output_expr}), 0) AS output_tokens
         FROM spans
         {where_clause}
         GROUP BY speed, model
         ORDER BY requests DESC"
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare speed_distribution query: {e}"))
    })?;

    let rows = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(SpeedBucket {
                speed: r.get::<_, Option<String>>(0)?,
                model: r.get::<_, String>(1)?,
                requests: r.get::<_, i64>(2)? as u64,
                input_tokens: r.get::<_, i64>(3)? as u64,
                output_tokens: r.get::<_, i64>(4)? as u64,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("{e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("{e}")))?;

    Ok(SpeedDistribution {
        rows,
        filters_applied: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite::schema;

    fn setup_test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        schema::initialize_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_query_logs_empty() {
        let conn = setup_test_db();
        let params = QueryParams::default();
        let logs = query_logs(&conn, &params).unwrap();
        assert_eq!(logs.len(), 0);
    }

    #[test]
    fn test_parse_log_row_tolerates_malformed_json() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO logs (
                timestamp, severity_number, body, attributes, resource
            ) VALUES (100, 9, 'corrupt log', '{', '[')",
            [],
        )
        .unwrap();

        let log = conn
            .query_row("SELECT * FROM logs", [], parse_log_row)
            .unwrap();

        assert!(log.attributes.is_empty());
        assert_eq!(log.resource, None);
    }

    #[test]
    fn test_parse_json_or_none_accepts_null() {
        let resource: Option<otelite_core::telemetry::Resource> =
            parse_json_or_none("null", "resource", "log record");

        assert_eq!(resource, None);
    }

    #[test]
    fn test_parse_span_row_tolerates_malformed_json() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO spans (
                trace_id, span_id, name, kind, start_time, end_time,
                attributes, events, resource, status_code
            ) VALUES ('trace', 'span', 'corrupt span', 0, 100, 200, '{', '[', '{', 1)",
            [],
        )
        .unwrap();

        let span = conn
            .query_row("SELECT * FROM spans", [], parse_span_row)
            .unwrap();

        assert!(span.attributes.is_empty());
        assert!(span.events.is_empty());
        assert_eq!(span.resource, None);
    }

    #[test]
    fn test_query_token_usage_tolerates_malformed_attributes() {
        // Regression: the LLM guard is used as a partial-index predicate and
        // is evaluated on every scanned row. Before the guard clauses were
        // gated with json_valid, a single span with corrupt `attributes` in
        // the time window made every GenAI query over that window fail with
        // "malformed JSON" — and would have rejected the INSERT itself now
        // that the index exists. Corrupt spans must be skipped instead.
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
             VALUES ('t1', 's1', 'llm.call', 0, 100, 200,
                     '{\"gen_ai.system\":\"anthropic\",\"gen_ai.request.model\":\"claude-opus-4-7\",\"gen_ai.usage.input_tokens\":50,\"gen_ai.usage.output_tokens\":25}', 1)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
             VALUES ('t2', 's2', 'corrupt', 0, 100, 200, '{', 1)",
            [],
        )
        .unwrap();

        let (summary, by_model, by_system) =
            query_token_usage(&conn, Some(50), Some(300), &GenAiFilters::default()).unwrap();

        assert_eq!(summary.total_requests, 1);
        assert_eq!(summary.total_input_tokens, 50);
        assert_eq!(summary.total_output_tokens, 25);
        assert_eq!(by_model.len(), 1);
        // Identity is `provider/model` when a provider is recorded (#143).
        assert_eq!(by_model[0].model, "anthropic/claude-opus-4-7");
        assert_eq!(by_model[0].input_tokens, 50);
        assert_eq!(by_model[0].output_tokens, 25);
        assert_eq!(by_system.len(), 1);
        assert_eq!(by_system[0].system, "anthropic");
    }

    #[test]
    fn test_parse_metric_row_tolerates_malformed_json() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO metrics (
                name, metric_type, timestamp, value_histogram, attributes, resource
            ) VALUES ('corrupt.histogram', 2, 100, '{', '{', '[')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metrics (
                name, metric_type, timestamp, value_summary, attributes, resource
            ) VALUES ('corrupt.summary', 3, 200, '[', '{', '[')",
            [],
        )
        .unwrap();

        let mut stmt = conn
            .prepare("SELECT * FROM metrics ORDER BY timestamp")
            .unwrap();
        let metrics = stmt
            .query_map([], parse_metric_row)
            .unwrap()
            .collect::<std::result::Result<Vec<_>, _>>()
            .unwrap();

        assert!(metrics.iter().all(|metric| metric.attributes.is_empty()));
        assert!(metrics.iter().all(|metric| metric.resource.is_none()));
        assert!(matches!(
            metrics[0].metric_type,
            otelite_core::telemetry::metric::MetricType::Histogram {
                count: 0,
                sum: 0.0,
                ref buckets
            } if buckets.is_empty()
        ));
        assert!(matches!(
            metrics[1].metric_type,
            otelite_core::telemetry::metric::MetricType::Summary {
                count: 0,
                sum: 0.0,
                ref quantiles
            } if quantiles.is_empty()
        ));
    }

    #[test]
    fn test_get_stats_empty() {
        let conn = setup_test_db();
        let stats = get_stats(&conn).unwrap();
        assert_eq!(stats.log_count, 0);
        assert_eq!(stats.span_count, 0);
        assert_eq!(stats.metric_count, 0);
        assert_eq!(stats.oldest_timestamp, None);
        assert_eq!(stats.newest_timestamp, None);
    }

    #[test]
    fn test_get_stats_min_max_across_tables() {
        let conn = setup_test_db();
        // Oldest is a log, newest span is found via MAX(end_time) — a
        // different column than MIN(start_time) uses — and the newest
        // metric sits between them. This exercises the per-table scalar
        // MIN/MAX aggregation (each an index seek, no full-table scan).
        conn.execute(
            "INSERT INTO logs (timestamp, severity_number, body) VALUES (500, 9, 'old')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time)
             VALUES ('t1', 's1', 'n', 0, 1000, 9000)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp) VALUES ('m', 1, 7000)",
            [],
        )
        .unwrap();

        let stats = get_stats(&conn).unwrap();
        assert_eq!(stats.log_count, 1);
        assert_eq!(stats.span_count, 1);
        assert_eq!(stats.metric_count, 1);
        // Oldest overall is the log at 500 (spans start at 1000).
        assert_eq!(stats.oldest_timestamp, Some(500));
        // Newest overall is the span's END time (9000), not its start (1000)
        // and not the metric (7000) — proves MAX uses end_time.
        assert_eq!(stats.newest_timestamp, Some(9000));
    }

    #[test]
    fn test_query_latest_metrics_per_name() {
        let conn = setup_test_db();
        for (name, ts) in [
            ("alpha", 100i64),
            ("alpha", 200),
            ("beta", 150),
            ("beta", 50),
        ] {
            conn.execute(
                "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes, resource)
                 VALUES (?1, 1, ?2, 1, '{}', '{}')",
                rusqlite::params![name, ts],
            )
            .unwrap();
        }

        let metrics = query_latest_metrics(&conn, &QueryParams::default()).unwrap();
        let got: Vec<(&str, i64)> = metrics
            .iter()
            .map(|m| (m.name.as_str(), m.timestamp))
            .collect();
        // One row per name (its most recent), sorted by name.
        assert_eq!(got, vec![("alpha", 200), ("beta", 150)]);
    }

    #[test]
    fn test_query_latest_metrics_ties_all_returned() {
        let conn = setup_test_db();
        // Two rows for the same name at the same maximum timestamp: the
        // previous HAVING form returned both, and the JOIN form must too.
        for _ in 0..2 {
            conn.execute(
                "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes, resource)
                 VALUES ('a', 1, 100, 1, '{}', '{}')",
                [],
            )
            .unwrap();
        }

        let metrics = query_latest_metrics(&conn, &QueryParams::default()).unwrap();
        assert_eq!(metrics.len(), 2);
        assert!(metrics.iter().all(|m| m.name == "a" && m.timestamp == 100));
    }

    #[test]
    fn test_query_latest_metrics_window_applied_after_dedup() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes, resource) VALUES ('a', 1, 1000, 1, '{}', '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes, resource) VALUES ('a', 1, 2000, 1, '{}', '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes, resource) VALUES ('b', 1, 500, 1, '{}', '{}')",
            [],
        )
        .unwrap();

        // Window that excludes 'a's latest point (2000) but includes an older
        // one (1000): dedup happens first, so 'a' is absent entirely and 'b'
        // (whose only point is outside the window) is too.
        let params = QueryParams {
            start_time: Some(1500),
            end_time: Some(1500),
            ..Default::default()
        };
        let metrics = query_latest_metrics(&conn, &params).unwrap();
        assert!(metrics.is_empty());

        // Window that includes 'a's latest point returns it.
        let params = QueryParams {
            start_time: Some(1999),
            end_time: Some(2001),
            ..Default::default()
        };
        let metrics = query_latest_metrics(&conn, &params).unwrap();
        let got: Vec<(&str, i64)> = metrics
            .iter()
            .map(|m| (m.name.as_str(), m.timestamp))
            .collect();
        assert_eq!(got, vec![("a", 2000)]);
    }

    #[test]
    fn test_query_latest_metrics_name_predicate_not_ambiguous() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes, resource)
             VALUES ('a', 1, 100, 1, '{}', '{}')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes, resource) VALUES ('b', 1, 200, 1, '{}', '{}')",
            [],
        )
        .unwrap();

        // A `name = ?` predicate must resolve against the table in the
        // JOIN form (the dedup subquery aliases its columns).
        let mut params = QueryParams::default();
        params.predicates.push(QueryPredicate {
            field: "name".to_string(),
            operator: Operator::Equal,
            value: QueryValue::String("b".to_string()),
        });
        let metrics = query_latest_metrics(&conn, &params).unwrap();
        assert_eq!(metrics.len(), 1);
        assert_eq!(metrics[0].name, "b");
    }

    #[test]
    fn test_query_distinct_metric_names_sorted() {
        let conn = setup_test_db();
        for name in ["zeta", "alpha", "zeta", "mid"] {
            conn.execute(
                "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes, resource)
                 VALUES (?1, 1, 1, 1, '{}', '{}')",
                [name],
            )
            .unwrap();
        }
        assert_eq!(
            query_distinct_metric_names(&conn).unwrap(),
            vec!["alpha".to_string(), "mid".to_string(), "zeta".to_string()]
        );
    }

    #[test]
    fn test_distinct_resource_keys_dedups_and_tolerates_corrupt_json() {
        let conn = setup_test_db();
        for i in 0..5 {
            conn.execute(
                "INSERT INTO logs (timestamp, severity_number, body, resource)
                 VALUES (?1, 9, 'b', '{\"attributes\":{\"service.name\":\"svc\",\"k\":\"v\"}}')",
                [i as i64],
            )
            .unwrap();
        }
        // A corrupt resource JSON must not fail the query (json_valid gate).
        conn.execute(
            "INSERT INTO logs (timestamp, severity_number, body, resource)
             VALUES (99, 9, 'b', '{not json')",
            [],
        )
        .unwrap();

        let keys = distinct_resource_keys(&conn, "logs").unwrap();
        assert_eq!(keys.len(), 2);
        assert!(keys.contains(&"service.name".to_string()));
        assert!(keys.contains(&"k".to_string()));
    }

    #[test]
    fn test_distinct_resource_keys_unknown_signal() {
        let conn = setup_test_db();
        assert!(distinct_resource_keys(&conn, "traces").is_err());
    }

    #[test]
    fn test_trace_list_ordering_matches_group_by_max() {
        let conn = setup_test_db();
        // Interleaved multi-span traces. Max start per trace:
        // t1 = 100, t2 = 95, t3 = 99, t4 = 80. Expected top-3: t1, t3, t2.
        // t1 also has a span OUTSIDE the window (start 10, end 15) which the
        // old outer query returned too — keep that behaviour.
        let spans: &[(i64, i64, &str)] = &[
            (100, 110, "t1"),
            (90, 95, "t1"),
            (10, 15, "t1"),
            (95, 96, "t2"),
            (99, 105, "t3"),
            (80, 81, "t4"),
        ];
        for (start, end, trace) in spans {
            conn.execute(
                "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                    attributes, events, resource, status_code)
                 VALUES (?1, ?1 || '-s', 'n', 0, ?2, ?3, '{}', '[]', '{}', 0)",
                rusqlite::params![trace, start, end],
            )
            .unwrap();
        }

        let params = QueryParams {
            start_time: Some(0),
            end_time: Some(1_000_000),
            ..Default::default()
        };
        let got = query_spans_for_trace_list(&conn, &params, 3).unwrap();
        // All spans of the three selected traces (t1's 3, t3's 1, t2's 1),
        // t4 excluded, ordered by start_time DESC overall.
        let trace_ids: Vec<&str> = got.iter().map(|s| s.trace_id.as_str()).collect();
        assert_eq!(trace_ids.len(), 5);
        assert!(!trace_ids.contains(&"t4"));
        let starts: Vec<i64> = got.iter().map(|s| s.start_time).collect();
        assert_eq!(starts, vec![100, 99, 95, 90, 10]);
    }

    #[test]
    fn test_trace_list_stops_at_limit() {
        let conn = setup_test_db();
        for i in 0..10 {
            conn.execute(
                "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                    attributes, events, resource, status_code)
                 VALUES (?1, ?1 || '-s', 'n', 0, ?2, ?2 + 1, '{}', '[]', '{}', 0)",
                rusqlite::params![format!("t{}", i), 1000 - i],
            )
            .unwrap();
        }
        let got = query_spans_for_trace_list(&conn, &QueryParams::default(), 4).unwrap();
        let trace_ids: Vec<&str> = got.iter().map(|s| s.trace_id.as_str()).collect();
        assert_eq!(trace_ids, vec!["t0", "t1", "t2", "t3"]);
    }

    #[test]
    fn test_trace_list_specific_trace_window_mismatch_empty() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                attributes, events, resource, status_code)
             VALUES ('t1', 's1', 'n', 0, 100, 110, '{}', '[]', '{}', 0)",
            [],
        )
        .unwrap();

        // Window that does not contain the trace's only span → empty, as the
        // old subquery-window semantics required.
        let mut params = QueryParams {
            trace_id: Some("t1".to_string()),
            start_time: Some(200),
            end_time: Some(300),
            ..Default::default()
        };
        assert!(query_spans_for_trace_list(&conn, &params, 10)
            .unwrap()
            .is_empty());

        // Window that contains it → the full trace comes back.
        params.start_time = Some(0);
        params.end_time = Some(1_000);
        let got = query_spans_for_trace_list(&conn, &params, 10).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].trace_id, "t1");
    }

    #[test]
    fn test_trace_list_predicates_constrain_trace_selection() {
        let conn = setup_test_db();
        // ta carries the gen_ai attribute (and is the newest trace), tb
        // does not. Before the fix the phase-1 scan ignored predicates,
        // so tb was selected and returned even though /spans filters it
        // out.
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                attributes, events, resource, status_code)
             VALUES ('ta', 'sa', 'n', 0, 200, 210,
                     '{\"gen_ai.request.model\":\"gpt-4o\"}', '[]', '{}', 0)",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time,
                                attributes, events, resource, status_code)
             VALUES ('tb', 'sb', 'n', 0, 100, 110, '{}', '[]', '{}', 0)",
            [],
        )
        .unwrap();

        let model_pred = QueryPredicate {
            field: "gen_ai.request.model".to_string(),
            operator: Operator::Equal,
            value: QueryValue::String("gpt-4o".to_string()),
        };

        // With the predicate: only ta qualifies.
        let params = QueryParams {
            predicates: vec![model_pred.clone()],
            ..Default::default()
        };
        let got = query_spans_for_trace_list(&conn, &params, 10).unwrap();
        let trace_ids: Vec<&str> = got.iter().map(|s| s.trace_id.as_str()).collect();
        assert_eq!(trace_ids, vec!["ta"]);

        // Without it: both traces come back (sanity — the predicate is
        // what filters, not anything about ordering or windows).
        let got = query_spans_for_trace_list(&conn, &QueryParams::default(), 10).unwrap();
        let trace_ids: Vec<&str> = got.iter().map(|s| s.trace_id.as_str()).collect();
        assert_eq!(trace_ids, vec!["ta", "tb"]);

        // Explicit trace + predicate the trace does not satisfy → empty,
        // matching the window-check semantics (a span in the window must
        // also match the predicates).
        let params = QueryParams {
            trace_id: Some("tb".to_string()),
            predicates: vec![model_pred],
            ..Default::default()
        };
        assert!(query_spans_for_trace_list(&conn, &params, 10)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn test_field_to_sql_for_attribute_field() {
        let sql = field_to_sql("logs", "gen_ai.system").unwrap();
        assert_eq!(sql, "json_extract(attributes, '$.\"gen_ai.system\"')");
    }

    #[test]
    fn test_field_to_sql_for_explicit_attribute_prefix() {
        let sql = field_to_sql("logs", "attributes.http.method").unwrap();
        assert_eq!(sql, "json_extract(attributes, '$.\"http.method\"')");
    }

    #[test]
    fn test_field_to_sql_for_resource_prefix() {
        let sql = field_to_sql("logs", "resource.service.name").unwrap();
        assert_eq!(
            sql,
            "json_extract(resource, '$.attributes.\"service.name\"')"
        );
    }

    #[test]
    fn test_json_key_accessor_quotes_dotted_keys() {
        assert_eq!(json_key_accessor("service.name"), ".\"service.name\"");
    }

    #[test]
    fn test_predicate_to_sql_for_attribute_equality() {
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let sql = predicate_to_sql(
            "logs",
            &QueryPredicate {
                field: "gen_ai.system".to_string(),
                operator: Operator::Equal,
                value: QueryValue::String("anthropic".to_string()),
            },
            &mut params,
        )
        .unwrap();

        assert_eq!(sql, "json_extract(attributes, '$.\"gen_ai.system\"') = ?");
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn test_predicate_to_sql_for_resource_equality() {
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
        let sql = predicate_to_sql(
            "logs",
            &QueryPredicate {
                field: "resource.service.name".to_string(),
                operator: Operator::Equal,
                value: QueryValue::String("gateway".to_string()),
            },
            &mut params,
        )
        .unwrap();

        assert_eq!(
            sql,
            "json_extract(resource, '$.attributes.\"service.name\"') = ?"
        );
        assert_eq!(params.len(), 1);
    }

    #[test]
    fn test_span_duration_predicate_requires_duration_value() {
        let mut params = Vec::new();
        let err = predicate_to_sql(
            "spans",
            &QueryPredicate {
                field: "duration".to_string(),
                operator: Operator::GreaterThan,
                value: QueryValue::Number(100.0),
            },
            &mut params,
        )
        .unwrap_err();

        assert!(err
            .to_string()
            .contains("requires a duration value like 500ms"));
    }

    #[test]
    fn test_query_logs_with_structured_attribute_and_resource_predicates() {
        let conn = setup_test_db();
        conn.execute(
            "INSERT INTO logs (
                timestamp, observed_timestamp, trace_id, span_id,
                severity_number, severity_text, body, attributes, resource, scope
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            rusqlite::params![
                1000_i64,
                1000_i64,
                "trace-a",
                "span-a",
                SeverityLevel::Info.to_i32(),
                "INFO",
                "matching log body",
                r#"{"gen_ai.system":"anthropic"}"#,
                r#"{"attributes":{"service.name":"gateway"}}"#,
                "{}",
            ],
        )
        .unwrap();

        let params = QueryParams {
            predicates: vec![
                QueryPredicate {
                    field: "gen_ai.system".to_string(),
                    operator: Operator::Equal,
                    value: QueryValue::String("anthropic".to_string()),
                },
                QueryPredicate {
                    field: "resource.service.name".to_string(),
                    operator: Operator::Equal,
                    value: QueryValue::String("gateway".to_string()),
                },
            ],
            ..Default::default()
        };

        let attr_match: Option<String> = conn
            .query_row(
                "SELECT json_extract(attributes, '$.\"gen_ai.system\"') FROM logs WHERE timestamp = 1000",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let resource_match: Option<String> = conn
            .query_row(
                "SELECT json_extract(resource, '$.attributes.\"service.name\"') FROM logs WHERE timestamp = 1000",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(attr_match.as_deref(), Some("anthropic"));
        assert_eq!(resource_match.as_deref(), Some("gateway"));

        let logs = query_logs(&conn, &params).unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].body, "matching log body");
    }

    static SPAN_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fn next_id() -> String {
        let n = SPAN_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        format!("id-{n}")
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_llm_span(
        conn: &Connection,
        model: &str,
        input: i64,
        output: i64,
        stop_reason: Option<&str>,
        context: Option<&str>,
    ) {
        let attrs = serde_json::json!({
            "model": model,
            "input_tokens": input,
            "output_tokens": output,
            "stop_reason": stop_reason,
            "llm_request.context": context,
        });
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
             VALUES (?, ?, 'claude_code.llm_request', 0, 1000000000, 2000000000, ?, '{}', '[]', '[]', '{}')",
            rusqlite::params![next_id(), next_id(), attrs.to_string()],
        ).unwrap();
    }

    fn insert_tool_decision(conn: &Connection, decision: &str, source: &str, tool_name: &str) {
        let attrs = serde_json::json!({
            "decision": decision,
            "source": source,
            "tool_name": tool_name,
        });
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
             VALUES (?, ?, 'claude_code.tool.blocked_on_user', 0, 1000000000, 2000000000, ?, '{}', '[]', '[]', '{}')",
            rusqlite::params![next_id(), next_id(), attrs.to_string()],
        ).unwrap();
    }

    fn insert_failed_tool(conn: &Connection, tool_name: &str, error: &str) {
        let attrs = serde_json::json!({
            "tool_name": tool_name,
            "success": "false",
            "error": error,
        });
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
             VALUES (?, ?, 'claude_code.tool.execution', 0, 1000000000, 2000000000, ?, '{}', '[]', '[]', '{}')",
            rusqlite::params![next_id(), next_id(), attrs.to_string()],
        ).unwrap();
    }

    #[test]
    fn test_query_tool_approvals_empty() {
        let conn = setup_test_db();
        let stats = query_tool_approvals(&conn, None, None, &GenAiFilters::default()).unwrap();
        assert_eq!(stats.total, 0);
        assert_eq!(stats.auto_accepted, 0);
        assert_eq!(stats.rejected, 0);
    }

    #[test]
    fn test_query_tool_approvals_counts() {
        let conn = setup_test_db();
        insert_tool_decision(&conn, "accept", "config", "Bash");
        insert_tool_decision(&conn, "accept", "config", "Read");
        insert_tool_decision(&conn, "accept", "user", "Write");
        insert_tool_decision(&conn, "reject", "user", "Bash");

        let stats = query_tool_approvals(&conn, None, None, &GenAiFilters::default()).unwrap();
        assert_eq!(stats.total, 4);
        assert_eq!(stats.auto_accepted, 2); // accept + source=config
        assert_eq!(stats.user_accepted, 1); // accept + source=user
        assert_eq!(stats.rejected, 1);
        assert_eq!(stats.unknown, 0);
        assert_eq!(stats.top_rejected.len(), 1);
        assert_eq!(stats.top_rejected[0].tool_name, "Bash");
    }

    #[test]
    fn test_query_stop_reasons_empty() {
        let conn = setup_test_db();
        let rows = query_stop_reasons(&conn, None, None, &GenAiFilters::default()).unwrap();
        // No LLM spans → empty vec (no stop_reason attribute, no groupable rows)
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn test_query_stop_reasons_with_data() {
        let conn = setup_test_db();
        insert_llm_span(&conn, "claude-sonnet", 100, 50, Some("tool_use"), None);
        insert_llm_span(&conn, "claude-sonnet", 200, 80, Some("end_turn"), None);
        insert_llm_span(&conn, "claude-sonnet", 150, 60, Some("tool_use"), None);

        let rows = query_stop_reasons(&conn, None, None, &GenAiFilters::default()).unwrap();
        let tool_use = rows
            .iter()
            .find(|r| r.reason == "tool_use")
            .map(|r| r.count);
        let end_turn = rows
            .iter()
            .find(|r| r.reason == "end_turn")
            .map(|r| r.count);
        assert_eq!(tool_use, Some(2));
        assert_eq!(end_turn, Some(1));
    }

    #[test]
    fn test_query_context_type_split_empty() {
        let conn = setup_test_db();
        let rows = query_context_type_split(&conn, None, None, &GenAiFilters::default()).unwrap();
        // Empty DB → no rows (nothing to group by context)
        assert!(rows.is_empty());
    }

    #[test]
    fn test_query_context_type_split_groups_by_context() {
        let conn = setup_test_db();
        insert_llm_span(&conn, "model-a", 100, 50, None, Some("interaction"));
        insert_llm_span(&conn, "model-b", 200, 80, None, Some("interaction"));
        insert_llm_span(&conn, "model-c", 150, 60, None, Some("sub_agent"));

        let rows = query_context_type_split(&conn, None, None, &GenAiFilters::default()).unwrap();
        let interaction = rows.iter().find(|r| r.context == "interaction");
        let sub_agent = rows.iter().find(|r| r.context == "sub_agent");
        assert!(interaction.is_some(), "interaction row missing");
        assert_eq!(interaction.unwrap().calls, 2);
        assert!(sub_agent.is_some(), "sub_agent row missing");
        assert_eq!(sub_agent.unwrap().calls, 1);
    }

    #[test]
    fn test_query_tool_errors_empty() {
        let conn = setup_test_db();
        let rows = query_tool_errors(&conn, None, None, &GenAiFilters::default(), 10).unwrap();
        assert_eq!(rows.len(), 0);
    }

    #[test]
    fn test_query_tool_errors_with_data() {
        let conn = setup_test_db();
        insert_failed_tool(&conn, "Bash", "Shell command failed");
        insert_failed_tool(&conn, "Bash", "Shell command failed");
        insert_failed_tool(&conn, "Read", "File not found");

        let rows = query_tool_errors(&conn, None, None, &GenAiFilters::default(), 10).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].tool_name, "Bash");
        assert_eq!(rows[0].count, 2);
        assert_eq!(rows[1].tool_name, "Read");
        assert_eq!(rows[1].count, 1);
    }

    #[test]
    fn test_query_hour_of_day_returns_24_buckets() {
        let conn = setup_test_db();
        let rows = query_hour_of_day(&conn, None, None, &GenAiFilters::default()).unwrap();
        assert_eq!(rows.len(), 24);
        assert_eq!(rows[0].hour, 0);
        assert_eq!(rows[23].hour, 23);
    }

    #[test]
    fn test_query_hour_of_day_empty_db_all_zero() {
        let conn = setup_test_db();
        let rows = query_hour_of_day(&conn, None, None, &GenAiFilters::default()).unwrap();
        assert!(rows.iter().all(|r| r.llm_calls == 0 && r.tool_calls == 0));
    }

    #[test]
    fn test_query_hour_of_day_data_driven() {
        let conn = setup_test_db();
        // Unix timestamp for 2024-01-01 14:00:00 UTC in nanoseconds → hour 14
        // 1704117600 seconds * 1_000_000_000 ns/s
        let ts_ns: i64 = 1_704_117_600_000_000_000;
        let attrs = serde_json::json!({"model": "test", "input_tokens": 10, "output_tokens": 5});
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
             VALUES (?, ?, 'claude_code.llm_request', 0, ?, ?, ?, '{}', '[]', '[]', '{}')",
            rusqlite::params![next_id(), next_id(), ts_ns, ts_ns + 1_000_000_000, attrs.to_string()],
        ).unwrap();
        // Insert a tool execution at the same hour
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
             VALUES (?, ?, 'claude_code.tool.execution', 0, ?, ?, '{}', '{}', '[]', '[]', '{}')",
            rusqlite::params![next_id(), next_id(), ts_ns, ts_ns + 500_000_000],
        ).unwrap();

        let rows = query_hour_of_day(&conn, None, None, &GenAiFilters::default()).unwrap();
        assert_eq!(rows.len(), 24);
        assert_eq!(rows[14].hour, 14);
        assert_eq!(rows[14].llm_calls, 1, "hour 14 should have 1 LLM call");
        assert_eq!(rows[14].tool_calls, 1, "hour 14 should have 1 tool call");
        // All other hours should be zero
        for (i, b) in rows.iter().enumerate() {
            if i != 14 {
                assert_eq!(b.llm_calls, 0, "hour {i} should have 0 LLM calls");
                assert_eq!(b.tool_calls, 0, "hour {i} should have 0 tool calls");
            }
        }
    }

    // ── session.id predicate (idx_spans_session_id) ────────────────────

    #[test]
    fn test_query_spans_session_id_predicate_returns_only_that_session() {
        let conn = setup_test_db();
        let insert = |sid: Option<&str>| {
            let attrs = serde_json::json!({"session.id": sid, "x": "1"});
            conn.execute(
                "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, status_code, attributes, resource, events, links, scope)
                 VALUES (?, ?, 'test.span', 0, 1000000000, 2000000000, 0, ?, '{}', '[]', '[]', '{}')",
                rusqlite::params![next_id(), next_id(), attrs.to_string()],
            )
            .unwrap();
        };
        insert(Some("ses_a"));
        insert(Some("ses_a"));
        insert(Some("ses_b"));
        insert(None);
        // Corrupt attributes must not raise (total-form expression).
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, status_code, attributes, resource, events, links, scope)
             VALUES (?, ?, 'test.span', 0, 1000000000, 2000000000, 0, '{', '{}', '[]', '[]', '{}')",
            rusqlite::params![next_id(), next_id()],
        )
        .unwrap();

        let params = QueryParams {
            predicates: vec![QueryPredicate {
                field: "session.id".to_string(),
                operator: Operator::Equal,
                value: QueryValue::String("ses_a".to_string()),
            }],
            ..Default::default()
        };
        let spans = query_spans(&conn, &params).unwrap();
        assert_eq!(spans.len(), 2, "exactly the two ses_a spans");

        // Attributes prefix form must behave identically.
        let params = QueryParams {
            predicates: vec![QueryPredicate {
                field: "attributes.session.id".to_string(),
                operator: Operator::Equal,
                value: QueryValue::String("ses_b".to_string()),
            }],
            ..Default::default()
        };
        let spans = query_spans(&conn, &params).unwrap();
        assert_eq!(spans.len(), 1);
        assert_eq!(
            spans[0].attributes.get("session.id"),
            Some(&"ses_b".to_string())
        );
    }

    #[test]
    fn test_query_spans_session_id_predicate_plan_uses_expression_index() {
        let conn = setup_test_db();
        let plan: String = conn
            .prepare("EXPLAIN QUERY PLAN SELECT * FROM spans WHERE 1=1 AND CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END = ? AND json_valid(attributes) AND json_extract(attributes, '$.\"session.id\"') IS NOT NULL ORDER BY start_time DESC LIMIT ?")
            .unwrap()
            .query_map(rusqlite::params!["ses_x", 10], |r| r.get::<_, String>(3))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert!(
            plan.contains("idx_spans_session_id"),
            "session-id predicate must seek idx_spans_session_id, got: {plan}"
        );
    }

    // ── finish reasons (idx_spans_finish_reason) ───────────────────────

    #[test]
    fn test_query_finish_reasons_unions_spans_and_logs() {
        let conn = setup_test_db();
        // Singular finish_reason span in the window.
        let attrs = serde_json::json!({"gen_ai.response.finish_reason": "stop"});
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
             VALUES (?, ?, 'gen_ai.chat', 0, 1000000000, 2000000000, ?, '{}', '[]', '[]', '{}')",
            rusqlite::params![next_id(), next_id(), attrs.to_string()],
        )
        .unwrap();
        // Plural finish_reasons span (a framework outside the LLM name
        // patterns — the guard is attribute-based, so it must be counted).
        let attrs = serde_json::json!({"gen_ai.response.finish_reasons": ["tool_calls", "stop"]});
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
             VALUES (?, ?, 'pi.llm_request', 0, 1000000000, 2000000000, ?, '{}', '[]', '[]', '{}')",
            rusqlite::params![next_id(), next_id(), attrs.to_string()],
        )
        .unwrap();
        // Finish reason outside the window: excluded.
        let attrs = serde_json::json!({"gen_ai.response.finish_reason": "length"});
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
             VALUES (?, ?, 'gen_ai.chat', 0, 900000000, 950000000, ?, '{}', '[]', '[]', '{}')",
            rusqlite::params![next_id(), next_id(), attrs.to_string()],
        )
        .unwrap();
        // Log with a stop_reason inside an API response body.
        let attrs = serde_json::json!({"body": {"stop_reason": "stop"}});
        conn.execute(
            "INSERT INTO logs (timestamp, severity_number, body, attributes, resource)
             VALUES (1500000000, 9, 'claude_code.api_response_body', ?, '{}')",
            rusqlite::params![attrs.to_string()],
        )
        .unwrap();

        let rows = query_finish_reasons(
            &conn,
            Some(1000000000),
            Some(2000000000),
            &GenAiFilters::default(),
        )
        .unwrap();
        let count = |reason: &str| {
            rows.iter()
                .find(|r| r.reason == reason)
                .map(|r| r.count)
                .unwrap_or(0)
        };
        assert_eq!(
            count("stop"),
            3,
            "singular span + plural array + log stop_reason"
        );
        assert_eq!(count("tool_calls"), 1);
        assert_eq!(count("length"), 0, "outside the window");
    }

    // ── tool usage (idx_spans_tool) ────────────────────────────────────

    #[test]
    fn test_query_tool_usage_covers_all_name_sources_and_window() {
        let conn = setup_test_db();
        let insert_tool = |attrs: serde_json::Value, name: &str, ts: i64| {
            conn.execute(
                "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
                 VALUES (?, ?, ?, 0, ?, ?, ?, '{}', '[]', '[]', '{}')",
                rusqlite::params![next_id(), next_id(), name, ts, ts + 1000000000, attrs.to_string()],
            )
            .unwrap();
        };
        insert_tool(
            serde_json::json!({"gen_ai.tool.name": "search"}),
            "agent.tool",
            1000000000,
        );
        insert_tool(
            serde_json::json!({"tool.name": "search"}),
            "agent.tool",
            2000000000,
        );
        insert_tool(
            serde_json::json!({"tool_name": "read_file"}),
            "agent.tool",
            3000000000,
        );
        insert_tool(serde_json::json!({}), "claude_code.tool.Bash", 4000000000);
        insert_tool(serde_json::json!({}), "gen_ai.chat", 5000000000); // not a tool
        insert_tool(
            serde_json::json!({"gen_ai.tool.name": "outside"}),
            "agent.tool",
            9000000000,
        ); // outside window

        let rows = query_tool_usage(
            &conn,
            Some(1000000000),
            Some(8000000000),
            &GenAiFilters::default(),
            10,
        )
        .unwrap();
        let get = |tool: &str| rows.iter().find(|r| r.tool_name == tool);
        assert_eq!(
            get("search").unwrap().count,
            2,
            "both attribute aliases count"
        );
        assert_eq!(get("read_file").unwrap().count, 1);
        assert_eq!(
            get("claude_code.tool.Bash").unwrap().count,
            1,
            "name-based fallback"
        );
        assert!(get("gen_ai.chat").is_none(), "non-tool span excluded");
        assert!(get("outside").is_none(), "outside the window");
    }

    // ── retrieval stats (idx_spans_retrieval) ──────────────────────────

    #[test]
    fn test_query_retrieval_stats_counts_retriever_spans() {
        let conn = setup_test_db();
        let insert = |attrs: serde_json::Value| {
            conn.execute(
                "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, resource, events, links, scope)
                 VALUES (?, ?, 'retrieval', 0, 1000000000, 2000000000, ?, '{}', '[]', '[]', '{}')",
                rusqlite::params![next_id(), next_id(), attrs.to_string()],
            )
            .unwrap();
        };
        insert(serde_json::json!({
            "openinference.span.kind": "RETRIEVER",
            "retrieval.documents": [{"document.score": 0.9}, {"document.score": 0.7}]
        }));
        insert(serde_json::json!({
            "retrieval.query": "what is otelite",
            "retrieval.documents": [{"document.score": 0.5}]
        }));
        insert(serde_json::json!({"gen_ai.request.model": "x"})); // not retrieval

        let stats = query_retrieval_stats(&conn, None, None, &GenAiFilters::default(), 10).unwrap();
        assert_eq!(
            stats.total_retrievals, 2,
            "RETRIEVER kind + retrieval.query span"
        );
        assert!((stats.avg_documents_per_query - 1.5).abs() < 1e-9);
        assert_eq!(
            stats.avg_top_document_score,
            Some(0.7),
            "average of 0.9 and 0.5"
        );
        assert_eq!(stats.top_queries.len(), 1);
        assert_eq!(stats.top_queries[0].query, "what is otelite");

        // Empty database returns defaults, not an error.
        let empty = setup_test_db();
        let stats =
            query_retrieval_stats(&empty, None, None, &GenAiFilters::default(), 10).unwrap();
        assert_eq!(stats.total_retrievals, 0);
    }

    // ── counter_window_deltas ────────────────────────────────────────────────

    fn insert_counter_row(
        conn: &Connection,
        name: &str,
        timestamp: i64,
        value: i64,
        attributes: &str,
    ) {
        conn.execute(
            "INSERT INTO metrics (
                name, metric_type, timestamp, value_int, attributes
            ) VALUES (?1, 1, ?2, ?3, ?4)",
            rusqlite::params![name, timestamp, value, attributes],
        )
        .unwrap();
    }

    fn label_key(labels: &[Option<String>]) -> String {
        labels
            .iter()
            .map(|l| l.clone().unwrap_or_else(|| String::from("(null)")))
            .collect::<Vec<_>>()
            .join("|")
    }

    fn deltas_by_label(deltas: Vec<CounterWindowDelta>) -> std::collections::HashMap<String, f64> {
        deltas
            .into_iter()
            .map(|d| (label_key(&d.labels), d.delta))
            .collect()
    }

    const COUNTER_TEST: &str = "opencode.token.usage";
    const T0: i64 = 1_700_000_000_000_000_000;

    #[test]
    fn counter_window_deltas_monotonic_series() {
        let conn = setup_test_db();
        // Series A (agent=a): 100 @ T0, 150 @ T0+1, 250 @ T0+2.
        let a = r#"{"agent":"a","model":"m","type":"input","session.id":"s1"}"#;
        insert_counter_row(&conn, COUNTER_TEST, T0, 100, a);
        insert_counter_row(&conn, COUNTER_TEST, T0 + 1, 150, a);
        insert_counter_row(&conn, COUNTER_TEST, T0 + 2, 250, a);

        // Window [T0+1, T0+2]: delta = 250 - 100 = 150.
        let deltas = counter_window_deltas(
            &conn,
            COUNTER_TEST,
            &["$.agent"],
            Some(T0 + 1),
            Some(T0 + 2),
        )
        .unwrap();
        let by_label = deltas_by_label(deltas);
        assert_eq!(by_label.get("a"), Some(&150.0));

        // Window starting at series start: no baseline -> delta = last value.
        let deltas =
            counter_window_deltas(&conn, COUNTER_TEST, &["$.agent"], Some(T0), Some(T0 + 2))
                .unwrap();
        let by_label = deltas_by_label(deltas);
        assert_eq!(by_label.get("a"), Some(&250.0));
    }

    #[test]
    fn counter_window_deltas_duplicate_timestamp_takes_max() {
        let conn = setup_test_db();
        // Two flushes at the same tick: the max value at the max timestamp wins.
        let a = r#"{"agent":"a","model":"m","type":"input","session.id":"s1"}"#;
        insert_counter_row(&conn, COUNTER_TEST, T0, 300, a);
        insert_counter_row(&conn, COUNTER_TEST, T0 + 1, 400, a);
        insert_counter_row(&conn, COUNTER_TEST, T0 + 1, 500, a);

        let deltas = counter_window_deltas(
            &conn,
            COUNTER_TEST,
            &["$.agent"],
            Some(T0 + 1),
            Some(T0 + 1),
        )
        .unwrap();
        let by_label = deltas_by_label(deltas);
        assert_eq!(
            by_label.get("a"),
            Some(&200.0),
            "duplicate-timestamp max (500) minus baseline (300)"
        );
    }

    #[test]
    fn counter_window_deltas_reset_restarts_from_zero() {
        let conn = setup_test_db();
        let a = r#"{"agent":"a","model":"m","type":"input","session.id":"s1"}"#;
        insert_counter_row(&conn, COUNTER_TEST, T0, 900, a);
        // Counter reset (app restart): value drops below baseline.
        insert_counter_row(&conn, COUNTER_TEST, T0 + 1, 50, a);
        insert_counter_row(&conn, COUNTER_TEST, T0 + 2, 120, a);

        let deltas = counter_window_deltas(
            &conn,
            COUNTER_TEST,
            &["$.agent"],
            Some(T0 + 1),
            Some(T0 + 2),
        )
        .unwrap();
        let by_label = deltas_by_label(deltas);
        assert_eq!(
            by_label.get("a"),
            Some(&120.0),
            "reset -> delta is in-window last value"
        );
    }

    #[test]
    fn counter_window_deltas_new_series_in_window() {
        let conn = setup_test_db();
        // Series b only exists inside the window -> no baseline, delta = last.
        let b = r#"{"agent":"b","model":"m","type":"input","session.id":"s2"}"#;
        insert_counter_row(&conn, COUNTER_TEST, T0 + 1, 70, b);
        insert_counter_row(&conn, COUNTER_TEST, T0 + 2, 200, b);

        let deltas = counter_window_deltas(
            &conn,
            COUNTER_TEST,
            &["$.agent"],
            Some(T0 + 1),
            Some(T0 + 2),
        )
        .unwrap();
        let by_label = deltas_by_label(deltas);
        assert_eq!(by_label.get("b"), Some(&200.0));
    }

    #[test]
    fn counter_window_deltas_no_start_returns_last_value() {
        let conn = setup_test_db();
        let a = r#"{"agent":"a","model":"m","type":"input","session.id":"s1"}"#;
        insert_counter_row(&conn, COUNTER_TEST, T0, 100, a);
        insert_counter_row(&conn, COUNTER_TEST, T0 + 1, 300, a);

        // No start bound: whole-history delta = last value seen at or before end.
        let deltas =
            counter_window_deltas(&conn, COUNTER_TEST, &["$.agent"], None, Some(T0 + 1)).unwrap();
        let by_label = deltas_by_label(deltas);
        assert_eq!(by_label.get("a"), Some(&300.0));

        // End bound excludes later rows.
        let deltas =
            counter_window_deltas(&conn, COUNTER_TEST, &["$.agent"], None, Some(T0)).unwrap();
        let by_label = deltas_by_label(deltas);
        assert_eq!(by_label.get("a"), Some(&100.0));
    }

    #[test]
    fn counter_window_deltas_zero_delta_series_dropped() {
        let conn = setup_test_db();
        // Series c has rows in the window but no progress -> zero delta, dropped.
        let c = r#"{"agent":"c","model":"m","type":"input","session.id":"s3"}"#;
        insert_counter_row(&conn, COUNTER_TEST, T0, 500, c);
        insert_counter_row(&conn, COUNTER_TEST, T0 + 1, 500, c);

        let deltas = counter_window_deltas(
            &conn,
            COUNTER_TEST,
            &["$.agent"],
            Some(T0 + 1),
            Some(T0 + 1),
        )
        .unwrap();
        assert!(deltas.is_empty(), "zero-delta series must be dropped");
    }

    #[test]
    fn counter_window_deltas_malformed_attributes_do_not_raise() {
        let conn = setup_test_db();
        // Corrupt attributes row: the json_valid-gated expressions must yield
        // NULL (never an error), and the row is still counted by name.
        insert_counter_row(&conn, COUNTER_TEST, T0, 100, "{not json");
        insert_counter_row(&conn, COUNTER_TEST, T0 + 1, 200, "{not json");

        let deltas = counter_window_deltas(
            &conn,
            COUNTER_TEST,
            &["$.agent"],
            Some(T0 + 1),
            Some(T0 + 1),
        )
        .unwrap();
        // Both rows have NULL agent -> one (null) series; baseline 100, last 200.
        let by_label = deltas_by_label(deltas);
        assert_eq!(by_label.get("(null)"), Some(&100.0));
    }

    #[allow(clippy::too_many_arguments)]
    fn insert_histogram_row(
        conn: &Connection,
        name: &str,
        timestamp: i64,
        count: i64,
        sum: f64,
        attributes: &str,
    ) {
        let hist = format!("[{count}, {sum}, []]");
        conn.execute(
            "INSERT INTO metrics (
                name, metric_type, timestamp, value_histogram, attributes
            ) VALUES (?1, 2, ?2, ?3, ?4)",
            rusqlite::params![name, timestamp, hist, attributes],
        )
        .unwrap();
    }

    #[test]
    fn counter_window_deltas_value_histogram_sum_field() {
        let conn = setup_test_db();
        let s = r#"{"session.id":"s1"}"#;
        // Cost counter: [count, sum]. Window captures the 0.25 step.
        insert_histogram_row(&conn, "opencode.session.cost.total", T0, 2, 0.5, s);
        insert_histogram_row(&conn, "opencode.session.cost.total", T0 + 1, 3, 0.75, s);
        insert_histogram_row(&conn, "opencode.session.cost.total", T0 + 2, 5, 1.25, s);

        let deltas = counter_window_deltas_value(
            &conn,
            "opencode.session.cost.total",
            &[r#"$."session.id""#],
            HISTOGRAM_SUM_VALUE_SQL,
            Some(T0 + 1),
            Some(T0 + 2),
        )
        .unwrap();
        let by_label = deltas_by_label(deltas);
        assert_eq!(
            by_label.get("s1"),
            Some(&0.75),
            "sum field: 1.25 - 0.5 = 0.75 (sub-cent precision preserved)"
        );
    }

    #[test]
    fn counter_window_deltas_value_histogram_count_field() {
        let conn = setup_test_db();
        let a = r#"{"session.id":"s1","tool_name":"Bash"}"#;
        insert_histogram_row(&conn, "opencode.tool.duration", T0, 10, 999.0, a);
        insert_histogram_row(&conn, "opencode.tool.duration", T0 + 1, 4, 1.0, a);
        insert_histogram_row(&conn, "opencode.tool.duration", T0 + 2, 14, 1001.0, a);

        // Count field: 14 - 10 = 4 (the sum field's 1001 - 999 = 2 must not leak in).
        let deltas = counter_window_deltas_value(
            &conn,
            "opencode.tool.duration",
            &[r#"$."session.id""#, "$.tool_name"],
            HISTOGRAM_COUNT_VALUE_SQL,
            Some(T0 + 1),
            Some(T0 + 2),
        )
        .unwrap();
        let by_label = deltas_by_label(deltas);
        assert_eq!(by_label.get("s1|Bash"), Some(&4.0));
    }

    #[test]
    fn counter_window_deltas_value_baseline_seeks_use_covering_indexes() {
        // Planner contract: the per-series baseline seeks of the histogram
        // counters must resolve via their covering partial indexes (see
        // schema.rs), not the generic idx_metrics_name_ts. An edit to those
        // index expressions that stops matching the reader's SQL verbatim
        // degrades every baseline seek to a table scan.
        let conn = setup_test_db();
        let explain = |sql: &str| -> String {
            let mut stmt = conn.prepare(sql).unwrap();
            // EXPLAIN QUERY PLAN columns: id, parent, notused, detail.
            stmt.query_map([], |r| r.get::<_, String>(3))
                .unwrap()
                .collect::<std::result::Result<Vec<_>, _>>()
                .unwrap()
                .join("\n")
        };

        let hist_count = "CASE WHEN json_valid(value_histogram) THEN CAST(json_extract(value_histogram, '$[0]') AS REAL) END";
        let hist_sum = "CASE WHEN json_valid(value_histogram) THEN CAST(json_extract(value_histogram, '$[1]') AS REAL) END";
        let sid = "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.\"session.id\"') END";
        let tool =
            "CASE WHEN json_valid(attributes) THEN json_extract(attributes, '$.tool_name') END";

        let plan_tool = explain(&format!(
            "EXPLAIN QUERY PLAN SELECT {hist_count} FROM metrics \
             WHERE name = 'opencode.tool.duration' AND timestamp < 5 \
               AND {sid} IS 's1' AND {tool} IS 'Bash' \
             ORDER BY timestamp DESC, {hist_count} DESC LIMIT 1"
        ));
        assert!(
            plan_tool.contains("idx_metrics_opencode_tool_duration"),
            "tool.duration baseline must use its covering index:\n{plan_tool}"
        );

        let plan2 = explain(&format!(
            "EXPLAIN QUERY PLAN SELECT {hist_sum} FROM metrics \
             WHERE name = 'opencode.session.cost.total' AND timestamp < 5 \
               AND {sid} IS 's1' \
             ORDER BY timestamp DESC, {hist_sum} DESC LIMIT 1"
        ));
        assert!(
            plan2.contains("idx_metrics_opencode_session_cost"),
            "session.cost.total baseline must use its covering index:\n{plan2}"
        );
    }

    #[test]
    fn counter_window_deltas_value_histogram_malformed_value_is_inert() {
        let conn = setup_test_db();
        let s = r#"{"session.id":"s1"}"#;
        insert_histogram_row(&conn, "opencode.session.cost.total", T0, 2, 0.5, s);
        // Corrupt value_histogram at the latest timestamp: value must be
        // treated as NULL (0), never an error, and must not mint a delta.
        conn.execute(
            "INSERT INTO metrics (
                name, metric_type, timestamp, value_histogram, attributes
            ) VALUES (?1, 2, ?2, ?3, ?4)",
            rusqlite::params!["opencode.session.cost.total", T0 + 1, "{broken", s],
        )
        .unwrap();

        let deltas = counter_window_deltas_value(
            &conn,
            "opencode.session.cost.total",
            &[r#"$."session.id""#],
            HISTOGRAM_SUM_VALUE_SQL,
            Some(T0 + 1),
            Some(T0 + 1),
        )
        .unwrap();
        assert!(
            deltas.is_empty(),
            "corrupt latest value reads as 0 < baseline -> reset -> zero delta, dropped"
        );
    }

    // ── provider attribution (issue #129) ────────────────────────────────────

    use otelite_core::api::RoleTokenUsage;

    fn sample_tokens() -> RoleTokenUsage {
        RoleTokenUsage {
            input: 10,
            output: 20,
            cache_read: 30,
            cache_write: 40,
            reasoning: 50,
        }
    }

    #[test]
    fn largest_remainder_split_single_bucket_keeps_all() {
        let out = largest_remainder_split(100, &[("p1".to_string(), 7)]);
        assert_eq!(out, vec![("p1".to_string(), 100)]);
    }

    #[test]
    fn largest_remainder_split_proportional_and_exact() {
        // 1:2 weights on 100 -> 33 / 67 (largest remainder to the 2-weight).
        let out = largest_remainder_split(100, &[("a".to_string(), 1), ("b".to_string(), 2)]);
        assert_eq!(out, vec![("a".to_string(), 33), ("b".to_string(), 67)]);
        // Parts must sum exactly to the total.
        assert_eq!(out.iter().map(|(_, v)| *v).sum::<u64>(), 100);

        // Even split is exact.
        let out = largest_remainder_split(100, &[("a".to_string(), 1), ("b".to_string(), 1)]);
        assert_eq!(out, vec![("a".to_string(), 50), ("b".to_string(), 50)]);

        // Zero total -> all zeros, no panic.
        let out = largest_remainder_split(0, &[("a".to_string(), 1), ("b".to_string(), 2)]);
        assert_eq!(out, vec![("a".to_string(), 0), ("b".to_string(), 0)]);

        // Zero total weight -> all zeros, no division by zero.
        let out = largest_remainder_split(10, &[("a".to_string(), 0), ("b".to_string(), 0)]);
        assert_eq!(out, vec![("a".to_string(), 0), ("b".to_string(), 0)]);

        // Empty weights -> empty result.
        assert!(largest_remainder_split(10, &[]).is_empty());
    }

    #[test]
    fn attribute_model_direct_single_provider() {
        let out =
            attribute_model_to_providers(sample_tokens(), Some(10.0), &[("bv".to_string(), 12)]);
        assert_eq!(out.len(), 1);
        let (provider, tokens, cost) = &out[0];
        assert_eq!(provider, "bv");
        assert_eq!(*tokens, sample_tokens(), "single provider keeps everything");
        assert_eq!(cost, &Some(10.0));
    }

    #[test]
    fn attribute_model_split_across_providers() {
        // 1:1 weights on the sample (total 150 tokens, $12): each provider
        // gets half of every token field and half the cost.
        let out = attribute_model_to_providers(
            sample_tokens(),
            Some(12.0),
            &[("bv".to_string(), 3), ("omlx".to_string(), 3)],
        );
        assert_eq!(out.len(), 2);
        let total: u64 = out.iter().map(|(_, t, _)| t.total()).sum();
        assert_eq!(total, 150, "split must preserve the token total");
        let cost_sum: f64 = out.iter().map(|(_, _, c)| c.unwrap_or(0.0)).sum();
        assert!(
            (cost_sum - 12.0).abs() < 1e-9,
            "split must preserve the cost total"
        );
        for (_, t, c) in &out {
            assert_eq!(t.input, 5);
            assert_eq!(t.output, 10);
            assert_eq!(t.cache_read, 15);
            assert_eq!(t.cache_write, 20);
            assert_eq!(t.reasoning, 25);
            assert!((c.unwrap() - 6.0).abs() < 1e-9);
        }
    }

    #[test]
    fn attribute_model_no_providers_yields_empty() {
        let out = attribute_model_to_providers(sample_tokens(), Some(1.0), &[]);
        assert!(out.is_empty());
    }

    #[test]
    fn cache_hit_rate_definition() {
        // 8 of 10 prompt tokens served from cache
        assert_eq!(cache_hit_rate(8, 2), Some(0.8));
    }

    #[test]
    fn cache_hit_rate_zero_denominator_is_none() {
        // no prompt tokens at all (reads and input both zero)
        assert_eq!(cache_hit_rate(0, 0), None);
    }

    #[test]
    fn cache_hit_rate_all_reads() {
        assert_eq!(cache_hit_rate(500, 0), Some(1.0));
    }

    #[test]
    fn cache_read_write_ratio_value() {
        assert_eq!(cache_read_write_ratio(8, 2), Some(4.0));
    }

    #[test]
    fn cache_read_write_ratio_no_writes_is_none() {
        assert_eq!(cache_read_write_ratio(100, 0), None);
    }

    #[test]
    fn reasoning_share_pct_definition() {
        // 1000 of 4000 output tokens were thinking
        assert_eq!(reasoning_share_pct(1000, 4000), Some(25.0));
    }

    #[test]
    fn reasoning_share_pct_no_output_is_none() {
        assert_eq!(reasoning_share_pct(1000, 0), None);
        assert_eq!(reasoning_share_pct(0, 0), None);
    }

    #[test]
    fn reasoning_share_pct_all_reasoning() {
        assert_eq!(reasoning_share_pct(500, 500), Some(100.0));
    }

    #[test]
    fn reasoning_share_pct_no_reasoning_is_zero() {
        assert_eq!(reasoning_share_pct(0, 500), Some(0.0));
    }

    // ── query_session_costs ───────────────────────────────────────────────

    fn sid_json(sid: &str, extra: &str) -> String {
        if extra.is_empty() {
            format!("{{\"session.id\":\"{sid}\"}}")
        } else {
            format!("{{\"session.id\":\"{sid}\",{extra}}}")
        }
    }

    #[test]
    fn session_costs_empty_db_returns_nothing() {
        let conn = setup_test_db();
        let rows = query_session_costs(&conn, Some(T0), Some(T0 + 10)).unwrap();
        assert!(rows.is_empty());
    }

    #[test]
    fn session_costs_opencode_takes_last_cumulative_value_per_session() {
        let conn = setup_test_db();
        // s1: cumulative cost re-emitted per flush — last value in the
        // window is the session total, not a sum.
        insert_histogram_row(
            &conn,
            "opencode.session.cost.total",
            T0,
            1,
            0.5,
            &sid_json("s1", r#""project.id":"proj-1""#),
        );
        insert_histogram_row(
            &conn,
            "opencode.session.cost.total",
            T0 + 1,
            2,
            0.75,
            &sid_json("s1", r#""project.id":"proj-1""#),
        );
        insert_histogram_row(
            &conn,
            "opencode.session.cost.total",
            T0 + 2,
            3,
            1.25,
            &sid_json("s1", r#""project.id":"proj-1""#),
        );
        // s2: single flush, zero cost (a free session is still a session).
        insert_histogram_row(
            &conn,
            "opencode.session.cost.total",
            T0 + 1,
            1,
            0.0,
            &sid_json("s2", ""),
        );
        // duration (ms) and token totals follow the same shape.
        insert_histogram_row(
            &conn,
            "opencode.session.duration",
            T0,
            1,
            1_000.0,
            &sid_json("s1", ""),
        );
        insert_histogram_row(
            &conn,
            "opencode.session.duration",
            T0 + 2,
            1,
            3_000.0,
            &sid_json("s1", ""),
        );
        insert_histogram_row(
            &conn,
            "opencode.session.token.total",
            T0,
            1,
            100.0,
            &sid_json("s1", ""),
        );
        insert_histogram_row(
            &conn,
            "opencode.session.token.total",
            T0 + 2,
            1,
            300.0,
            &sid_json("s1", ""),
        );
        // s3 only has an out-of-window row → absent from the result.
        insert_histogram_row(
            &conn,
            "opencode.session.cost.total",
            T0 - 1,
            1,
            9.0,
            &sid_json("s3", ""),
        );

        let rows = query_session_costs(&conn, Some(T0), Some(T0 + 2)).unwrap();
        assert_eq!(rows.len(), 2, "window rows only, no s3: {rows:?}");

        let s1 = rows.iter().find(|r| r.session_id == "s1").unwrap();
        assert_eq!(s1.agent, "opencode");
        assert_eq!(s1.counter_cost_usd, Some(1.25), "last value, not a sum");
        assert_eq!(s1.tokens, 300, "last token total, not a sum");
        assert_eq!(s1.duration_secs, Some(3.0), "3000 ms → 3.0 s");
        assert_eq!(s1.project_id.as_deref(), Some("proj-1"));

        let s2 = rows.iter().find(|r| r.session_id == "s2").unwrap();
        assert_eq!(s2.counter_cost_usd, Some(0.0));
        assert_eq!(s2.project_id, None);
        assert_eq!(s2.duration_secs, None);
    }

    #[test]
    fn session_costs_opencode_malformed_histogram_never_overrides_valid() {
        let conn = setup_test_db();
        insert_histogram_row(
            &conn,
            "opencode.session.cost.total",
            T0,
            1,
            0.5,
            &sid_json("s1", ""),
        );
        // A corrupt flush later in the window must not reset the total to 0.
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_histogram, attributes) \
             VALUES ('opencode.session.cost.total', 2, ?1, 'garbage', ?2)",
            rusqlite::params![T0 + 5, sid_json("s1", "")],
        )
        .unwrap();

        let rows = query_session_costs(&conn, Some(T0), Some(T0 + 5)).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].counter_cost_usd, Some(0.5));
    }

    #[test]
    fn session_costs_claude_sums_span_tokens_per_model() {
        let conn = setup_test_db();
        let span = |ts: i64, attrs: &str| {
            conn.execute(
                "INSERT INTO spans (
                    trace_id, span_id, name, kind, start_time, end_time,
                    attributes, events, resource, status_code
                ) VALUES ('t', ?1, 'claude_code.llm_request', 0, ?2, ?2 + 1000, ?3, '[]', '{}', 0)",
                rusqlite::params![format!("sp{ts}"), ts, attrs],
            )
            .unwrap();
        };
        // c1/model-1: two requests → summed; model-2: one request.
        span(
            T0,
            r#"{"session.id":"c1","model":"model-1","input_tokens":"10","output_tokens":"2","cache_read_tokens":"0","cache_creation_tokens":"0"}"#,
        );
        span(
            T0 + 1_000_000_000,
            r#"{"session.id":"c1","model":"model-1","input_tokens":"5","output_tokens":"1","cache_read_tokens":"100","cache_creation_tokens":"4"}"#,
        );
        span(
            T0 + 2_000_000_000,
            r#"{"session.id":"c1","gen_ai.request.model":"model-2","input_tokens":"7","output_tokens":"3","cache_read_tokens":"0","cache_creation_tokens":"0"}"#,
        );
        // c2: single request → zero duration.
        span(
            T0 + 1,
            r#"{"session.id":"c2","model":"model-1","input_tokens":"1","output_tokens":"1","cache_read_tokens":"0","cache_creation_tokens":"0"}"#,
        );
        // No session.id → skipped, not misattributed.
        span(
            T0 + 2,
            r#"{"model":"model-1","input_tokens":"500","output_tokens":"500","cache_read_tokens":"0","cache_creation_tokens":"0"}"#,
        );
        // Wrong span name → ignored.
        span(
            T0 + 3,
            r#"{"session.id":"c1","model":"other","input_tokens":"9","output_tokens":"9","cache_read_tokens":"0","cache_creation_tokens":"0"}"#,
        );
        conn.execute(
            "UPDATE spans SET name = 'claude_code.tool.execution' WHERE span_id = ?1",
            rusqlite::params![format!("sp{}", T0 + 3)],
        )
        .unwrap();

        let rows = query_session_costs(&conn, Some(T0), Some(T0 + 10_000_000_000)).unwrap();
        let c1 = rows.iter().find(|r| r.session_id == "c1").unwrap();
        assert_eq!(c1.agent, "claude");
        assert_eq!(c1.counter_cost_usd, None);
        assert_eq!(c1.tokens, 10 + 2 + 5 + 1 + 100 + 4 + 7 + 3);
        // 2 seconds between first and last request.
        assert_eq!(c1.duration_secs, Some(2.0));
        let models: std::collections::BTreeMap<_, _> =
            c1.models.iter().map(|(m, t)| (m.as_str(), t)).collect();
        assert_eq!(models["model-1"].input, 15);
        assert_eq!(models["model-1"].output, 3);
        assert_eq!(models["model-1"].cache_read, 100);
        assert_eq!(models["model-1"].cache_write, 4);
        // gen_ai.request.model fallback picked up model-2.
        assert_eq!(models["model-2"].input, 7);
        assert_eq!(models.len(), 2);

        let c2 = rows.iter().find(|r| r.session_id == "c2").unwrap();
        assert_eq!(c2.tokens, 2);
        assert_eq!(
            c2.duration_secs, None,
            "single request → no measurable duration"
        );

        assert_eq!(rows.len(), 2, "sessionless span skipped: {rows:?}");
    }

    use otelite_core::semconv::agent_names as anames;
    use otelite_core::semconv::codex_token_types as ctt;
    use otelite_core::semconv::metric_names as mnames;
    use otelite_core::semconv::opencode_token_types as otypes;

    const W0: i64 = 1_700_000_000_000_000_000;
    const W1: i64 = W0 + 5;

    fn seed(conn: &Connection) {
        // Session→project map + session counts (opencode.session.count).
        let sess = |sid: &str, project: Option<&str>, subagent: bool| {
            let mut attrs = format!("{{\"session.id\":\"{sid}\"");
            if let Some(p) = project {
                attrs.push_str(&format!(",\"project.id\":\"{p}\""));
            }
            if subagent {
                attrs.push_str(",\"is_subagent\":\"true\"");
            }
            attrs.push('}');
            insert_counter_row(conn, mnames::OPENCODE_SESSION_COUNT, W0, 1, &attrs);
        };
        sess("s1", Some("projA"), false);
        sess("s2", Some("projB"), false);
        sess("s3", None, false);
        sess("s4", Some("projA"), true);

        // Token counters (cumulative per series).
        let tok = |sid: &str, model: &str, kind: &str, ts: i64, v: i64| {
            let attrs = format!(
                "{{\"agent\":\"opencode\",\"model\":\"{model}\",\"type\":\"{kind}\",\"session.id\":\"{sid}\"}}"
            );
            insert_counter_row(conn, mnames::OPENCODE_TOKEN_USAGE, ts, v, &attrs);
        };
        tok("s1", "modelA", otypes::INPUT, W0, 100);
        tok("s1", "modelA", otypes::INPUT, W0 + 1, 150);
        tok("s2", "modelA", otypes::OUTPUT, W0 + 1, 50);
        tok("s3", "modelB", otypes::INPUT, W0 + 2, 30);
        tok("s4", "modelA", otypes::INPUT, W0 + 3, 70);

        // Per-session cost counters.
        insert_histogram_row(
            conn,
            mnames::OPENCODE_SESSION_COST_TOTAL,
            W0,
            1,
            0.5,
            r#"{"session.id":"s1"}"#,
        );
        insert_histogram_row(
            conn,
            mnames::OPENCODE_SESSION_COST_TOTAL,
            W0 + 1,
            2,
            0.75,
            r#"{"session.id":"s1"}"#,
        );
        insert_histogram_row(
            conn,
            mnames::OPENCODE_SESSION_COST_TOTAL,
            W0,
            1,
            0.1,
            r#"{"session.id":"s3"}"#,
        );

        // Codex: turn tokens (histogram sum field) + cli thread starts.
        insert_histogram_row(
            conn,
            mnames::CODEX_TURN_TOKEN_USAGE,
            W0 + 1,
            1,
            400.0,
            &format!("{{\"model\":\"gpt-x\",\"token_type\":\"{}\"}}", ctt::INPUT),
        );
        insert_counter_row(
            conn,
            mnames::CODEX_THREAD_STARTED,
            W0,
            2,
            r#"{"session_source":"cli"}"#,
        );
    }

    fn model_map(
        r: &otelite_core::api::ProjectRollupStorage,
    ) -> std::collections::HashMap<&str, &otelite_core::api::AgentTokenUsage> {
        r.models.iter().map(|(m, t)| (m.as_str(), t)).collect()
    }

    fn by_project(
        rows: Vec<otelite_core::api::ProjectRollupStorage>,
    ) -> std::collections::HashMap<String, otelite_core::api::ProjectRollupStorage> {
        rows.into_iter()
            .map(|r| (r.project_id.clone(), r))
            .collect()
    }

    #[test]
    fn project_rollup_empty_db_returns_nothing() {
        let conn = setup_test_db();
        let rows = query_project_rollup(&conn, Some(W0), Some(W1)).unwrap();
        assert!(rows.is_empty(), "no metrics → no projects: {rows:?}");
    }

    #[test]
    fn project_rollup_attributes_tokens_cost_and_sessions() {
        let conn = setup_test_db();
        seed(&conn);
        let rows = by_project(query_project_rollup(&conn, Some(W0), Some(W1)).unwrap());

        let a = rows.get("projA").expect("projA missing: {rows:?}");
        assert_eq!(a.sessions, 1, "subagent session s4 must not count");
        assert_eq!(a.counter_cost_usd, Some(0.75), "s1's counter delta");
        assert!(
            !a.counter_disjoint_from_tokens,
            "opencode counter + tokens are one population"
        );
        let mm = model_map(a);
        assert_eq!(
            mm.get("modelA").expect("modelA tokens").input,
            220,
            "s1 delta 150 + s4 delta 70"
        );
        assert_eq!(a.models.len(), 1);

        let b = rows.get("projB").expect("projB missing");
        assert_eq!(b.sessions, 1);
        assert_eq!(b.counter_cost_usd, None, "s2 emitted no cost counter");
        assert_eq!(model_map(b).get("modelA").unwrap().output, 50);
    }

    #[test]
    fn project_rollup_unattributed_merges_labelless_opencode_with_codex() {
        let conn = setup_test_db();
        seed(&conn);
        let rows = by_project(query_project_rollup(&conn, Some(W0), Some(W1)).unwrap());

        let u = rows.get("unattributed").expect("unattributed missing");
        assert_eq!(
            u.sessions, 3,
            "1 label-less opencode + 2 codex cli sessions: {u:?}"
        );
        assert_eq!(u.counter_cost_usd, Some(0.1), "s3's cost counter");
        assert!(
            u.counter_disjoint_from_tokens,
            "counter (opencode) + tokens (opencode+codex) are disjoint"
        );
        assert_eq!(model_map(u).get("modelB").unwrap().input, 30);
        assert_eq!(
            model_map(u).get("gpt-x").unwrap().input,
            400,
            "codex histogram $[1] sum"
        );
        assert!(
            !rows.contains_key(anames::CODEX) && !rows.contains_key(anames::CLAUDE),
            "codex/claude are folded into unattributed, not their own rows"
        );
    }

    #[test]
    fn project_rollup_respects_window() {
        let conn = setup_test_db();
        seed(&conn);
        // Narrow window that excludes every s1/s2 token step except s1@W0.
        let rows = by_project(query_project_rollup(&conn, Some(W0), Some(W0)).unwrap());
        let a = rows.get("projA").expect("projA missing");
        assert_eq!(
            model_map(a).get("modelA").unwrap().input,
            100,
            "only the W0 step"
        );
        assert_eq!(
            a.counter_cost_usd,
            Some(0.5),
            "baseline-free last value in window"
        );
        // No session.count rows outside the narrow window… they are all at W0,
        // so sessions are still counted.
        assert_eq!(a.sessions, 1);
    }

    use otelite_core::semconv::metric_names as plmnames;

    /// claude-style LLM request span: duration from start/end, flat `model`
    /// attribute, optional normalized `ttft_ms` attribute.
    fn insert_llm_request_span(
        conn: &Connection,
        span_id: &str,
        model: &str,
        start_ns: i64,
        values: (i64, Option<i64>, Option<i64>),
    ) {
        let (duration_ms, ttft_ms, output_tokens) = values;
        let mut attrs = format!("{{\"session.id\":\"s1\",\"model\":\"{model}\"");
        if let Some(ttft) = ttft_ms {
            attrs.push_str(&format!(",\"ttft_ms\":\"{ttft}\""));
        }
        if let Some(toks) = output_tokens {
            attrs.push_str(&format!(",\"output_tokens\":{toks}"));
        }
        attrs.push('}');
        let end_ns = start_ns + duration_ms * 1_000_000;
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
             VALUES ('t', ?1, 'claude_code.llm_request', 0, ?2, ?3, ?4, 1)",
            rusqlite::params![span_id, start_ns, end_ns, attrs],
        )
        .unwrap();
    }

    fn percentile_rows(resp: &otelite_core::api::LatencyPercentilesResponse) -> usize {
        resp.metrics
            .values()
            .map(|s| s.all.len() + s.models.values().map(Vec::len).sum::<usize>())
            .sum()
    }

    #[test]
    fn latency_percentiles_empty_db() {
        let conn = setup_test_db();
        let resp = query_latency_percentiles(
            &conn,
            None,
            None,
            3600,
            &["duration", "ttft"],
            &GenAiFilters::default(),
            None,
        )
        .unwrap();
        assert!(percentile_rows(&resp) == 0, "expected no points");
    }

    #[test]
    fn latency_percentiles_rejects_bad_inputs() {
        let conn = setup_test_db();
        assert!(query_latency_percentiles(
            &conn,
            None,
            None,
            0,
            &["duration"],
            &GenAiFilters::default(),
            None
        )
        .is_err());
        assert!(query_latency_percentiles(
            &conn,
            None,
            None,
            3600,
            &["nope"],
            &GenAiFilters::default(),
            None
        )
        .is_err());
    }

    #[test]
    fn expand_histogram_midpoints_cases() {
        // count == 1 → exact sum
        let v = expand_histogram_midpoints(r#"[1, 250.5, []]"#).unwrap();
        assert_eq!(v, vec![250.5]);

        // two observations across bounded buckets → bucket midpoints
        let v = expand_histogram_midpoints(
            r#"[2, 300.0, [
            {"upper_bound":200.0,"count":1},
            {"upper_bound":400.0,"count":1}
        ]]"#,
        )
        .unwrap();
        assert_eq!(v, vec![100.0, 300.0]);

        // open tail: remaining mass falls back to the running mean
        let v = expand_histogram_midpoints(
            r#"[3, 300.0, [
            {"upper_bound":200.0,"count":1}
        ]]"#,
        )
        .unwrap();
        assert_eq!(v.len(), 3);
        assert!((v[0] - 100.0).abs() < 1e-9);
        let tail = (300.0 - v[0]) / 2.0;
        assert!((v[1] - tail).abs() < 1e-9 && (v[2] - tail).abs() < 1e-9);

        assert!(expand_histogram_midpoints("not json").is_none());
        assert!(expand_histogram_midpoints(r#"{"a":1}"#).is_none());
    }

    #[test]
    fn latency_percentiles_span_cohorts() {
        let conn = setup_test_db();
        // bucket = 10s. Two buckets: T0..T0+10s and T0+10s..T0+20s.
        // modelA: durations 100ms, 300ms (bucket 1) and 500ms (bucket 2)
        insert_llm_request_span(&conn, "a1", "modelA", T0, (100, Some(20), Some(10)));
        insert_llm_request_span(
            &conn,
            "a2",
            "modelA",
            T0 + 5_000_000_000,
            (300, Some(90), None),
        );
        insert_llm_request_span(
            &conn,
            "a3",
            "modelA",
            T0 + 12_000_000_000,
            (500, None, Some(30)),
        );
        // modelB: duration 400ms, ttft 150ms (bucket 1)
        insert_llm_request_span(
            &conn,
            "b1",
            "modelB",
            T0 + 2_000_000_000,
            (400, Some(150), None),
        );

        let resp = query_latency_percentiles(
            &conn,
            None,
            None,
            10,
            &["duration", "ttft"],
            &GenAiFilters::default(),
            None,
        )
        .unwrap();

        let dur = &resp.metrics["duration"];
        assert_eq!(dur.all.len(), 2, "two duration buckets for all");
        let b0: Vec<_> = dur.all.iter().filter(|p| p.ts == T0).collect();
        let b1: Vec<_> = dur
            .all
            .iter()
            .filter(|p| p.ts == T0 + 10_000_000_000)
            .collect();
        assert!(b0.len() == 1 && b1.len() == 1);
        assert_eq!(b0[0].count, 3, "bucket 0: 3 requests");
        // sorted [100, 300, 400]: p50=300 (idx (3-1)*0.5=1), p90=400, p95=400, p99=400
        assert_eq!(b0[0].p50_ms, Some(300.0));
        assert_eq!(b0[0].p90_ms, Some(400.0));
        assert_eq!(b0[0].p95_ms, Some(400.0));
        assert_eq!(b0[0].p99_ms, Some(400.0));
        assert_eq!(b1[0].count, 1);
        assert_eq!(b1[0].p50_ms, Some(500.0));

        // per-model
        let a = &dur.models["modelA"];
        assert_eq!(a.len(), 2);
        assert_eq!(a[0].count, 2);
        assert_eq!(
            a[0].p50_ms,
            Some(300.0),
            "[100,300] → idx (2-1)*0.5=0.5.round()=1 → 300"
        );
        let b = &dur.models["modelB"];
        assert_eq!(b[0].count, 1);
        assert_eq!(b[0].p50_ms, Some(400.0));

        // ttft: modelA [20, 90] bucket 0 + modelB [150] bucket 0 → all [20, 90, 150]
        let tt = &resp.metrics["ttft"];
        assert_eq!(tt.all.len(), 1);
        assert_eq!(tt.all[0].count, 3);
        assert_eq!(tt.all[0].p50_ms, Some(90.0));
        assert_eq!(tt.all[0].p90_ms, Some(150.0));
        assert_eq!(tt.models["modelA"][0].count, 2);
        assert_eq!(tt.models["modelB"][0].count, 1);

        // window filter excludes everything
        let resp = query_latency_percentiles(
            &conn,
            Some(T0 + 100_000_000_000),
            None,
            10,
            &["duration"],
            &GenAiFilters::default(),
            None,
        )
        .unwrap();
        assert!(percentile_rows(&resp) == 0);
    }

    fn insert_raw_histogram_row(
        conn: &Connection,
        name: &str,
        timestamp: i64,
        value_histogram: &str,
        attributes: &str,
    ) {
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_histogram, attributes)
             VALUES (?1, 2, ?2, ?3, ?4)",
            rusqlite::params![name, timestamp, value_histogram, attributes],
        )
        .unwrap();
    }

    #[test]
    fn latency_percentiles_codex_ttft_histogram() {
        let conn = setup_test_db();
        // No spans at all: codex TTFT must come from the histogram alone.
        // count == 1 → exact; count == 2 → midpoints of [0,100) and [100,300).
        insert_raw_histogram_row(
            &conn,
            plmnames::CODEX_TURN_TTFT,
            T0,
            r#"[1, 120.0, []]"#,
            r#"{"model":"gpt-x"}"#,
        );
        insert_raw_histogram_row(
            &conn,
            plmnames::CODEX_TURN_TTFT,
            T0 + 1_000_000_000,
            r#"[2, 300.0, [{"upper_bound":100.0,"count":1},{"upper_bound":300.0,"count":1}]]"#,
            r#"{"model":"gpt-x"}"#,
        );

        let resp = query_latency_percentiles(
            &conn,
            None,
            None,
            3600,
            &["ttft"],
            &GenAiFilters::default(),
            None,
        )
        .unwrap();
        let tt = &resp.metrics["ttft"];
        assert_eq!(tt.all.len(), 1, "single hourly bucket");
        assert_eq!(tt.all[0].count, 3, "1 exact + 2 expanded");
        // values: 120 (exact), 50 (midpoint 0-100), 200 (midpoint 100-300)
        // sorted [50, 120, 200]: p50=120
        assert_eq!(tt.all[0].p50_ms, Some(120.0));
        assert_eq!(tt.all[0].p90_ms, Some(200.0));
        assert_eq!(tt.models["gpt-x"][0].count, 3);
        assert!(
            !resp.metrics.contains_key("duration"),
            "duration not requested"
        );
    }

    fn insert_tool_span(conn: &Connection, span_id: &str, start_ns: i64, duration_ms: i64) {
        let attrs = r#"{"session.id":"s1","name":"Bash"}"#;
        let end_ns = start_ns + duration_ms * 1_000_000;
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
             VALUES ('t', ?1, 'claude_code.tool.execution', 0, ?2, ?3, ?4, 1)",
            rusqlite::params![span_id, start_ns, end_ns, attrs],
        )
        .unwrap();
    }

    fn dist_count(resp: &otelite_core::api::DistributionResponse) -> u64 {
        resp.buckets.iter().map(|b| b.count).sum()
    }

    #[test]
    fn distribution_empty_db() {
        let conn = setup_test_db();
        // session_cost is priced in the API layer, so it errors here by
        // design — only the storage-resolved cohorts are covered.
        for metric in ["tool_duration", "llm_duration", "ttft", "output_tokens"] {
            let resp = query_distribution(
                &conn,
                metric,
                None,
                None,
                20,
                "linear",
                &GenAiFilters::default(),
            )
            .unwrap();
            assert!(
                resp.buckets.is_empty() && resp.stats.is_none(),
                "{metric}: expected empty"
            );
            assert_eq!(resp.metric, metric);
        }
        assert!(query_distribution(
            &conn,
            "session_cost",
            None,
            None,
            20,
            "linear",
            &GenAiFilters::default()
        )
        .is_err());
    }

    #[test]
    fn distribution_rejects_bad_inputs() {
        let conn = setup_test_db();
        assert!(query_distribution(
            &conn,
            "bogus",
            None,
            None,
            20,
            "linear",
            &GenAiFilters::default()
        )
        .is_err());
        assert!(query_distribution(
            &conn,
            "ttft",
            None,
            None,
            20,
            "exponential",
            &GenAiFilters::default()
        )
        .is_err());
    }

    #[test]
    fn distribution_span_resolvers() {
        let conn = setup_test_db();
        // durations 100/300/500/400 ms; ttft 20/90/—/150; output tokens 10/—/30/—
        insert_llm_request_span(&conn, "a1", "modelA", T0, (100, Some(20), Some(10)));
        insert_llm_request_span(
            &conn,
            "a2",
            "modelA",
            T0 + 5_000_000_000,
            (300, Some(90), None),
        );
        insert_llm_request_span(
            &conn,
            "a3",
            "modelA",
            T0 + 12_000_000_000,
            (500, None, Some(30)),
        );
        insert_llm_request_span(
            &conn,
            "b1",
            "modelB",
            T0 + 2_000_000_000,
            (400, Some(150), None),
        );
        // codex ttft histogram: one exact 777ms observation
        insert_raw_histogram_row(
            &conn,
            plmnames::CODEX_TURN_TTFT,
            T0,
            r#"[1, 777.0, []]"#,
            r#"{"model":"gpt-x"}"#,
        );
        insert_tool_span(&conn, "t1", T0, 25);
        insert_tool_span(&conn, "t2", T0 + 1_000_000_000, 900);

        let d = |m: &str, scale: &str| {
            query_distribution(&conn, m, None, None, 4, scale, &GenAiFilters::default()).unwrap()
        };

        let llm = d("llm_duration", "linear");
        assert_eq!(llm.unit, "ms");
        assert_eq!(dist_count(&llm), 4);
        let s = llm.stats.as_ref().unwrap();
        assert_eq!(s.min, 100.0);
        assert_eq!(s.max, 500.0);
        assert!((s.mean - 325.0).abs() < 1e-9);
        // sorted [100,300,400,500]: p95 -> idx (4-1)*0.95=2.85.round()=3 -> 500
        assert_eq!(s.p95, 500.0);

        // ttft = 3 span values + 1 codex histogram observation
        let tt = d("ttft", "linear");
        assert_eq!(dist_count(&tt), 4);
        let s = tt.stats.as_ref().unwrap();
        assert_eq!(s.min, 20.0);
        assert_eq!(s.max, 777.0);

        let ot = d("output_tokens", "linear");
        assert_eq!(ot.unit, "tokens");
        assert_eq!(dist_count(&ot), 2, "only spans carrying output_tokens");
        let s = ot.stats.as_ref().unwrap();
        assert_eq!(s.min, 10.0);
        assert_eq!(s.max, 30.0);

        let tool = d("tool_duration", "linear");
        assert_eq!(dist_count(&tool), 2);
        let s = tool.stats.as_ref().unwrap();
        assert_eq!(s.min, 25.0);
        assert_eq!(s.max, 900.0);

        // log scale bins the same values without error; counts are preserved
        let llm_log = d("llm_duration", "log");
        assert_eq!(llm_log.buckets.len(), 4);
        assert_eq!(dist_count(&llm_log), 4);
    }

    // ── session context (#134) ────────────────────────────────────────

    fn insert_ctx_log(conn: &Connection, ts: i64, body: &str, severity: i32, attrs: &str) {
        conn.execute(
            "INSERT INTO logs (timestamp, severity_number, body, attributes)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![ts, severity, body, attrs],
        )
        .unwrap();
    }

    fn insert_ctx_metric(conn: &Connection, name: &str, ts: i64, value: f64, attrs: &str) {
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_double, attributes)
             VALUES (?1, 0, ?2, ?3, ?4)",
            rusqlite::params![name, ts, value, attrs],
        )
        .unwrap();
    }

    fn insert_ctx_metric_int(conn: &Connection, name: &str, ts: i64, value: i64, attrs: &str) {
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes)
             VALUES (?1, 1, ?2, ?3, ?4)",
            rusqlite::params![name, ts, value, attrs],
        )
        .unwrap();
    }

    #[test]
    fn session_context_empty_db_returns_none() {
        let conn = setup_test_db();
        assert!(query_session_context(&conn, "nope", None, None, 500)
            .unwrap()
            .is_none());
    }

    #[test]
    fn session_context_mixed_session() {
        let conn = setup_test_db();
        // claude-style spans carrying session.id + model (helper hardcodes
        // session "s1" — relabel to the test session)
        insert_llm_request_span(
            &conn,
            "c1",
            "claude-sonnet-5",
            T0,
            (100, Some(20), Some(10)),
        );
        insert_llm_request_span(
            &conn,
            "c2",
            "claude-sonnet-5",
            T0 + 9_000_000_000,
            (300, None, None),
        );
        conn.execute(
            "UPDATE spans SET attributes = json_set(attributes, '$.\"session.id\"', 'c-1') WHERE span_id IN ('c1', 'c2')",
            [],
        )
        .unwrap();
        // claude event log: event name in the body, session in attributes
        insert_ctx_log(
            &conn,
            T0 + 1_000_000_000,
            "claude_code.api_request",
            13,
            r#"{"session.id":"c-1","event.name":"api_request","model":"claude-sonnet-5"}"#,
        );
        insert_ctx_log(
            &conn,
            T0 + 2_000_000_000,
            "claude_code.tool_result",
            17,
            r#"{"session.id":"c-1","event.name":"tool_result"}"#,
        );
        // claude emits no metrics: metrics for this session are empty,
        // which must not break the response.
        let resp = query_session_context(&conn, "c-1", None, None, 500)
            .unwrap()
            .unwrap();
        assert_eq!(resp.session.id, "c-1");
        assert_eq!(resp.session.agent.as_deref(), Some("claude"));
        assert_eq!(resp.session.span_coverage, "full");
        assert_eq!(resp.spans_total, 2);
        assert_eq!(resp.logs_total, 2);
        assert!(resp.metrics.is_empty());
        assert_eq!(resp.spans[0].model.as_deref(), Some("claude-sonnet-5"));
        assert_eq!(resp.spans[0].duration_ns, 100_000_000);
        assert_eq!(resp.logs[0].severity.as_deref(), Some("WARN"));
        assert_eq!(resp.logs[1].severity.as_deref(), Some("ERROR"));
        // timeline: 2 spans + 2 logs merged ascending, capped at limit
        assert_eq!(resp.timeline.len(), 4);
        assert_eq!(resp.timeline[0].kind, "span");
        assert_eq!(resp.timeline[0].ts, T0);
        assert_eq!(resp.timeline[1].kind, "log");
        assert_eq!(resp.timeline[1].ts, T0 + 1_000_000_000);
        assert_eq!(
            resp.timeline
                .iter()
                .find(|e| e.kind == "span")
                .unwrap()
                .label,
            "claude_code.llm_request claude-sonnet-5"
        );
    }

    #[test]
    fn session_context_opencode_partial_coverage_and_metrics() {
        let conn = setup_test_db();
        let sid = "ses_abc";
        // opencode llm span (labeled) — the span cohort is partial by design.
        // The helper writes a claude-shaped span (name + model attr), so
        // rename it to the opencode span shape for agent detection.
        insert_llm_request_span(&conn, "o1", "qwen-3.8", T0, (100, None, Some(42)));
        conn.execute(
            "UPDATE spans SET name = 'opencode.llm' WHERE span_id = 'o1'",
            [],
        )
        .unwrap();
        // relabel that span's session
        conn.execute(
            "UPDATE spans SET attributes = json_set(attributes, '$.\"session.id\"', ?1) WHERE span_id = 'o1'",
            rusqlite::params![sid],
        )
        .unwrap();
        insert_ctx_log(
            &conn,
            T0 + 1_000_000_000,
            "opencode tool finished",
            9,
            &format!("{{\"session.id\":\"{sid}\",\"event.name\":\"tool_result\"}}"),
        );
        insert_ctx_metric_int(
            &conn,
            "opencode.message.count",
            T0,
            5,
            &format!("{{\"session.id\":\"{sid}\",\"project.id\":\"proj-1\"}}"),
        );
        insert_ctx_metric_int(
            &conn,
            "opencode.message.count",
            T0 + 100_000_000,
            9,
            &format!("{{\"session.id\":\"{sid}\",\"project.id\":\"proj-1\"}}"),
        );
        insert_ctx_metric(
            &conn,
            "opencode.tool.duration",
            T0 + 200_000_000,
            12.5,
            &format!("{{\"session.id\":\"{sid}\"}}"),
        );

        let resp = query_session_context(&conn, sid, None, None, 500)
            .unwrap()
            .unwrap();
        assert_eq!(resp.session.agent.as_deref(), Some("opencode"));
        assert_eq!(resp.session.span_coverage, "partial");
        assert_eq!(resp.session.project_id.as_deref(), Some("proj-1"));
        assert_eq!(resp.metrics.len(), 2);
        let msg = resp
            .metrics
            .iter()
            .find(|m| m.name == "opencode.message.count")
            .unwrap();
        assert_eq!(msg.count, 2);
        assert_eq!(msg.sum, Some(14.0));
        assert_eq!(msg.min, Some(5.0));
        assert_eq!(msg.max, Some(9.0));
        assert_eq!(msg.metric_type, 1);
        assert_eq!(msg.first_ts, T0);
        assert_eq!(msg.last_ts, T0 + 100_000_000);
    }

    #[test]
    fn session_context_window_does_not_leak_unlabeled_metrics() {
        let conn = setup_test_db();
        let sid = "wl-1";
        // a session-labelled metric inside the window
        insert_ctx_metric_int(
            &conn,
            "opencode.message.count",
            T0,
            5,
            &format!("{{\"session.id\":\"{sid}\"}}"),
        );
        // an UNLABELLED metric in the same window: a bare
        // json_extract(...) = ? predicate would wrongly match it
        insert_ctx_metric_int(&conn, "opencode.message.count", T0, 7, "{}");
        // and a labelled metric OUTSIDE the window
        insert_ctx_metric_int(
            &conn,
            "opencode.message.count",
            T0 - 100_000_000_000,
            9,
            &format!("{{\"session.id\":\"{sid}\"}}"),
        );

        let resp = query_session_context(&conn, sid, Some(T0), Some(T0 + 1_000_000_000), 500)
            .unwrap()
            .unwrap();
        let m = resp
            .metrics
            .iter()
            .find(|m| m.name == "opencode.message.count")
            .unwrap();
        assert_eq!(m.count, 1, "only the in-window labelled point counts");
        assert_eq!(m.sum, Some(5.0), "unlabelled point must not leak in");
        assert_eq!(m.first_ts, T0);
        assert_eq!(m.last_ts, T0);
    }

    #[test]
    fn session_context_codex_logs_via_conversation_id() {
        let conn = setup_test_db();
        // codex session: logs carry conversation.id (not session.id);
        // metrics carry no session identifier at all.
        let sid = "01a00000-0000-0000-0000-000000000000";
        insert_ctx_log(
            &conn,
            T0,
            "codex_otel",
            9,
            &format!("{{\"event.name\":\"user_prompt\",\"conversation.id\":\"{sid}\"}}"),
        );
        insert_ctx_metric(
            &conn,
            "codex.turn.token_usage",
            T0,
            100.0,
            r#"{"session_source":"cli"}"#,
        );
        let resp = query_session_context(&conn, sid, None, None, 500)
            .unwrap()
            .unwrap();
        assert_eq!(resp.session.agent.as_deref(), Some("codex"));
        assert_eq!(resp.session.span_coverage, "partial");
        assert_eq!(resp.logs_total, 1);
        // the codex metric has no session id — it must not leak in
        assert!(resp.metrics.is_empty());
    }

    #[test]
    fn session_context_limit_and_window() {
        let conn = setup_test_db();
        let sid = "lim-1";
        for i in 0..6 {
            insert_llm_request_span(
                &conn,
                &format!("l{i}"),
                "m",
                T0 + i * 1_000_000_000,
                (10, None, None),
            );
            conn.execute(
                "UPDATE spans SET attributes = json_set(attributes, '$.\"session.id\"', ?1) WHERE span_id = ?2",
                rusqlite::params![sid, format!("l{i}")],
            )
            .unwrap();
        }
        // limit caps rows but not totals
        let resp = query_session_context(&conn, sid, None, None, 4)
            .unwrap()
            .unwrap();
        assert_eq!(resp.spans.len(), 4);
        assert_eq!(resp.spans_total, 6);
        assert_eq!(resp.timeline.len(), 4);
        // window filters by span start_time
        let resp = query_session_context(&conn, sid, Some(T0 + 3_000_000_000), None, 500)
            .unwrap()
            .unwrap();
        assert_eq!(
            resp.spans.len(),
            3,
            "spans 3,4,5 start after the window start"
        );
        assert_eq!(
            resp.spans_total, 3,
            "totals count the queried scope (window, when given)"
        );
    }

    #[test]
    fn distribution_respects_window() {
        let conn = setup_test_db();
        insert_llm_request_span(&conn, "a1", "modelA", T0, (100, Some(20), None));
        insert_llm_request_span(
            &conn,
            "a2",
            "modelA",
            T0 + 40_000_000_000,
            (300, None, None),
        );

        let resp = query_distribution(
            &conn,
            "llm_duration",
            Some(T0 + 10_000_000_000),
            None,
            4,
            "linear",
            &GenAiFilters::default(),
        )
        .unwrap();
        assert_eq!(dist_count(&resp), 1, "only the later span is in the window");
        let s = resp.stats.as_ref().unwrap();
        assert_eq!(s.min, 300.0);
        assert_eq!(s.max, 300.0);
    }

    // ── New insight query tests (#157–#164) ──────────────────────────────

    fn insert_metric(conn: &Connection, name: &str, ts: i64, value_int: i64, attributes: &str) {
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes) VALUES (?1, 'counter', ?2, ?3, ?4)",
            rusqlite::params![name, ts, value_int, attributes],
        )
        .unwrap();
    }

    fn insert_span_simple(conn: &Connection, name: &str, start: i64, end: i64, attributes: &str) {
        conn.execute(
            "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code, events, resource) \
             VALUES ('t1', random() || '', ?1, 0, ?2, ?3, ?4, 0, '[]', '{}')",
            rusqlite::params![name, start, end, attributes],
        )
        .unwrap();
    }

    #[test]
    fn test_query_effort_breakdown_empty() {
        let conn = setup_test_db();
        let result = query_effort_breakdown(&conn, None, None).unwrap();
        assert!(result.rows.is_empty());
    }

    #[test]
    fn test_query_effort_breakdown_aggregates_by_effort() {
        let conn = setup_test_db();
        // Two rows for effort=high, one for effort=low
        insert_metric(
            &conn,
            "claude_code.token.usage",
            1000,
            500,
            r#"{"effort":"high","model":"claude-opus","type":"input"}"#,
        );
        insert_metric(
            &conn,
            "claude_code.token.usage",
            2000,
            300,
            r#"{"effort":"high","model":"claude-opus","type":"output"}"#,
        );
        insert_metric(
            &conn,
            "claude_code.token.usage",
            3000,
            100,
            r#"{"effort":"low","model":"claude-haiku","type":"input"}"#,
        );
        let result = query_effort_breakdown(&conn, None, None).unwrap();
        let efforts: Vec<&str> = result.rows.iter().map(|r| r.effort.as_str()).collect();
        assert!(efforts.contains(&"high"), "effort=high should be present");
        assert!(efforts.contains(&"low"), "effort=low should be present");
    }

    #[test]
    fn test_query_efficiency_stats_empty() {
        let conn = setup_test_db();
        let result = query_efficiency_stats(&conn, None, None).unwrap();
        assert_eq!(result.total_tokens, 0);
        assert_eq!(result.total_commits, 0);
    }

    #[test]
    fn test_query_efficiency_stats_counts_tokens() {
        let conn = setup_test_db();
        insert_metric(
            &conn,
            "claude_code.token.usage",
            1000,
            1000,
            r#"{"type":"input"}"#,
        );
        insert_metric(
            &conn,
            "claude_code.token.usage",
            2000,
            500,
            r#"{"type":"output"}"#,
        );
        insert_metric(&conn, "claude_code.commit.count", 1000, 5, "{}");
        let result = query_efficiency_stats(&conn, None, None).unwrap();
        // tokens: counter window delta includes input (1000) + output (500) = 1500
        // but counter_window_deltas with no start uses "last value before window" = None
        // so delta = last_value = cumulative value. We only have single entries per series.
        assert!(result.total_tokens >= 1000, "should count input tokens");
        assert_eq!(
            result
                .by_agent
                .iter()
                .find(|a| a.agent == "claude_code")
                .map(|a| a.commits)
                .unwrap_or(0),
            5
        );
    }

    #[test]
    fn test_query_codex_ttft_empty() {
        let conn = setup_test_db();
        let result = query_codex_ttft(&conn, None, None).unwrap();
        assert!(result.models.is_empty());
    }

    #[test]
    fn test_query_agent_project_rollup_empty() {
        let conn = setup_test_db();
        let result = query_agent_project_rollup(&conn, None, None).unwrap();
        assert!(result.projects.is_empty());
    }

    #[test]
    fn test_query_agent_project_rollup_extracts_cwd_basename() {
        let conn = setup_test_db();
        insert_span_simple(
            &conn,
            "run_sampling_request",
            1_000_000_000,
            2_000_000_000,
            r#"{"cwd":"/Users/test/src/otelite","model":"gpt-4"}"#,
        );
        let result = query_agent_project_rollup(&conn, None, None).unwrap();
        let projects: Vec<&str> = result.projects.iter().map(|p| p.project.as_str()).collect();
        assert!(
            projects.contains(&"otelite"),
            "basename should be 'otelite', got: {projects:?}"
        );
    }

    #[test]
    fn test_query_mcp_health_empty() {
        let conn = setup_test_db();
        let result = query_mcp_health(&conn, None, None).unwrap();
        assert!(result.entries.is_empty());
    }

    #[test]
    fn test_query_mcp_health_computes_error_rate() {
        let conn = setup_test_db();
        insert_metric(
            &conn,
            "codex.mcp.call",
            1000,
            1,
            r#"{"mcp_server":"memory","mcp_tool":"search","status":"ok"}"#,
        );
        insert_metric(
            &conn,
            "codex.mcp.call",
            2000,
            1,
            r#"{"mcp_server":"memory","mcp_tool":"search","status":"error"}"#,
        );
        let result = query_mcp_health(&conn, None, None).unwrap();
        assert!(!result.entries.is_empty(), "should have MCP health entries");
        let entry = result
            .entries
            .iter()
            .find(|e| e.server == "memory" && e.tool == "search")
            .unwrap();
        assert_eq!(entry.ok_calls, 1);
        assert_eq!(entry.error_calls, 1);
        assert!(
            (entry.error_rate - 0.5).abs() < 0.001,
            "error rate should be 50%"
        );
    }

    #[test]
    fn test_query_guardian_stats_empty() {
        let conn = setup_test_db();
        let result = query_guardian_stats(&conn, None, None).unwrap();
        assert_eq!(result.total_reviews, 0);
    }

    #[test]
    fn test_query_guardian_stats_by_risk() {
        let conn = setup_test_db();
        insert_metric(
            &conn,
            "codex.guardian.review",
            1000,
            1,
            r#"{"risk_level":"low","decision":"approved","action":"allow"}"#,
        );
        insert_metric(
            &conn,
            "codex.guardian.review",
            2000,
            1,
            r#"{"risk_level":"high","decision":"approved","action":"allow"}"#,
        );
        insert_metric(
            &conn,
            "codex.guardian.review",
            3000,
            1,
            r#"{"risk_level":"high","decision":"denied","action":"block"}"#,
        );
        let result = query_guardian_stats(&conn, None, None).unwrap();
        assert_eq!(result.total_reviews, 3);
        let high = result
            .by_risk_level
            .iter()
            .find(|r| r.risk_level == "high")
            .unwrap();
        assert_eq!(high.count, 2);
    }

    #[test]
    fn test_query_multi_agent_stats_empty() {
        let conn = setup_test_db();
        let result = query_multi_agent_stats(&conn, None, None).unwrap();
        assert_eq!(result.total_spawns, 0);
        assert_eq!(result.total_resumes, 0);
        assert!(result.roles.is_empty());
    }

    #[test]
    fn test_query_multi_agent_stats_spawn_and_resume() {
        let conn = setup_test_db();
        insert_metric(
            &conn,
            "codex.multi_agent.spawn",
            1000,
            3,
            r#"{"role":"researcher"}"#,
        );
        insert_metric(
            &conn,
            "codex.multi_agent.resume",
            2000,
            1,
            r#"{"role":"researcher"}"#,
        );
        let result = query_multi_agent_stats(&conn, None, None).unwrap();
        assert_eq!(result.total_spawns, 3);
        assert_eq!(result.total_resumes, 1);
        let researcher = result
            .roles
            .iter()
            .find(|r| r.role == "researcher")
            .unwrap();
        assert_eq!(researcher.spawns, 3);
        assert_eq!(researcher.resumes, 1);
        assert!((researcher.share_pct - 100.0).abs() < 0.001);
    }

    #[test]
    fn test_query_codex_turn_breakdown_empty() {
        let conn = setup_test_db();
        let result = query_codex_turn_breakdown(&conn, None, None).unwrap();
        assert!(result.rows.is_empty());
    }

    #[test]
    fn test_query_codex_turn_breakdown_busy_idle() {
        let conn = setup_test_db();
        // Insert a run_sampling_request span with busy_ns and idle_ns
        insert_span_simple(
            &conn,
            "run_sampling_request",
            0,
            2_000_000_000, // 2 seconds
            r#"{"model":"gpt-4o","cwd":"/Users/x/src/myproject","busy_ns":1000000000,"idle_ns":500000000}"#,
        );
        let result = query_codex_turn_breakdown(&conn, None, None).unwrap();
        assert!(!result.rows.is_empty(), "should have turn breakdown rows");
        let row = &result.rows[0];
        assert_eq!(row.model, "gpt-4o");
        assert_eq!(row.project, "myproject");
        assert_eq!(row.turn_count, 1);
        assert!(
            (row.avg_duration_ms - 2000.0).abs() < 1.0,
            "avg duration should be ~2000ms"
        );
        assert!(
            (row.avg_busy_ms - 1000.0).abs() < 1.0,
            "avg busy should be ~1000ms"
        );
        assert!(
            (row.avg_idle_ms - 500.0).abs() < 1.0,
            "avg idle should be ~500ms"
        );
        assert!(
            (row.busy_ratio - 0.5).abs() < 0.01,
            "busy ratio should be ~0.5"
        );
    }

    #[test]
    fn test_query_session_model_breakdown_empty() {
        let conn = setup_test_db();
        let result = query_session_model_breakdown(&conn, None, None).unwrap();
        assert!(result.rows.is_empty());
    }

    #[test]
    fn test_query_session_model_breakdown_groups_by_session_and_model() {
        let conn = setup_test_db();
        // Session A used sonnet (2 spans), session B used opus (1 span)
        for _ in 0..2 {
            insert_span_simple(
                &conn,
                "claude_code.llm_request",
                0,
                1_000_000_000,
                r#"{"session.id":"session-a","gen_ai.request.model":"claude-sonnet-5","gen_ai.usage.input_tokens":100,"gen_ai.usage.output_tokens":50}"#,
            );
        }
        insert_span_simple(
            &conn,
            "claude_code.llm_request",
            0,
            1_000_000_000,
            r#"{"session.id":"session-b","gen_ai.request.model":"claude-opus-5","gen_ai.usage.input_tokens":200,"gen_ai.usage.output_tokens":80}"#,
        );
        let result = query_session_model_breakdown(&conn, None, None).unwrap();
        assert!(!result.rows.is_empty());
        // session-a/sonnet should have 2 requests
        let sonnet_row = result
            .rows
            .iter()
            .find(|r| r.session_id == "session-a")
            .unwrap();
        assert_eq!(sonnet_row.model, "claude-sonnet-5");
        assert_eq!(sonnet_row.requests, 2);
        assert_eq!(sonnet_row.input_tokens, 200);
        assert_eq!(sonnet_row.output_tokens, 100);
        // session-b/opus should have 1 request
        let opus_row = result
            .rows
            .iter()
            .find(|r| r.session_id == "session-b")
            .unwrap();
        assert_eq!(opus_row.model, "claude-opus-5");
        assert_eq!(opus_row.requests, 1);
    }

    #[test]
    fn test_query_speed_distribution_empty() {
        let conn = setup_test_db();
        let result = query_speed_distribution(&conn, None, None).unwrap();
        assert!(result.rows.is_empty());
    }

    #[test]
    fn test_query_speed_distribution_groups_by_speed_and_model() {
        let conn = setup_test_db();
        // Two normal spans, one extended span, all for claude-sonnet
        for _ in 0..2 {
            insert_span_simple(
                &conn,
                "claude_code.llm_request",
                0,
                1_000_000_000,
                r#"{"speed":"normal","gen_ai.request.model":"claude-sonnet-5","gen_ai.usage.input_tokens":100,"gen_ai.usage.output_tokens":50}"#,
            );
        }
        insert_span_simple(
            &conn,
            "claude_code.llm_request",
            0,
            1_000_000_000,
            r#"{"speed":"extended","gen_ai.request.model":"claude-sonnet-5","gen_ai.usage.input_tokens":300,"gen_ai.usage.output_tokens":200}"#,
        );
        let result = query_speed_distribution(&conn, None, None).unwrap();
        assert!(!result.rows.is_empty());
        // "normal" should have 2 requests (sorted first because 2 > 1)
        let normal = result
            .rows
            .iter()
            .find(|r| r.speed.as_deref() == Some("normal"))
            .unwrap();
        assert_eq!(normal.model, "claude-sonnet-5");
        assert_eq!(normal.requests, 2);
        assert_eq!(normal.input_tokens, 200);
        // "extended" should have 1 request
        let extended = result
            .rows
            .iter()
            .find(|r| r.speed.as_deref() == Some("extended"))
            .unwrap();
        assert_eq!(extended.requests, 1);
        assert_eq!(extended.input_tokens, 300);
    }
}

// ── New insight queries ───────────────────────────────────────────────────────

/// Cross-tool TTFT comparison from span-level `ttft_ms` attribute.
///
/// Reads `ttft_ms` from every span that carries it, normalises the
/// `otel.scope.name` to a short tool label, and returns per-(tool, model)
/// aggregates. Codex TTFT comes from the separate `query_codex_ttft` histogram
/// path and is NOT included here (those spans lack a `ttft_ms` attribute).
pub fn query_cross_tool_ttft(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::CrossToolTtftResponse> {
    use otelite_core::api::{CrossToolTtftResponse, CrossToolTtftRow};

    let mut where_parts = vec!["json_valid(attributes)".to_string()];
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    // Only spans that actually carry ttft_ms with a positive value
    where_parts.push("CAST(json_extract(attributes,'$.\"ttft_ms\"') AS REAL) > 0".to_string());

    if let Some(s) = start_time {
        where_parts.push("start_time >= ?".to_string());
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_parts.push("start_time <= ?".to_string());
        params.push(Box::new(e));
    }

    let where_clause = format!("WHERE {}", where_parts.join(" AND "));

    let sql = format!(
        r#"
        SELECT
          CASE
            WHEN json_extract(attributes,'$."otel.scope.name"') LIKE '%claude_code%' THEN 'claude_code'
            WHEN json_extract(attributes,'$."otel.scope.name"') LIKE 'com.opencode' THEN 'opencode'
            WHEN json_extract(attributes,'$."otel.scope.name"') LIKE '%opencode%'    THEN 'opencode'
            WHEN json_extract(attributes,'$."otel.scope.name"') LIKE 'pi-otel'       THEN 'pi'
            ELSE COALESCE(json_extract(attributes,'$."otel.scope.name"'), 'unknown')
          END AS tool,
          COALESCE(json_extract(attributes,'$."model"'), '(unknown)') AS model,
          COUNT(*) AS cnt,
          AVG(CAST(json_extract(attributes,'$."ttft_ms"') AS REAL)) AS avg_ms,
          MIN(CAST(json_extract(attributes,'$."ttft_ms"') AS REAL)) AS min_ms,
          MAX(CAST(json_extract(attributes,'$."ttft_ms"') AS REAL)) AS max_ms
        FROM spans
        {where_clause}
        GROUP BY tool, model
        HAVING cnt > 0
        ORDER BY tool ASC, avg_ms ASC
        "#
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare cross_tool_ttft query: {e}"))
    })?;

    // We also need individual values to compute p90; do a second pass per group.
    let rows: Vec<CrossToolTtftRow> = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)? as u64,
                r.get::<_, f64>(3)?,
                r.get::<_, f64>(4)?,
                r.get::<_, f64>(5)?,
            ))
        })
        .map_err(|e| StorageError::QueryError(format!("{e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("{e}")))?
        .into_iter()
        .map(|(tool, model, count, avg_ms, min_ms, max_ms)| {
            // p90: fetch sorted ttft values for this model and compute in Rust.
            // Only when there are enough samples; inline model into SQL to avoid
            // closure-lifetime issues with the existing params Vec.
            let p90_ms = if count >= 10 {
                let values_sql = format!(
                    r#"
                    SELECT CAST(json_extract(attributes,'$."ttft_ms"') AS REAL)
                    FROM spans
                    {where_clause}
                      AND COALESCE(json_extract(attributes,'$."model"'), '(unknown)') = '{model}'
                    ORDER BY CAST(json_extract(attributes,'$."ttft_ms"') AS REAL) ASC
                    "#
                );
                if let Ok(mut vstmt) = conn.prepare(&values_sql) {
                    if let Ok(vals) = vstmt
                        .query_map(param_refs.as_slice(), |r| r.get::<_, f64>(0))
                        .and_then(|rows| rows.collect::<std::result::Result<Vec<_>, _>>())
                    {
                        if !vals.is_empty() {
                            let idx = ((vals.len() as f64 * 0.90) as usize).min(vals.len() - 1);
                            Some(vals[idx])
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

            CrossToolTtftRow {
                tool,
                model,
                count,
                avg_ms,
                min_ms,
                p90_ms,
                max_ms,
            }
        })
        .collect();

    Ok(CrossToolTtftResponse {
        rows,
        filters_applied: Vec::new(),
    })
}

/// Codex hook overhead: total and average invocation time per hook event type.
///
/// Reads the `codex.hooks.run.duration_ms` histogram metric which carries a
/// `hook_name` attribute. Uses the histogram sum (index 1 of the JSON array)
/// as the aggregate duration, consistent with how we handle other histograms.
pub fn query_hook_overhead(
    conn: &Connection,
    start_time: Option<i64>,
    end_time: Option<i64>,
) -> Result<otelite_core::api::HookOverheadResponse> {
    use otelite_core::api::{HookOverheadResponse, HookOverheadRow};
    use otelite_core::semconv::metric_names as mnames;

    let mut where_clause =
        String::from("WHERE name = ? AND json_valid(attributes) AND value_histogram IS NOT NULL");
    let mut params: Vec<Box<dyn rusqlite::ToSql>> =
        vec![Box::new(mnames::CODEX_HOOKS_RUN_DURATION.to_string())];

    if let Some(s) = start_time {
        where_clause.push_str(" AND timestamp >= ?");
        params.push(Box::new(s));
    }
    if let Some(e) = end_time {
        where_clause.push_str(" AND timestamp <= ?");
        params.push(Box::new(e));
    }

    let sql = format!(
        r#"
        SELECT
          COALESCE(json_extract(attributes,'$.hook_name'), '(unknown)') AS event,
          SUM(json_extract(value_histogram,'$[0]'))                     AS total_cnt,
          SUM(json_extract(value_histogram,'$[1]'))                     AS total_ms
        FROM metrics
        {where_clause}
        GROUP BY event
        ORDER BY total_ms DESC
        "#
    );

    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut stmt = conn.prepare(&sql).map_err(|e| {
        StorageError::QueryError(format!("Failed to prepare hook_overhead query: {e}"))
    })?;

    let rows: Vec<HookOverheadRow> = stmt
        .query_map(param_refs.as_slice(), |r| {
            let event: String = r.get(0)?;
            let count: i64 = r.get(1).unwrap_or(0);
            let total_ms: f64 = r.get(2).unwrap_or(0.0);
            let avg_ms = if count > 0 {
                total_ms / count as f64
            } else {
                0.0
            };
            Ok(HookOverheadRow {
                event,
                count: count as u64,
                total_ms,
                avg_ms,
            })
        })
        .map_err(|e| StorageError::QueryError(format!("{e}")))?
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| StorageError::QueryError(format!("{e}")))?;

    let grand_total_ms: f64 = rows.iter().map(|r| r.total_ms).sum();

    Ok(HookOverheadResponse {
        rows,
        grand_total_ms,
        filters_applied: Vec::new(),
    })
}

#[cfg(test)]
mod new_insight_tests {
    use super::*;
    use rusqlite::Connection;

    fn setup_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::sqlite::schema::initialize_schema(&conn).unwrap();
        conn
    }

    #[test]
    fn test_query_cross_tool_ttft_empty() {
        let conn = setup_db();
        let result = query_cross_tool_ttft(&conn, None, None).unwrap();
        assert!(result.rows.is_empty());
    }

    #[test]
    fn test_query_hook_overhead_empty() {
        let conn = setup_db();
        let result = query_hook_overhead(&conn, None, None).unwrap();
        assert!(result.rows.is_empty());
        assert_eq!(result.grand_total_ms, 0.0);
    }

    #[test]
    fn test_query_hook_overhead_with_data() {
        let conn = setup_db();
        // Insert a codex.hooks.run.duration_ms histogram metric
        // Format: [count, sum, [...buckets...]]
        let hist = r#"[3,450.0,[{"upper_bound":100.0,"count":1},{"upper_bound":500.0,"count":2}]]"#;
        conn.execute(
            "INSERT INTO metrics (name, metric_type, timestamp, value_histogram, attributes, flags, created_at)
             VALUES (?,2,1000000000,?,?,0,1000000000)",
            rusqlite::params![
                "codex.hooks.run.duration_ms",
                hist,
                r#"{"hook_name":"PreToolUse","model":"gpt-5.6-terra","otel.scope.name":"codex"}"#,
            ],
        )
        .unwrap();

        let result = query_hook_overhead(&conn, None, None).unwrap();
        assert_eq!(result.rows.len(), 1);
        assert_eq!(result.rows[0].event, "PreToolUse");
        assert_eq!(result.rows[0].count, 3);
        assert!((result.rows[0].total_ms - 450.0).abs() < 0.01);
        assert!((result.rows[0].avg_ms - 150.0).abs() < 0.01);
        assert!((result.grand_total_ms - 450.0).abs() < 0.01);
    }
}
