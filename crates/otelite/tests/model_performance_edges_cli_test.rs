//! CLI model-performance edge-matrix test (#121/#155): `otelite
//! model-performance --format json-compact` must deep-equal the frozen
//! edge fixture, which was captured from the API, and the pretty output
//! must render the exact UTC intervals of the daylight-saving range
//! with the timezone echoed.
//!
//! Regenerating the CLI golden: `OTELITE_MP_PARITY_CAPTURE=1 cargo test
//! -p otelite --test model_performance_edges_cli_test`.

use otelite_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode as SpanStatusCode};
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::{StorageBackend, StorageConfig};

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
        "/../otelite-api/tests/fixtures/model_performance_edges_v1.json"
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

fn freeze(section: &str, value: serde_json::Value) {
    let mut fx = fixture();
    fx[section] = value;
    let mut out = serde_json::to_string_pretty(&fx).unwrap();
    out.push('\n');
    std::fs::write(fixture_path(), out).unwrap();
}

async fn build_fixture_db(dir: &std::path::Path) {
    let config = StorageConfig::default().with_data_dir(dir.to_path_buf());
    let mut storage = SqliteBackend::new(config);
    storage.initialize().await.unwrap();
    let fx = fixture();
    for value in fx["spans"].as_array().unwrap() {
        let f: FixtureSpan = serde_json::from_value(value.clone()).unwrap();
        let off_ns = match (f.hour, f.min, f.sec) {
            (Some(h), _, _) => h * 3_600_000_000_000,
            (None, Some(m), _) => m * 60_000_000_000,
            (None, None, Some(x)) => x * 1_000_000_000,
            _ => 0,
        };
        let span = Span {
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
        };
        storage.write_span(&span).await.unwrap();
    }
    drop(storage);
}

fn run_cli(data_dir: &std::path::Path, extra_args: &[&str]) -> serde_json::Value {
    let home = tempfile::TempDir::new().unwrap();
    let mut args: Vec<String> = vec![
        "model-performance".into(),
        "--data-dir".into(),
        data_dir.to_string_lossy().into_owned(),
    ];
    args.extend(extra_args.iter().map(|a| a.to_string()));
    let output = assert_cmd::Command::cargo_bin("otelite")
        .unwrap()
        .args(&args)
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cli failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[tokio::test]
async fn cli_edges_match_frozen_fixture() {
    let temp = tempfile::TempDir::new().unwrap();
    build_fixture_db(temp.path()).await;
    let fx = fixture();

    let args: Vec<&str> = fx["cli_args"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let got = run_cli(temp.path(), &args);
    if capture() {
        freeze("cli", got.clone());
    }
    let expected = fixture()["cli"].clone();
    assert_eq!(
        got, expected,
        "CLI edge-matrix output drifted from frozen fixture v1"
    );
}

#[test]
fn api_and_cli_edge_goldens_deep_equal() {
    if capture() {
        return;
    }
    let fx = fixture();
    assert_eq!(fx["api"], fx["cli"], "API and CLI goldens must deep-equal");
    assert_eq!(fx["api_empty"], fx["cli_empty"]);
}

#[tokio::test]
async fn cli_edges_empty_window() {
    let temp = tempfile::TempDir::new().unwrap();
    build_fixture_db(temp.path()).await;
    let fx = fixture();

    let args: Vec<&str> = fx["cli_args_empty"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let got = run_cli(temp.path(), &args);
    if capture() {
        freeze("cli_empty", got.clone());
    }
    let expected = fixture()["cli_empty"].clone();
    assert_eq!(got, expected);
    assert_eq!(got["identities"], serde_json::json!([]));
    assert_eq!(got["assessments"], serde_json::json!([]));
}

#[tokio::test]
async fn cli_edges_pretty_renders_dst_range_in_utc_with_timezone_echoed() {
    let temp = tempfile::TempDir::new().unwrap();
    build_fixture_db(temp.path()).await;

    let home = tempfile::TempDir::new().unwrap();
    let output = assert_cmd::Command::cargo_bin("otelite")
        .unwrap()
        .args([
            "model-performance",
            "--data-dir",
            &temp.path().to_string_lossy(),
            "--start",
            "2026-03-28",
            "--end",
            "2026-03-30",
            "--rolling",
            "6d",
            "--timezone",
            "Europe/London",
            "--format",
            "pretty",
        ])
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "cli failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let out = String::from_utf8_lossy(&output.stdout).to_string();
    // The range straddles the UK spring-forward; the rendered intervals are
    // the exact UTC bounds (the timezone is echoed for calendar alignment,
    // never applied to the boundaries).
    assert!(out.contains("Current:   [2026-03-28T00:00:00Z → 2026-03-30T00:00:00Z)"));
    assert!(out.contains("Preceding: [2026-03-26T00:00:00Z → 2026-03-28T00:00:00Z)"));
    assert!(out.contains("Rolling:   [2026-03-20T00:00:00Z → 2026-03-26T00:00:00Z)"));
    assert!(out.contains("Timezone:  Europe/London"));
    // Tail-only regression vocabulary renders verbatim.
    assert!(out.contains("tail_regression"));
    // Both identity splits are present and labelled by fingerprint.
    assert!(out.contains("openai/shared-model"));
    assert!(out.contains("anthropic/shared-model"));
    assert!(out.contains("fingerprint: genai-v1-"));
}
