//! MCP (Model Context Protocol) server: JSON-RPC 2.0 over stdio (#37).
//!
//! `otelite mcp` exposes otelite telemetry to AI agents (Claude Code,
//! Cursor, ...) as four read-only tools: `query_logs`, `query_traces`,
//! `get_trace`, and `get_usage`. Messages are newline-delimited JSON —
//! one JSON-RPC request per line, one response per line (notifications
//! get no response), which is the MCP stdio transport.
//!
//! Protocol-level failures (bad JSON, unknown method, unknown tool) are
//! JSON-RPC errors; tool-level failures (bad arguments, missing trace,
//! storage error) are MCP tool results with `isError: true`, so the
//! agent sees an actionable message instead of a protocol fault.
//!
//! stdout carries only JSON-RPC traffic; everything else (tracing,
//! CLI-level errors) goes to stderr — see `run_cli`.

use crate::error::{Error, Result};
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::stdin;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use otelite_core::filters::GenAiFilters;
use otelite_core::query::{Operator, QueryPredicate, QueryValue};
use otelite_core::storage::{QueryParams, StorageBackend};
use otelite_core::telemetry::log::SeverityLevel;
use otelite_core::telemetry::trace::{Span, StatusCode};

const SERVER_NAME: &str = "otelite";
/// Fallback protocol version when the client doesn't state one.
const DEFAULT_PROTOCOL_VERSION: &str = "2025-06-18";
const DEFAULT_LOG_LIMIT: u64 = 50;
const MAX_LOG_LIMIT: u64 = 500;
const DEFAULT_TRACE_LIMIT: u64 = 20;
const MAX_TRACE_LIMIT: u64 = 100;

/// Run the stdio loop until stdin closes (the client going away is the
/// only shutdown signal; agents manage the process lifetime).
pub async fn run(storage: Arc<dyn StorageBackend>) -> Result<()> {
    let mut lines = BufReader::new(stdin()).lines();
    let mut out = tokio::io::stdout();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        if let Some(response) = handle_line(&line, &storage).await {
            let bytes = format!("{response}\n").into_bytes();
            out.write_all(&bytes).await?;
            out.flush().await?;
        }
    }
    Ok(())
}

/// Dispatch one JSON-RPC line. Returns `None` for notifications (they
/// are fire-and-forget per the JSON-RPC spec, so the client does not
/// expect a line back).
async fn handle_line(line: &str, storage: &Arc<dyn StorageBackend>) -> Option<Value> {
    let request: Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": Value::Null,
                "error": { "code": -32700, "message": format!("Parse error: {e}") },
            }));
        },
    };

    let id = request.get("id").cloned().unwrap_or(Value::Null);
    let method = match request.get("method").and_then(Value::as_str) {
        Some(m) => m,
        None => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32600, "message": "Invalid Request: missing method" },
            }));
        },
    };

    match method {
        // Notifications are never answered.
        m if m.starts_with("notifications/") => None,
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "protocolVersion": request
                    .pointer("/params/protocolVersion")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .unwrap_or(DEFAULT_PROTOCOL_VERSION),
                "capabilities": { "tools": {} },
                "serverInfo": {
                    "name": SERVER_NAME,
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "instructions": "Query otelite telemetry: logs, traces, and GenAI token usage. \
                                 Timestamps are nanoseconds since the Unix epoch; fields named \
                                 `time` are the same instant as RFC 3339 UTC for readability. \
                                 Window arguments named `since` take a number with an h, d, or m \
                                 suffix (e.g. '24h', '7d', '30m').",
            },
        })),
        "ping" => Some(json!({ "jsonrpc": "2.0", "id": id, "result": {} })),
        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tool_definitions() },
        })),
        "tools/call" => {
            let name = match request.pointer("/params/name").and_then(Value::as_str) {
                Some(n) => n,
                None => {
                    return Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32602,
                            "message": "Invalid params: missing tool name",
                        },
                    }));
                },
            };
            let tool = match tool_name(name) {
                Some(t) => t,
                None => {
                    return Some(json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32602,
                            "message": format!("Unknown tool: {name}"),
                        },
                    }));
                },
            };
            let args = request
                .pointer("/params/arguments")
                .cloned()
                .unwrap_or_else(|| json!({}));
            let (is_error, text) = invoke_tool(tool, storage, &args).await;
            Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "content": [{ "type": "text", "text": text }],
                    "isError": is_error,
                },
            }))
        },
        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": -32601, "message": format!("Method not found: {method}") },
        })),
    }
}

