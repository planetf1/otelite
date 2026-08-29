//! CLI model-performance parity test (#121/#153): `otelite
//! model-performance --format json-compact` must deep-equal the frozen
//! fixture, which was captured from the API — proving the CLI and API
//! present the same diagnosis.
//!
//! Regenerating the CLI golden: `OTELITE_MP_PARITY_CAPTURE=1 cargo test
//! -p otelite --test model_performance_parity_cli_test`.

use otelite_core::telemetry::trace::{Span, SpanKind, SpanStatus, StatusCode as SpanStatusCode};
use otelite_storage::sqlite::SqliteBackend;
use otelite_storage::{StorageBackend, StorageConfig};

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
        "/../otelite-api/tests/fixtures/model_performance_parity_v1.json"
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
async fn cli_model_performance_matches_frozen_fixture() {
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
        "CLI model-performance output drifted from frozen fixture v1"
    );
}

#[test]
fn api_and_cli_goldens_deep_equal() {
    // Requirement: the CLI json-compact output deep-equals the API
    // response. Both goldens were captured independently — assert the
    // equality directly, not just each side against itself.
    if capture() {
        return;
    }
    let fx = fixture();
    assert_eq!(fx["api"], fx["cli"], "API and CLI goldens must deep-equal");
    assert_eq!(fx["api_empty"], fx["cli_empty"]);
}

#[tokio::test]
async fn cli_model_performance_empty_window() {
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
}

#[tokio::test]
async fn cli_model_performance_low_sample_and_zero_baseline_survive() {
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

    let find = |model: &str| -> &serde_json::Value {
        got["assessments"]
            .as_array()
            .unwrap()
            .iter()
            .find(|a| a["model"] == model)
            .unwrap_or_else(|| panic!("missing assessment {model}"))
    };

    // Low sample: first-class insufficient state with the count.
    let low = find("claude-sonnet-4-5");
    assert_eq!(low["overall_class"], "insufficient_telemetry");
    assert_eq!(low["metrics"][0]["eligible_current"], 5);

    // Zero baseline: the percentage-unavailable state survives as JSON null.
    let errored = find("o2");
    assert_eq!(errored["overall_class"], "error_associated");
    assert!(errored["metrics"][0]["error_association"]["relative"].is_null());
}

#[tokio::test]
async fn cli_model_performance_pretty_prints_intervals_and_states() {
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
            "2026-08-25",
            "--end",
            "2026-08-26",
            "--rolling",
            "24h",
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
    // Exact selected intervals, preceding and rolling baselines, timezone.
    assert!(out.contains("Current:   [2026-08-25T00:00:00Z → 2026-08-26T00:00:00Z)"));
    assert!(out.contains("Preceding: [2026-08-24T00:00:00Z → 2026-08-25T00:00:00Z)"));
    assert!(out.contains("Rolling:   [2026-08-23T00:00:00Z → 2026-08-24T00:00:00Z)"));
    assert!(out.contains("Timezone:  Europe/London"));
    // Identity is (provider, request model, fingerprint); response models
    // are shown separately, never merged.
    assert!(out.contains("openai/gpt-4o"));
    assert!(out.contains("fingerprint: genai-v1-"));
    // First-class states render verbatim.
    assert!(out.contains("typical_regression"));
    assert!(out.contains("insufficient_telemetry"));
    assert!(out.contains("mixed_evidence"));
    assert!(out.contains("workload_shift_correlated"));
    assert!(out.contains("error_associated"));
    assert!(out.contains("correlation, not causation"));
    assert!(out.contains("attribution is prevented"));
}
