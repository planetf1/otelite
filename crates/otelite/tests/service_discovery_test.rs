//! #107 regression: `status`/`stop` discovery must not depend on the PID
//! file, which only `otelite start` writes. A service-managed or hand-run
//! `serve` (no PID file) is discovered through its TCP listener instead.
//!
//! The throwaway daemons run on ephemeral OTLP ports (via
//! OTELITE_OTLP_GRPC_PORT / OTELITE_OTLP_HTTP_PORT), not the standard
//! 4317/4318: nextest runs integration test binaries in parallel, so a
//! sibling binary — or a real daemon on a dev machine — holding the
//! standard ports would make a default-port `serve` die on its second bind
//! and race the discovery assertions (observed CI failures 2026-09-04).

use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use nix::unistd::Pid;

fn wait_for_port(port: u16, deadline: Instant) {
    loop {
        if TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), port)).is_ok() {
            return;
        }
        assert!(Instant::now() < deadline, "serve did not start listening");
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Wait until `local_otelite_pid(port)` returns `Some` — i.e. the listener is
/// both connectable *and* attributed to an otelite process by `lsof`. This is
/// a stricter precondition than `wait_for_port` alone; without it `start` may
/// run discovery before `lsof` has registered the new process and see the port
/// as free.
fn wait_for_otelite_attribution(port: u16, deadline: Instant) {
    loop {
        if otelite::commands::service::local_otelite_pid(port)
            .ok()
            .flatten()
            .is_some()
        {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "otelite process was not attributed to port {port} within the deadline"
        );
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn test_discovery_finds_serve_without_pid_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    let storage = data_dir.join("otelite.db");
    let pid_file = data_dir.join("otelite.pid");

    let dashboard_port = free_port();
    let grpc_port = free_port();
    let http_port = free_port();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args([
            "serve",
            "--addr",
            &format!("127.0.0.1:{dashboard_port}"),
            "--storage-path",
            &storage.to_string_lossy(),
        ])
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();

    wait_for_port(grpc_port, Instant::now() + Duration::from_secs(15));

    // The regression itself: no PID file, yet the listener is discovered
    // and the PID matches the serve process.
    assert!(
        !pid_file.exists(),
        "serve must not write a PID file (premise of #107)"
    );
    let found = otelite::commands::service::local_otelite_pid(grpc_port)
        .unwrap()
        .expect("listener discovery must find the serve process");
    assert_eq!(found, pid, "discovered PID must be the serve process");

    // End-to-end through the CLI: `status` must report the no-PID-file
    // daemon as running with its PID, and `stop` must reach it through
    // the same discovery (this is the unix code path CI exercises).
    let status_out = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .arg("status")
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .output()
        .unwrap();
    let status_text = String::from_utf8_lossy(&status_out.stdout);

    // On a dev machine with a launchd-managed daemon, `status` reports
    // that daemon first — intended product behaviour, and `stop` would
    // then signal the *real* daemon. The discovery regression is already
    // asserted directly above; only run the e2e half where `status`
    // reports the throwaway (i.e. CI, no launchd daemon).
    if status_text.contains("Running (launchd") {
        let _ = child.kill();
        let _ = child.wait();
        return;
    }

    assert!(
        status_out.status.success(),
        "status must succeed: {}",
        String::from_utf8_lossy(&status_out.stderr)
    );
    assert!(
        status_text.contains("Status: Running"),
        "status output: {status_text}"
    );
    assert!(
        status_text.contains(&format!("PID: {pid}")),
        "status must report the discovered PID; output: {status_text}"
    );

    let stop_out = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .arg("stop")
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .output()
        .unwrap();
    assert!(
        stop_out.status.success(),
        "stop must succeed: {}",
        String::from_utf8_lossy(&stop_out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&stop_out.stdout).contains("Otelite daemon stopped"),
        "stop output: {}",
        String::from_utf8_lossy(&stop_out.stdout)
    );

    // The daemon must actually be gone.
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait().unwrap() {
            Some(_) => break,
            None => {
                assert!(Instant::now() < deadline, "serve did not exit after `stop`");
                std::thread::sleep(Duration::from_millis(100));
            },
        }
    }
    let _ = TcpListener::bind(format!("127.0.0.1:{grpc_port}"));
}

fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    listener.local_addr().unwrap().port()
}

fn free_port_held() -> (u16, TcpListener) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind free port");
    (listener.local_addr().unwrap().port(), listener)
}

/// #M11 regression: `start` must discover a running daemon that has no
/// PID file (launchd / hand-run `serve`) and refuse to spawn a second
/// one, instead of leaving a PID file for a process that dies on bind.
#[test]
fn test_start_refuses_when_daemon_has_no_pid_file() {
    let grpc_port = free_port();
    let http_port = free_port();

    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");

    // A throwaway serve on ephemeral OTLP ports, so the test is
    // self-contained regardless of what else (a real daemon, a sibling
    // test binary) runs on this machine.
    let storage = data_dir.join("otelite.db");
    let dashboard_port = free_port();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args([
            "serve",
            "--addr",
            &format!("127.0.0.1:{dashboard_port}"),
            "--storage-path",
            &storage.to_string_lossy(),
        ])
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    wait_for_port(grpc_port, Instant::now() + Duration::from_secs(15));
    wait_for_otelite_attribution(grpc_port, Instant::now() + Duration::from_secs(15));

    // `start` from a different (empty) data dir: no PID file to trip
    // on, only port discovery can find the daemon. It sees the same OTLP
    // ports via the environment, so discovery targets the throwaway.
    let start_data_dir = temp.path().join("start-data");
    let start_port = free_port();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args(["start", "--addr", &format!("127.0.0.1:{start_port}")])
        .env("OTELITE_DATA_DIR", start_data_dir.as_os_str())
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "start must refuse when a daemon is already listening: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("already running"),
        "expected an already-running refusal, got: {text}"
    );
    // The refusal must happen before any spawn: no PID file appears.
    assert!(
        !start_data_dir.join("otelite.pid").exists(),
        "a refused start must not leave a PID file"
    );

    let _ = child.kill();
    let _ = child.wait();
}

/// #M12 regression: when the spawned daemon dies immediately (its
/// dashboard port is taken), `start` must fail with a clear error and
/// roll back the PID file — never print "started with PID X".
#[test]
fn test_start_reports_immediate_exit_and_cleans_pid_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    let storage = data_dir.join("otelite.db");
    let grpc_port = free_port();
    let http_port = free_port();

    // Occupy the dashboard port the child will try to bind.
    let (taken_port, _held) = free_port_held();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args([
            "start",
            "--addr",
            &format!("127.0.0.1:{taken_port}"),
            "--storage-path",
            &storage.to_string_lossy(),
        ])
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        // The child inherits these, so it can start on a dev machine
        // where the standard OTLP ports are already held.
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .output()
        .unwrap();

    assert!(
        !out.status.success(),
        "start must fail when the dashboard port is taken: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        text.contains("exited immediately"),
        "expected an immediate-exit error, got: {text}"
    );
    assert!(
        !text.contains("daemon started"),
        "a dead daemon must not be reported as started: {text}"
    );
    assert!(
        !data_dir.join("otelite.pid").exists(),
        "the PID file must be rolled back when the daemon dies"
    );
}

/// #M15 regression: a corrupt PID file (torn write, stale garbage) must
/// not make `status` fail with an "Invalid PID" error — it is removed
/// and discovery proceeds.
#[test]
fn test_status_recovers_from_corrupt_pid_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    std::fs::create_dir_all(&data_dir).unwrap();
    let pid_file = data_dir.join("otelite.pid");
    std::fs::write(&pid_file, "definitely-not-a-pid\n").unwrap();

    let out = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .arg("status")
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        .output()
        .unwrap();

    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success(),
        "status must not fail on a corrupt PID file: {text}"
    );
    assert!(
        !text.to_lowercase().contains("invalid pid"),
        "a corrupt PID file must be recovered from, not surfaced as an error: {text}"
    );
    // On a machine where launchd supervises otelite, `status` reports
    // via launchd and never touches the PID file — the recovery path
    // (and its file removal) is exercised by the other checks.
    if !text.contains("Running (launchd") {
        assert!(!pid_file.exists(), "the corrupt PID file must be removed");
    }
}

