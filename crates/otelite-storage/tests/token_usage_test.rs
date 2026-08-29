//! Tests for token usage query functionality

use otelite_core::filters::GenAiFilters;
use otelite_storage::sqlite::{reader, schema};
use rusqlite::Connection;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema::initialize_schema(&conn).unwrap();
    conn
}

#[test]
fn test_query_token_usage_empty() {
    let conn = setup_test_db();
    let (summary, by_model, by_system) =
        reader::query_token_usage(&conn, None, None, &GenAiFilters::default()).unwrap();

    assert_eq!(summary.total_input_tokens, 0);
    assert_eq!(summary.total_output_tokens, 0);
    assert_eq!(summary.total_requests, 0);
    assert_eq!(summary.total_cache_creation_tokens, 0);
    assert_eq!(summary.total_cache_read_tokens, 0);
    assert_eq!(by_model.len(), 0);
    assert_eq!(by_system.len(), 0);
}

#[test]
fn test_query_token_usage_with_data() {
    let conn = setup_test_db();

    // Insert test spans with GenAI attributes
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace1', 'span1', 'llm.call', 0, 1000, 2000,
                   '{"gen_ai.system":"openai","gen_ai.request.model":"gpt-4","gen_ai.usage.input_tokens":"1000","gen_ai.usage.output_tokens":"500"}',
                   1)"#,
        [],
    )
    .unwrap();

    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace2', 'span2', 'llm.call', 0, 3000, 4000,
                   '{"gen_ai.system":"anthropic","gen_ai.request.model":"claude-sonnet-4","gen_ai.usage.input_tokens":"2000","gen_ai.usage.output_tokens":"800"}',
                   1)"#,
        [],
    )
    .unwrap();

    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace3', 'span3', 'llm.call', 0, 5000, 6000,
                   '{"gen_ai.system":"openai","gen_ai.request.model":"gpt-4","gen_ai.usage.input_tokens":"1500","gen_ai.usage.output_tokens":"600"}',
                   1)"#,
        [],
    )
    .unwrap();

    let (summary, by_model, by_system) =
        reader::query_token_usage(&conn, None, None, &GenAiFilters::default()).unwrap();

    // Check summary
    assert_eq!(summary.total_input_tokens, 4500); // 1000 + 2000 + 1500
    assert_eq!(summary.total_output_tokens, 1900); // 500 + 800 + 600
    assert_eq!(summary.total_requests, 3);

    // Check by_model (sorted by total tokens desc)
    assert_eq!(by_model.len(), 2);
    assert_eq!(by_model[0].model, "openai/gpt-4"); // provider-prefixed identity (#143)
    assert_eq!(by_model[0].input_tokens, 2500); // 1000 + 1500
    assert_eq!(by_model[0].output_tokens, 1100); // 500 + 600
    assert_eq!(by_model[0].requests, 2);

    assert_eq!(by_model[1].model, "anthropic/claude-sonnet-4"); // provider-prefixed identity (#143)
    assert_eq!(by_model[1].input_tokens, 2000);
    assert_eq!(by_model[1].output_tokens, 800);
    assert_eq!(by_model[1].requests, 1);

    // Check by_system (sorted by total tokens desc)
    assert_eq!(by_system.len(), 2);
    assert_eq!(by_system[0].system, "openai");
    assert_eq!(by_system[0].input_tokens, 2500);
    assert_eq!(by_system[0].output_tokens, 1100);
    assert_eq!(by_system[0].requests, 2);

    assert_eq!(by_system[1].system, "anthropic");
    assert_eq!(by_system[1].input_tokens, 2000);
    assert_eq!(by_system[1].output_tokens, 800);
    assert_eq!(by_system[1].requests, 1);
}

#[test]
fn test_query_token_usage_with_time_filter() {
    let conn = setup_test_db();

    // Insert spans at different times
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace1', 'span1', 'llm.call', 0, 1000, 2000,
                   '{"gen_ai.system":"openai","gen_ai.request.model":"gpt-4","gen_ai.usage.input_tokens":"1000","gen_ai.usage.output_tokens":"500"}',
                   1)"#,
        [],
    )
    .unwrap();

    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace2', 'span2', 'llm.call', 0, 5000, 6000,
                   '{"gen_ai.system":"openai","gen_ai.request.model":"gpt-4","gen_ai.usage.input_tokens":"2000","gen_ai.usage.output_tokens":"800"}',
                   1)"#,
        [],
    )
    .unwrap();

    // Query with time filter (only first span)
    let (summary, by_model, _) =
        reader::query_token_usage(&conn, Some(0), Some(3000), &GenAiFilters::default()).unwrap();

    assert_eq!(summary.total_input_tokens, 1000);
    assert_eq!(summary.total_output_tokens, 500);
    assert_eq!(summary.total_requests, 1);
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_model[0].model, "openai/gpt-4"); // provider-prefixed identity (#143)
}

#[test]
fn test_query_token_usage_ignores_non_genai_spans() {
    let conn = setup_test_db();

    // Insert GenAI span
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace1', 'span1', 'llm.call', 0, 1000, 2000,
                   '{"gen_ai.system":"openai","gen_ai.request.model":"gpt-4","gen_ai.usage.input_tokens":"1000","gen_ai.usage.output_tokens":"500"}',
                   1)"#,
        [],
    )
    .unwrap();

    // Insert non-GenAI span (no gen_ai.system attribute)
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace2', 'span2', 'http.request', 0, 3000, 4000,
                   '{"http.method":"GET","http.url":"/api/users"}',
                   1)"#,
        [],
    )
    .unwrap();

    let (summary, by_model, by_system) =
        reader::query_token_usage(&conn, None, None, &GenAiFilters::default()).unwrap();

    // Should only count the GenAI span
    assert_eq!(summary.total_input_tokens, 1000);
    assert_eq!(summary.total_output_tokens, 500);
    assert_eq!(summary.total_requests, 1);
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_system.len(), 1);
}

