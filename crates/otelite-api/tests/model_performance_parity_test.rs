//! Model-performance parity test (#121/#153): the API diagnosis envelope
//! must match the frozen fixture `model_performance_parity_v1.json`, and
//! range/model selection must be honoured.
//!
//! Regenerating the goldens (after an intentional change): run with
//! `OTELITE_MP_PARITY_CAPTURE=1 cargo test -p otelite-api
//! --test model_performance_parity_test`, review the fixture diff, and
//! commit it together with the change.

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
        "/tests/fixtures/model_performance_parity_v1.json"
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
async fn model_performance_matches_frozen_fixture() {
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
        "model-performance diagnosis drifted from frozen fixture v1"
    );
}

#[tokio::test]
async fn model_performance_empty_window_is_first_class() {
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
async fn model_performance_model_selection() {
    let (storage, _temp) = build_storage().await;
    let server = DashboardServer::new(DashboardConfig::default(), storage);
    let app = server.build_router();

    let args = fixture()["api_args"].clone();
    let uri = format!(
        "/api/genai/model-performance?start_time={}&end_time={}&rolling_ns={}&model=o2",
        args["start_time"], args["end_time"], args["rolling_ns"]
    );
    let got = get_json(&app, &uri).await;
    let ids = got["identities"].as_array().unwrap();
    assert_eq!(
        ids.len(),
        1,
        "model filter must select exactly one identity"
    );
    assert_eq!(ids[0]["model"], "o2");
    let assessments = got["assessments"].as_array().unwrap();
    assert_eq!(assessments.len(), 1);
    assert_eq!(assessments[0]["model"], "o2");
    assert_eq!(assessments[0]["overall_class"], "error_associated");
}

#[tokio::test]
async fn model_performance_provider_selection() {
    let (storage, _temp) = build_storage().await;
    let server = DashboardServer::new(DashboardConfig::default(), storage);
    let app = server.build_router();

    let args = fixture()["api_args"].clone();
    let uri = format!(
        "/api/genai/model-performance?start_time={}&end_time={}&provider=anthropic",
        args["start_time"], args["end_time"]
    );
    let got = get_json(&app, &uri).await;
    let ids = got["identities"].as_array().unwrap();
    assert_eq!(ids.len(), 1);
    assert_eq!(ids[0]["provider"], "anthropic");
}

#[tokio::test]
async fn model_performance_rejects_invalid_interval() {
    let (storage, _temp) = build_storage().await;
    let server = DashboardServer::new(DashboardConfig::default(), storage);
    let app = server.build_router();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/genai/model-performance?start_time=200&end_time=100")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/genai/model-performance?start_time=100&end_time=100&rolling_ns=-5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
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

    // Six identities, none merged: providers, models and rerouted response
    // models stay separate.
    let ids = api["identities"].as_array().unwrap();
    assert_eq!(ids.len(), 6);
    let find = |model: &str| -> &serde_json::Value {
        ids.iter()
            .find(|i| i["model"] == model)
            .unwrap_or_else(|| panic!("missing identity {model}"))
    };
    let find_assessment = |model: &str| -> &serde_json::Value {
        api["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["model"] == model)
            .unwrap_or_else(|| panic!("missing assessment {model}"))
    };

    // A: typical regression on duration; throughput regressed in kind.
    let a = find_assessment("gpt-4o");
    assert_eq!(a["overall_class"], "typical_regression");
    let duration = &a["metrics"][0];
    assert_eq!(duration["class"], "typical_regression");
    assert_eq!(duration["eligible_current"], 12);
    assert_eq!(duration["current_median"], 1300.0);
    assert_eq!(duration["preceding_median"], 1000.0);
    assert_eq!(duration["rolling_median"], 1000.0);
    let d = &duration["median_delta_vs_preceding"];
    assert_eq!(d["absolute"], 300.0);
    assert!((d["relative"].as_f64().unwrap() - 0.3).abs() < 1e-9);

    // B: low sample — insufficient telemetry, count reported.
    let b = find_assessment("claude-sonnet-4-5");
    assert_eq!(b["overall_class"], "insufficient_telemetry");
    assert_eq!(b["metrics"][0]["eligible_current"], 5);
    assert!(b["metrics"][0]["notes"][0]
        .as_str()
        .unwrap()
        .contains("10 minimum"));

    // C: workload-correlated; the correlation evidence carries the token
    // deltas and the non-causal relationship label.
    let c = find_assessment("o1");
    assert_eq!(c["overall_class"], "workload_shift_correlated");
    let c_dur = &c["metrics"][0];
    assert_eq!(c_dur["class"], "workload_shift_correlated");
    let shift = &c_dur["workload_shift"];
    assert_eq!(shift["relationship"], "correlation (not causation)");
    assert_eq!(shift["material"], true);
    assert!(shift["output_tokens"]["relative"].as_f64().unwrap() >= 0.99);

    // D: error-associated; the zero baseline makes the relative state
    // percentage-unavailable (null), never a fabricated 0%.
    let d = find_assessment("o2");
    assert_eq!(d["overall_class"], "error_associated");
    let d_err = &d["metrics"][3];
    assert_eq!(d_err["class"], "error_associated");
    let d_dur = &d["metrics"][0];
    assert_eq!(d_dur["class"], "error_associated");
    assert!(d_dur["error_association"]["relative"].is_null());
    assert!((d_dur["error_association"]["absolute"].as_f64().unwrap() - 4.0 / 12.0).abs() < 1e-9);

    // E: mixed evidence — both baseline deltas survive JSON.
    let e = find_assessment("o3");
    assert_eq!(e["overall_class"], "mixed_evidence");
    let e_dur = &e["metrics"][0];
    assert_eq!(e_dur["class"], "mixed_evidence");
    assert!(e_dur["median_delta_vs_preceding"].is_object());
    assert!(e_dur["median_delta_vs_rolling"].is_object());
    assert!(e_dur["notes"][0]
        .as_str()
        .unwrap()
        .contains("mixed evidence"));

    // F: flat duration, but untrusted (degenerate) TTFT prevents
    // attribution — the note says so explicitly.
    let f = find_assessment("stable-1");
    assert_eq!(f["overall_class"], "no_material_change");
    let f_ttft = &f["metrics"][2];
    assert_eq!(f_ttft["class"], "insufficient_telemetry");
    assert!(f_ttft["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n.as_str().unwrap().contains("attribution is prevented")));

    // The raw identity for D keeps the error counts — every reported
    // baseline and count survives JSON.
    assert_eq!(find("o2")["error_rate"]["current"]["errors"], 4);
    assert_eq!(find("o2")["error_rate"]["current"]["requests"], 12);
}
