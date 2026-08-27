//! Capability parity test (#120): the API capability report must match the
//! frozen fixture `capability_parity_v1.json` exactly, and the CLI's
//! json-compact output must deep-equal it (asserted in the CLI test).

use axum::body::Body;
use axum::http::Request;
use otelite_api::{DashboardConfig, DashboardServer};
use otelite_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode as SpanStatusCode};
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::{StorageBackend, StorageConfig};
use std::sync::Arc;
use tower::ServiceExt;

const W0: i64 = 1_787_529_600_000_000_000;

#[derive(serde::Deserialize)]
struct FixtureSpan {
    trace: String,
    id: String,
    name: String,
    #[serde(default)]
    hour: Option<i64>,
    #[serde(default)]
    min: Option<i64>,
    #[serde(default)]
    sec: Option<i64>,
    dur_ms: i64,
    attrs: std::collections::HashMap<String, String>,
}

fn fixture() -> serde_json::Value {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/capability_parity_v1.json"
    ))
    .unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn span_from(f: &FixtureSpan) -> Span {
    let off_ns = match (f.hour, f.min, f.sec) {
        (Some(h), _, _) => h * 3_600_000_000_000,
        (None, Some(m), _) => m * 60_000_000_000,
        (None, None, Some(x)) => x * 1_000_000_000,
        _ => 0,
    };
    Span {
        resource: None,
        trace_id: f.trace.clone(),
        span_id: f.id.clone(),
        parent_span_id: None,
        name: f.name.clone(),
        kind: SpanKind::Internal,
        start_time: W0 + off_ns,
        end_time: W0 + off_ns + f.dur_ms * 1_000_000,
        attributes: f.attrs.clone(),
        events: vec![],
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
    }
}

async fn build_storage() -> (Arc<dyn StorageBackend>, tempfile::TempDir) {
    let temp_dir = tempfile::TempDir::new().unwrap();
    let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut storage = SqliteBackend::new(config);
    storage.initialize().await.unwrap();
    let fx = fixture();
    for value in fx["spans"].as_array().unwrap() {
        let f: FixtureSpan = serde_json::from_value(value.clone()).unwrap();
        storage.write_span(&span_from(&f)).await.unwrap();
    }
    (Arc::new(storage), temp_dir)
}

async fn get_json(app: &axum::Router, uri: &str) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::OK);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap()
}

#[tokio::test]
async fn capability_report_matches_frozen_fixture() {
    let (storage, _temp) = build_storage().await;
    let server = DashboardServer::new(DashboardConfig::default(), storage);
    let app = server.build_router();

    let got = get_json(
        &app,
        "/api/genai/capabilities?start_time=1787529600000000000&end_time=1787702400000000000",
    )
    .await;
    let expected = fixture()["api"].clone();
    assert_eq!(
        got, expected,
        "capability report drifted from frozen fixture v1"
    );
}

#[tokio::test]
async fn capability_report_empty_window() {
    let (storage, _temp) = build_storage().await;
    let server = DashboardServer::new(DashboardConfig::default(), storage);
    let app = server.build_router();

    let got = get_json(&app, "/api/genai/capabilities?start_time=100&end_time=200").await;
    let expected = fixture()["api_empty"].clone();
    assert_eq!(got, expected);
}

#[tokio::test]
async fn capability_fixture_semantic_guards() {
    let fx = fixture();
    let api = &fx["api"];

    // Duplicate OTLP delivery of (d0,s0) collapsed to one canonical span.
    assert_eq!(api["canonical_span_count"], 28);
    assert_eq!(api["duplicate_span_count"], 1);
    assert_eq!(api["truncated"], false);

    let reports: Vec<&serde_json::Value> = api["reports"].as_array().unwrap().iter().collect();
    assert_eq!(reports.len(), 6);

    let find = |emitter: &str, model: &str| -> &serde_json::Value {
        reports
            .iter()
            .find(|r| r["emitter"] == emitter && r["model"].as_str() == Some(model))
            .unwrap_or_else(|| panic!("missing report {emitter}/{model}"))
    };

    // Full native telemetry.
    let gpt4o = find("standard_otel", "gpt-4o");
    assert_eq!(gpt4o["provider"], "openai");
    assert_eq!(gpt4o["request_count"], 5);
    assert_eq!(gpt4o["input_tokens"]["availability"], "available");
    assert_eq!(gpt4o["input_tokens"]["quality"], "reliable");
    assert_eq!(gpt4o["input_tokens"]["derivation"], "native");
    assert_eq!(gpt4o["ttft"]["availability"], "available");
    assert_eq!(gpt4o["ttft"]["quality"], "reliable");
    assert_eq!(gpt4o["cache_creation_tokens"]["availability"], "absent");
    assert_eq!(gpt4o["cache_creation_tokens"]["derivation"], "unavailable");

    // Sparse output + invalid TTFT: vocabulary must stay distinct.
    let mini = find("standard_otel", "gpt-4o-mini");
    assert_eq!(mini["output_tokens"]["availability"], "sparse");
    assert_eq!(mini["output_tokens"]["valid_count"], 2);
    assert_eq!(mini["ttft"]["availability"], "sparse");
    assert_eq!(mini["ttft"]["quality"], "invalid");
    assert_eq!(mini["ttft"]["invalid_count"], 2);
    assert_eq!(mini["ttft"]["observed_count"], 3);

    // Degenerate TTFT group.
    let sonnet = find("standard_otel", "claude-sonnet-4-5");
    assert_eq!(sonnet["ttft"]["availability"], "available");
    assert_eq!(sonnet["ttft"]["quality"], "degenerate");
    assert_eq!(sonnet["ttft"]["valid_count"], 12);

    // Claude Code flat keys; provider None (no gen_ai.system).
    let claude = find("claude_code", "claude-opus-4-6");
    assert_eq!(claude["provider"], serde_json::Value::Null);
    assert_eq!(
        claude["request_count"], 3,
        "duplicate must not inflate the count"
    );
    assert_eq!(
        claude["input_tokens"]["source_attributes"]["prompt_tokens"],
        3
    );
    assert_eq!(
        claude["output_tokens"]["source_attributes"]["completion_tokens"],
        3
    );

    // OpenCode zero-input tokens: zero is a valid observation.
    let opencode = find("opencode", "gpt-5");
    assert_eq!(opencode["input_tokens"]["valid_count"], 2);
    assert_eq!(opencode["input_tokens"]["availability"], "available");

    // Codex timing-only: nothing token-related is assessed as native.
    let codex = find("codex", "gpt-5");
    for metric in [
        "input_tokens",
        "output_tokens",
        "cache_creation_tokens",
        "cache_read_tokens",
        "ttft",
    ] {
        assert_eq!(codex[metric]["derivation"], "unavailable", "{metric}");
        assert_eq!(codex[metric]["availability"], "absent", "{metric}");
    }
    assert_eq!(codex["correlation"]["rule"], "none");

    // Unidentified span contributes nothing.
    assert!(
        !reports
            .iter()
            .any(|r| r["emitter"] == "unknown" || r["adapter_rule"] == "unidentified-v1"),
        "unidentified span must not be reported"
    );

    // Correlation provenance is all-none until a join rule ships.
    for r in &reports {
        assert_eq!(r["correlation"]["rule"], "none");
        assert_eq!(r["correlation"]["matched_count"], 0);
    }

    let empty = &fx["api_empty"];
    assert_eq!(empty["reports"].as_array().unwrap().len(), 0);
    assert_eq!(empty["canonical_span_count"], 0);
}
