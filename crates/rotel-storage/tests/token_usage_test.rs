//! Tests for token usage query functionality

use rotel_storage::sqlite::{reader, schema};
use rusqlite::Connection;

fn setup_test_db() -> Connection {
    let conn = Connection::open_in_memory().unwrap();
    schema::initialize_schema(&conn).unwrap();
    conn
}

#[test]
fn test_query_token_usage_empty() {
    let conn = setup_test_db();
    let (summary, by_model, by_system) = reader::query_token_usage(&conn, None, None).unwrap();

    assert_eq!(summary.total_input_tokens, 0);
    assert_eq!(summary.total_output_tokens, 0);
    assert_eq!(summary.total_requests, 0);
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

    let (summary, by_model, by_system) = reader::query_token_usage(&conn, None, None).unwrap();

    // Check summary
    assert_eq!(summary.total_input_tokens, 4500); // 1000 + 2000 + 1500
    assert_eq!(summary.total_output_tokens, 1900); // 500 + 800 + 600
    assert_eq!(summary.total_requests, 3);

    // Check by_model (sorted by total tokens desc)
    assert_eq!(by_model.len(), 2);
    assert_eq!(by_model[0].model, "gpt-4");
    assert_eq!(by_model[0].input_tokens, 2500); // 1000 + 1500
    assert_eq!(by_model[0].output_tokens, 1100); // 500 + 600
    assert_eq!(by_model[0].requests, 2);

    assert_eq!(by_model[1].model, "claude-sonnet-4");
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
    let (summary, by_model, _) = reader::query_token_usage(&conn, Some(0), Some(3000)).unwrap();

    assert_eq!(summary.total_input_tokens, 1000);
    assert_eq!(summary.total_output_tokens, 500);
    assert_eq!(summary.total_requests, 1);
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_model[0].model, "gpt-4");
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

    let (summary, by_model, by_system) = reader::query_token_usage(&conn, None, None).unwrap();

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

    let (summary, by_model, _by_system) = reader::query_token_usage(&conn, None, None).unwrap();

    // Should handle missing fields gracefully (COALESCE to 0)
    assert_eq!(summary.total_input_tokens, 0);
    assert_eq!(summary.total_output_tokens, 0);
    assert_eq!(summary.total_requests, 1);
    assert_eq!(by_model.len(), 1);
    assert_eq!(by_model[0].model, "gpt-4");
    assert_eq!(by_model[0].input_tokens, 0);
    assert_eq!(by_model[0].output_tokens, 0);
}