#[test]
fn test_query_token_usage_handles_missing_token_fields() {
    let conn = setup_test_db();

    // Insert span with gen_ai.system but no token counts
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace1', 'span1', 'llm.call', 0, 1000, 2000,
                   '{"gen_ai.system":"openai","gen_ai.request.model":"gpt-4"}',
                   1)"#,
        [],
    )
    .unwrap();

    let (summary, by_model, _by_system) =
        reader::query_token_usage(&conn, None, None, &GenAiFilters::default()).unwrap();

    // Should handle missing fields gracefully (COALESCE to 0)
    assert_eq!(summary.total_input_tokens, 0);
    assert_eq!(summary.total_output_tokens, 0);
    assert_eq!(summary.total_requests, 1);
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_model[0].model, "openai/gpt-4"); // provider-prefixed identity (#143)
    assert_eq!(by_model[0].input_tokens, 0);
    assert_eq!(by_model[0].output_tokens, 0);
}

#[test]
fn test_output_context_ratio_includes_cache_tokens() {
    let conn = setup_test_db();

    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace-no-cache', 'span-no-cache', 'llm.call', 0, 0, 1000000000,
                   '{"gen_ai.system":"anthropic","gen_ai.request.model":"uncached-model","gen_ai.usage.input_tokens":"100","gen_ai.usage.output_tokens":"100"}', 1)"#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('trace-cache', 'span-cache', 'llm.call', 0, 0, 1000000000,
                   '{"gen_ai.system":"anthropic","gen_ai.request.model":"cached-model","gen_ai.usage.input_tokens":"100","gen_ai.usage.output_tokens":"100","gen_ai.usage.cache_read.input_tokens":"900","gen_ai.usage.cache_creation.input_tokens":"1000"}', 1)"#,
        [],
    )
    .unwrap();

    let stats = reader::query_latency_stats(
        &conn,
        None,
        None,
        &GenAiFilters {
            model: Some("cached-model".to_string()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(stats.len(), 1);
    assert_eq!(stats[0].output_input_ratio_p50, Some(0.05));
    assert_eq!(stats[0].output_input_ratio_p95, Some(0.05));

    let top_spans = reader::query_top_spans(
        &conn,
        None,
        None,
        &GenAiFilters::default(),
        10,
        otelite_core::api::TopSpanSort::OutputInputRatio,
        false,
    )
    .unwrap();
    assert_eq!(top_spans[0].span_id, "span-no-cache");
}

#[test]
fn test_analytics_adapters_select_codex_requests_and_opencode_llm_calls() {
    let conn = setup_test_db();

    // Codex tags many internal spans with `model`. Only run_sampling_request is
    // one completed model call; the nested transport span must not be counted.
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('codex-trace', 'codex-request', 'run_sampling_request', 0, 0, 4000000000,
                   '{"model":"codex-test-model","otel.scope.name":"codex_cli_rs"}', 1)"#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, parent_span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('codex-trace', 'codex-http', 'codex-request', 'model_client.stream_responses_api', 0, 0, 3000000000,
                   '{"model":"codex-test-model","otel.scope.name":"codex_cli_rs"}', 1)"#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('other-trace', 'other-request', 'run_sampling_request', 0, 0, 5000000000,
                   '{"model":"codex-test-model","otel.scope.name":"other_exporter"}', 1)"#,
        [],
    )
    .unwrap();

    // OpenCode already emits OpenInference LLM spans. Keep that existing
    // adapter path covered alongside the Codex-specific one.
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('opencode-trace', 'opencode-request', 'opencode.llm', 0, 0, 2000000000,
                   '{"openinference.span.kind":"LLM","llm.system":"opencode-go","llm.model_name":"opencode-test-model","llm.usage.prompt_tokens":"100","llm.usage.completion_tokens":"40"}', 1)"#,
        [],
    )
    .unwrap();

    let (summary, by_model, by_system) =
        reader::query_token_usage(&conn, None, None, &GenAiFilters::default()).unwrap();
    assert_eq!(summary.total_requests, 1);
    assert_eq!(summary.total_input_tokens, 100);
    assert_eq!(summary.total_output_tokens, 40);

    assert!(
        !by_model.iter().any(|row| row.model == "codex-test-model"),
        "token analytics must not present unavailable Codex usage as zero"
    );

    let opencode = by_model
        .iter()
        .find(|row| row.model == "opencode-go/opencode-test-model") // provider-prefixed identity (#143)
        .expect("OpenCode model should appear in analytics");
    assert_eq!(opencode.requests, 1);
    assert_eq!(opencode.input_tokens, 100);
    assert_eq!(opencode.output_tokens, 40);
    assert_eq!(by_system.len(), 1);
    assert_eq!(by_system[0].system, "opencode-go");

    let latency = reader::query_latency_stats(&conn, None, None, &GenAiFilters::default()).unwrap();
    let codex_latency = latency
        .iter()
        .find(|row| row.model.as_deref() == Some("codex-test-model"))
        .expect("Codex latency should use run_sampling_request");
    assert_eq!(codex_latency.count, 1);
    assert_eq!(codex_latency.avg_ms, 4000.0);
    assert_eq!(codex_latency.input_tokens_p50, None);
    assert_eq!(codex_latency.derived_tokens_per_sec_p50, None);
    assert_eq!(codex_latency.output_input_ratio_p50, None);

    let series = reader::query_latency_series(
        &conn,
        None,
        None,
        3600,
        &GenAiFilters::default(),
        false,
        None,
    )
    .unwrap();
    assert_eq!(series.len(), 2);
    assert!(series.iter().any(|row| {
        row.model.as_deref() == Some("codex-test-model") && row.count == 1 && row.avg_ms == 4000.0
    }));

    let calls =
        reader::query_calls_series(&conn, None, None, &GenAiFilters::default(), 3600, false)
            .unwrap();
    assert!(calls
        .iter()
        .any(|row| { row.model.as_deref() == Some("codex-test-model") && row.requests == 1 }));

    // Codex does not emit usage attributes, so it must not appear in cost
    // analytics as a fabricated zero-cost request.
    let cost_series = reader::query_cost_series(
        &conn,
        None,
        None,
        3600 * 1_000_000_000,
        &GenAiFilters::default(),
    )
    .unwrap();
    assert_eq!(cost_series.len(), 1);
    assert_eq!(
        cost_series[0].model.as_deref(),
        Some("opencode-go/opencode-test-model")
    ); // #143

    let top_spans = reader::query_top_spans(
        &conn,
        None,
        None,
        &GenAiFilters::default(),
        10,
        otelite_core::api::TopSpanSort::TotalTokens,
        false,
    )
    .unwrap();
    assert_eq!(top_spans.len(), 1);
    assert_eq!(
        top_spans[0].model.as_deref(),
        Some("opencode-go/opencode-test-model")
    ); // #143
}

