//! Versioned parity fixture for the throughput/latency analytics family
//! (issue #119 slice #144).
//!
//! The fixture (`fixtures/throughput_parity_v1.json`, `version: 1`) freezes:
//! - the span set (populated / low-sample / cached / missing-output /
//!   buffered-TTFT / rerouted cohorts over a 2-day UTC window),
//! - the exact API JSON for the five endpoints the web renders,
//! - the exact CLI JSON for the matching `otelite usage` flag set.
//!
//! The API and CLI projections must agree on every field (modulo the
//! network-dependent pricing paths listed in `normalization`). This test
//! re-derives the API side from the spans and deep-compares it against the
//! frozen JSON.
//!
//! To refresh the fixture after an intentional API change: bump `version`,
//! regenerate the expected JSON (see the test doc in #144), and update the
//! hand-checked spot assertions below so the new values are verified, not
//! just recorded.

use axum::body::Body;
use axum::http::Request;
use otelite_api::pricing_cache::PricingCache;
use otelite_api::{DashboardConfig, DashboardServer};
use otelite_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode as SpanStatusCode};
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::{StorageBackend, StorageConfig};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use tower::ServiceExt;

#[derive(Deserialize)]
struct Fixture {
    version: u32,
    window: Window,
    spans: Vec<FixtureSpan>,
    api: serde_json::Value,
    api_empty: serde_json::Value,
}

#[derive(Deserialize)]
struct Window {
    start_ns: i64,
    end_ns: i64,
}

#[derive(Deserialize)]
struct FixtureSpan {
    span_id: String,
    model: String,
    system: String,
    hour: i64,
    #[serde(default)]
    offset_s: i64,
    duration_ms: i64,
    input: i64,
    #[serde(default)]
    output: Option<i64>,
    #[serde(default)]
    ttft_ms: Option<i64>,
    #[serde(default)]
    cache_read: Option<i64>,
    #[serde(default)]
    cache_creation: Option<i64>,
    #[serde(default)]
    response_model: Option<String>,
}

fn span_from_fixture(s: &FixtureSpan, window: &Window) -> Span {
    let mut attributes: HashMap<String, String> = HashMap::new();
    attributes.insert("model".into(), s.model.clone());
    attributes.insert("gen_ai.request.model".into(), s.model.clone());
    attributes.insert("gen_ai.system".into(), s.system.clone());
    attributes.insert("gen_ai.usage.input_tokens".into(), s.input.to_string());
    if let Some(o) = s.output {
        attributes.insert("gen_ai.usage.output_tokens".into(), o.to_string());
    }
    if let Some(t) = s.ttft_ms {
        attributes.insert("ttft_ms".into(), t.to_string());
    }
    if let Some(c) = s.cache_read {
        attributes.insert("gen_ai.usage.cache_read.input_tokens".into(), c.to_string());
    }
    if let Some(c) = s.cache_creation {
        attributes.insert(
            "gen_ai.usage.cache_creation.input_tokens".into(),
            c.to_string(),
        );
    }
    if let Some(rm) = &s.response_model {
        attributes.insert("gen_ai.response.model".into(), rm.clone());
    }
    let start = window.start_ns + (s.hour * 3600 + s.offset_s) * 1_000_000_000;
    Span {
        resource: None,
        trace_id: "t".into(),
        span_id: s.span_id.clone(),
        parent_span_id: None,
        name: "claude_code.llm_request".into(),
        kind: SpanKind::Internal,
        start_time: start,
        end_time: start + s.duration_ms * 1_000_000,
        attributes,
        events: vec![],
        status: SpanStatus {
            code: SpanStatusCode::Ok,
            message: None,
        },
    }
}

async fn get_json(app: &axum::Router, uri: &str) -> serde_json::Value {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&body).unwrap_or_else(|e| panic!("parse {uri}: {e} (status {status})"))
}

