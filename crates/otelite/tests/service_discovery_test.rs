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

use nix::sys::signal::{kill, Signal};
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

    // Clean up: graceful SIGTERM, bounded wait.
    kill(Pid::from_raw(pid as i32), Signal::SIGTERM).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match child.try_wait().unwrap() {
            Some(_) => break,
            None => {
                assert!(Instant::now() < deadline, "serve did not exit on SIGTERM");
                std::thread::sleep(Duration::from_millis(100));
            },
        }
    }
    let _ = TcpListener::bind("127.0.0.1:4317");
}