/// Tool identity, kept in sync with `tool_definitions`.
fn tool_name(name: &str) -> Option<&'static str> {
    match name {
        "query_logs" => Some("query_logs"),
        "query_traces" => Some("query_traces"),
        "get_trace" => Some("get_trace"),
        "get_usage" => Some("get_usage"),
        _ => None,
    }
}

/// The tool catalogue. Descriptions are written for an LLM reader:
/// what each argument does, the units, and the failure modes.
fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "query_logs",
            "description": "Query stored log records, newest first. Each result has an RFC 3339 \
                            `time`, the epoch-nanosecond `timestamp`, severity, body, and \
                            attributes. `severity` filters to that level and above (e.g. 'WARN' \
                            returns WARN, ERROR, FATAL). `search` full-text matches bodies and \
                            attribute values. `since` bounds the query to a rolling window.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "severity": {
                        "type": "string",
                        "enum": ["TRACE", "DEBUG", "INFO", "WARN", "ERROR", "FATAL"],
                        "description": "Minimum severity level (case-insensitive)"
                    },
                    "search": {
                        "type": "string",
                        "description": "Full-text search over log bodies and attribute values"
                    },
                    "since": {
                        "type": "string",
                        "description": "Rolling window: number with an h, d, or m suffix, e.g. '24h', '7d', '30m'"
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 500,
                        "description": "Maximum logs to return (default 50, capped at 500)"
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "query_traces",
            "description": "List recent traces, newest first. Each entry summarizes one trace: \
                            root span name, start time, duration in milliseconds, span count, \
                            involved services, and overall status (ERROR if any span errored). \
                            `service` filters on the exact resource service name. `min_duration` \
                            and `status` are applied after the most-recent `limit` traces are \
                            selected, so results may be fewer than `limit`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": {
                        "type": "string",
                        "enum": ["ERROR", "OK"],
                        "description": "Overall trace status (case-insensitive)"
                    },
                    "min_duration": {
                        "type": "number",
                        "description": "Minimum trace duration in milliseconds"
                    },
                    "service": {
                        "type": "string",
                        "description": "Exact service name (resource.service.name)"
                    },
                    "limit": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 100,
                        "description": "Maximum traces to return (default 20, capped at 100)"
                    }
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "get_trace",
            "description": "Get a full trace: every span with name, kind, start/end times \
                            (nanoseconds since the Unix epoch), attributes, events, status, and \
                            resource. Requires the exact `trace_id` returned by query_traces.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "trace_id": { "type": "string", "description": "The exact trace id" }
                },
                "required": ["trace_id"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "get_usage",
            "description": "GenAI token usage over a rolling window: overall totals, per-model \
                            and per-provider breakdowns (input/output tokens, request counts, \
                            prompt-cache creation/read tokens). `model` accepts an exact model \
                            name or a glob with * (e.g. 'claude-opus-*').",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "since": {
                        "type": "string",
                        "description": "Rolling window: number with an h, d, or m suffix, e.g. '24h', '7d' (default '24h')"
                    },
                    "model": {
                        "type": "string",
                        "description": "Exact model name, or a glob containing *"
                    }
                },
                "additionalProperties": false
            }
        }),
    ]
}

/// Run one tool; returns (is_error, text). The text is JSON on success
/// and a plain, actionable message on failure.
async fn invoke_tool(
    tool: &str,
    storage: &Arc<dyn StorageBackend>,
    args: &Value,
) -> (bool, String) {
    let result = match tool {
        "query_logs" => tool_query_logs(storage, args).await,
        "query_traces" => tool_query_traces(storage, args).await,
        "get_trace" => tool_get_trace(storage, args).await,
        "get_usage" => tool_get_usage(storage, args).await,
        _ => unreachable!("tool_name validates"),
    };
    match result {
        Ok(text) => (false, text),
        Err(e) => (true, e),
    }
}

/// Optional, non-empty string argument.
fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
}

/// Positive integer argument, capped. Values above the cap are clamped
/// (the tool stays usable when an agent over-asks) and the cap is
/// documented in the schema.
fn arg_limit(args: &Value, key: &str, default: u64, cap: u64) -> u64 {
    args.get(key)
        .and_then(Value::as_u64)
        .unwrap_or(default)
        .clamp(1, cap)
}