#[tokio::test]
async fn api_matches_parity_fixture() {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/throughput_parity_v1.json"
    ))
    .unwrap();
    let fixture: Fixture = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        fixture.version, 1,
        "bump the fixture version and re-freeze expected JSON"
    );

    let temp_dir = tempfile::TempDir::new().unwrap();
    let storage_config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
    let mut storage = SqliteBackend::new(storage_config);
    storage.initialize().await.unwrap();
    for s in &fixture.spans {
        storage
            .write_span(&span_from_fixture(s, &fixture.window))
            .await
            .unwrap();
    }
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);
    let server = DashboardServer::with_pricing_cache(
        DashboardConfig::default(),
        storage,
        PricingCache::new(),
    );
    let app = server.build_router();

    let start = fixture.window.start_ns;
    let end = fixture.window.end_ns;
    let q = |extra: &str| format!("/api/genai/{extra}?start_time={start}&end_time={end}");

    // Deep-compare each frozen endpoint response.
    let actual = serde_json::json!({
        "token_usage": get_json(&app, &q("usage")).await,
        "latency_stats": get_json(&app, &q("latency_stats")).await,
        "latency_percentiles_rolling": get_json(&app, &(q("latency_percentiles") + "&bucket_secs=3600&metrics=duration,ttft")).await,
        "latency_percentiles_calendar": get_json(&app, &(q("latency_percentiles") + "&bucket_secs=3600&metrics=duration&calendar_day=1&timezone=UTC")).await,
        "latency_series": get_json(&app, &(q("latency_series") + "&bucket_secs=3600")).await,
    });
    assert_eq!(
        actual, fixture.api,
        "API JSON drifted from the v1 parity fixture — regenerate it only after a deliberate change"
    );

    // Empty window states (no calls at all in the queried range).
    let empty_actual = serde_json::json!({
        "token_usage": get_json(&app, "/api/genai/usage?start_time=100&end_time=200").await,
        "latency_stats": get_json(&app, "/api/genai/latency_stats?start_time=100&end_time=200").await,
        "latency_percentiles_calendar": get_json(&app, "/api/genai/latency_percentiles?start_time=1787702400000000000&end_time=1787788800000000000&bucket_secs=3600&metrics=duration&calendar_day=1&timezone=UTC").await,
        "latency_series": get_json(&app, "/api/genai/latency_series?start_time=100&end_time=200&bucket_secs=3600").await,
    });
    assert_eq!(
        empty_actual, fixture.api_empty,
        "empty-window API JSON drifted from the v1 parity fixture"
    );

    // Hand-checked semantic guards — these must stay true even if the
    // fixture is ever regenerated wholesale.
    let api = &fixture.api;
    let summary = &api["token_usage"]["summary"];
    assert_eq!(summary["total_requests"], 37);
    // Rerouting: exactly one pop-model call reported a different response model.
    let pop = api["token_usage"]["by_model"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["model"] == "pop-sys/pop-model")
        .unwrap();
    assert_eq!(pop["rerouted_count"], 1);
    assert_eq!(pop["response_model"], "pop-alias");
    // Cache cohort totals.
    assert_eq!(summary["total_cache_read_tokens"], 150);
    assert_eq!(summary["total_cache_creation_tokens"], 30);
    // Throughput-ineligible model: calls present, no eligible samples.
    let no_out = api["latency_stats"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["model"] == "pop-sys/no-out-model")
        .unwrap();
    assert_eq!(no_out["throughput_sample_count"], 0);
    // Buffered TTFT: 12/12 degenerate meets the n>=10 & >=90% threshold.
    let ttft = api["latency_stats"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["model"] == "pop-sys/ttft-model")
        .unwrap();
    assert_eq!(ttft["ttft_degenerate"], true);
    assert_eq!(ttft["ttft_degenerate_count"], 12);
    // Calendar grid contains empty (count=0) days for single-day cohorts.
    let calendar = &api["latency_percentiles_calendar"]["metrics"]["duration"]["models"];
    let low = &calendar["pop-sys/low-model"];
    assert_eq!(low[0]["count"], 0, "D0 must be an explicit empty grid day");
    assert_eq!(low[1]["count"], 7);
}
