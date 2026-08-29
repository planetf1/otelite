//! Model-performance edge-matrix test (#121/#155): the frozen fixture
//! `model_performance_edges_v1.json` covers the assessment paths the
//! v1 parity fixture does not — tail-only regression, overlapping model
//! names across providers, the same provider+model split by emitter
//! fingerprint, and a daylight-saving range boundary.
//!
//! Regenerating the goldens (after an intentional change): run with
//! `OTELITE_MP_PARITY_CAPTURE=1 cargo test -p otelite-api
//! --test model_performance_edges_test`, review the fixture diff, and
//! commit it together with the change.

use axum::body::Body;
use axum::http::Request;
use otelite_api::{DashboardConfig, DashboardServer};
use otelite_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode as SpanStatusCode};
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::{StorageBackend, StorageConfig};
use std::sync::Arc;
use tower::ServiceExt;

const W0: i64 = 1_774_656_000_000_000_000;

#[derive(serde::Deserialize)]
struct FixtureSpan {
    trace: String,
    id: String,
    name: String,
    #[serde(default)]
    parent: Option<String>,
    #[serde(default)]
    hour: Option<i64>,
    #[serde(default)]
    min: Option<i64>,
    #[serde(default)]
    sec: Option<i64>,
    dur_ms: i64,
    #[serde(default)]
    status: i32,
    attrs: std::collections::HashMap<String, String>,
}

fn fixture_path() -> String {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/model_performance_edges_v1.json"
    )
    .to_string()
}

fn fixture() -> serde_json::Value {
    let raw = std::fs::read_to_string(fixture_path()).unwrap();
    serde_json::from_str(&raw).unwrap()
}

fn capture() -> bool {
    std::env::var("OTELITE_MP_PARITY_CAPTURE").is_ok()
}

