//! #107 regression: `status`/`stop` discovery must not depend on the PID
//! file, which only `otelite start` writes. A service-managed or hand-run
//! `serve` (no PID file) is discovered through its TCP listener instead.
//!
//! `serve` always owns OTLP port 4317, so the spawn-and-discover half only
//! runs when 4317 is free (CI). When a real daemon holds 4317 — the common
//! dev-machine state — the daemon itself is a no-PID-file listener (the
//! launchd/service path never writes one), so discovery against it is
//! asserted directly.

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

#[test]
fn test_discovery_finds_serve_without_pid_file() {
    // Probe 4317: if something owns it, a daemon (or another test) is
    // already in the no-PID-file state and the regression is asserted
    // against it below. Otherwise we can spawn a throwaway serve.
    let probe = TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), 4317));
    if probe.is_ok() {
        let found = otelite::commands::service::local_otelite_pid(4317)
            .unwrap()
            .expect("a listening daemon on 4317 must be discovered");
        assert!(
            nix::sys::signal::kill(Pid::from_raw(found as i32), None).is_ok(),
            "discovered PID must be alive"
        );
        return;
    }

    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    let storage = data_dir.join("otelite.db");
    let pid_file = data_dir.join("otelite.pid");

    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args([
            "serve",
            "--addr",
            "127.0.0.1:13999",
            "--storage-path",
            &storage.to_string_lossy(),
        ])
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();

    wait_for_port(4317, Instant::now() + Duration::from_secs(15));

    // The regression itself: no PID file, yet the listener is discovered
    // and the PID matches the serve process.
    assert!(
        !pid_file.exists(),
        "serve must not write a PID file (premise of #107)"
    );
    let found = otelite::commands::service::local_otelite_pid(4317)
        .unwrap()
        .expect("listener discovery must find the serve process");
    assert_eq!(found, pid, "discovered PID must be the serve process");

    // End-to-end through the CLI: `status` must report the no-PID-file
    // daemon as running with its PID, and `stop` must reach it through
    // the same discovery (this is the unix code path CI exercises).
    let status_out = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .arg("status")
        .env("OTELITE_DATA_DIR", data_dir.as_os_str())
        .output()
        .unwrap();
    assert!(
        status_out.status.success(),
        "status must succeed: {}",
        String::from_utf8_lossy(&status_out.stderr)
    );
    let status_text = String::from_utf8_lossy(&status_out.stdout);
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
    let _ = TcpListener::bind("127.0.0.1:4317");
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
    let otelite_on_4317 = otelite::commands::service::local_otelite_pid(4317).unwrap();

    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");

    // A throwaway serve so the test is self-contained on machines
    // (CI) where no daemon holds 4317.
    let mut throwaway = None;
    let dashboard_port = free_port();
    if otelite_on_4317.is_none() {
        let storage = data_dir.join("otelite.db");
        let child = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
            .args([
                "serve",
                "--addr",
                &format!("127.0.0.1:{dashboard_port}"),
                "--storage-path",
                &storage.to_string_lossy(),
            ])
            .env("OTELITE_DATA_DIR", data_dir.as_os_str())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        wait_for_port(4317, Instant::now() + Duration::from_secs(15));
        throwaway = Some(child);
    }

    // `start` from a different (empty) data dir: no PID file to trip
    // on, only port discovery can find the daemon.
    let start_data_dir = temp.path().join("start-data");
    let start_port = free_port();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_otelite"))
        .args(["start", "--addr", &format!("127.0.0.1:{start_port}")])
        .env("OTELITE_DATA_DIR", start_data_dir.as_os_str())
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

    if let Some(mut child) = throwaway {
        let _ = child.kill();
        let _ = child.wait();
    }
}

/// #M12 regression: when the spawned daemon dies immediately (its
/// dashboard port is taken), `start` must fail with a clear error and
/// roll back the PID file — never print "started with PID X".
#[test]
fn test_start_reports_immediate_exit_and_cleans_pid_file() {
    // Only meaningful when nothing otelite-owned is on 4317, otherwise
    // the (earlier) discovery refusal masks the exit path.
    if otelite::commands::service::local_otelite_pid(4317).unwrap().is_some() {
        return;
    }

    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    let storage = data_dir.join("otelite.db");

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
        assert!(
            !pid_file.exists(),
            "the corrupt PID file must be removed"
        );
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
    // Only meaningful when the OTLP ports are free; otherwise a throwaway
    // serve could not start at all.
    if TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), 4317)).is_ok()
        || TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), 4318)).is_ok()
    {
        return;
    }

    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
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

    // The dashboard listener must be released.
    let freed = TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), dashboard_port))
        .is_err();
    assert!(freed, "dashboard port must be released after SIGTERM");
    let _ = TcpListener::bind("127.0.0.1:4317");
    let _ = TcpListener::bind("127.0.0.1:4318");
}

/// M17 regression: the daemon's log must go through the daily-rotating
/// appender (`--log-file`), not an ever-growing plain file. A throwaway
/// `serve` must produce a dated log file in its data dir.
#[test]
fn test_serve_writes_rotating_log_file() {
    // Only meaningful when the OTLP ports are free.
    if TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), 4317)).is_ok()
        || TcpStream::connect((std::net::IpAddr::from([127, 0, 0, 1]), 4318)).is_ok()
    {
        return;
    }

    let temp = tempfile::TempDir::new().unwrap();
    let data_dir = temp.path().join("data");
    let storage = data_dir.join("otelite.db");
    let dashboard_port = free_port();

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
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();

    wait_for_port(dashboard_port, Instant::now() + Duration::from_secs(15));
    // Give the non-blocking appender a moment to flush the startup lines.
    std::thread::sleep(Duration::from_secs(1));

    // The rotating appender names its file otelite.log.YYYY-MM-DD.
    let rotated = std::fs::read_dir(&data_dir)
        .expect("data dir exists")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name())
        .find(|name| {
            name.to_string_lossy()
                .starts_with("otelite.log.")
        });
    assert!(
        rotated.is_some(),
        "expected a dated otelite.log.* file; data dir contents: {:?}",
        std::fs::read_dir(&data_dir)
            .map(|d| d.map(|e| e.unwrap().file_name()).collect::<Vec<_>>())
            .unwrap_or_default()
    );
    let rotated_path = data_dir.join(rotated.unwrap());
    assert!(
        std::fs::metadata(&rotated_path).unwrap().len() > 0,
        "the startup log lines must have been flushed"
    );

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
    let _ = TcpListener::bind("127.0.0.1:4317");
    let _ = TcpListener::bind("127.0.0.1:4318");
}
