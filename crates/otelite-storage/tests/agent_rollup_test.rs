//! Tests for `query_agent_rollup` (issue #125).
//!
//! Synthetic three-harness data: opencode cumulative counters (with a
//! sub-agent session that must not count), codex per-event metrics (with a
//! sub-agent thread source that must not count and a `total` token_type that
//! must not double-count), and claude per-event token sums plus tool-exec
//! spans.

use otelite_core::api::AgentTokenUsage;
use otelite_storage::sqlite::{reader, schema};
use rusqlite::Connection;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema::initialize_schema(&conn).unwrap();
    conn
}

/// Window start, in nanoseconds, aligned to a 1-second boundary.
const T0: i64 = 1_700_000_000_000_000_000;
const T1: i64 = T0 + 1_000_000_000; // 1s window [T1, T1]
const SEC: i64 = 1_000_000_000;

fn insert_metric_row(conn: &Connection, name: &str, timestamp: i64, value: i64, attributes: &str) {
    conn.execute(
        "INSERT INTO metrics (name, metric_type, timestamp, value_int, attributes)
         VALUES (?1, 1, ?2, ?3, ?4)",
        rusqlite::params![name, timestamp, value, attributes],
    )
    .unwrap();
}

/// Insert a counter row with a double-typed value in value_double (#270).
fn insert_double_metric_row(
    conn: &Connection,
    name: &str,
    timestamp: i64,
    value: f64,
    attributes: &str,
) {
    conn.execute(
        "INSERT INTO metrics (name, metric_type, timestamp, value_double, attributes)
         VALUES (?1, 1, ?2, ?3, ?4)",
        rusqlite::params![name, timestamp, value, attributes],
    )
    .unwrap();
}

/// Insert a histogram metric row with explicit count and sum.
#[allow(clippy::too_many_arguments)]
fn insert_histogram_row(
    conn: &Connection,
    name: &str,
    timestamp: i64,
    count: i64,
    sum: f64,
    attributes: &str,
) {
    conn.execute(
        "INSERT INTO metrics (name, metric_type, timestamp, value_histogram, attributes)
         VALUES (?1, 2, ?2, ?3, ?4)",
        rusqlite::params![name, timestamp, format!("[{count}, {sum}, []]"), attributes],
    )
    .unwrap();
}

fn insert_tool_exec_span(conn: &Connection, start: i64) {
    conn.execute(
        "INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
         VALUES ('t', ?1, 'claude_code.tool.execution', 0, ?2, ?2 + 1, '{}', 1)",
        rusqlite::params![format!("sp{}", start), start],
    )
    .unwrap();
}

fn find<'a>(
    agents: &'a [otelite_core::api::AgentRollupStorage],
    name: &str,
) -> &'a otelite_core::api::AgentRollupStorage {
    agents.iter().find(|a| a.agent == name).unwrap_or_else(|| {
        panic!(
            "agent {name} not in rollup: {:?}",
            agents.iter().map(|a| &a.agent).collect::<Vec<_>>()
        )
    })
}

#[test]
fn agent_rollup_empty_db_returns_no_agents() {
    let conn = setup_test_db();
    let agents = reader::query_agent_rollup(&conn, Some(T0), Some(T1), 1).unwrap();
    assert!(agents.is_empty(), "no harness activity -> no agent rows");
}

