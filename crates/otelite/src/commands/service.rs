//! Service management commands for running otelite as a background daemon

use crate::error::{Error, Result};
use otelite_storage::StorageConfig;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::{info, warn};

#[cfg(target_os = "macos")]
const LAUNCHD_SERVICE_LABEL: &str = "dev.otelite.daemon";

#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq, Eq)]
enum LaunchdServiceState {
    Loaded,
    Running(u32),
}

/// Get the directory for otelite runtime files (PID, logs, database).
///
/// `OTELITE_DATA_DIR` isolates the runtime files together with the
/// database — the same variable the storage layer honours — so a
/// second otelite instance (or a test) can run without touching the
/// default instance's PID file or log.
fn get_runtime_dir() -> Result<PathBuf> {
    let runtime_dir = match std::env::var("OTELITE_DATA_DIR") {
        Ok(dir) if !dir.trim().is_empty() => PathBuf::from(dir),
        _ => StorageConfig::default_data_dir(),
    };

    if !runtime_dir.exists() {
        fs::create_dir_all(&runtime_dir).map_err(|e| {
            Error::ConfigError(format!("Failed to create runtime directory: {}", e))
        })?;
    }

    Ok(runtime_dir)
}

/// The OTLP gRPC port the local daemon is expected to listen on.
/// Honours `OTELITE_OTLP_GRPC_PORT` (the same override `serve` uses to
/// bind) so discovery and stop target the right port for non-standard
/// setups; falls back to the standard 4317.
pub fn otlp_grpc_port() -> u16 {
    std::env::var("OTELITE_OTLP_GRPC_PORT")
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(4317)
}

/// Get the path to the PID file
fn get_pid_file() -> Result<PathBuf> {
    Ok(get_runtime_dir()?.join("otelite.pid"))
}

/// Get the path to the log file
fn get_log_file() -> Result<PathBuf> {
    Ok(get_runtime_dir()?.join("otelite.log"))
}

/// Read the PID from a PID file.
///
/// A missing file means "not started via `otelite start`". Corrupt
/// content (torn write, stale garbage) means the same, except the file
/// is removed so it cannot trip every subsequent command — port-based
/// discovery still finds a live daemon.
pub fn read_pid_file(pid_file: &Path) -> Result<Option<u32>> {
    if !pid_file.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(pid_file)
        .map_err(|e| Error::ConfigError(format!("Failed to read PID file: {}", e)))?;

    match content.trim().parse::<u32>() {
        Ok(pid) if pid != 0 => Ok(Some(pid)),
        _ => {
            warn!("Corrupt PID file at {}, removing it", pid_file.display());
            if let Err(e) = fs::remove_file(pid_file) {
                warn!(
                    "Could not remove corrupt PID file {}: {}",
                    pid_file.display(),
                    e
                );
            }
            Ok(None)
        },
    }
}

/// Read the PID from the PID file
fn read_pid() -> Result<Option<u32>> {
    read_pid_file(&get_pid_file()?)
}

/// Write the PID to a PID file atomically (temp file + fsync + rename).
///
/// A crash mid-write must not leave a torn PID file behind: the rename
/// makes the file appear either with the old or the new content, never
/// half-written.
pub fn write_pid_file(pid: u32, pid_file: &Path) -> Result<()> {
    let tmp_file = pid_file.with_extension("pid.tmp");

    {
        let mut file = fs::File::create(&tmp_file)
            .map_err(|e| Error::ConfigError(format!("Failed to create PID file: {}", e)))?;

        file.write_all(pid.to_string().as_bytes())
            .map_err(|e| Error::ConfigError(format!("Failed to write PID file: {}", e)))?;

        file.sync_all()
            .map_err(|e| Error::ConfigError(format!("Failed to sync PID file: {}", e)))?;
    }

    fs::rename(&tmp_file, pid_file).map_err(|e| {
        let _ = fs::remove_file(&tmp_file);
        Error::ConfigError(format!("Failed to move PID file into place: {}", e))
    })
}