/// Epoch nanoseconds as an RFC 3339 UTC string.
fn iso_utc(ns: i64) -> Value {
    let dt = chrono::DateTime::from_timestamp_nanos(ns);
    Value::from(dt.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string())
}

async fn tool_query_logs(
    storage: &Arc<dyn StorageBackend>,
    args: &Value,
) -> std::result::Result<String, String> {
    let mut params = QueryParams {
        limit: Some(arg_limit(args, "limit", DEFAULT_LOG_LIMIT, MAX_LOG_LIMIT) as usize),
        ..Default::default()
    };

    if let Some(s) = arg_str(args, "severity") {
        params.min_severity = Some(parse_severity(s)?);
    }
    if let Some(s) = arg_str(args, "search") {
        params.search_text = Some(s.to_string());
    }
    if let Some(s) = arg_str(args, "since") {
        let (start, _end) = parse_since(s)?;
        params.start_time = Some(start);
    }

    let logs = storage
        .query_logs(&params)
        .await
        .map_err(|e| format!("Failed to query logs: {e}"))?;

    let items: Vec<Value> = logs
        .iter()
        .map(|l| {
            let mut v = json!({
                "time": iso_utc(l.timestamp),
                "timestamp": l.timestamp,
                "severity": l.severity,
                "body": l.body,
                "attributes": l.attributes,
            });
            if let Some(text) = &l.severity_text {
                v["severity_text"] = json!(text);
            }
            if let Some(id) = &l.trace_id {
                v["trace_id"] = json!(id);
            }
            if let Some(id) = &l.span_id {
                v["span_id"] = json!(id);
            }
            v
        })
        .collect();

    Ok(json!({ "count": items.len(), "logs": items }).to_string())
}

async fn tool_query_traces(
    storage: &Arc<dyn StorageBackend>,
    args: &Value,
) -> std::result::Result<String, String> {
    let limit = arg_limit(args, "limit", DEFAULT_TRACE_LIMIT, MAX_TRACE_LIMIT);
    let status = arg_str(args, "status").map(|s| s.to_uppercase());
    if let Some(s) = &status {
        if *s != "ERROR" && *s != "OK" {
            return Err(format!("Invalid status {s}: expected ERROR or OK"));
        }
    }
    let min_duration_ms = args.get("min_duration").and_then(Value::as_u64);

    let mut params = QueryParams {
        limit: Some(limit as usize),
        ..Default::default()
    };
    if let Some(service) = arg_str(args, "service") {
        params.predicates.push(QueryPredicate {
            field: "resource.service.name".to_string(),
            operator: Operator::Equal,
            value: QueryValue::String(service.to_string()),
        });
    }

    // Two-step inside the backend: the most-recent `limit` distinct
    // traces matching the predicates, then all their spans.
    let spans = storage
        .query_spans_for_trace_list(&params, limit as usize)
        .await
        .map_err(|e| format!("Failed to query traces: {e}"))?;

    // Group by trace_id, preserving the newest-first order the backend
    // returned (first appearance of each id wins).
    let mut order: Vec<String> = Vec::new();
    let mut by_trace: std::collections::HashMap<String, Vec<&Span>> =
        std::collections::HashMap::new();
    for span in &spans {
        let entry = by_trace.entry(span.trace_id.clone()).or_default();
        if entry.is_empty() {
            order.push(span.trace_id.clone());
        }
        entry.push(span);
    }

    let mut traces: Vec<Value> = Vec::new();
    for trace_id in &order {
        let group = &by_trace[trace_id];
        let start_time = group.iter().map(|s| s.start_time).min().unwrap_or(0);
        let end_time = group.iter().map(|s| s.end_time).max().unwrap_or(0);
        let duration_ms = (end_time.saturating_sub(start_time)) / 1_000_000;

        let root = group
            .iter()
            .find(|s| s.parent_span_id.is_none())
            .or_else(|| group.first());
        let name = root
            .map(|s| s.name.clone())
            .unwrap_or_else(|| "Unknown".to_string());

        let services: Vec<String> = {
            let mut names: Vec<String> = group
                .iter()
                .filter_map(|s| s.resource.as_ref())
                .filter_map(|r| r.attributes.get("service.name"))
                .cloned()
                .collect();
            names.sort();
            names.dedup();
            names
        };

        let has_errors = group.iter().any(|s| s.status.code == StatusCode::Error);
        let trace_status = if has_errors { "ERROR" } else { "OK" };

        if let Some(wanted) = &status {
            if wanted != trace_status {
                continue;
            }
        }
        if let Some(min) = min_duration_ms {
            if duration_ms < min as i64 {
                continue;
            }
        }

        traces.push(json!({
            "trace_id": trace_id,
            "name": name,
            "time": iso_utc(start_time),
            "start_time": start_time,
            "duration_ms": duration_ms,
            "span_count": group.len(),
            "services": services,
            "status": trace_status,
        }));
    }

    Ok(json!({ "count": traces.len(), "traces": traces }).to_string())
}