#[test]
fn agent_rollup_opencode_counter_deltas_and_subagent_excluded() {
    let conn = setup_test_db();

    // Sessions: s1 starts before the window (not counted), s2 in-window,
    // s3 is a sub-agent (excluded).
    insert_metric_row(
        &conn,
        "opencode.session.count",
        T0,
        1,
        r#"{"session.id":"s1","is_subagent":"false"}"#,
    );
    insert_metric_row(
        &conn,
        "opencode.session.count",
        T1,
        1,
        r#"{"session.id":"s2","is_subagent":"false"}"#,
    );
    insert_metric_row(
        &conn,
        "opencode.session.count",
        T1,
        1,
        r#"{"session.id":"s3","is_subagent":"true"}"#,
    );

    // Token counters (cumulative): s1/m1 input 100 -> 250, reasoning 50 -> 60.
    insert_metric_row(
        &conn,
        "opencode.token.usage",
        T0,
        100,
        r#"{"agent":"a","model":"m1","type":"input","session.id":"s1"}"#,
    );
    insert_metric_row(
        &conn,
        "opencode.token.usage",
        T1,
        250,
        r#"{"agent":"a","model":"m1","type":"input","session.id":"s1"}"#,
    );
    insert_metric_row(
        &conn,
        "opencode.token.usage",
        T0,
        50,
        r#"{"agent":"a","model":"m1","type":"reasoning","session.id":"s1"}"#,
    );
    insert_metric_row(
        &conn,
        "opencode.token.usage",
        T1,
        60,
        r#"{"agent":"a","model":"m1","type":"reasoning","session.id":"s1"}"#,
    );

    // Cost counter (cumulative histogram sum): 0.5 -> 1.25, delta 0.75.
    insert_histogram_row(
        &conn,
        "opencode.session.cost.total",
        T0,
        2,
        0.5,
        r#"{"session.id":"s1"}"#,
    );
    insert_histogram_row(
        &conn,
        "opencode.session.cost.total",
        T1,
        5,
        1.25,
        r#"{"session.id":"s1"}"#,
    );

    // Tool calls (cumulative histogram count): 10 -> 14, delta 4.
    insert_histogram_row(
        &conn,
        "opencode.tool.duration",
        T0,
        10,
        999.0,
        r#"{"session.id":"s1","tool_name":"Bash"}"#,
    );
    insert_histogram_row(
        &conn,
        "opencode.tool.duration",
        T1,
        14,
        1001.0,
        r#"{"session.id":"s1","tool_name":"Bash"}"#,
    );

    // Retries (cumulative counter): 3 -> 5, delta 2.
    insert_metric_row(
        &conn,
        "opencode.retry.count",
        T0,
        3,
        r#"{"session.id":"s1"}"#,
    );
    insert_metric_row(
        &conn,
        "opencode.retry.count",
        T1,
        5,
        r#"{"session.id":"s1"}"#,
    );

    let agents = reader::query_agent_rollup(&conn, Some(T1), Some(T1), 1).unwrap();
    let oc = find(&agents, "opencode");

    assert_eq!(
        oc.sessions, 1,
        "only in-window top-level starts count (s1 started earlier, s3 is a sub-agent)"
    );
    assert_eq!(oc.tool_calls, 4, "tool.duration histogram count delta");
    assert_eq!(oc.retries, Some(2), "retry.count counter delta");
    assert_eq!(oc.counter_cost_usd, Some(0.75), "cost histogram sum delta");

    let m1 = oc.models.iter().find(|(m, _)| m == "m1").unwrap().1;
    assert_eq!(
        &m1,
        &AgentTokenUsage {
            input: 150,
            output: 0,
            cache_read: 0,
            cache_write: 0,
            reasoning: 10,
        }
    );

    // Series: single 1s bucket at T1 with the window's tokens.
    assert_eq!(oc.series.len(), 1);
    assert_eq!(oc.series[0].0, T1, "bucket aligned to bucket_secs");
    let (model, tokens) = &oc.series[0].1[0];
    assert_eq!(model, "m1");
    assert_eq!(tokens.total(), 160);
}

