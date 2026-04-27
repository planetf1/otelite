use assert_cmd::Command;
use std::io::Write;
use tempfile::TempDir;

// One-liner OTLP JSON fixtures (matching sample_*.json in otelite-core)
const METRICS_LINE: &str = r#"{"resourceMetrics":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"ci-runner"}}]},"scopeMetrics":[{"metrics":[{"name":"ci.duration","unit":"s","gauge":{"dataPoints":[{"asDouble":42.5,"timeUnixNano":"1713355200000000000"}]}}]}]}]}"#;
const LOGS_LINE: &str = r#"{"resourceLogs":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"ci-runner"}}]},"scopeLogs":[{"logRecords":[{"timeUnixNano":"1713355200000000000","severityNumber":9,"severityText":"INFO","body":{"stringValue":"build succeeded"}}]}]}]}"#;
const TRACES_LINE: &str = r#"{"resourceSpans":[{"resource":{"attributes":[{"key":"service.name","value":{"stringValue":"ci-runner"}}]},"scopeSpans":[{"spans":[{"traceId":"0102030405060708090a0b0c0d0e0f10","spanId":"0102030405060708","name":"ci-step","kind":1,"startTimeUnixNano":"1713355200000000000","endTimeUnixNano":"1713355201000000000"}]}]}]}"#;

fn write_jsonl(dir: &TempDir, name: &str, lines: &[&str]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    let mut f = std::fs::File::create(&path).unwrap();
    for line in lines {
        writeln!(f, "{}", line).unwrap();
    }
    path
}

fn otelite_cmd() -> Command {
    Command::cargo_bin("otelite").unwrap()
}

#[test]
fn import_metrics_creates_db() {
    let data_dir = TempDir::new().unwrap();
    let input_dir = TempDir::new().unwrap();
    let jsonl = write_jsonl(&input_dir, "metrics.jsonl", &[METRICS_LINE]);

    otelite_cmd()
        .args([
            "import",
            jsonl.to_str().unwrap(),
            "--signal-type",
            "metrics",
            "--storage-path",
            data_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(data_dir.path().join("otelite.db").exists());
}

#[test]
fn import_logs_auto_detect() {
    let data_dir = TempDir::new().unwrap();
    let input_dir = TempDir::new().unwrap();
    let jsonl = write_jsonl(&input_dir, "logs.jsonl", &[LOGS_LINE]);

    // No --signal-type — rely on auto-detection from "resourceLogs" key
    otelite_cmd()
        .args([
            "import",
            jsonl.to_str().unwrap(),
            "--storage-path",
            data_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn import_traces_auto_detect() {
    let data_dir = TempDir::new().unwrap();
    let input_dir = TempDir::new().unwrap();
    let jsonl = write_jsonl(&input_dir, "traces.jsonl", &[TRACES_LINE]);

    otelite_cmd()
        .args([
            "import",
            jsonl.to_str().unwrap(),
            "--storage-path",
            data_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn import_multiple_lines() {
    let data_dir = TempDir::new().unwrap();
    let input_dir = TempDir::new().unwrap();
    // Three metrics lines — should import 3 data points
    let jsonl = write_jsonl(
        &input_dir,
        "multi.jsonl",
        &[METRICS_LINE, METRICS_LINE, METRICS_LINE],
    );

    otelite_cmd()
        .args([
            "import",
            jsonl.to_str().unwrap(),
            "--signal-type",
            "metrics",
            "--storage-path",
            data_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn import_skips_invalid_lines() {
    let data_dir = TempDir::new().unwrap();
    let input_dir = TempDir::new().unwrap();
    let jsonl = write_jsonl(
        &input_dir,
        "mixed.jsonl",
        &[METRICS_LINE, "{ not valid json {{", METRICS_LINE],
    );

    // Should succeed even with one bad line
    otelite_cmd()
        .args([
            "import",
            jsonl.to_str().unwrap(),
            "--signal-type",
            "metrics",
            "--storage-path",
            data_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn import_skips_empty_lines() {
    let data_dir = TempDir::new().unwrap();
    let input_dir = TempDir::new().unwrap();
    let jsonl = write_jsonl(
        &input_dir,
        "spaced.jsonl",
        &["", METRICS_LINE, "", METRICS_LINE],
    );

    otelite_cmd()
        .args([
            "import",
            jsonl.to_str().unwrap(),
            "--signal-type",
            "metrics",
            "--storage-path",
            data_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn import_empty_file_succeeds() {
    let data_dir = TempDir::new().unwrap();
    let input_dir = TempDir::new().unwrap();
    let jsonl = write_jsonl(&input_dir, "empty.jsonl", &[]);

    otelite_cmd()
        .args([
            "import",
            jsonl.to_str().unwrap(),
            "--signal-type",
            "metrics",
            "--storage-path",
            data_dir.path().to_str().unwrap(),
        ])
        .assert()
        .success();
}

#[test]
fn import_nonexistent_file_fails() {
    otelite_cmd()
        .args(["import", "/tmp/does-not-exist-otelite-test.jsonl"])
        .assert()
        .failure();
}

#[test]
fn import_unknown_signal_type_fails() {
    let data_dir = TempDir::new().unwrap();
    let input_dir = TempDir::new().unwrap();
    let jsonl = write_jsonl(&input_dir, "m.jsonl", &[METRICS_LINE]);

    otelite_cmd()
        .args([
            "import",
            jsonl.to_str().unwrap(),
            "--signal-type",
            "events",
            "--storage-path",
            data_dir.path().to_str().unwrap(),
        ])
        .assert()
        .failure();
}