#[test]
fn test_genai_capability_report_keeps_codex_usage_unavailable_and_deduplicates_spans() {
    let conn = setup_test_db();
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('codex-trace', 'request', 'run_sampling_request', 0, 0, 4000000000,
                   '{"model":"codex-test-model","otel.scope.name":"codex_cli_rs"}', 1)"#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('codex-trace', 'request', 'run_sampling_request', 0, 0, 4000000000,
                   '{"model":"codex-test-model","otel.scope.name":"codex_cli_rs"}', 1)"#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('opencode-trace', 'request', 'opencode.llm', 0, 0, 2000000000,
                   '{"openinference.span.kind":"LLM","llm.system":"opencode-go","llm.model_name":"opencode-test-model","llm.usage.prompt_tokens":"0","llm.usage.completion_tokens":"40"}', 1)"#,
        [],
    )
    .unwrap();

    let report =
        reader::query_genai_capabilities(&conn, None, None, &GenAiFilters::default()).unwrap();
    assert_eq!(report.canonical_span_count, 2);
    assert_eq!(report.duplicate_span_count, 1);
    let codex = report
        .reports
        .iter()
        .find(|row| row.emitter == "codex")
        .unwrap();
    assert_eq!(codex.output_tokens.availability, "absent");
    assert_eq!(codex.output_tokens.derivation, "unavailable");
    // No usage spans were delivered, so the rule is applied but everything
    // is a request-level gap.
    assert_eq!(codex.correlation.rule, "codex-one-to-one-v1");
    assert_eq!(codex.correlation.unmatched_count, 1);
    assert_eq!(codex.correlation.matched_count, 0);

    let opencode = report
        .reports
        .iter()
        .find(|row| row.emitter == "opencode")
        .unwrap();
    assert_eq!(opencode.input_tokens.availability, "available");
    assert_eq!(opencode.input_tokens.valid_count, 1);
    assert_eq!(opencode.input_tokens.derivation, "native");
    assert_eq!(
        opencode
            .input_tokens
            .source_attributes
            .get("llm.usage.prompt_tokens"),
        Some(&1)
    );
}

#[test]
fn test_genai_capability_report_correlates_codex_usage_one_to_one() {
    let conn = setup_test_db();
    let insert_span = |trace: &str, id: &str, name: &str, parent: &str, attributes: &str| {
        conn.execute(
            r#"INSERT INTO spans (trace_id, span_id, parent_span_id, name, kind, start_time, end_time, attributes, status_code)
               VALUES (?1, ?2, ?3, ?4, 0, 0, 4000000000, ?5, 0)"#,
            rusqlite::params![trace, id, parent, name, attributes],
        )
        .unwrap();
    };

    // t1: clean one-to-one join (request -> attempt -> usage span).
    insert_span("t1", "turn1", "run_turn", "", "{}");
    insert_span(
        "t1",
        "req1",
        "run_sampling_request",
        "turn1",
        r#"{"model":"m1","otel.scope.name":"codex_cli_rs"}"#,
    );
    insert_span("t1", "try1", "try_run_sampling_request", "req1", "{}");
    insert_span(
        "t1",
        "usage1",
        "handle_responses",
        "try1",
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.input_tokens":"100","gen_ai.usage.output_tokens":"50"}"#,
    );

    // t2: two usage candidates under one request (retry) -> ambiguous.
    insert_span("t2", "turn2", "run_turn", "", "{}");
    insert_span(
        "t2",
        "req2",
        "run_sampling_request",
        "turn2",
        r#"{"model":"m1","otel.scope.name":"codex_cli_rs"}"#,
    );
    insert_span("t2", "try2a", "try_run_sampling_request", "req2", "{}");
    insert_span("t2", "try2b", "try_run_sampling_request", "req2", "{}");
    insert_span(
        "t2",
        "usage2a",
        "handle_responses",
        "try2a",
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"5"}"#,
    );
    insert_span(
        "t2",
        "usage2b",
        "handle_responses",
        "try2b",
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"6"}"#,
    );

    // t3: request with no usage span -> unmatched.
    insert_span("t3", "turn3", "run_turn", "", "{}");
    insert_span(
        "t3",
        "req3",
        "run_sampling_request",
        "turn3",
        r#"{"model":"m1","otel.scope.name":"codex_cli_rs"}"#,
    );

    // t4: orphan usage span whose chain never reaches a request span.
    insert_span("t4", "orphan", "turn_context.make", "", "{}");
    insert_span(
        "t4",
        "usage4",
        "handle_responses",
        "orphan",
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"7"}"#,
    );

    // t5: errored request with a usage span -> rejected.
    insert_span("t5", "turn5", "run_turn", "", "{}");
    insert_span(
        "t5",
        "req5",
        "run_sampling_request",
        "turn5",
        r#"{"model":"m1","otel.scope.name":"codex_cli_rs"}"#,
    );
    insert_span(
        "t5",
        "usage5",
        "handle_responses",
        "req5",
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"8"}"#,
    );
    conn.execute(
        r#"UPDATE spans SET status_code = 2 WHERE trace_id = 't5' AND span_id = 'req5'"#,
        [],
    )
    .unwrap();

    // t6: usage span whose model conflicts with the request -> rejected.
    insert_span("t6", "turn6", "run_turn", "", "{}");
    insert_span(
        "t6",
        "req6",
        "run_sampling_request",
        "turn6",
        r#"{"model":"m1","otel.scope.name":"codex_cli_rs"}"#,
    );
    insert_span(
        "t6",
        "usage6",
        "handle_responses",
        "req6",
        r#"{"otel.scope.name":"codex_cli_rs","model":"m2","gen_ai.usage.output_tokens":"9"}"#,
    );

    // A non-Codex identity shares the sample and must keep rule `none`.
    insert_span(
        "opencode-trace",
        "request",
        "opencode.llm",
        "",
        r#"{"openinference.span.kind":"LLM","llm.system":"opencode-go","llm.model_name":"opencode-test-model","llm.usage.prompt_tokens":"0","llm.usage.completion_tokens":"40"}"#,
    );

    let report =
        reader::query_genai_capabilities(&conn, None, None, &GenAiFilters::default()).unwrap();

    let codex = report
        .reports
        .iter()
        .find(|row| row.emitter == "codex")
        .unwrap();
    // t4 deliberately has no request span: five requests in the group.
    assert_eq!(codex.request_count, 5);
    assert_eq!(codex.correlation.rule, "codex-one-to-one-v1");
    assert_eq!(codex.correlation.matched_count, 1);
    assert_eq!(codex.correlation.unmatched_count, 1);
    assert_eq!(codex.correlation.rejected_count, 2);
    assert_eq!(codex.correlation.ambiguous_count, 2);

    // The single clean join surfaces as correlated derivation with the
    // candidate's source attributes; the request denominator stays 5.
    assert_eq!(codex.input_tokens.derivation, "correlated");
    assert_eq!(codex.input_tokens.availability, "sparse");
    assert_eq!(codex.input_tokens.valid_count, 1);
    assert_eq!(codex.input_tokens.eligible_count, 5);
    assert_eq!(
        codex
            .input_tokens
            .source_attributes
            .get("gen_ai.usage.input_tokens"),
        Some(&1)
    );
    assert_eq!(codex.output_tokens.derivation, "correlated");
    assert_eq!(codex.output_tokens.valid_count, 1);
    // Rejected, ambiguous and unmatched requests contribute no values.
    assert_eq!(codex.cache_read_tokens.derivation, "unavailable");
    assert_eq!(codex.cache_read_tokens.valid_count, 0);

    let opencode = report
        .reports
        .iter()
        .find(|row| row.emitter == "opencode")
        .unwrap();
    assert_eq!(opencode.correlation.rule, "none");
    assert_eq!(opencode.input_tokens.derivation, "native");
}