/// #M13 regression: the PID file round-trips exactly and is replaced
/// atomically (temp file + rename), never left half-written or with a
/// stray temp file behind.
#[test]
fn test_pid_file_atomic_roundtrip() {
    let temp = tempfile::TempDir::new().unwrap();
    let pid_file = temp.path().join("otelite.pid");

    otelite::commands::service::write_pid_file(4242, &pid_file).unwrap();
    assert_eq!(
        otelite::commands::service::read_pid_file(&pid_file).unwrap(),
        Some(4242)
    );
    assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), "4242");

    // Overwriting replaces the content wholesale.
    otelite::commands::service::write_pid_file(7, &pid_file).unwrap();
    assert_eq!(
        otelite::commands::service::read_pid_file(&pid_file).unwrap(),
        Some(7)
    );
    assert_eq!(std::fs::read_to_string(&pid_file).unwrap(), "7");

    // No temp file left behind.
    assert!(
        !temp.path().join("otelite.pid.tmp").exists(),
        "the atomic write must not leave a temp file"
    );

    // Missing file → None (the "not started via `otelite start`" case).
    std::fs::remove_file(&pid_file).unwrap();
    assert_eq!(
        otelite::commands::service::read_pid_file(&pid_file).unwrap(),
        None
    );
}

/// M16 regression: SIGTERM used to be ignored — `axum::serve` blocks
/// forever, so the daemon only died when `otelite stop` escalated to
/// SIGKILL after 10 s. It must now exit gracefully and release its
/// listener.
#[test]
fn test_serve_exits_gracefully_on_sigterm() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    let storage = data_dir.join("otelite.db");
    let dashboard_port = free_port();
    let grpc_port = free_port();
    let http_port = free_port();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args([
            "serve",
            "--addr",
            &format!("127.0.0.1:{dashboard_port}"),
            "--storage-path",
            &storage.to_string_lossy(),
        ])
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        // OTLP on free ports: this test must not collide with (or depend
        // on) any daemon holding the standard 4317/4318.
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    wait_for_port(dashboard_port, Instant::now() + Duration::from_secs(15));

    let started = Instant::now();
    nix::sys::signal::kill(
        Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();

    // Must exit on its own (no SIGKILL escalation) and in bounded time.
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait().unwrap() {
            Some(status) => {
                assert!(
                    started.elapsed() < Duration::from_secs(8),
                    "graceful shutdown took too long: {:?}",
                    started.elapsed()
                );
                let _ = status;
                break;
            },
            None => {
                assert!(
                    Instant::now() < deadline,
                    "serve did not exit within 8 s of SIGTERM"
                );
                std::thread::sleep(Duration::from_millis(100));
            },
        }
    }

    // The dashboard and OTLP listeners must be released.
    let freed =
        TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), dashboard_port)).is_err();
    assert!(freed, "dashboard port must be released after SIGTERM");
    for port in [grpc_port, http_port] {
        assert!(
            TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), port)).is_err(),
            "OTLP port {port} must be released after SIGTERM"
        );
    }
}

