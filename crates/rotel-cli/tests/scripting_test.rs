//! Integration tests for CLI scripting and Unix conventions
//!
//! These tests verify that the CLI follows Unix conventions:
//! - Exit codes: 0=success, 1=error, 2=connection, 3=not found
//! - Errors write to stderr
//! - Data writes to stdout
//! - JSON output is parseable by jq

use assert_cmd::Command;
use mockito::Server;
use predicates::prelude::*;

// T083: Integration test for exit codes (0, 1, 2, 3)

#[tokio::test]
async fn test_exit_code_success() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/logs")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"[]"#)
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg(server.url())
        .arg("logs")
        .arg("list")
        .assert()
        .success() // Exit code 0
        .stdout(predicate::str::contains("No logs found"));
}

#[tokio::test]
async fn test_exit_code_connection_error() {
    // Use invalid endpoint to trigger connection error
    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg("http://localhost:9999") // Non-existent server
        .arg("--timeout")
        .arg("1") // Short timeout
        .arg("logs")
        .arg("list")
        .assert()
        .failure()
        .code(2) // Connection error exit code
        .stderr(predicate::str::contains("Failed to connect"))
        .stderr(predicate::str::contains("Suggestions"));
}

#[tokio::test]
async fn test_exit_code_not_found() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/logs/nonexistent-id")
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
        .arg("nonexistent-id")
        .assert()
        .failure()
        .code(3) // Not found exit code
        .stderr(predicate::str::contains("not found"))
        .stderr(predicate::str::contains("Suggestions"));
}

#[tokio::test]
async fn test_exit_code_invalid_argument() {
    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--timeout")
        .arg("invalid") // Invalid timeout value
        .arg("logs")
        .arg("list")
        .assert()
        .failure()
        .code(2); // clap returns 2 for usage errors
}

// T084: Integration test for stderr error output

#[tokio::test]
async fn test_errors_write_to_stderr() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/logs/test-id")
        .with_status(404)
        .create_async()
        .await;

    let mut cmd = Command::cargo_bin("rotel-cli").unwrap();
    cmd.arg("--endpoint")
        .arg(server.url())
        .arg("logs")
        .arg("show")
        .arg("test-id")
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"))
        .stdout(predicate::str::is_empty()); // No output to stdout on error
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
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[{
                "id": "log-123",
                "timestamp": "2024-01-15T10:30:00Z",
                "severity": "INFO",
                "message": "Test log message",
                "attributes": {}
            }]"#,
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
        .stdout(predicate::str::contains("log-123"))
        .stdout(predicate::str::contains("Test log message"))
        .stderr(predicate::str::is_empty()); // No errors to stderr
}

#[tokio::test]
async fn test_json_output_to_stdout() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/metrics")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[{
                "name": "cpu_usage",
                "type": "gauge",
                "value": 45.2,
                "timestamp": "2024-01-15T10:30:00Z",
                "labels": {"host": "server1"}
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
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[{
                "id": "log-1",
                "timestamp": "2024-01-15T10:30:00Z",
                "severity": "INFO",
                "message": "Test",
                "attributes": {}
            }]"#,
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

    // Verify JSON is parseable
    let stdout = String::from_utf8(output.stdout).expect("Invalid UTF-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON output");

    // Verify it's an array
    assert!(parsed.is_array());
    let array = parsed.as_array().unwrap();
    assert_eq!(array.len(), 1);

    // Verify structure
    assert_eq!(array[0]["id"], "log-1");
    assert_eq!(array[0]["severity"], "INFO");
}

#[tokio::test]
async fn test_json_output_can_be_piped() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/traces")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"[{
                "id": "trace-123",
                "root_span": "http-request",
                "duration_ms": 150,
                "status": "ok",
                "spans": []
            }]"#,
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

    // Verify JSON can be parsed (simulating jq)
    let stdout = String::from_utf8(output.stdout).expect("Invalid UTF-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON");

    // Simulate jq query: .[0].id
    let trace_id = parsed[0]["id"].as_str().unwrap();
    assert_eq!(trace_id, "trace-123");

    // Simulate jq query: .[0].duration_ms
    let duration = parsed[0]["duration_ms"].as_f64().unwrap();
    assert_eq!(duration, 150.0);
}

#[tokio::test]
async fn test_empty_json_array_is_valid() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", "/api/metrics")
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

    // Verify empty array is valid JSON
    let stdout = String::from_utf8(output.stdout).expect("Invalid UTF-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("Invalid JSON");
    assert!(parsed.is_array());
    assert_eq!(parsed.as_array().unwrap().len(), 0);
}

// Made with Bob
