//! #253 regression: `serve` must honour `OTELITE_DATA_DIR` (and the rest of
//! the storage environment) like the direct-DB CLI paths (`usage`,
//! `sessions`, …). Previously `run_dashboard` built
//! `StorageConfig::default()`, so a service-installed daemon — whose launchd
//! plist bakes `OTELITE_DATA_DIR` in and passes no `--storage-path` —
//! ingested into `~/.otelite/data` while the CLI read the env-configured
//! database.
//!
//! Note: `otelite logs list` is *not* used for the read-side assertion —
//! list-style commands proxy to a running daemon over HTTP
//! (ApiClient → config.endpoint) and never open a database; asserting
//! against them would read whatever daemon happens to be running. The
//! daemon's write location is asserted directly on the database file
//! instead; the CLI read side's env wiring (`create_storage` →
//! `StorageConfig::from_env`) is covered by the from_env unit tests.
//!
//! Test isolation: the spawned serve runs on ephemeral OTLP ports
//! (OTELITE_OTLP_GRPC_PORT / OTELITE_OTLP_HTTP_PORT) and against a
//! `tempfile` data dir, never the standard ports or the developer's data.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn wait_for_port(port: u16, deadline: Instant) {
    loop {
        if TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), port)).is_ok() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "serve did not start listening on port {port}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Minimal OTLP/HTTP log export — one INFO record with a marker body.
const LOGS_JSON: &str = r#"{
    "resourceLogs": [{
        "scopeLogs": [{
            "logRecords": [{
                "timeUnixNano": "1700000000000000000",
                "severityText": "INFO",
                "body": { "stringValue": "serve-env-test-marker" }
            }]
        }]
    }]
}"#;

/// Raw HTTP/1.1 POST over a bare socket (no client dependency in
/// dev-dependencies); returns the response status line.
fn post_otlp_logs(http_port: u16) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", http_port)).unwrap();
    let req = format!(
        "POST /v1/logs HTTP/1.1\r\nHost: 127.0.0.1:{http_port}\r\n\
         Content-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        LOGS_JSON.len(),
        LOGS_JSON
    );
    stream.write_all(req.as_bytes()).unwrap();
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).unwrap();
    String::from_utf8_lossy(&buf[..n])
        .split("\r\n")
        .next()
        .unwrap_or("")
        .to_string()
}

/// Wait until the database file appears in `dir` (schema init creates it).
fn wait_for_db(db: &std::path::Path, deadline: Instant) {
    loop {
        if db.exists() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "database never appeared at {}",
            db.display()
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

fn stop(child: &mut std::process::Child) {
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    );
    let _ = child.wait();
}

/// OTELITE_DATA_DIR (no --storage-path): the daemon creates its database in
/// the env dir, ingests there, and `logs list` with the same env sees the
/// record.
#[test]
fn serve_honours_otelite_data_dir_env() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().to_path_buf();
    let db = data_dir.join("otelite.db");

    let dashboard_port = free_port();
    let grpc_port = free_port();
    let http_port = free_port();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args(["serve", "--addr", &format!("127.0.0.1:{dashboard_port}")])
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    wait_for_port(http_port, Instant::now() + Duration::from_secs(45));

    let status = post_otlp_logs(http_port);
    assert!(status.contains("200"), "OTLP ingest was rejected: {status}");

    // The daemon must have created its database in the env dir AND ingested
    // the record into it. (Before the fix the database appeared in
    // ~/.otelite/data and never in the env dir — wait_for_db below would
    // time out.)
    wait_for_db(&db, Instant::now() + Duration::from_secs(15));

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut found = false;
    while !found && Instant::now() < deadline {
        let conn = rusqlite::Connection::open(&db).unwrap();
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM logs WHERE body LIKE '%serve-env-test-marker%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        found = count >= 1;
        if !found {
            std::thread::sleep(Duration::from_millis(200));
        }
    }
    assert!(found, "ingested log missing from {}", db.display());

    stop(&mut child);
}

/// --storage-path wins over OTELITE_DATA_DIR: the database appears at the
/// flag path and not in the env dir.
#[test]
fn serve_storage_path_flag_wins_over_env() {
    let temp = tempfile::TempDir::new().unwrap();
    let env_dir = temp.path().join("env-dir");
    let flag_dir = temp.path().join("flag-dir");
    let flag_db = flag_dir.join("otelite.db");
    let env_db = env_dir.join("otelite.db");

    let dashboard_port = free_port();
    let grpc_port = free_port();
    let http_port = free_port();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args([
            "serve",
            "--addr",
            &format!("127.0.0.1:{dashboard_port}"),
            "--storage-path",
            &flag_db.to_string_lossy(),
        ])
        .env("OTELITE_DATA_DIR", env_dir.as_os_str())
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    wait_for_port(http_port, Instant::now() + Duration::from_secs(45));
    wait_for_db(&flag_db, Instant::now() + Duration::from_secs(15));

    stop(&mut child);

    assert!(flag_db.exists(), "database missing at --storage-path");
    assert!(
        !env_db.exists(),
        "OTELITE_DATA_DIR must lose to --storage-path, but a database was created there"
    );
}