async fn tool_get_trace(
    storage: &Arc<dyn StorageBackend>,
    args: &Value,
) -> std::result::Result<String, String> {
    let trace_id = arg_str(args, "trace_id")
        .ok_or_else(|| "trace_id is required (an exact id from query_traces)".to_string())?;

    let params = QueryParams {
        trace_id: Some(trace_id.to_string()),
        ..Default::default()
    };
    let mut spans: Vec<Span> = storage
        .query_spans(&params)
        .await
        .map_err(|e| format!("Failed to query trace: {e}"))?;
    if spans.is_empty() {
        return Err(format!("Trace not found: {trace_id}"));
    }
    spans.sort_by_key(|s| s.start_time);

    let items: Vec<Value> = spans
        .iter()
        .map(|s| serde_json::to_value(s).expect("Span is Serialize"))
        .collect();
    Ok(json!({
        "trace_id": trace_id,
        "span_count": items.len(),
        "spans": items,
    })
    .to_string())
}

async fn tool_get_usage(
    storage: &Arc<dyn StorageBackend>,
    args: &Value,
) -> std::result::Result<String, String> {
    let since = arg_str(args, "since").unwrap_or("24h");
    let (start, end) = parse_since(since)?;

    // Exact model names go through `model`; globs (containing *) through
    // the ORed `models` patterns — same convention as `otelite usage`.
    let model = arg_str(args, "model").map(str::to_string);
    let filters = GenAiFilters {
        models: model
            .as_ref()
            .filter(|m| m.contains('*'))
            .map(|m| vec![m.clone()]),
        model: model.filter(|m| !m.contains('*')),
        ..Default::default()
    };

    let (summary, by_model, by_system) = storage
        .query_token_usage(Some(start), Some(end), &filters)
        .await
        .map_err(|e| format!("Failed to query token usage: {e}"))?;

    Ok(json!({
        "window": { "since": since, "start": iso_utc(start), "end": iso_utc(end) },
        "summary": summary,
        "by_model": by_model,
        "by_system": by_system,
    })
    .to_string())
}

/// A `since` duration ("24h", "7d", "30m") as (start_ns, end_ns).
fn parse_since(since: &str) -> std::result::Result<(i64, i64), String> {
    match crate::commands::usage::parse_time_range(since) {
        Ok(range) => Ok(range),
        Err(Error::ApiError(msg)) => Err(msg),
        Err(e) => Err(e.to_string()),
    }
}