#[test]
fn agent_rollup_codex_events_subagent_and_total_excluded() {
    let conn = setup_test_db();

    // Sessions: 3 cli threads + 2 sub-agent threads (excluded).
    insert_metric_row(
        &conn,
        "codex.thread.started",
        T1,
        3,
        r#"{"session_source":"cli"}"#,
    );
    insert_metric_row(
        &conn,
        "codex.thread.started",
        T1,
        2,
        r#"{"session_source":"subagent_task"}"#,
    );

    // Turn tokens: input 100, output 50, total 150 (must not be counted).
    insert_histogram_row(
        &conn,
        "codex.turn.token_usage",
        T1,
        1,
        100.0,
        r#"{"model":"c1","token_type":"input"}"#,
    );
    insert_histogram_row(
        &conn,
        "codex.turn.token_usage",
        T1,
        1,
        50.0,
        r#"{"model":"c1","token_type":"output"}"#,
    );
    insert_histogram_row(
        &conn,
        "codex.turn.token_usage",
        T1,
        1,
        150.0,
        r#"{"model":"c1","token_type":"total"}"#,
    );

    // Tool calls: 2 events.
    insert_metric_row(&conn, "codex.tool.call", T1, 1, r#"{"tool":"shell"}"#);
    insert_metric_row(&conn, "codex.tool.call", T1, 1, r#"{"tool":"apply_patch"}"#);

    // Retries: 1 failed request out of 2.
    insert_metric_row(&conn, "codex.api_request", T1, 1, r#"{"success":"true"}"#);
    insert_metric_row(&conn, "codex.api_request", T1, 1, r#"{"success":"false"}"#);

    let agents = reader::query_agent_rollup(&conn, Some(T0), Some(T1), 1).unwrap();
    let cx = find(&agents, "codex");

    assert_eq!(cx.sessions, 3, "sub-agent thread source must not count");
    assert_eq!(cx.tool_calls, 2);
    assert_eq!(cx.retries, Some(1), "success='false' api_request rows");
    assert_eq!(cx.counter_cost_usd, None, "codex has no cost counter");

    let c1 = cx.models.iter().find(|(m, _)| m == "c1").unwrap().1;
    assert_eq!(
        &c1,
        &AgentTokenUsage {
            input: 100,
            output: 50,
            cache_read: 0,
            cache_write: 0,
            reasoning: 0,
        },
        "total token_type must not double-count"
    );
}

#[test]
fn agent_rollup_claude_events_and_tool_spans() {
    let conn = setup_test_db();

    // Sessions.
    insert_metric_row(
        &conn,
        "claude_code.session.count",
        T0,
        1,
        r#"{"session.id":"s1"}"#,
    );
    insert_metric_row(
        &conn,
        "claude_code.session.count",
        T1,
        1,
        r#"{"session.id":"s2"}"#,
    );

    // Per-event tokens: 1000 input + 300 output on k1 in the window.
    insert_metric_row(
        &conn,
        "claude_code.token.usage",
        T1,
        1000,
        r#"{"session.id":"s1","model":"k1","type":"input"}"#,
    );
    insert_metric_row(
        &conn,
        "claude_code.token.usage",
        T1,
        300,
        r#"{"session.id":"s1","model":"k1","type":"output"}"#,
    );

    // Tool calls via execution spans: 2 in window, 1 before it.
    insert_tool_exec_span(&conn, T0 / 2);
    insert_tool_exec_span(&conn, T1);
    insert_tool_exec_span(&conn, T1 + SEC);

    let agents = reader::query_agent_rollup(&conn, Some(T1), Some(T1), 1).unwrap();
    let cl = find(&agents, "claude");

    // s1's start marker is before the window; only s2's counts.
    assert_eq!(cl.sessions, 1);
    assert_eq!(cl.tool_calls, 1, "only in-window execution spans count");
    assert_eq!(cl.retries, None, "claude emits no retry telemetry");
    assert_eq!(cl.counter_cost_usd, None);

    let k1 = cl.models.iter().find(|(m, _)| m == "k1").unwrap().1;
    assert_eq!(
        &k1,
        &AgentTokenUsage {
            input: 1000,
            output: 300,
            cache_read: 0,
            cache_write: 0,
            reasoning: 0,
        }
    );
}

#[test]
fn agent_rollup_window_excludes_out_of_window_activity() {
    let conn = setup_test_db();

    // A claude session that ended before the window, and tokens in that
    // session before the window: neither may appear in a later window.
    insert_metric_row(
        &conn,
        "claude_code.session.count",
        T0 - SEC,
        1,
        r#"{"session.id":"old"}"#,
    );
    insert_metric_row(
        &conn,
        "claude_code.token.usage",
        T0 - SEC,
        500,
        r#"{"session.id":"old","model":"k1","type":"input"}"#,
    );

    let agents = reader::query_agent_rollup(&conn, Some(T1), Some(T1 + SEC), 1).unwrap();
    assert!(
        agents.is_empty(),
        "no in-window activity -> agent omitted: {agents:?}"
    );
}

#[test]
fn agent_rollup_double_typed_values_in_value_double() {
    let conn = setup_test_db();

    // claude: one int-stored and one double-stored token row; SUM over a
    // mixed int/real column is REAL and must not fail the i64-era fetch.
    insert_double_metric_row(
        &conn,
        "claude_code.session.count",
        T1,
        1.0,
        r#"{"session.id":"s1"}"#,
    );
    insert_metric_row(
        &conn,
        "claude_code.token.usage",
        T1,
        1000,
        r#"{"session.id":"s1","model":"k1","type":"input"}"#,
    );
    insert_double_metric_row(
        &conn,
        "claude_code.token.usage",
        T1,
        250.0,
        r#"{"session.id":"s1","model":"k1","type":"input"}"#,
    );

    // codex: all three per-event counters double-stored.
    insert_double_metric_row(
        &conn,
        "codex.thread.started",
        T1,
        3.0,
        r#"{"session_source":"cli"}"#,
    );
    insert_double_metric_row(&conn, "codex.tool.call", T1, 2.0, r#"{"tool":"shell"}"#);
    insert_double_metric_row(
        &conn,
        "codex.api_request",
        T1,
        1.0,
        r#"{"success":"false"}"#,
    );

    let agents = reader::query_agent_rollup(&conn, Some(T1), Some(T1), 1).unwrap();

    let cl = find(&agents, "claude");
    let k1 = cl.models.iter().find(|(m, _)| m == "k1").unwrap().1;
    assert_eq!(k1.input, 1250, "int + double addends must sum");

    let cx = find(&agents, "codex");
    assert_eq!(cx.sessions, 3);
    assert_eq!(cx.tool_calls, 2);
    assert_eq!(cx.retries, Some(1));
}

#[test]
fn agent_rollup_series_bucket_alignment() {
    let conn = setup_test_db();

    // Two claude events inside the 2s bucket starting at T1: same bucket.
    // (T1 + SEC would sit exactly on the next bucket's boundary.)
    insert_metric_row(
        &conn,
        "claude_code.token.usage",
        T1,
        100,
        r#"{"session.id":"s1","model":"k1","type":"input"}"#,
    );
    insert_metric_row(
        &conn,
        "claude_code.token.usage",
        T1 + 500_000_000,
        50,
        r#"{"session.id":"s1","model":"k1","type":"input"}"#,
    );

    let agents = reader::query_agent_rollup(&conn, Some(T0), Some(T1 + 2 * SEC), 2).unwrap();
    let cl = find(&agents, "claude");
    assert_eq!(cl.series.len(), 1, "both events fall in one 2s bucket");
    // T1 sits 1s into the 2s bucket that starts at T0.
    assert_eq!(cl.series[0].0, T0, "bucket start aligned to bucket_secs");
    assert_eq!(cl.series[0].0 % (2 * SEC), 0, "bucket start aligned");
    assert_eq!(cl.series[0].1[0].1.input, 150);
}