/// Freeze `section` into the fixture file (capture mode only).
fn freeze(section: &str, value: serde_json::Value) {
    let mut fx = fixture();
    fx[section] = value;
    let mut out = serde_json::to_string_pretty(&fx).unwrap();
    out.push('\n');
    std::fs::write(fixture_path(), out).unwrap();
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
        parent_span_id: f.parent.clone(),
        name: f.name.clone(),
        kind: SpanKind::Internal,
        start_time: W0 + off_ns,
        end_time: W0 + off_ns + f.dur_ms * 1_000_000,
        attributes: f.attrs.clone(),
        events: vec![],
        status: SpanStatus {
            code: SpanStatusCode::from_i32(f.status).unwrap_or(SpanStatusCode::Unset),
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
async fn edges_match_frozen_fixture() {
    let (storage, _temp) = build_storage().await;
    let server = DashboardServer::new(DashboardConfig::default(), storage);
    let app = server.build_router();

    let args = fixture()["api_args"].clone();
    // The '/' in the IANA zone must be percent-encoded for a valid URI.
    let timezone = args["timezone"].as_str().unwrap().replace('/', "%2F");
    let uri = format!(
        "/api/genai/model-performance?start_time={}&end_time={}&rolling_ns={}&timezone={timezone}",
        args["start_time"], args["end_time"], args["rolling_ns"]
    );
    let got = get_json(&app, &uri).await;
    if capture() {
        freeze("api", got.clone());
    }
    let expected = fixture()["api"].clone();
    assert_eq!(
        got, expected,
        "edge-matrix diagnosis drifted from frozen fixture v1"
    );
}

#[tokio::test]
async fn edges_empty_window_is_first_class() {
    let (storage, _temp) = build_storage().await;
    let server = DashboardServer::new(DashboardConfig::default(), storage);
    let app = server.build_router();

    let args = fixture()["api_empty_args"].clone();
    let uri = format!(
        "/api/genai/model-performance?start_time={}&end_time={}",
        args["start_time"], args["end_time"]
    );
    let got = get_json(&app, &uri).await;
    if capture() {
        freeze("api_empty", got.clone());
    }
    let expected = fixture()["api_empty"].clone();
    assert_eq!(got, expected);
    assert_eq!(
        got["identities"],
        serde_json::json!([]),
        "no population -> no identities"
    );
    assert_eq!(got["assessments"], serde_json::json!([]));
}

#[tokio::test]
async fn fixture_semantic_guards() {
    // In capture mode the other tests rewrite this fixture; asserting on a
    // mid-rewrite file is a race, so the guards run only against a frozen
    // fixture.
    if capture() {
        return;
    }
    let fx = fixture();
    let api = &fx["api"];
    let h = 3_600_000_000_000_i64;
    let d = 86_400_000_000_000_i64;

    // Five identities: the two `shared-model` identities (openai vs
    // anthropic) and the two `split-emit` identities (emitter-x vs
    // emitter-y) must not be merged.
    let ids = api["identities"].as_array().unwrap();
    assert_eq!(ids.len(), 5, "identities: {:?}", ids);
    let shared: Vec<&serde_json::Value> = ids
        .iter()
        .filter(|i| i["model"] == "shared-model")
        .collect();
    assert_eq!(
        shared.len(),
        2,
        "same model name, two providers, must not merge"
    );
    let providers: Vec<&str> = shared
        .iter()
        .map(|i| i["provider"].as_str().unwrap())
        .collect();
    assert_eq!(providers, vec!["anthropic", "openai"]);
    let fingerprints: Vec<&str> = shared
        .iter()
        .map(|i| i["emitter_fingerprint"].as_str().unwrap())
        .collect();
    assert_ne!(
        fingerprints[0], fingerprints[1],
        "different emitters -> different fingerprints"
    );
    let split: Vec<&serde_json::Value> =
        ids.iter().filter(|i| i["model"] == "split-emit").collect();
    assert_eq!(
        split.len(),
        2,
        "same provider+model, two emitters, must not merge"
    );
    assert_eq!(
        split[0]["provider"], split[1]["provider"],
        "both split-emit identities share the provider"
    );
    assert_ne!(
        split[0]["emitter_fingerprint"], split[1]["emitter_fingerprint"],
        "the emitter fingerprint is part of the identity key"
    );

    let find_assessment = |model: &str| -> &serde_json::Value {
        api["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["model"] == model)
            .unwrap_or_else(|| panic!("missing assessment {model}"))
    };

    // Tail-only regression: the median did not move, the p95 tail did.
    let tail = find_assessment("tail-x");
    assert_eq!(tail["overall_class"], "tail_regression");
    let dur = &tail["metrics"][0];
    assert_eq!(dur["class"], "tail_regression");
    assert_eq!(dur["current_median"], 1000.0);
    assert_eq!(dur["preceding_median"], 1000.0);
    let md = &dur["median_delta_vs_preceding"];
    assert_eq!(md["absolute"], 0.0);
    let td = &dur["tail_delta_vs_preceding"];
    assert_eq!(td["absolute"], 1500.0);
    assert!(td["relative"].as_f64().unwrap() >= 1.49);
    assert_eq!(dur["current_tail"], 2500.0);
    assert_eq!(dur["preceding_tail"], 1000.0);

    // Flat identities: no material change on duration/throughput/errors,
    // TTFT is absent -> first-class insufficient with the zero count.
    for model in ["shared-model", "split-emit"] {
        let a = find_assessment(model);
        assert_eq!(a["overall_class"], "no_material_change");
        let flat = &a["metrics"][0];
        assert_eq!(flat["class"], "no_material_change");
        let ttft = &a["metrics"][2];
        assert_eq!(ttft["class"], "insufficient_telemetry");
        assert_eq!(ttft["eligible_current"], 0);
        assert!(ttft["notes"][0].as_str().unwrap().contains("10 minimum"));
        // No rolling spans exist: the rolling baseline is reported as
        // absent, never fabricated.
        assert!(flat["rolling_median"].is_null());
        assert!(flat["median_delta_vs_rolling"].is_null());
    }

    // Daylight-saving boundary: the window is an exact UTC interval that
    // straddles the transition; the timezone is echoed, not applied.
    let dst = 1_774_746_000_000_000_000_i64; // 2026-03-29T01:00:00Z (UK spring-forward)
    let cur = &api["current_window"];
    assert_eq!(cur["start_time"], W0);
    assert_eq!(cur["end_time"], W0 + 2 * 24 * h);
    assert!(
        W0 < dst && dst < W0 + 2 * 24 * h,
        "transition must sit inside the window"
    );
    assert_eq!(api["timezone"], "Europe/London");
    // The current window is 48h, so the derived preceding is 48h long and
    // the rolling baseline sits before it.
    let prev = &api["preceding_window"];
    assert_eq!(prev["start_time"], W0 - 2 * 24 * h);
    assert_eq!(prev["end_time"], W0);
    let roll = &api["rolling_window"];
    assert_eq!(roll["start_time"], W0 - 2 * 24 * h - 6 * d);
    assert_eq!(roll["end_time"], W0 - 2 * 24 * h);
    assert_eq!(api["truncated"], false);

    // API and CLI goldens must deep-equal (captured independently).
    assert_eq!(
        fx["api"], fx["cli"],
        "API and CLI edge goldens must deep-equal"
    );
    assert_eq!(fx["api_empty"], fx["cli_empty"]);
}