/// Minimum-severity parsing (case-insensitive, TRACE..FATAL), mirroring
/// the HTTP API's level set. Unknown values are a tool error that lists
/// the valid ones — silently widening the filter would return data the
/// agent asked to exclude.
fn parse_severity(s: &str) -> std::result::Result<SeverityLevel, String> {
    match s.to_uppercase().as_str() {
        "TRACE" => Ok(SeverityLevel::Trace),
        "DEBUG" => Ok(SeverityLevel::Debug),
        "INFO" => Ok(SeverityLevel::Info),
        "WARN" => Ok(SeverityLevel::Warn),
        "ERROR" => Ok(SeverityLevel::Error),
        "FATAL" => Ok(SeverityLevel::Fatal),
        _ => Err(format!(
            "Invalid severity {s}: expected one of TRACE, DEBUG, INFO, WARN, ERROR, FATAL"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_core::telemetry::log::SeverityLevel;
    use otelite_core::telemetry::resource::Resource;
    use otelite_core::telemetry::trace::{SpanKind, SpanStatus};
    use otelite_storage::sqlite::SqliteBackend;
    use otelite_storage::{StorageBackend, StorageConfig};
    use std::collections::HashMap;
    use std::time::{SystemTime, UNIX_EPOCH};

    async fn test_storage() -> (Arc<dyn StorageBackend>, tempfile::TempDir) {
        let tmp = tempfile::tempdir().unwrap();
        let mut storage =
            SqliteBackend::new(StorageConfig::default().with_data_dir(tmp.path().to_path_buf()));
        storage.initialize().await.unwrap();
        (Arc::new(storage), tmp)
    }

    fn now_ns() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64
    }

    fn make_log(severity: SeverityLevel, body: &str) -> otelite_core::telemetry::log::LogRecord {
        otelite_core::telemetry::log::LogRecord {
            timestamp: now_ns(),
            observed_timestamp: None,
            severity,
            severity_text: Some(format!("{severity:?}")),
            body: body.to_string(),
            attributes: HashMap::from([("component".to_string(), "mcp-test".to_string())]),
            trace_id: None,
            span_id: None,
            resource: None,
        }
    }

    fn make_span(trace_id: &str, span_id: &str, parent: Option<&str>) -> Span {
        let mut resource = Resource::new();
        resource
            .attributes
            .insert("service.name".to_string(), "test-svc".to_string());
        Span {
            trace_id: trace_id.to_string(),
            span_id: span_id.to_string(),
            parent_span_id: parent.map(str::to_string),
            name: "op".to_string(),
            kind: SpanKind::Internal,
            start_time: now_ns() - 2_000_000_000,
            end_time: now_ns(),
            attributes: HashMap::new(),
            events: vec![],
            status: SpanStatus {
                code: StatusCode::Ok,
                message: None,
            },
            resource: Some(resource),
        }
    }

    async fn call(storage: &Arc<dyn StorageBackend>, name: &str, args: Value) -> (bool, String) {
        let line = json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "tools/call",
            "params": { "name": name, "arguments": args },
        })
        .to_string();
        let resp = handle_line(&line, storage).await.unwrap();
        let result = &resp["result"];
        (
            result["isError"].as_bool().unwrap(),
            result["content"][0]["text"].as_str().unwrap().to_string(),
        )
    }

    #[tokio::test]
    async fn initialize_echoes_protocol_version_and_advertises_tools() {
        let (storage, _tmp) = test_storage().await;
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
            &storage,
        )
        .await
        .unwrap();
        assert_eq!(resp["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(resp["result"]["serverInfo"]["name"], "otelite");
        assert!(resp["result"]["capabilities"]["tools"].is_object());

        let list = handle_line(
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#,
            &storage,
        )
        .await
        .unwrap();
        let names: Vec<&str> = list["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            vec!["query_logs", "query_traces", "get_trace", "get_usage"]
        );
    }

    #[tokio::test]
    async fn notifications_get_no_response_and_unknown_method_errors() {
        let (storage, _tmp) = test_storage().await;
        assert!(handle_line(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            &storage,
        )
        .await
        .is_none());
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":3,"method":"bogus/method"}"#,
            &storage,
        )
        .await
        .unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }

    #[tokio::test]
    async fn malformed_and_invalid_requests_produce_jsonrpc_errors() {
        let (storage, _tmp) = test_storage().await;
        let resp = handle_line("this is not json", &storage).await.unwrap();
        assert_eq!(resp["error"]["code"], -32700);
        assert!(resp["id"].is_null());

        let resp = handle_line(r#"{"jsonrpc":"2.0","id":4,"params":{}}"#, &storage)
            .await
            .unwrap();
        assert_eq!(resp["error"]["code"], -32600);
    }

    #[tokio::test]
    async fn unknown_tool_is_a_protocol_error() {
        let (storage, _tmp) = test_storage().await;
        let resp = handle_line(
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope"}}"#,
            &storage,
        )
        .await
        .unwrap();
        assert_eq!(resp["error"]["code"], -32602);
    }

    #[tokio::test]
    async fn query_logs_filters_by_severity_search_and_since() {
        let (storage, _tmp) = test_storage().await;
        storage
            .write_log(&make_log(SeverityLevel::Info, "all good"))
            .await
            .unwrap();
        storage
            .write_log(&make_log(SeverityLevel::Error, "database timeout"))
            .await
            .unwrap();

        let (err, text) = call(&storage, "query_logs", json!({"severity": "error"})).await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["count"], 1);
        assert_eq!(v["logs"][0]["severity"], "Error");
        assert_eq!(v["logs"][0]["body"], "database timeout");
        assert!(v["logs"][0]["time"].as_str().unwrap().ends_with('Z'));

        let (err, text) = call(&storage, "query_logs", json!({"search": "timeout"})).await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["count"], 1);
        assert_eq!(v["logs"][0]["body"], "database timeout");

        let (err, text) = call(&storage, "query_logs", json!({"since": "1h"})).await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["count"], 2);
    }

    #[tokio::test]
    async fn query_logs_rejects_bad_severity_and_since() {
        let (storage, _tmp) = test_storage().await;
        let (err, text) = call(&storage, "query_logs", json!({"severity": "CRITICAL"})).await;
        assert!(err);
        assert!(text.contains("Invalid severity"), "{text}");

        let (err, text) = call(&storage, "query_logs", json!({"since": "yesterday"})).await;
        assert!(err);
        assert!(text.contains("Invalid"), "{text}");
    }

    #[tokio::test]
    async fn query_traces_groups_filters_and_reports_errors() {
        let (storage, _tmp) = test_storage().await;
        // Two traces: one clean (two spans), one with an errored span.
        storage
            .write_span(&make_span("tr-clean", "sp-1", None))
            .await
            .unwrap();
        storage
            .write_span(&make_span("tr-clean", "sp-2", Some("sp-1")))
            .await
            .unwrap();
        let mut bad = make_span("tr-bad", "sp-3", None);
        bad.status.code = StatusCode::Error;
        storage.write_span(&bad).await.unwrap();

        let (err, text) = call(&storage, "query_traces", json!({})).await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["count"], 2);
        let statuses: Vec<&str> = v["traces"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["status"].as_str().unwrap())
            .collect();
        assert!(statuses.contains(&"OK") && statuses.contains(&"ERROR"));

        let (err, text) = call(&storage, "query_traces", json!({"status": "error"})).await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["count"], 1);
        assert_eq!(v["traces"][0]["trace_id"], "tr-bad");
        assert_eq!(v["traces"][0]["services"], json!(["test-svc"]));

        let (err, text) = call(&storage, "query_traces", json!({"status": "MAYBE"})).await;
        assert!(err);
        assert!(text.contains("Invalid status"), "{text}");
    }

    #[tokio::test]
    async fn get_trace_returns_all_spans_and_errors_on_missing() {
        let (storage, _tmp) = test_storage().await;
        storage
            .write_span(&make_span("tr-x", "sp-1", None))
            .await
            .unwrap();
        storage
            .write_span(&make_span("tr-x", "sp-2", Some("sp-1")))
            .await
            .unwrap();

        let (err, text) = call(&storage, "get_trace", json!({"trace_id": "tr-x"})).await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["span_count"], 2);
        assert_eq!(v["spans"][0]["span_id"], "sp-1");
        assert_eq!(v["spans"][1]["parent_span_id"], "sp-1");

        let (err, text) = call(&storage, "get_trace", json!({})).await;
        assert!(err);
        assert!(text.contains("trace_id is required"), "{text}");

        let (err, text) = call(&storage, "get_trace", json!({"trace_id": "missing"})).await;
        assert!(err);
        assert!(text.contains("Trace not found"), "{text}");
    }

    #[tokio::test]
    async fn get_usage_sums_token_attributes_over_the_window() {
        let (storage, _tmp) = test_storage().await;
        // An LLM-shaped span: the vendor request-span name makes it pass
        // the llm_span_guard, and the usage attributes carry the tokens.
        let mut span = make_span("tr-u", "sp-u", None);
        span.name = "claude_code.llm_request".to_string();
        span.attributes
            .insert("gen_ai.request.model".to_string(), "test-model".to_string());
        span.attributes
            .insert("gen_ai.usage.input_tokens".to_string(), "100".to_string());
        span.attributes
            .insert("gen_ai.usage.output_tokens".to_string(), "50".to_string());
        storage.write_span(&span).await.unwrap();

        let (err, text) = call(&storage, "get_usage", json!({"since": "1h"})).await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["summary"]["total_input_tokens"], 100);
        assert_eq!(v["summary"]["total_output_tokens"], 50);
        assert_eq!(v["summary"]["total_requests"], 1);
        assert_eq!(v["by_model"][0]["model"], "test-model");

        // A glob matching the model keeps it; a non-matching exact model
        // returns zero totals.
        let (err, text) = call(
            &storage,
            "get_usage",
            json!({"since": "1h", "model": "test-*"}),
        )
        .await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["summary"]["total_requests"], 1);

        let (err, text) = call(
            &storage,
            "get_usage",
            json!({"since": "1h", "model": "other-model"}),
        )
        .await;
        assert!(!err, "{text}");
        let v: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["summary"]["total_requests"], 0);
    }
}
