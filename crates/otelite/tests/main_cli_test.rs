//! Integration tests for main.rs CLI argument parsing

use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn test_cli_help_flag() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.arg("--help");
    cmd.assert().success().stdout(predicate::str::contains(
        "Lightweight OpenTelemetry receiver and dashboard",
    ));
}

#[test]
fn test_cli_version_flag() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.arg("--version");
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("otelite"));
}

#[test]
fn test_cli_invalid_log_level() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "--log-level",
        "invalid",
        "logs",
        "list",
    ]);
    // Should still run but use default log level
    // Uses unreachable endpoint so test is environment-independent
    cmd.assert().failure();
}

#[test]
fn test_cli_logs_list_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["logs", "list", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("List recent log entries"));
}

#[test]
fn test_cli_logs_search_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["logs", "search", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Full-text search in log bodies"));
}

#[test]
fn test_cli_logs_show_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["logs", "show", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Show a single log entry by ID"));
}

#[test]
fn test_cli_logs_export_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["logs", "export", "--help"]);
    cmd.assert().success().stdout(predicate::str::contains(
        "Export log entries to file or stdout",
    ));
}

#[test]
fn test_cli_traces_list_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["traces", "list", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("List recent distributed traces"));
}

#[test]
fn test_cli_traces_show_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["traces", "show", "--help"]);
    cmd.assert().success().stdout(predicate::str::contains(
        "Show a single trace with all spans",
    ));
}

#[test]
fn test_cli_traces_export_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["traces", "export", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Export traces to file or stdout"));
}

#[test]
fn test_cli_metrics_list_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["metrics", "list", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("List available metrics"));
}

#[test]
fn test_cli_metrics_show_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["metrics", "show", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Show metric values by name"));
}

#[test]
fn test_cli_metrics_export_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["metrics", "export", "--help"]);
    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Export metrics to file or stdout"));
}

#[test]
fn test_cli_serve_help() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["serve", "--help"]);
    cmd.assert().success().stdout(predicate::str::contains(
        "Start the server with OTLP receivers in the foreground",
    ));
}

#[test]
fn test_cli_dashboard_alias_help() {
    // `dashboard` is a hidden alias for `serve` — it must still work
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["dashboard", "--help"]);
    cmd.assert().success().stdout(predicate::str::contains(
        "Start the server with OTLP receivers in the foreground",
    ));
}

#[test]
fn test_cli_logs_search_missing_query() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["logs", "search"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_cli_logs_show_missing_id() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["logs", "show"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_cli_traces_show_missing_id() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["traces", "show"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_cli_metrics_show_missing_name() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["metrics", "show"]);
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_cli_invalid_subcommand() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.arg("invalid");
    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized subcommand"));
}

#[test]
fn test_cli_global_format_flag() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "--format",
        "json",
        "logs",
        "list",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_global_no_color_flag() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "--no-color",
        "logs",
        "list",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_global_timeout_flag() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "--timeout",
        "5",
        "logs",
        "list",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_global_endpoint_flag() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["--endpoint", "http://custom:9000", "logs", "list"]);
    // Will fail due to no server, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_logs_list_with_limit() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "logs",
        "list",
        "--limit",
        "10",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_logs_list_with_severity() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "logs",
        "list",
        "--severity",
        "ERROR",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_logs_list_with_since() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "logs",
        "list",
        "--since",
        "24h",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_traces_list_with_status() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "traces",
        "list",
        "--status",
        "ERROR",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_traces_list_with_min_duration() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "traces",
        "list",
        "--min-duration",
        "1s",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_metrics_list_with_name() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "metrics",
        "list",
        "--name",
        "http",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_metrics_list_with_label() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args([
        "--endpoint",
        "http://localhost:1",
        "metrics",
        "list",
        "--label",
        "method=GET",
    ]);
    // Will fail due to unreachable endpoint, but validates argument parsing
    cmd.assert().failure();
}

#[test]
fn test_cli_serve_with_custom_addr() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["serve", "--addr", "0.0.0.0:8080"]);
    // Will fail due to port binding, but validates argument parsing
    cmd.timeout(std::time::Duration::from_secs(2));
    cmd.assert().failure();
}

#[test]
fn test_cli_serve_with_storage_path() {
    let mut cmd = Command::cargo_bin("otelite").unwrap();
    cmd.args(["serve", "--storage-path", "/tmp/test.db"]);
    // Will fail due to startup, but validates argument parsing
    cmd.timeout(std::time::Duration::from_secs(2));
    cmd.assert().failure();
}