/// Write the PID to the PID file
fn write_pid(pid: u32) -> Result<()> {
    write_pid_file(pid, &get_pid_file()?)
}

/// Remove the PID file
fn remove_pid_file() -> Result<()> {
    let pid_file = get_pid_file()?;

    if pid_file.exists() {
        fs::remove_file(&pid_file)
            .map_err(|e| Error::ConfigError(format!("Failed to remove PID file: {}", e)))?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn parse_launchd_service_state(output: &str) -> LaunchdServiceState {
    let is_running = output.lines().any(|line| line.trim() == "state = running");
    let pid = output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("pid = ")
            .and_then(|value| value.parse::<u32>().ok())
    });

    match (is_running, pid) {
        (true, Some(pid)) => LaunchdServiceState::Running(pid),
        _ => LaunchdServiceState::Loaded,
    }
}

#[cfg(target_os = "macos")]
fn launchd_service_target() -> String {
    use nix::unistd::getuid;

    format!("gui/{}/{}", getuid().as_raw(), LAUNCHD_SERVICE_LABEL)
}

#[cfg(target_os = "macos")]
fn launchd_service_state() -> Result<Option<LaunchdServiceState>> {
    let service_target = launchd_service_target();
    let output = Command::new("launchctl")
        .args(["print", &service_target])
        .output()
        .map_err(|e| Error::ConfigError(format!("Failed to query launchd service: {}", e)))?;

    if !output.status.success() {
        return Ok(None);
    }

    Ok(Some(parse_launchd_service_state(&String::from_utf8_lossy(
        &output.stdout,
    ))))
}