#[test]
fn test_genai_capability_report_correlation_edge_cases() {
    let conn = setup_test_db();
    let insert_span = |trace: &str,
                       id: &str,
                       name: &str,
                       parent: &str,
                       start_ms: i64,
                       attributes: &str| {
        conn.execute(
            r#"INSERT INTO spans (trace_id, span_id, parent_span_id, name, kind, start_time, end_time, attributes, status_code)
               VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6, ?7, 0)"#,
            rusqlite::params![
                trace,
                id,
                parent,
                name,
                start_ms * 1_000_000,
                start_ms * 1_000_000 + 4_000_000_000,
                attributes
            ],
        )
        .unwrap();
    };
    let codex_req = r#"{"model":"m1","otel.scope.name":"codex_cli_rs"}"#;

    // e1: cancellation-style request — status UNSET (0), not OK and not
    // ERROR — still joins: only explicit ERROR rejects.
    insert_span("e1", "e1_req", "run_sampling_request", "", 1_000, codex_req);
    insert_span(
        "e1",
        "e1_usage",
        "handle_responses",
        "e1_req",
        1_001,
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"5"}"#,
    );

    // e2: late delivery — the usage span was ingested (smaller row id) and
    // started before its request span was stored. The join is structural,
    // so it must still match.
    insert_span(
        "e2",
        "e2_usage",
        "handle_responses",
        "e2_try",
        2_005,
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"6"}"#,
    );
    insert_span(
        "e2",
        "e2_try",
        "try_run_sampling_request",
        "e2_req",
        2_001,
        "{}",
    );
    insert_span("e2", "e2_req", "run_sampling_request", "", 2_000, codex_req);

    // e3: zero-token result — zero is a valid observation, not absent.
    insert_span("e3", "e3_req", "run_sampling_request", "", 3_000, codex_req);
    insert_span(
        "e3",
        "e3_usage",
        "handle_responses",
        "e3_req",
        3_001,
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"0"}"#,
    );

    // e4: turn-level counter — a usage span under the turn, not under any
    // request. It must never be attributed to a request.
    insert_span("e4", "e4_turn", "run_turn", "", 4_000, "{}");
    insert_span(
        "e4",
        "e4_req",
        "run_sampling_request",
        "e4_turn",
        4_001,
        codex_req,
    );
    insert_span(
        "e4",
        "e4_turn_usage",
        "handle_responses",
        "e4_turn",
        4_002,
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"999"}"#,
    );

    // e5: deep nesting (four hops below the request) still joins.
    insert_span("e5", "e5_req", "run_sampling_request", "", 5_000, codex_req);
    insert_span("e5", "e5_a", "mid_a", "e5_req", 5_001, "{}");
    insert_span("e5", "e5_b", "mid_b", "e5_a", 5_002, "{}");
    insert_span("e5", "e5_c", "mid_c", "e5_b", 5_003, "{}");
    insert_span(
        "e5",
        "e5_usage",
        "handle_responses",
        "e5_c",
        5_004,
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"7"}"#,
    );

    // e6: a parent chain longer than the walk cap (64) never reaches a
    // request span — the candidate is dropped, the walk terminates.
    let mut parent = String::new();
    for hop in 0..70 {
        let id = format!("e6_h{hop}");
        if hop > 0 {
            insert_span("e6", &id, "deep", &parent, 6_000 + hop as i64, "{}");
        } else {
            insert_span("e6", &id, "deep", "", 6_000, "{}");
        }
        parent = id;
    }
    insert_span(
        "e6",
        "e6_usage",
        "handle_responses",
        &parent,
        7_000,
        r#"{"otel.scope.name":"codex_cli_rs","gen_ai.usage.output_tokens":"8"}"#,
    );

    let report =
        reader::query_genai_capabilities(&conn, None, None, &GenAiFilters::default()).unwrap();
    let codex = report
        .reports
        .iter()
        .find(|row| row.emitter == "codex")
        .unwrap();

    // Five requests: e1 (unset), e2 (late), e3 (zero), e4 (no usage),
    // e5 (deep). e4's request has no usage descendant — a request-level
    // gap; e6's chain is too deep and e4's turn-level counter is a sibling,
    // so neither is ever attributed.
    assert_eq!(codex.request_count, 5);
    assert_eq!(codex.correlation.matched_count, 4);
    assert_eq!(codex.correlation.unmatched_count, 1);
    assert_eq!(codex.correlation.rejected_count, 0);
    assert_eq!(codex.correlation.ambiguous_count, 0);

    // The zero-token result is a valid correlated observation...
    assert_eq!(codex.output_tokens.valid_count, 4);
    assert_eq!(codex.output_tokens.eligible_count, 5);
    assert_eq!(codex.output_tokens.availability, "sparse");
    assert_eq!(codex.output_tokens.derivation, "correlated");
    // ...and the turn-level 999 never leaked into request attribution.
    assert_eq!(codex.output_tokens.observed_count, 0);
    assert_eq!(
        codex
            .output_tokens
            .source_attributes
            .get("gen_ai.usage.output_tokens"),
        Some(&4)
    );
}

