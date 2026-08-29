//! CLI error-handling, timeout, and invalid-input tests (#6).
//!
//! Drives the real binary (assert_cmd) against mockito endpoints and a
//! closed port, asserting exit codes and stderr content. Exit-code
//! contract: 1 = API/argument/JSON errors, 2 = connection/timeout,
//! 3 = not found.

use assert_cmd::Command;
use mockito::Server;
use predicates::prelude::*;
use std::net::TcpListener;

fn otelite() -> Command {
    Command::cargo_bin("otelite").expect("otelite binary should build")
}

/// Bind a listener and drop it, returning a port that is (almost
/// certainly) refused immediately.
fn closed_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local listener");
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

// --- Invalid CLI input -------------------------------------------------

#[test]
fn test_cli_invalid_endpoint_explains_error() {
    let mut cmd = otelite();
    cmd.args(["--endpoint", "not-a-url", "logs", "list"]);
    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("Invalid --endpoint 'not-a-url'"))
        .stderr(predicate::str::contains("expected a URL"));
}

#[test]
fn test_cli_zero_timeout_is_error() {
    let mut cmd = otelite();
    cmd.args([
        "--timeout",
        "0",
        "--endpoint",
        "http://127.0.0.1:1",
        "logs",
        "list",
    ]);
    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains(
            "timeout must be at least 1 second",
        ));
}

#[test]
fn test_cli_negative_timeout_is_rejected() {
    let mut cmd = otelite();
    cmd.args(["--timeout=-1", "logs", "list"]);
    // clap rejects the negative value for the u64 flag before the CLI
    // runs, with a message that names the flag.
    cmd.assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "invalid value '-1' for '--timeout <TIMEOUT>'",
        ));
}

// --- Network errors ----------------------------------------------------

#[test]
fn test_cli_connection_refused_exit_2() {
    let port = closed_port();
    let mut cmd = otelite();
    cmd.args([
        "--endpoint",
        &format!("http://127.0.0.1:{}", port),
        "logs",
        "list",
    ]);
    cmd.assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "Failed to connect to Otelite backend",
        ));
}

/// A server that accepts connections but never responds, used to
/// exercise the CLI's request timeout.
async fn start_silent_server() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind silent server");
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                // Hold the connection open; the client's timeout fires
                // long before we would respond.
                let _ = socket;
                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            });
        }
    });
    format!("http://{}", addr)
}

#[tokio::test]
async fn test_cli_request_timeout_exit_2() {
    let endpoint = start_silent_server().await;

    let mut cmd = otelite();
    cmd.args([
        "--endpoint",
        endpoint.as_str(),
        "--timeout",
        "1",
        "logs",
        "list",
    ]);
    cmd.assert()
        .failure()
        .code(2)
        .stderr(predicate::str::contains(
            "Failed to connect to Otelite backend",
        ));
}

// --- HTTP errors -------------------------------------------------------

#[tokio::test]
async fn test_cli_api_404_exit_3() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/api/logs")
        .match_query(mockito::Matcher::Any)
        .with_status(404)
        .with_body("not found")
        .create_async()
        .await;

    let mut cmd = otelite();
    cmd.args(["--endpoint", server.url().as_str(), "logs", "list"]);
    cmd.assert()
        .failure()
        .code(3)
        .stderr(predicate::str::contains("resource not found"));
}

#[tokio::test]
async fn test_cli_api_500_exit_1_readable() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/api/logs")
        .match_query(mockito::Matcher::Any)
        .with_status(500)
        .with_body("internal error")
        .create_async()
        .await;

    let mut cmd = otelite();
    cmd.args(["--endpoint", server.url().as_str(), "logs", "list"]);
    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("HTTP 500"));
}

#[tokio::test]
async fn test_cli_malformed_json_exit_1_no_panic() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/api/logs")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body("this is not json {")
        .create_async()
        .await;

    let mut cmd = otelite();
    cmd.args(["--endpoint", server.url().as_str(), "logs", "list"]);
    // A panic would abort with a different code; the CLI must report a
    // decode error to stderr and exit 1.
    cmd.assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("error decoding response body"))
        .stderr(predicate::str::contains("panic").not());
}

#[tokio::test]
async fn test_cli_limit_zero_returns_empty() {
    let mut server = Server::new_async().await;
    server
        .mock("GET", "/api/logs")
        .match_query(mockito::Matcher::Any)
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"logs": [], "total": 0, "limit": 0, "offset": 0}"#)
        .create_async()
        .await;

    let mut cmd = otelite();
    cmd.args([
        "--endpoint",
        server.url().as_str(),
        "logs",
        "list",
        "--limit",
        "0",
    ]);
    // Documented behaviour: --limit 0 is a valid empty page, not an error.
    cmd.assert().success();
}

// --- usage command (previously untested) -------------------------------

/// Point the binary at a fresh empty database in a temp dir and run it.
fn run_usage(args: &[&str]) -> (assert_cmd::assert::Assert, tempfile::TempDir) {
    let tmp = tempfile::tempdir().expect("temp dir");
    let mut cmd = otelite();
    cmd.args(args).env("OTELITE_DATA_DIR", tmp.path());
    (cmd.assert(), tmp)
}

#[test]
fn test_usage_empty_database() {
    let (assert, _tmp) = run_usage(&["usage"]);
    assert.success();
}

#[test]
fn test_usage_since_24h() {
    let (assert, _tmp) = run_usage(&["usage", "--since", "24h"]);
    assert.success();
}

#[test]
fn test_usage_json_is_valid_json() {
    let (assert, _tmp) = run_usage(&["usage", "--format", "json"]);
    let stdout = assert.success().get_output().stdout.clone();
    let json = std::str::from_utf8(&stdout).expect("utf-8 stdout");
    serde_json::from_str::<serde_json::Value>(json)
        .expect("usage --format json must emit valid JSON");
}