/// M17 regression: the daemon's log must go through the daily-rotating
/// appender (`--log-file`), not an ever-growing plain file. A throwaway
/// `serve` must produce a dated log file in its data dir, and the startup
/// lines must actually reach it — either while the worker thread is running
/// or via the final flush when the appender's guard drops on clean shutdown.
#[test]
fn test_serve_writes_rotating_log_file() {
    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    let storage = data_dir.join("otelite.db");
    let dashboard_port = free_port();
    let grpc_port = free_port();
    let http_port = free_port();

    // Capture serve's stderr: with --log-file the tracing output goes to the
    // log file, but panics and startup errors (e.g. the appender worker
    // thread failing to spawn) land on stderr — exactly what a 0-byte log
    // file in the Nix build sandbox needed to explain itself.
    let stderr_path = data_dir.join("serve-stderr.log");
    std::fs::create_dir_all(&data_dir).unwrap();
    let stderr_file = std::fs::File::create(&stderr_path).unwrap();

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args([
            "serve",
            "--addr",
            &format!("127.0.0.1:{dashboard_port}"),
            "--storage-path",
            &storage.to_string_lossy(),
            "--log-file",
            &data_dir.join("otelite.log").to_string_lossy(),
        ])
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        .env("OTELITE_OTLP_GRPC_PORT", grpc_port.to_string())
        .env("OTELITE_OTLP_HTTP_PORT", http_port.to_string())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::from(stderr_file))
        .spawn()
        .unwrap();

    wait_for_port(dashboard_port, Instant::now() + Duration::from_secs(15));

    let dated_log = |dir: &std::path::Path| -> Option<std::path::PathBuf> {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .find(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().starts_with("otelite.log."))
                    .unwrap_or(false)
            })
    };
    let log_len = |dir: &std::path::Path| -> u64 {
        dated_log(dir)
            .and_then(|p| std::fs::metadata(p).ok())
            .map(|m| m.len())
            .unwrap_or(0)
    };
    let stderr_tail = |dir: &std::path::Path| -> String {
        std::fs::read_to_string(dir.join("serve-stderr.log"))
            .unwrap_or_default()
            .lines()
            .take(20)
            .collect::<Vec<_>>()
            .join("\n")
    };
    // File sizes, not just names: a 0-byte sqlite db next to a 0-byte log
    // is a very different story from a healthy db next to a 0-byte log.
    let dir_listing = |dir: &std::path::Path| -> String {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok())
            .map(|e| {
                format!(
                    "{} ({} bytes)",
                    e.file_name().to_string_lossy(),
                    e.metadata().map(|m| m.len()).unwrap_or(0)
                )
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    // Linux-only forensics for environment-specific failures (the Nix build
    // sandbox): the child's resource limits and open file descriptors, so a
    // 0-byte log file can say *why*.
    let proc_snapshot = |pid: u32| -> String {
        let mut s = String::new();
        let limits = format!("/proc/{pid}/limits");
        if let Ok(l) = std::fs::read_to_string(&limits) {
            s.push_str(&format!("--- {limits} ---\n{l}\n"));
        }
        let fd_dir = format!("/proc/{pid}/fd");
        if let Ok(fds) = std::fs::read_dir(&fd_dir) {
            for e in fds.filter_map(|e| e.ok()) {
                if let Ok(link) = std::fs::read_link(e.path()) {
                    s.push_str(&format!(
                        "fd {} -> {}\n",
                        e.file_name().to_string_lossy(),
                        link.display()
                    ));
                }
            }
        }
        if s.is_empty() {
            s.push_str("(no /proc access — not Linux?)\n");
        }
        s
    };

    // The rotating appender names its file otelite.log.YYYY-MM-DD (the M17
    // regression itself) and a background worker writes the lines into it.
    // The worker is normally fast, but a loaded build sandbox may starve it
    // for seconds; poll for live content, then fall back to the shutdown
    // flush (the appender guard drops on clean exit) before failing.
    let mut live_len = 0u64;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        live_len = log_len(&data_dir);
        if live_len > 0 {
            break;
        }
        assert!(
            dated_log(&data_dir).is_some(),
            "expected a dated otelite.log.* file (the M17 regression); data dir contents: {:?}, serve stderr:\n{}",
            std::fs::read_dir(&data_dir)
                .map(|d| d.map(|e| e.unwrap().file_name()).collect::<Vec<_>>())
                .unwrap_or_default(),
            stderr_tail(&data_dir)
        );
        std::thread::sleep(Duration::from_millis(200));
    }

    // Snapshot the child's environment while it is still alive.
    let snapshot = proc_snapshot(child.id());

    nix::sys::signal::kill(
        Pid::from_raw(child.id() as i32),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        match child.try_wait().unwrap() {
            Some(_) => break,
            None => {
                assert!(Instant::now() < deadline, "serve did not exit");
                std::thread::sleep(Duration::from_millis(100));
            },
        }
    }

    let final_len = log_len(&data_dir);
    assert!(
        final_len > 0,
        "no log lines reached the dated file ({} bytes while running, {} bytes after clean shutdown);\ndata dir: {};\nserve stderr:\n{};\n{}",
        live_len,
        final_len,
        dir_listing(&data_dir),
        stderr_tail(&data_dir),
        snapshot
    );
}