#[test]
fn test_genai_capability_report_diagnoses_unidentified_emitters() {
    let conn = setup_test_db();
    // Three LLM-ish spans no verified signature matches, in three different
    // missing-attribute classes:
    //   u1: model + system, no usage/TTFT -> missing a usage attribute
    //   u2: model only                   -> missing system + usage
    //   u3: model, TTFT present          -> fully standard-eligible would
    //       match, so use a Codex span name without the verified scope
    //       instead -> missing otel.scope.name (x2 for the count)
    let insert = |id: &str, name: &str, attrs: &str| {
        conn.execute(
            r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
               VALUES (?1, ?1, ?2, 0, 0, 1000000000, ?3, 0)"#,
            rusqlite::params![id, name, attrs],
        )
        .unwrap();
    };
    insert(
        "u1",
        "llm.call",
        "{\"gen_ai.system\":\"openai\",\"gen_ai.request.model\":\"mystery\"}",
    );
    insert("u2", "llm.call", "{\"gen_ai.request.model\":\"mystery\"}");
    insert(
        "u3",
        "run_sampling_request",
        "{\"model\":\"mystery\",\"gen_ai.system\":\"openai\"}",
    );
    insert(
        "u4",
        "run_sampling_request",
        "{\"model\":\"mystery\",\"gen_ai.system\":\"openai\"}",
    );

    let report =
        reader::query_genai_capabilities(&conn, None, None, &GenAiFilters::default()).unwrap();

    // Unidentifiable spans never become groups.
    assert!(report.reports.is_empty());
    assert_eq!(report.canonical_span_count, 0);

    let by_names = |names: &[&str]| {
        report
            .unidentified
            .iter()
            .find(|u| u.required_attributes == names)
    };
    assert_eq!(
        by_names(&["gen_ai.usage.input_tokens"]).unwrap().span_count,
        1,
        "u1 lacks only a standard usage attribute"
    );
    assert_eq!(
        by_names(&["gen_ai.provider.name", "gen_ai.usage.input_tokens"])
            .unwrap()
            .span_count,
        1,
        "u2 lacks a system attribute and a usage attribute"
    );
    assert_eq!(
        by_names(&["otel.scope.name"]).unwrap().span_count,
        2,
        "the two Codex-named spans share the scope gap"
    );
    assert_eq!(report.unidentified.len(), 3);

    // Privacy invariant: names and counts only.
    let json = serde_json::to_string(&report).unwrap();
    assert!(!json.contains("mystery"), "attribute values must not leak");
    assert!(!json.contains("u1"), "span identifiers must not leak");
}

#[test]
fn test_ttft_normalisation_agrees_across_capability_and_latency_paths() {
    let conn = setup_test_db();
    // One model, six requests of 1.0s duration each, exercising every TTFT
    // edge the two paths used to normalise separately:
    //   p1 spec seconds (valid), p2 custom ms (valid, mixed units),
    //   p3 negative (invalid), p4 greater than duration (invalid),
    //   p5 non-numeric (invalid), p6 both keys present (priority: spec wins;
    //   the custom key alone would exceed the duration).
    let insert = |id: &str, start_ms: i64, attrs: &str| {
        conn.execute(
            r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
               VALUES (?1, ?1, 'llm.request', 0, ?2, ?3, ?4, 0)"#,
            rusqlite::params![
                id,
                start_ms * 1_000_000,
                start_ms * 1_000_000 + 1_000_000_000,
                attrs
            ],
        )
        .unwrap();
    };
    let base = r#"{"gen_ai.system":"openai","gen_ai.request.model":"parity-m1","gen_ai.usage.input_tokens":"10","gen_ai.usage.output_tokens":"2""#;
    insert(
        "p1",
        1_000,
        &format!("{base},\"gen_ai.server.time_to_first_token\":\"0.2\"}}"),
    );
    insert("p2", 2_000, &format!("{base},\"ttft_ms\":\"150\"}}"));
    insert(
        "p3",
        3_000,
        &format!("{base},\"gen_ai.server.time_to_first_token\":\"-1\"}}"),
    );
    insert(
        "p4",
        4_000,
        &format!("{base},\"gen_ai.server.time_to_first_token\":\"1.5\"}}"),
    );
    insert(
        "p5",
        5_000,
        &format!("{base},\"gen_ai.server.time_to_first_token\":\"not-a-number\"}}"),
    );
    insert(
        "p6",
        6_000,
        &format!("{base},\"gen_ai.server.time_to_first_token\":\"0.3\",\"ttft_ms\":\"999\"}}"),
    );

    // Capability path (core normaliser over the request spans).
    let report =
        reader::query_genai_capabilities(&conn, None, None, &GenAiFilters::default()).unwrap();
    let cap = report
        .reports
        .iter()
        .find(|row| row.model.as_deref() == Some("parity-m1"))
        .unwrap()
        .ttft
        .clone();

    // Latency path (storage normaliser feeding the percentile accumulators).
    let stats = reader::query_latency_stats(&conn, None, None, &GenAiFilters::default()).unwrap();
    // The latency path reports the composite provider/model identity.
    let stat = stats
        .iter()
        .find(|row| row.model.as_deref() == Some("openai/parity-m1"))
        .unwrap();

    // Both paths must split the same six observations identically.
    assert_eq!(cap.eligible_count, 6);
    assert_eq!(cap.valid_count, 3, "p1, p2 and p6 are valid");
    assert_eq!(cap.invalid_count, 3, "p3, p4 and p5 are invalid");
    assert_eq!(cap.availability, "sparse");
    assert_eq!(cap.quality, "invalid");
    assert_eq!(stat.ttft_count, cap.valid_count, "valid counts must agree");
    assert_eq!(
        stat.ttft_invalid_count, cap.invalid_count,
        "invalid counts must agree"
    );
    // The priority decision is shared too: the valid set is {150, 200, 300}
    // ms — p6 contributes 0.3s from the spec key, not 999ms from ttft_ms.
    assert_eq!(
        stat.ttft_p50_ms,
        Some(200),
        "median of 150/200/300 ms — the 999ms custom key was shadowed"
    );
}