#[cfg(target_os = "macos")]
fn stop_launchd_service() -> Result<()> {
    let service_target = launchd_service_target();
    let output = Command::new("launchctl")
        .args(["bootout", &service_target])
        .output()
        .map_err(|e| Error::ConfigError(format!("Failed to stop launchd service: {}", e)))?;

    if output.status.success() {
        return Ok(());
    }

    Err(Error::ConfigError(format!(
        "Failed to stop launchd service {}: {}",
        LAUNCHD_SERVICE_LABEL,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

#[cfg(target_os = "macos")]
fn restart_launchd_service() -> Result<()> {
    let service_target = launchd_service_target();
    let output = Command::new("launchctl")
        .args(["kickstart", "-k", &service_target])
        .output()
        .map_err(|e| Error::ConfigError(format!("Failed to restart launchd service: {}", e)))?;

    if output.status.success() {
        return Ok(());
    }

    Err(Error::ConfigError(format!(
        "Failed to restart launchd service {}: {}",
        LAUNCHD_SERVICE_LABEL,
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

#[cfg(unix)]
fn is_otelite_command(command: &str) -> bool {
    Path::new(command.trim())
        .file_name()
        .is_some_and(|name| name == "otelite")
}

#[cfg(unix)]
fn is_otelite_process(pid: u32) -> Result<bool> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map_err(|e| Error::ConfigError(format!("Failed to inspect local process: {}", e)))?;

    Ok(output.status.success() && is_otelite_command(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(unix)]
fn ensure_otelite_process(pid: u32) -> Result<()> {
    if is_otelite_process(pid)? {
        return Ok(());
    }

    Err(Error::ConfigError(format!(
        "Otelite process {} exited or was replaced; refusing to signal it",
        pid
    )))
}

#[cfg(unix)]
fn pid_file_otelite_pid() -> Result<Option<u32>> {
    let Some(pid) = read_pid()? else {
        return Ok(None);
    };

    if is_process_running(pid) && is_otelite_process(pid)? {
        Ok(Some(pid))
    } else {
        Ok(None)
    }
}

/// Discover the otelite process listening on the given TCP port (default
/// OTLP gRPC: 4317). This is how `status`/`stop` find daemons started
/// without `otelite start` (no PID file): service-managed `serve` and
/// hand-run `serve` alike (issue #107).
#[cfg(unix)]
pub fn local_otelite_pid(port: u16) -> Result<Option<u32>> {
    let output = Command::new("lsof")
        .args(["-nP", "-t", &format!("-iTCP:{port}"), "-sTCP:LISTEN"])
        .output()
        .map_err(|e| {
            Error::ConfigError(format!("Failed to discover local otelite process: {}", e))
        })?;

    if !output.status.success() {
        return Ok(None);
    }

    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Ok(pid) = line.parse::<u32>() else {
            continue;
        };

        if is_otelite_process(pid)? {
            return Ok(Some(pid));
        }
    }

    Ok(None)
}

/// Check if a process with the given PID is running
fn is_process_running(pid: u32) -> bool {
    #[cfg(unix)]
    {
        use nix::sys::signal::kill;
        use nix::unistd::Pid;

        // Send signal 0 to check if process exists without delivering a signal
        match kill(Pid::from_raw(pid as i32), None) {
            Ok(_) => true,
            Err(nix::errno::Errno::ESRCH) => false, // No such process
            Err(_) => true, // Process exists but we can't signal it (permission issue)
        }
    }

    #[cfg(not(unix))]
    {
        // On non-Unix systems, just check if PID file exists
        // This is a fallback and not as reliable
        warn!("Process check not fully supported on this platform");
        true
    }
}

/// Start otelite as a background daemon
pub async fn handle_start(storage_path: Option<PathBuf>, addr: String) -> Result<()> {
    if let Some(pid) = read_pid()? {
        if is_process_running(pid) {
            return Err(Error::ConfigError(format!(
                "Otelite is already running with PID {}",
                pid
            )));
        } else {
            warn!("Stale PID file found, removing it");
            remove_pid_file()?;
        }
    }

    // The PID file says no (or said so and went stale), but a daemon
    // started by launchd, a hand-run `serve`, or `otelite start` from
    // another data dir leaves no PID file here. Discover it the same
    // way `status` does, instead of spawning a second daemon that can
    // only fail to bind the OTLP ports.
    #[cfg(unix)]
    let otlp_port = otlp_grpc_port();
    #[cfg(unix)]
    if let Some(pid) = local_otelite_pid(otlp_port)? {
        return Err(Error::ConfigError(format!(
            "Otelite is already running with PID {pid} (discovered via OTLP gRPC port {otlp_port})"
        )));
    }

    info!("Starting otelite daemon...");

    let exe_path = std::env::current_exe()
        .map_err(|e| Error::ConfigError(format!("Failed to get executable path: {}", e)))?;

    let log_file = get_log_file()?;

    let log_file_handle = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .map_err(|e| Error::ConfigError(format!("Failed to open log file: {}", e)))?;

    let args = daemon_command_args(&addr, &log_file, &storage_path);
    let mut cmd = Command::new(&exe_path);
    for arg in &args {
        cmd.arg(arg);
    }
    let mut child =
        cmd.stdin(Stdio::null())
            .stdout(log_file_handle.try_clone().map_err(|e| {
                Error::ConfigError(format!("Failed to clone log file handle: {}", e))
            })?)
            .stderr(log_file_handle)
            .spawn()
            .map_err(|e| Error::ConfigError(format!("Failed to spawn daemon process: {}", e)))?;

    let pid = child.id();
    write_pid(pid)?;

    // A port collision makes `serve` exit within milliseconds. Confirm
    // the child is still alive before reporting success, and roll back
    // the PID file if it died — "started with PID X" must never be
    // printed for a process that is already gone.
    // A port collision makes `serve` exit within milliseconds. Confirm
    // the child is still alive before reporting success, and roll back
    // the PID file if it died — "started with PID X" must never be
    // printed for a process that is already gone.
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(750);
    let mut exited = None;
    while exited.is_none() {
        exited = child
            .try_wait()
            .map_err(|e| Error::ConfigError(format!("Failed to check daemon process: {}", e)))?;
        if exited.is_none() && std::time::Instant::now() >= deadline {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    if let Some(status) = exited {
        remove_pid_file()?;
        return Err(Error::ConfigError(format!(
            "Daemon exited immediately after start ({status}); see the log at {}",
            log_file.display()
        )));
    }

    let storage_display = storage_path
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| StorageConfig::default_data_dir().display().to_string());

    println!("✓ Otelite daemon started with PID {}", pid);
    println!("  Logs: {}.* (rotates daily)", log_file.display());
    println!("  Storage: {}", storage_display);
    println!("  Dashboard: http://{}", addr);
    println!("\nUse 'otelite stop' to stop the daemon");
    println!("Use 'otelite status' to check daemon status");

    Ok(())
}

/// Build the argument list for a spawned daemon. Factored out so the
/// wiring (in particular the daily-rotating `--log-file`) is testable
/// without spawning anything.
fn daemon_command_args(
    addr: &str,
    log_file: &Path,
    storage_path: &Option<PathBuf>,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = vec![
        "serve".into(),
        "--addr".into(),
        addr.into(),
        // Route the child's tracing through the daily-rotating appender
        // instead of an ever-growing appended file. The stderr
        // redirection in `handle_start` is kept for output that happens
        // before/around tracing (panics, startup errors); it is
        // small-volume.
        "--log-file".into(),
        log_file.as_os_str().to_os_string(),
    ];
    if let Some(path) = storage_path {
        args.push("--storage-path".into());
        args.push(path.as_os_str().to_os_string());
    }
    args
}

/// Stop the otelite daemon
pub async fn handle_stop() -> Result<()> {
    #[cfg(target_os = "macos")]
    if launchd_service_state()?.is_some() {
        stop_launchd_service()?;
        println!("✓ Otelite launchd service stopped");
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    let pid = pid_file_otelite_pid()?
        .or(local_otelite_pid(otlp_grpc_port())?)
        .ok_or_else(|| Error::ConfigError("Otelite daemon is not running".to_string()))?;

    #[cfg(not(target_os = "macos"))]
    let pid = {
        let mut pid = pid_file_otelite_pid()?;
        if pid.is_none() {
            pid = local_otelite_pid(otlp_grpc_port())?;
        }
        match pid {
            Some(pid) => {
                if let Some(file_pid) = read_pid()? {
                    if file_pid != pid {
                        warn!(
                            "PID file names {} but the listening daemon is {}; stopping the daemon",
                            file_pid, pid
                        );
                    }
                }
                pid
            },
            None => {
                if read_pid()?.is_some() {
                    warn!("Stale PID file found, removing it");
                    remove_pid_file()?;
                }
                return Err(Error::ConfigError(
                    "Otelite daemon is not running (no PID file and nothing listening on 4317)"
                        .to_string(),
                ));
            },
        }
    };

    info!("Stopping otelite daemon (PID {})...", pid);

    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        // Send SIGTERM for graceful shutdown
        #[cfg(unix)]
        ensure_otelite_process(pid)?;
        kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
            .map_err(|e| Error::ConfigError(format!("Failed to send SIGTERM to process: {}", e)))?;

        // Wait for process to exit (with timeout).
        //
        // Every iteration re-checks the process *identity*, not just its
        // existence: once otelite exits, its PID can be recycled by an
        // unrelated process. A plain `kill(pid, 0)` liveness check would
        // report that stranger as "still running", stall the loop until
        // the timeout, and then report a failed stop for a stop that had
        // already succeeded. `is_otelite_process` returns false for a
        // dead PID and for a live non-otelite process alike — both mean
        // the original daemon is gone.
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(10);

        loop {
            if !is_otelite_process(pid)? {
                break;
            }
            if start.elapsed() > timeout {
                warn!("Process did not exit gracefully, sending SIGKILL");
                // Re-verify identity immediately before the forceful
                // kill — never signal a recycled PID.
                if is_otelite_process(pid)? {
                    kill(Pid::from_raw(pid as i32), Signal::SIGKILL).map_err(|e| {
                        Error::ConfigError(format!("Failed to send SIGKILL to process: {}", e))
                    })?;
                }
                break;
            }
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        }
    }

    #[cfg(not(unix))]
    {
        return Err(Error::ConfigError(
            "Stop command not supported on this platform".to_string(),
        ));
    }

    if read_pid()? == Some(pid) {
        remove_pid_file()?;
    }
    println!("✓ Otelite daemon stopped");

    Ok(())
}

/// Stop the running daemon and start a fresh one
pub async fn handle_restart(storage_path: Option<PathBuf>, addr: String) -> Result<()> {
    #[cfg(target_os = "macos")]
    if launchd_service_state()?.is_some() {
        restart_launchd_service()?;
        println!("✓ Otelite launchd service restarted");
        return Ok(());
    }

    // Verify a daemon is actually running before attempting restart
    #[cfg(unix)]
    {
        let running = read_pid().ok().flatten().is_some_and(is_process_running)
            || local_otelite_pid(otlp_grpc_port()).ok().flatten().is_some();
        if !running {
            return Err(Error::ConfigError(
                "No otelite daemon is running. Use 'otelite start' to start one.".to_string(),
            ));
        }
    }
    #[cfg(not(unix))]
    match read_pid()? {
        Some(pid) if is_process_running(pid) => {},
        _ => {
            return Err(Error::ConfigError(
                "No otelite daemon is running. Use 'otelite start' to start one.".to_string(),
            ));
        },
    }

    println!("Stopping daemon...");
    handle_stop().await?;

    println!("Daemon stopped. Starting fresh...");
    handle_start(storage_path, addr).await
}

fn display_running_status(pid: u32, supervisor: Option<&str>) -> Result<()> {
    match supervisor {
        Some(supervisor) => println!("Status: Running ({})", supervisor),
        None => println!("Status: Running"),
    }
    println!("PID: {}", pid);

    // Try to get process uptime on Unix systems
    #[cfg(unix)]
    {
        if let Ok(output) = Command::new("ps")
            .args(["-p", &pid.to_string(), "-o", "etime="])
            .output()
        {
            if output.status.success() {
                if let Ok(uptime) = String::from_utf8(output.stdout) {
                    println!("Uptime: {}", uptime.trim());
                }
            }
        }
    }

    let log_file = get_log_file()?;
    println!("Logs: {}.* (rotates daily)", log_file.display());

    let runtime_dir = get_runtime_dir()?;
    println!("Runtime directory: {}", runtime_dir.display());

    Ok(())
}

/// Show the status of the otelite daemon
pub async fn handle_status() -> Result<()> {
    #[cfg(target_os = "macos")]
    if let Some(LaunchdServiceState::Running(pid)) = launchd_service_state()? {
        return display_running_status(pid, Some("launchd: dev.otelite.daemon"));
    }

    #[cfg(target_os = "macos")]
    if let Some(pid) = pid_file_otelite_pid()? {
        return display_running_status(pid, Some("local process"));
    }

    #[cfg(target_os = "macos")]
    if let Some(pid) = local_otelite_pid(otlp_grpc_port())? {
        return display_running_status(pid, Some("local process"));
    }

    #[cfg(not(target_os = "macos"))]
    {
        let mut pid = pid_file_otelite_pid()?;
        if pid.is_none() {
            pid = local_otelite_pid(otlp_grpc_port())?;
        }
        if let Some(pid) = pid {
            return display_running_status(pid, None);
        }
        if read_pid()?.is_some() {
            println!("Status: Not running (stale PID file)");
            warn!("Cleaning up stale PID file");
            remove_pid_file()?;
        } else {
            println!("Status: Not running");
        }
    }

    #[cfg(target_os = "macos")]
    if read_pid()?.is_some() {
        println!("Status: Not running (stale PID file)");
        warn!("Cleaning up stale PID file");
        remove_pid_file()?;
    } else {
        println!("Status: Not running");
    }

    Ok(())
}

/// Install otelite as a system service
pub async fn handle_service_install() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        install_launchd_service().await
    }

    #[cfg(target_os = "linux")]
    {
        install_systemd_service().await
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(Error::ConfigError(
            "Service installation not supported on this platform".to_string(),
        ))
    }
}

/// Install otelite as a launchd service on macOS
#[cfg(target_os = "macos")]
async fn install_launchd_service() -> Result<()> {
    let home = std::env::var("HOME")
        .map_err(|_| Error::ConfigError("HOME environment variable not set".to_string()))?;

    let launch_agents_dir = PathBuf::from(&home).join("Library/LaunchAgents");

    if !launch_agents_dir.exists() {
        fs::create_dir_all(&launch_agents_dir).map_err(|e| {
            Error::ConfigError(format!("Failed to create LaunchAgents directory: {}", e))
        })?;
    }

    let plist_path = launch_agents_dir.join("dev.otelite.daemon.plist");
    let exe_path = std::env::current_exe()
        .map_err(|e| Error::ConfigError(format!("Failed to get executable path: {}", e)))?;

    let log_file = get_log_file()?;

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>dev.otelite.daemon</string>
    <key>ProgramArguments</key>
    <array>
        <string>{}</string>
        <string>serve</string>
        <string>--log-file</string>
        <string>{}</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}</string>
    <key>StandardErrorPath</key>
    <string>{}</string>
</dict>
</plist>
"#,
        exe_path.display(),
        log_file.display(),
        log_file.display(),
        log_file.display()
    );

    fs::write(&plist_path, plist_content)
        .map_err(|e| Error::ConfigError(format!("Failed to write plist file: {}", e)))?;

    println!(
        "✓ Service configuration created at {}",
        plist_path.display()
    );
    println!("\nTo enable the service, run:");
    println!("  launchctl load {}", plist_path.display());
    println!("\nTo disable the service, run:");
    println!("  launchctl unload {}", plist_path.display());

    Ok(())
}

/// Install otelite as a systemd service on Linux
#[cfg(target_os = "linux")]
async fn install_systemd_service() -> Result<()> {
    let home = std::env::var("HOME")
        .map_err(|_| Error::ConfigError("HOME environment variable not set".to_string()))?;

    let systemd_user_dir = PathBuf::from(&home).join(".config/systemd/user");

    // Create directory if it doesn't exist
    if !systemd_user_dir.exists() {
        fs::create_dir_all(&systemd_user_dir).map_err(|e| {
            Error::ConfigError(format!("Failed to create systemd user directory: {}", e))
        })?;
    }

    let unit_path = systemd_user_dir.join("otelite.service");
    let exe_path = std::env::current_exe()
        .map_err(|e| Error::ConfigError(format!("Failed to get executable path: {}", e)))?;

    let log_file = get_log_file()?;

    let unit_content = format!(
        r#"[Unit]
Description=Otelite OpenTelemetry Collector
After=network.target

[Service]
Type=simple
ExecStart={} serve --log-file {}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        exe_path.display(),
        log_file.display()
    );

    fs::write(&unit_path, unit_content)
        .map_err(|e| Error::ConfigError(format!("Failed to write systemd unit file: {}", e)))?;

    println!("✓ Service configuration created at {}", unit_path.display());
    println!("\nTo enable and start the service, run:");
    println!("  systemctl --user daemon-reload");
    println!("  systemctl --user enable otelite.service");
    println!("  systemctl --user start otelite.service");
    println!("\nTo check service status:");
    println!("  systemctl --user status otelite.service");
    println!("\nTo disable the service:");
    println!("  systemctl --user stop otelite.service");
    println!("  systemctl --user disable otelite.service");

    Ok(())
}

#[cfg(test)]
#[cfg(unix)]
mod tests {
    #[cfg(target_os = "macos")]
    use super::parse_launchd_service_state;
    use super::{is_process_running, local_otelite_pid, otlp_grpc_port};

    /// #107 regression helper: discovery must report `None` (not an error)
    /// when nothing otelite-shaped listens, and report a live PID when the
    /// real daemon owns the port. Both outcomes are asserted consistently
    /// with process liveness so the test holds on dev machines (daemon
    /// running) and CI (no daemon) alike.
    #[test]
    fn test_local_otelite_pid_consistent_with_liveness() {
        if let Some(pid) = local_otelite_pid(otlp_grpc_port()).unwrap() {
            assert!(is_process_running(pid));
        }
    }

    #[cfg(target_os = "macos")]
    use super::LaunchdServiceState;

    #[cfg(target_os = "macos")]
    #[test]
    fn test_parse_launchd_service_state_detects_running_service() {
        let output = r#"
gui/501/dev.otelite.daemon = {
    state = running
    pid = 7351
}
"#;

        assert_eq!(
            parse_launchd_service_state(output),
            LaunchdServiceState::Running(7351)
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_parse_launchd_service_state_detects_loaded_non_running_service() {
        let output = r#"
gui/501/dev.otelite.daemon = {
    state = spawn scheduled
    pid = 7351
}
"#;

        assert_eq!(
            parse_launchd_service_state(output),
            LaunchdServiceState::Loaded
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn test_is_otelite_command_accepts_only_otelite_executable() {
        assert!(super::is_otelite_command(
            "/Users/jonesn/.local/bin/otelite"
        ));
        assert!(!super::is_otelite_command("/usr/bin/python3"));
    }

    #[test]
    fn test_is_otelite_command_trims_and_rejects_similar_names() {
        // `ps` output can carry trailing whitespace; the name match must
        // be exact (no `otelite-old`, `otelite-cli`, ...).
        assert!(super::is_otelite_command("otelite"));
        assert!(super::is_otelite_command("otelite "));
        assert!(!super::is_otelite_command("otelite-old"));
        assert!(!super::is_otelite_command("/opt/homebrew/bin/otelite-cli"));
    }

    #[test]
    fn test_is_process_running_distinguishes_live_and_dead_pids() {
        // PID 1 (launchd on macOS, init/systemd on Linux) always exists.
        assert!(is_process_running(1));
        // Above both Linux's pid_max (4194304) and macOS's kern.pids_max
        // (99999), and positive as i32 — `u32::MAX` would cast to -1 and
        // `kill(-1, 0)` would report the whole process group as alive.
        assert!(!is_process_running(2_000_000_000));
    }

    #[test]
    fn test_is_otelite_process_is_false_for_dead_and_foreign_pids() {
        // The stop wait loop relies on this returning `Ok(false)` — not
        // an error — both for a dead PID and for a live process that is
        // not otelite (i.e. a recycled PID).
        assert!(
            !super::is_otelite_process(2_000_000_000).unwrap(),
            "a dead PID must read as 'not otelite', not as a failure"
        );

        let mut sleep = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        assert!(
            !super::is_otelite_process(sleep.id()).unwrap(),
            "a live non-otelite process (e.g. a recycled PID) must read as 'not otelite'"
        );
        let _ = sleep.kill();
        let _ = sleep.wait();
    }

    /// Environment-mutating tests share the process environment, so they
    /// must not run concurrently with each other.
    static ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_otlp_grpc_port_env_override() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let key = "OTELITE_OTLP_GRPC_PORT";
        let old = std::env::var(key).ok();

        std::env::remove_var(key);
        assert_eq!(otlp_grpc_port(), 4317);

        std::env::set_var(key, "14317");
        assert_eq!(otlp_grpc_port(), 14317);

        // Discovery must stay usable on a misconfigured machine: an
        // invalid value falls back to the standard port rather than
        // failing.
        std::env::set_var(key, "not-a-port");
        assert_eq!(otlp_grpc_port(), 4317);

        match old {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn test_runtime_dir_and_pid_file_wrappers_follow_env() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let key = "OTELITE_DATA_DIR";
        let old = std::env::var(key).ok();

        let temp = tempfile::TempDir::new().unwrap();
        std::env::set_var(key, temp.path());

        // PID file and log file live in the (env-overridden) data dir.
        assert_eq!(super::get_runtime_dir().unwrap(), temp.path());
        assert_eq!(
            super::get_pid_file().unwrap(),
            temp.path().join("otelite.pid")
        );

        // Wrapper round-trip, including removal (which is a no-op when
        // the file is missing).
        assert_eq!(super::read_pid().unwrap(), None);
        assert!(super::remove_pid_file().is_ok());
        super::write_pid(4321).unwrap();
        assert_eq!(super::read_pid().unwrap(), Some(4321));
        super::remove_pid_file().unwrap();
        assert_eq!(super::read_pid().unwrap(), None);

        match old {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn test_get_runtime_dir_defaults_without_env() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let key = "OTELITE_DATA_DIR";
        let old = std::env::var(key).ok();
        std::env::remove_var(key);

        let dir = super::get_runtime_dir().unwrap();
        assert!(
            dir.ends_with(".otelite/data"),
            "default runtime dir must be under ~/.otelite/data, got {dir:?}"
        );

        match old {
            Some(value) => std::env::set_var(key, value),
            None => std::env::remove_var(key),
        }
    }
}

#[cfg(test)]
mod pid_file_tests {
    use super::*;

    #[test]
    fn test_read_pid_file_corrupt_is_removed_and_treated_as_missing() {
        let temp = tempfile::TempDir::new().unwrap();
        let pid_file = temp.path().join("otelite.pid");
        std::fs::write(&pid_file, "not-a-pid\n").unwrap();

        assert_eq!(read_pid_file(&pid_file).unwrap(), None);
        assert!(
            !pid_file.exists(),
            "the corrupt file must be removed so it cannot clog later commands"
        );
    }

    #[test]
    fn test_read_pid_file_zero_pid_treated_as_missing() {
        let temp = tempfile::TempDir::new().unwrap();
        let pid_file = temp.path().join("otelite.pid");
        std::fs::write(&pid_file, "0").unwrap();

        assert_eq!(read_pid_file(&pid_file).unwrap(), None);
        assert!(!pid_file.exists());
    }

    #[test]
    fn test_read_pid_file_missing_is_none() {
        let temp = tempfile::TempDir::new().unwrap();
        assert_eq!(
            read_pid_file(&temp.path().join("otelite.pid")).unwrap(),
            None
        );
    }
}

#[cfg(test)]
mod daemon_args_tests {
    use super::*;

    #[test]
    fn test_daemon_command_args_route_logs_through_rotating_appender() {
        let args = daemon_command_args("127.0.0.1:3000", Path::new("/tmp/data/otelite.log"), &None);
        let flat: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            flat,
            vec![
                "serve",
                "--addr",
                "127.0.0.1:3000",
                "--log-file",
                "/tmp/data/otelite.log"
            ]
        );

        let storage = PathBuf::from("/tmp/data/otelite.db");
        let args = daemon_command_args(
            "127.0.0.1:3000",
            Path::new("/tmp/data/otelite.log"),
            &Some(storage),
        );
        let flat: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            flat,
            vec![
                "serve",
                "--addr",
                "127.0.0.1:3000",
                "--log-file",
                "/tmp/data/otelite.log",
                "--storage-path",
                "/tmp/data/otelite.db"
            ]
        );
    }
}
