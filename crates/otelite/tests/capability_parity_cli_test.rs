//! CLI capability parity test (#120): `otelite capabilities --format
//! json-compact` must deep-equal the frozen fixture, which was captured from
//! the API — proving the CLI and API present the same capability evidence.

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
    attrs: std::collections::HashMap<String, String>,
}

fn fixture() -> serde_json::Value {
    let raw = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../otelite-api/tests/fixtures/capability_parity_v3.json"
    ))
    .unwrap();
    serde_json::from_str(&raw).unwrap()
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
                code: SpanStatusCode::Ok,
                message: None,
            },
        };
        storage.write_span(&span).await.unwrap();
    }
    // Close the pool so the WAL is checkpointed before the CLI opens the DB.
    drop(storage);
}

fn run_cli(data_dir: &std::path::Path, extra_args: &[&str]) -> serde_json::Value {
    let home = tempfile::TempDir::new().unwrap();
    let mut args: Vec<String> = vec![
        "capabilities".into(),
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
async fn cli_capabilities_matches_frozen_fixture() {
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
    assert_eq!(
        got, fx["cli"],
        "CLI capabilities output drifted from frozen fixture v1"
    );
}

#[tokio::test]
async fn cli_capabilities_empty_window() {
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
    assert_eq!(got, fx["cli_empty"]);
}

#[tokio::test]
async fn cli_capabilities_pretty_prints_vocabulary() {
    let temp = tempfile::TempDir::new().unwrap();
    build_fixture_db(temp.path()).await;

    let home = tempfile::TempDir::new().unwrap();
    let output = assert_cmd::Command::cargo_bin("otelite")
        .unwrap()
        .args([
            "capabilities",
            "--data-dir",
            &temp.path().to_string_lossy(),
            "--start",
            "2026-08-24",
            "--end",
            "2026-08-26",
        ])
        .env("HOME", home.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Vocabulary must stay distinct in the pretty view (issue #120).
    assert!(stdout.contains("openai/gpt-4o-mini"));
    assert!(stdout.contains("sparse/invalid"));
    assert!(stdout.contains("available/degenerate"));
    assert!(stdout.contains("absent/not_assessed/unavailable"));
    assert!(stdout.contains("1 duplicate deliveries collapsed"));
    // Codex correlation provenance renders as candidate counts (v2).
    assert!(stdout.contains("1/2/1/2"));
    assert!(stdout.contains("sparse/reliable/correlated"));
    // Unidentified-emitter diagnostics render attribute names only (v3, #149).
    assert!(stdout.contains("Unidentified emitters"));
    assert!(stdout.contains("Attribute names only"));
    assert!(!stdout.contains("mystery-model"));
}