#[test]
fn test_genai_capability_report_join_is_bounded_by_the_sample() {
    let conn = setup_test_db();
    // 10_005 filler spans start after the Codex spans, so the bounded
    // newest-first sample holds only filler and `truncated` is true.
    conn.execute_batch(
        r#"
        CREATE TEMP TABLE filler (i INTEGER PRIMARY KEY);
        INSERT INTO filler (i) VALUES (1);
        WITH RECURSIVE seq(i) AS (
            SELECT i FROM filler
            UNION ALL SELECT i + 1 FROM seq WHERE i < 10005
        )
        INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
        SELECT 'noise-' || i, 'n' || i, 'noise', 0, 2000000000 + i, 3000000000 + i, '{}', 0
        FROM seq;
        "#,
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('b1', 'req', 'run_sampling_request', 0, 0, 4000000000,
                   '{"model":"m1","otel.scope.name":"codex_cli_rs"}', 0)"#,
        [],
    )
    .unwrap();

    let report =
        reader::query_genai_capabilities(&conn, None, None, &GenAiFilters::default()).unwrap();
    assert!(
        report.truncated,
        "sample of 10_001 spans must be flagged truncated"
    );
    // The Codex request fell outside the bounded sample: no group, no guess.
    assert_eq!(report.canonical_span_count, 0);
    assert!(report.reports.is_empty());
}

#[test]
fn test_latency_stats_normalizes_ttft_and_flags_degenerate_groups() {
    let conn = setup_test_db();

    for index in 0..10 {
        conn.execute(
            r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
               VALUES (?1, ?2, 'claude_code.llm_request', 0, 0, 1000000000,
                       '{"model":"buffered-model","input_tokens":"100","ttft_ms":"950"}', 1)"#,
            rusqlite::params![format!("buffered-trace-{index}"), format!("buffered-span-{index}")],
        )
        .unwrap();
    }
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('otel-trace', 'otel-span', 'chat', 0, 0, 1000000000,
                   '{"gen_ai.system":"openai","gen_ai.request.model":"otel-model",
                     "gen_ai.server.time_to_first_token":"0.5"}', 1)"#,
        [],
    )
    .unwrap();
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('invalid-trace', 'invalid-span', 'claude_code.llm_request', 0, 0, 1000000000,
                   '{"model":"invalid-model","input_tokens":"100","ttft_ms":"1500"}', 1)"#,
        [],
    )
    .unwrap();

    let stats = reader::query_latency_stats(&conn, None, None, &GenAiFilters::default()).unwrap();
    let buffered = stats
        .iter()
        .find(|row| row.model.as_deref() == Some("buffered-model"))
        .unwrap();
    assert_eq!(buffered.ttft_count, 10);
    assert_eq!(buffered.ttft_p50_ms, Some(950));
    assert_eq!(buffered.ttft_degenerate_count, 10);
    assert!(buffered.ttft_degenerate);

    let otel = stats
        .iter()
        .find(|row| row.model.as_deref() == Some("openai/otel-model"))
        .unwrap();
    assert_eq!(otel.ttft_p50_ms, Some(500));
    assert!(!otel.ttft_degenerate);

    let invalid = stats
        .iter()
        .find(|row| row.model.as_deref() == Some("invalid-model"))
        .unwrap();
    assert_eq!(invalid.ttft_count, 0);
    assert_eq!(invalid.ttft_invalid_count, 1);

    let series = reader::query_latency_series(
        &conn,
        None,
        None,
        3600,
        &GenAiFilters::default(),
        false,
        None,
    )
    .unwrap();
    let buffered_series = series
        .iter()
        .find(|row| row.model.as_deref() == Some("buffered-model"))
        .unwrap();
    assert_eq!(buffered_series.ttft_count, 10);
    assert_eq!(buffered_series.ttft_degenerate_count, 10);
    assert!(buffered_series.ttft_degenerate);

    let context =
        reader::query_latency_by_context(&conn, None, None, &GenAiFilters::default()).unwrap();
    let buffered_context = context
        .iter()
        .find(|row| row.model.as_deref() == Some("buffered-model"))
        .unwrap();
    assert_eq!(buffered_context.ttft_count, 10);
    assert_eq!(buffered_context.ttft_degenerate_count, 10);
    assert!(buffered_context.ttft_degenerate);
}

// ---------------------------------------------------------------------------
// Model-performance comparison (#121/#151)
// ---------------------------------------------------------------------------

use otelite_core::api::{ModelPerformanceQuery, ModelPerformanceWindow};

