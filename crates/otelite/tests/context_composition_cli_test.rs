//! End-to-end test for `otelite usage --context-composition` (#113,
//! option B): seed a temp database with cache-bearing LLM spans, run the
//! compiled binary against it, and deep-check the emitted JSON.

use assert_cmd::Command;
use otelite_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode as SpanStatusCode};
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::{StorageBackend, StorageConfig};
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_ns() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as i64
}

fn llm_span(session: Option<&str>, cache_read: Option<i64>, offset_ns: i64) -> Span {
    let start = now_ns() - 3_600_000_000_000 + offset_ns; // 1h ago + offset
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("gen_ai.request.model".into(), "test-model".into());
    attributes.insert("gen_ai.usage.input_tokens".into(), "10".into());
    attributes.insert("gen_ai.usage.output_tokens".into(), "5".into());
    if let Some(s) = session {
        attributes.insert("session.id".into(), s.into());
    }
    if let Some(c) = cache_read {
        attributes.insert("gen_ai.usage.cache_read.input_tokens".into(), c.to_string());
    }
    Span {
        resource: None,
        trace_id: "t".into(),
        span_id: format!("sp-{}", start),
        parent_span_id: None,
        name: "claude_code.llm_request".into(),
        kind: SpanKind::Internal,
        start_time: start,
        end_time: start + 1_000_000_000,
        attributes,
        events: vec![],
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
    }
}

async fn build_db(data_dir: &std::path::Path) {
    let storage_config = StorageConfig::default().with_data_dir(data_dir.to_path_buf());
    let mut storage = SqliteBackend::new(storage_config);
    storage.initialize().await.unwrap();
    // Session A: fixed prefix 100, grows to 120. Session B: flat 50.
    // Session C: LLM activity without cache telemetry (excluded).
    for span in [
        llm_span(Some("sess-a"), Some(100), 0),
        llm_span(Some("sess-a"), Some(120), 60_000_000_000),
        llm_span(Some("sess-a"), Some(110), 120_000_000_000),
        llm_span(Some("sess-b"), Some(50), 0),
        llm_span(Some("sess-c"), None, 0),
        llm_span(Some("sess-c"), None, 60_000_000_000),
    ] {
        storage.write_span(&span).await.unwrap();
    }
}

#[tokio::test]
async fn usage_context_composition_json() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    build_db(&data_dir).await;

    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let output = Command::cargo_bin("otelite")
        .expect("otelite binary should build")
        .args([
            "--format",
            "json",
            "usage",
            "--context-composition",
            "--since",
            "24h",
        ])
        .env("OTELITE_DATA_DIR", &data_dir)
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let actual: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("CLI stdout must be JSON");

    let rows = actual
        .get("context_composition")
        .and_then(serde_json::Value::as_array)
        .expect("context_composition array in JSON output");
    assert_eq!(rows.len(), 2, "only cache-bearing sessions: {rows:?}");

    let a = rows
        .iter()
        .find(|r| r["session_id"] == "sess-a")
        .expect("sess-a present");
    assert_eq!(a["request_count"], 3);
    assert_eq!(a["cached_requests"], 3);
    assert_eq!(a["fixed_prefix"], 100);
    assert_eq!(a["peak_context"], 120);
    assert_eq!(a["growth"], 20);

    let b = rows
        .iter()
        .find(|r| r["session_id"] == "sess-b")
        .expect("sess-b present");
    assert_eq!(b["fixed_prefix"], 50);
    assert_eq!(b["peak_context"], 50);
    assert_eq!(b["growth"], 0);

    assert!(
        !rows.iter().any(|r| r["session_id"] == "sess-c"),
        "session without cache telemetry must be excluded"
    );
}

#[tokio::test]
async fn usage_context_composition_pretty() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    build_db(&data_dir).await;

    let home = temp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();

    let output = Command::cargo_bin("otelite")
        .expect("otelite binary should build")
        .args(["usage", "--context-composition", "--since", "24h"])
        .env("OTELITE_DATA_DIR", &data_dir)
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "CLI failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Context Composition"), "{stdout}");
    assert!(stdout.contains("sess-a"), "top session row: {stdout}");
    assert!(stdout.contains("sess-b"), "{stdout}");
    assert!(
        !stdout.contains("sess-c"),
        "excluded session leaked: {stdout}"
    );
    // The derived numbers must be visible in the table.
    assert!(stdout.contains("100"), "fixed prefix: {stdout}");
    assert!(stdout.contains("120"), "peak context: {stdout}");
}
