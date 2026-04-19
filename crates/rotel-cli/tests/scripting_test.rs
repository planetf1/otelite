//! Integration tests for CLI scripting and Unix conventions
//!
//! These tests verify that the CLI follows Unix conventions:
//! - Exit codes: 0=success, 1=error, 2=connection, 3=not found
//! - Errors write to stderr
//! - Data writes to stdout
//! - JSON output is parseable by jq

use assert_cmd::Command;
use mockito::{Matcher, Server};
use predicates::prelude::*;

// T083: Integration test for exit codes (0, 1, 2, 3)

#[tokio::test]
async fn test_exit_code_success() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/logs")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "50".into()),
            Matcher::UrlEncoded("since".into(), "1h".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"logs": [], "total": 0, "limit": 50, "offset": 0}"#)
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg(server.url())
        .arg("logs")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("No logs found"));
}

#[tokio::test]
async fn test_exit_code_connection_error() {
    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg("http://localhost:9999")
        .arg("--timeout")
        .arg("1")
        .arg("logs")
        .arg("list")
        .assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains("Failed to connect"))
        .stderr(predicate::str::contains("Suggestions"));
}

#[tokio::test]
async fn test_exit_code_not_found() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/logs/9999999999999999")
        .with_status(404)
        .with_header("content-type", "application/json")
        .with_body(r#"{"error":"Not found"}"#)
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg(server.url())
        .arg("logs")
        .arg("show")
        .arg("9999999999999999")
        .assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("not found"))
        .stderr(predicate::str::contains("Suggestions"));
}

#[tokio::test]
async fn test_exit_code_invalid_argument() {
    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--timeout")
        .arg("invalid")
        .arg("logs")
        .arg("list")
        .assert()
        .failure()
        .code(2);
}

// T084: Integration test for stderr error output

#[tokio::test]
async fn test_errors_write_to_stderr() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/logs/1705315800000000000")
        .with_status(404)
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg(server.url())
        .arg("logs")
        .arg("show")
        .arg("1705315800000000000")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"))
        .stdout(predicate::str::is_empty());
}

#[tokio::test]
async fn test_connection_error_message_format() {
    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg("http://localhost:9999")
        .arg("--timeout")
        .arg("1")
        .arg("logs")
        .arg("list")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to connect"))
        .stderr(predicate::str::contains("Suggestions:"))
        .stderr(predicate::str::contains("Check if Rotel server is running"))
        .stderr(predicate::str::contains("Verify the endpoint URL"));
}

#[tokio::test]
async fn test_not_found_error_message_format() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/traces/missing-trace")
        .with_status(404)
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg(server.url())
        .arg("traces")
        .arg("show")
        .arg("missing-trace")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"))
        .stderr(predicate::str::contains("Suggestions:"))
        .stderr(predicate::str::contains("Verify the ID is correct"));
}

// T085: Integration test for stdout data output

#[tokio::test]
async fn test_data_writes_to_stdout() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/logs")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "50".into()),
            Matcher::UrlEncoded("since".into(), "1h".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs": [{
                "timestamp": 1705315800000000000,
                "severity": "INFO",
                "severity_text": "INFO",
                "body": "Test log message",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            }], "total": 1, "limit": 50, "offset": 0}"#,
        )
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg(server.url())
        .arg("logs")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("1705315800000000000"))
        .stdout(predicate::str::contains("Test log message"))
        .stderr(predicate::str::is_empty());
}

#[tokio::test]
async fn test_json_output_to_stdout() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/metrics")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "50".into()),
            Matcher::UrlEncoded("since".into(), "1h".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[{
                "name": "cpu_usage",
                "description": null,
                "unit": null,
                "metric_type": "gauge",
                "value": 45.2,
                "timestamp": 1705315800000000000,
                "attributes": {"host": "server1"},
                "resource": null
            }]"#,
        )
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg(server.url())
        .arg("--format")
        .arg("json")
        .arg("metrics")
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("cpu_usage"))
        .stdout(predicate::str::contains("45.2"))
        .stderr(predicate::str::is_empty());
}

// T086: Integration test for JSON piping to jq

#[tokio::test]
async fn test_json_output_is_valid() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/logs")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "50".into()),
            Matcher::UrlEncoded("since".into(), "1h".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"logs": [{
                "timestamp": 1705315800000000000,
                "severity": "INFO",
                "severity_text": "INFO",
                "body": "Test",
                "attributes": {},
                "resource_attributes": {},
                "scope_name": "test",
                "trace_id": null,
                "span_id": null
            }], "total": 1, "limit": 50, "offset": 0}"#,
        )
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    let output = cmd
        .arg("--endpoint")
        .arg(server.url())
        .arg("--format")
        .arg("json")
        .arg("logs")
        .arg("list")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("Invalid UTF-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    assert!(parsed.is_array());
    let array = parsed.as_array().unwrap();
    assert_eq!(array.len(), 1);
    assert_eq!(array[0]["timestamp"], 1705315800000000000_i64);
    assert_eq!(array[0]["severity"], "INFO");
}

#[tokio::test]
async fn test_json_output_can_be_piped() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/traces")
        .match_query(Matcher::UrlEncoded("limit".into(), "50".into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{"traces": [{
                "trace_id": "trace-123",
                "root_span_name": "http-request",
                "start_time": 1705315800000000000,
                "duration": 150000000,
                "span_count": 1,
                "service_names": [],
                "has_errors": false
            }], "total": 1, "limit": 50, "offset": 0}"#,
        )
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    let output = cmd
        .arg("--endpoint")
        .arg(server.url())
        .arg("--format")
        .arg("json")
        .arg("traces")
        .arg("list")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("Invalid UTF-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON");

    let trace_id = parsed[0]["trace_id"].as_str().unwrap();
    assert_eq!(trace_id, "trace-123");

    let duration = parsed[0]["duration"].as_i64().unwrap();
    assert_eq!(duration, 150000000);
}

#[tokio::test]
async fn test_empty_json_array_is_valid() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/metrics")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("limit".into(), "50".into()),
            Matcher::UrlEncoded("since".into(), "1h".into()),
        ]))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[]"#)
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    let output = cmd
        .arg("--endpoint")
        .arg(server.url())
        .arg("--format")
        .arg("json")
        .arg("metrics")
        .arg("list")
        .output()
        .expect("Failed to execute command");

    assert!(output.status.success());

    let stdout = String::from_utf8(output.stdout).expect("Invalid UTF-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON");
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 0);
}

// Made with Bob