/// One standard-otel request span: model + system + usage + TTFT + duration.
#[allow(clippy::too_many_arguments)] // one fixture knob per argument
fn mp_span(
    conn: &Connection,
    id: &str,
    start_ns: i64,
    duration_ms: i64,
    output_tokens: Option<&str>,
    ttft_secs: Option<&str>,
    input_tokens: &str,
    status: i32,
) {
    let mut attrs = format!(
        "{{\"gen_ai.system\":\"openai\",\"gen_ai.request.model\":\"mp-1\",\"gen_ai.response.model\":\"mp-1\",\"gen_ai.usage.input_tokens\":\"{input_tokens}\""
    );
    if let Some(t) = output_tokens {
        attrs.push_str(&format!(",\"gen_ai.usage.output_tokens\":\"{t}\""));
    }
    if let Some(t) = ttft_secs {
        attrs.push_str(&format!(",\"gen_ai.server.time_to_first_token\":\"{t}\""));
    }
    attrs.push('}');
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES (?1, ?1, 'llm.call', 0, ?2, ?3, ?4, ?5)"#,
        rusqlite::params![id, start_ns, start_ns + duration_ms * 1_000_000, attrs, status],
    )
    .unwrap();
}

#[test]
fn test_model_performance_windows_identity_and_deltas() {
    let conn = setup_test_db();
    // Current window: [100s, 200s); preceding: [0s, 100s); rolling: none.
    // Preceding: durations 100/200/300 ms (p50=200), output 100 tok @100ms
    // (throughput 1000 tok/s). Current: durations 100/200/400 ms (p50=200,
    // p95=400), output 200 tok @100ms (2000 tok/s), one error span.
    for (i, dur) in [100, 200, 300].into_iter().enumerate() {
        mp_span(
            &conn,
            &format!("p{i}"),
            i as i64 * 10_000_000_000,
            dur,
            Some("100"),
            Some("0.05"),
            "10",
            0,
        );
    }
    for (i, dur) in [100, 200, 400].into_iter().enumerate() {
        let status = if i == 2 { 2 } else { 0 }; // 2 = Error
        mp_span(
            &conn,
            &format!("c{i}"),
            100_000_000_000 + i as i64 * 10_000_000_000,
            dur,
            Some("200"),
            Some("0.1"),
            "20",
            status,
        );
    }

    let resp = reader::query_model_performance(
        &conn,
        &ModelPerformanceQuery {
            current: ModelPerformanceWindow {
                start_time: 100_000_000_000,
                end_time: 200_000_000_000,
            },
            rolling: None,
            model: None,
            provider: None,
        },
    )
    .unwrap();

    assert_eq!(resp.current_window.start_time, 100_000_000_000);
    assert_eq!(
        resp.preceding_window,
        ModelPerformanceWindow {
            start_time: 0,
            end_time: 100_000_000_000
        },
        "preceding window is the equal-length interval before current"
    );
    assert!(resp.rolling_window.is_none());
    assert!(!resp.truncated);

    let id = resp
        .identities
        .iter()
        .find(|i| i.model.as_deref() == Some("mp-1"))
        .unwrap();
    assert_eq!(id.provider.as_deref(), Some("openai"));
    assert_eq!(id.response_models, vec!["mp-1".to_string()]);
    assert_eq!(
        (
            id.request_counts.current,
            id.request_counts.preceding,
            id.request_counts.rolling
        ),
        (3, 3, 0)
    );

    // Duration: p50 unchanged (200ms), p95 worsened (300 -> 400 ms).
    let cur = id.duration.current.as_ref().unwrap();
    assert_eq!(cur.eligible_count, 3);
    let p50 = cur.percentiles.iter().find(|p| p.percentile == 50).unwrap();
    let p95 = cur.percentiles.iter().find(|p| p.percentile == 95).unwrap();
    assert_eq!(p50.value, 200.0);
    assert_eq!(p50.delta_vs_preceding.unwrap().absolute, 0.0);
    assert_eq!(p50.delta_vs_preceding.unwrap().relative, Some(0.0));
    assert_eq!(p95.value, 400.0);
    assert_eq!(p95.delta_vs_preceding.unwrap().absolute, 100.0);
    assert_eq!(
        p95.delta_vs_preceding.unwrap().relative,
        Some(100.0 / 300.0)
    );
    assert!(id.duration.rolling.is_none());

    // Throughput is per call (tokens / that call's duration): preceding
    // {333.3, 500, 1000} tok/s, current {500, 1000, 2000} tok/s.
    let tcur = id.throughput.current.as_ref().unwrap();
    let tpre = id.throughput.preceding.as_ref().unwrap();
    assert_eq!(tpre.eligible_count, 3);
    let t50 = tcur
        .percentiles
        .iter()
        .find(|p| p.percentile == 50)
        .unwrap();
    let t50_base = tpre
        .percentiles
        .iter()
        .find(|p| p.percentile == 50)
        .unwrap();
    assert_eq!(t50_base.value, 500.0, "median of 333.3/500/1000");
    assert_eq!(t50.value, 1000.0, "median of 500/1000/2000");
    assert_eq!(t50.delta_vs_preceding.unwrap().relative, Some(1.0));

    // Error rate: 0/3 -> 1/3 with an absolute-points delta.
    let err = id.error_rate.current.as_ref().unwrap();
    assert_eq!((err.requests, err.errors), (3, 1));
    assert!((err.rate - 1.0 / 3.0).abs() < 1e-12);
    let delta = err.delta_vs_preceding.as_ref().unwrap();
    assert!((delta.absolute - 1.0 / 3.0).abs() < 1e-12);
    assert_eq!(
        delta.relative, None,
        "zero baseline rate: percentage unavailable"
    );
}

#[test]
fn test_model_performance_zero_baseline_reports_percentage_unavailable() {
    let conn = setup_test_db();
    // Preceding window: requests exist but no output tokens (ineligible for
    // throughput). Current: one eligible throughput sample.
    mp_span(&conn, "p0", 0, 100, None, None, "10", 0);
    mp_span(&conn, "c0", 100_000_000_000, 100, Some("50"), None, "10", 0);

    let resp = reader::query_model_performance(
        &conn,
        &ModelPerformanceQuery {
            current: ModelPerformanceWindow {
                start_time: 100_000_000_000,
                end_time: 200_000_000_000,
            },
            rolling: None,
            model: None,
            provider: None,
        },
    )
    .unwrap();

    let id = resp.identities.first().unwrap();
    // Canonical population keeps the token-less request in the preceding
    // window; throughput eligibility is separate.
    assert_eq!(id.request_counts.preceding, 1);
    assert!(
        id.throughput.preceding.is_none(),
        "no eligible throughput samples -> absent, never a measured zero"
    );
    let cur = id.throughput.current.as_ref().unwrap();
    let t50 = cur.percentiles.iter().find(|p| p.percentile == 50).unwrap();
    assert_eq!(t50.value, 500.0, "50 tokens / 100 ms");
    assert!(
        t50.delta_vs_preceding.is_none(),
        "no baseline sample: no delta at all (distinct from zero baseline)"
    );

    // Duration has a baseline in both windows, so every current percentile
    // carries a delta.
    let cur = id.duration.current.as_ref().unwrap();
    let pre = id.duration.preceding.as_ref().unwrap();
    assert_eq!(pre.percentiles[0].value, 100.0);
    assert!(cur
        .percentiles
        .iter()
        .all(|p| p.delta_vs_preceding.is_some()));
}

#[test]
fn test_model_performance_rejects_invalid_windows() {
    let conn = setup_test_db();
    let bad_current = ModelPerformanceQuery {
        current: ModelPerformanceWindow {
            start_time: 100,
            end_time: 100,
        },
        rolling: None,
        model: None,
        provider: None,
    };
    let err = reader::query_model_performance(&conn, &bad_current).unwrap_err();
    assert!(
        err.to_string().contains("empty or inverted"),
        "empty current window is rejected: {err}"
    );

    let overlapping = ModelPerformanceQuery {
        current: ModelPerformanceWindow {
            start_time: 100_000_000_000,
            end_time: 200_000_000_000,
        },
        rolling: Some(ModelPerformanceWindow {
            start_time: 50_000_000_000,
            end_time: 150_000_000_000,
        }),
        model: None,
        provider: None,
    };
    let err = reader::query_model_performance(&conn, &overlapping).unwrap_err();
    assert!(
        err.to_string().contains("overlaps"),
        "rolling baseline must exclude the comparison windows: {err}"
    );
}

#[test]
fn test_model_performance_duration_only_emitter_is_invisible_to_throughput() {
    let conn = setup_test_db();
    // Codex-style timing-only request spans: one per window (rolling,
    // preceding, current).
    let codex = |id: &str, start_ns: i64| {
        conn.execute(
            r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
               VALUES (?1, ?1, 'run_sampling_request', 0, ?2, ?3,
                       '{"model":"codex-1","otel.scope.name":"codex_cli_rs"}', 0)"#,
            rusqlite::params![id, start_ns, start_ns + 500_000_000],
        )
        .unwrap();
    };
    codex("rr0", -50_000_000_000);
    codex("rp0", 10_000_000_000);
    codex("rc0", 110_000_000_000);

    let resp = reader::query_model_performance(
        &conn,
        &ModelPerformanceQuery {
            current: ModelPerformanceWindow {
                start_time: 100_000_000_000,
                end_time: 200_000_000_000,
            },
            rolling: Some(ModelPerformanceWindow {
                start_time: -100_000_000_000,
                end_time: 0,
            }),
            model: None,
            provider: None,
        },
    )
    .unwrap();
    let id = resp
        .identities
        .iter()
        .find(|i| i.model.as_deref() == Some("codex-1"))
        .unwrap();
    assert_eq!(
        (
            id.request_counts.current,
            id.request_counts.preceding,
            id.request_counts.rolling
        ),
        (1, 1, 1)
    );
    // Duration eligible everywhere; throughput never eligible (no output
    // tokens) — visible, not zero.
    assert!(id.duration.current.is_some());
    assert!(id.throughput.current.is_none());
    assert!(id.throughput.preceding.is_none());
    assert!(id.throughput.rolling.is_none());
    assert!(id.ttft.current.is_none());
    assert!(id.error_rate.current.is_some());
}

#[test]
fn test_model_performance_model_and_provider_filters() {
    let conn = setup_test_db();
    mp_span(
        &conn,
        "a0",
        100_000_000_000,
        100,
        Some("10"),
        Some("0.01"),
        "5",
        0,
    );
    // A second model in the same window.
    conn.execute(
        r#"INSERT INTO spans (trace_id, span_id, name, kind, start_time, end_time, attributes, status_code)
           VALUES ('b', 'b', 'llm.call', 0, 100000000001, 100100000001,
                   '{"gen_ai.system":"anthropic","gen_ai.request.model":"mp-2","gen_ai.usage.input_tokens":"5","gen_ai.usage.output_tokens":"10"}', 0)"#,
        [],
    )
    .unwrap();

    let base = ModelPerformanceQuery {
        current: ModelPerformanceWindow {
            start_time: 100_000_000_000,
            end_time: 200_000_000_000,
        },
        rolling: None,
        model: None,
        provider: None,
    };
    let all = reader::query_model_performance(&conn, &base).unwrap();
    assert_eq!(all.identities.len(), 2);

    let mut filtered = base.clone();
    filtered.model = Some("mp-1".to_string());
    let by_model = reader::query_model_performance(&conn, &filtered).unwrap();
    assert_eq!(by_model.identities.len(), 1);
    assert_eq!(by_model.identities[0].model.as_deref(), Some("mp-1"));

    let mut filtered = base.clone();
    filtered.provider = Some("anthropic".to_string());
    let by_provider = reader::query_model_performance(&conn, &filtered).unwrap();
    assert_eq!(by_provider.identities.len(), 1);
    assert_eq!(
        by_provider.identities[0].provider.as_deref(),
        Some("anthropic")
    );
}

#[test]
fn test_model_performance_empty_windows_are_first_class() {
    let conn = setup_test_db();
    // Spans exist only outside both windows.
    mp_span(
        &conn,
        "x0",
        900_000_000_000,
        100,
        Some("10"),
        Some("0.01"),
        "5",
        0,
    );

    let resp = reader::query_model_performance(
        &conn,
        &ModelPerformanceQuery {
            current: ModelPerformanceWindow {
                start_time: 100_000_000_000,
                end_time: 200_000_000_000,
            },
            rolling: None,
            model: None,
            provider: None,
        },
    )
    .unwrap();
    assert!(
        resp.identities.is_empty(),
        "no population -> no identities, not an error"
    );
    assert!(!resp.truncated);
}
