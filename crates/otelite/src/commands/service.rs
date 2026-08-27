//! Service management commands for running otelite as a background daemon

use crate::error::{Error, Result};
use otelite_storage::StorageConfig;
use std::fs;
use std::io::Write;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tracing::{info, warn};

#[cfg(target_os = "macos")]
const LAUNCHD_SERVICE_LABEL: &str = "dev.otelite.daemon";

#[derive(Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
struct DaemonState {
    dashboard_addr: SocketAddr,
    grpc_addr: SocketAddr,
    http_addr: SocketAddr,
}

#[cfg(target_os = "macos")]
#[derive(Debug, PartialEq, Eq)]
enum LaunchdServiceState {
    Loaded,
    Running(u32),
}

/// Get the directory for otelite runtime files (PID, logs, database).
/// Delegates to StorageConfig so the path is always consistent with the server.
fn get_runtime_dir() -> Result<PathBuf> {
    let runtime_dir = StorageConfig::default_data_dir();

    if !runtime_dir.exists() {
        fs::create_dir_all(&runtime_dir).map_err(|e| {
            Error::ConfigError(format!("Failed to create runtime directory: {}", e))
        })?;
    }

    Ok(runtime_dir)
}

/// Get the path to the PID file
fn get_pid_file() -> Result<PathBuf> {
    Ok(get_runtime_dir()?.join("otelite.pid"))
}

/// Get the path to the log file
fn get_log_file() -> Result<PathBuf> {
    Ok(get_runtime_dir()?.join("otelite.log"))
}

fn get_state_file() -> Result<PathBuf> {
    Ok(get_runtime_dir()?.join("otelite-daemon.json"))
}

fn write_daemon_state(state_file: &Path, state: &DaemonState) -> Result<()> {
    let contents = serde_json::to_vec_pretty(state).map_err(|e| {
        Error::ConfigError(format!("Failed to serialize daemon endpoint state: {}", e))
    })?;
    fs::write(state_file, contents).map_err(|e| {
        Error::ConfigError(format!(
            "Failed to write daemon endpoint state at {}: {}",
            state_file.display(),
            e
        ))
    })
}

fn read_daemon_state(state_file: &Path) -> Result<Option<DaemonState>> {
    if !state_file.exists() {
        return Ok(None);
    }
    let contents = fs::read(state_file).map_err(|e| {
        Error::ConfigError(format!(
            "Failed to read daemon endpoint state at {}: {}",
            state_file.display(),
            e
        ))
    })?;
    serde_json::from_slice(&contents).map(Some).map_err(|e| {
        Error::ConfigError(format!(
            "Invalid daemon endpoint state at {}: {}",
            state_file.display(),
            e
        ))
    })
}

fn remove_daemon_state(state_file: &Path) -> Result<()> {
    if state_file.exists() {
        fs::remove_file(state_file).map_err(|e| {
            Error::ConfigError(format!(
                "Failed to remove daemon endpoint state at {}: {}",
                state_file.display(),
                e
            ))
        })?;
    }
    Ok(())
}

/// Read the PID from the PID file
fn read_pid() -> Result<Option<u32>> {
    let pid_file = get_pid_file()?;

    if !pid_file.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&pid_file)
        .map_err(|e| Error::ConfigError(format!("Failed to read PID file: {}", e)))?;

    let pid = content
        .trim()
        .parse::<u32>()
        .map_err(|e| Error::ConfigError(format!("Invalid PID in file: {}", e)))?;

    Ok(Some(pid))
}

/// Write the PID to the PID file
fn write_pid(pid: u32) -> Result<()> {
    let pid_file = get_pid_file()?;

    let mut file = fs::File::create(&pid_file)
        .map_err(|e| Error::ConfigError(format!("Failed to create PID file: {}", e)))?;

    file.write_all(pid.to_string().as_bytes())
        .map_err(|e| Error::ConfigError(format!("Failed to write PID file: {}", e)))?;

    Ok(())
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

#[cfg(target_os = "macos")]
fn is_otelite_command(command: &str) -> bool {
    Path::new(command.trim())
        .file_name()
        .is_some_and(|name| name == "otelite")
}

#[cfg(target_os = "macos")]
fn is_otelite_process(pid: u32) -> Result<bool> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "comm="])
        .output()
        .map_err(|e| Error::ConfigError(format!("Failed to inspect local process: {}", e)))?;

    Ok(output.status.success() && is_otelite_command(&String::from_utf8_lossy(&output.stdout)))
}

#[cfg(target_os = "macos")]
fn ensure_otelite_process(pid: u32) -> Result<()> {
    if is_otelite_process(pid)? {
        return Ok(());
    }

    Err(Error::ConfigError(format!(
        "Otelite process {} exited or was replaced; refusing to signal it",
        pid
    )))
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn local_otelite_pid() -> Result<Option<u32>> {
    let output = Command::new("lsof")
        .args(["-nP", "-t", "-iTCP:4317", "-sTCP:LISTEN"])
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
pub async fn handle_start(
    storage_path: Option<PathBuf>,
    addr: SocketAddr,
    grpc_addr: SocketAddr,
    http_addr: SocketAddr,
) -> Result<()> {
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

    info!("Starting otelite daemon...");

    let exe_path = std::env::current_exe()
        .map_err(|e| Error::ConfigError(format!("Failed to get executable path: {}", e)))?;

    let log_file = get_log_file()?;

    let log_file_handle = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_file)
        .map_err(|e| Error::ConfigError(format!("Failed to open log file: {}", e)))?;

    let mut cmd = Command::new(&exe_path);
    cmd.arg("serve")
        .arg("--addr")
        .arg(addr.to_string())
        .arg("--grpc-addr")
        .arg(grpc_addr.to_string())
        .arg("--http-addr")
        .arg(http_addr.to_string());
    if let Some(path) = &storage_path {
        cmd.arg("--storage-path").arg(path);
    }
    let child =
        cmd.stdin(Stdio::null())
            .stdout(log_file_handle.try_clone().map_err(|e| {
                Error::ConfigError(format!("Failed to clone log file handle: {}", e))
            })?)
            .stderr(log_file_handle)
            .spawn()
            .map_err(|e| Error::ConfigError(format!("Failed to spawn daemon process: {}", e)))?;

    let pid = child.id();
    let state_file = get_state_file()?;
    write_daemon_state(
        &state_file,
        &DaemonState {
            dashboard_addr: addr,
            grpc_addr,
            http_addr,
        },
    )?;
    write_pid(pid)?;

    let storage_display = storage_path
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| StorageConfig::default_data_dir().display().to_string());

    println!("✓ Otelite daemon started with PID {}", pid);
    println!("  Logs: {}", log_file.display());
    println!("  Storage: {}", storage_display);
    println!("  Dashboard: http://{}", addr);
    println!("  OTLP gRPC: {}", grpc_addr);
    println!("  OTLP HTTP: {}", http_addr);
    println!("\nUse 'otelite stop' to stop the daemon");
    println!("Use 'otelite status' to check daemon status");

    Ok(())
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
        .or(local_otelite_pid()?)
        .ok_or_else(|| Error::ConfigError("Otelite daemon is not running".to_string()))?;

    #[cfg(not(target_os = "macos"))]
    let pid = match read_pid()? {
        Some(pid) if is_process_running(pid) => pid,
        Some(_) => {
            warn!("PID file exists but process is not running, cleaning up");
            remove_pid_file()?;
            return Err(Error::ConfigError(
                "Otelite daemon is not running".to_string(),
            ));
        },
        None => {
            return Err(Error::ConfigError(
                "Otelite daemon is not running (no PID file found)".to_string(),
            ));
        },
    };

    info!("Stopping otelite daemon (PID {})...", pid);

    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        // Send SIGTERM for graceful shutdown
        #[cfg(target_os = "macos")]
        ensure_otelite_process(pid)?;
        kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
            .map_err(|e| Error::ConfigError(format!("Failed to send SIGTERM to process: {}", e)))?;

        // Wait for process to exit (with timeout)
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(10);

        while is_process_running(pid) {
            if start.elapsed() > timeout {
                warn!("Process did not exit gracefully, sending SIGKILL");
                #[cfg(target_os = "macos")]
                ensure_otelite_process(pid)?;
                kill(Pid::from_raw(pid as i32), Signal::SIGKILL).map_err(|e| {
                    Error::ConfigError(format!("Failed to send SIGKILL to process: {}", e))
                })?;
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
    remove_daemon_state(&get_state_file()?)?;
    println!("✓ Otelite daemon stopped");

    Ok(())
}

/// Stop the running daemon and start a fresh one
pub async fn handle_restart(
    storage_path: Option<PathBuf>,
    addr: SocketAddr,
    grpc_addr: SocketAddr,
    http_addr: SocketAddr,
) -> Result<()> {
    #[cfg(target_os = "macos")]
    if launchd_service_state()?.is_some() {
        restart_launchd_service()?;
        println!("✓ Otelite launchd service restarted");
        return Ok(());
    }

    // Verify a daemon is actually running before attempting restart
    match read_pid()? {
        None => {
            return Err(Error::ConfigError(
                "No otelite daemon is running. Use 'otelite start' to start one.".to_string(),
            ));
        },
        Some(pid) if !is_process_running(pid) => {
            return Err(Error::ConfigError(
                "No otelite daemon is running. Use 'otelite start' to start one.".to_string(),
            ));
        },
        _ => {},
    }

    println!("Stopping daemon...");
    handle_stop().await?;

    println!("Daemon stopped. Starting fresh...");
    handle_start(storage_path, addr, grpc_addr, http_addr).await
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
    println!("Logs: {}", log_file.display());

    let runtime_dir = get_runtime_dir()?;
    println!("Runtime directory: {}", runtime_dir.display());

    if let Some(state) = read_daemon_state(&get_state_file()?)? {
        println!("Dashboard: http://{}", state.dashboard_addr);
        println!("OTLP gRPC: {}", state.grpc_addr);
        println!("OTLP HTTP: {}", state.http_addr);
    }

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
    if let Some(pid) = local_otelite_pid()? {
        return display_running_status(pid, Some("local process"));
    }

    #[cfg(not(target_os = "macos"))]
    match read_pid()? {
        Some(pid) if is_process_running(pid) => return display_running_status(pid, None),
        Some(_) => warn!("PID file exists but process is not running"),
        None => {},
    }

    if read_pid()?.is_some() {
        println!("Status: Not running (stale PID file)");
        warn!("Cleaning up stale PID file");
        remove_pid_file()?;
        remove_daemon_state(&get_state_file()?)?;
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

    let unit_content = format!(
        r#"[Unit]
Description=Otelite OpenTelemetry Collector
After=network.target

[Service]
Type=simple
ExecStart={} serve
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
"#,
        exe_path.display()
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
mod tests {
    use super::{
        get_state_file, read_daemon_state, remove_daemon_state, write_daemon_state, DaemonState,
    };
    #[cfg(target_os = "macos")]
    use super::{parse_launchd_service_state, LaunchdServiceState};
    use std::net::SocketAddr;
    use tempfile::TempDir;

    #[test]
    fn daemon_state_path_uses_runtime_directory() {
        assert_eq!(
            get_state_file().unwrap().file_name().unwrap(),
            "otelite-daemon.json"
        );
    }

    #[test]
    fn daemon_state_round_trips_resolved_endpoints() {
        let temp_dir = TempDir::new().unwrap();
        let state_file = temp_dir.path().join("daemon.json");
        let state = DaemonState {
            dashboard_addr: SocketAddr::from(([127, 0, 0, 1], 3852)),
            grpc_addr: SocketAddr::from(([127, 0, 0, 1], 3850)),
            http_addr: SocketAddr::from(([127, 0, 0, 1], 3851)),
        };

        write_daemon_state(&state_file, &state).unwrap();

        assert_eq!(read_daemon_state(&state_file).unwrap(), Some(state));
    }

    #[test]
    fn removing_daemon_state_makes_it_absent() {
        let temp_dir = TempDir::new().unwrap();
        let state_file = temp_dir.path().join("daemon.json");
        let state = DaemonState {
            dashboard_addr: SocketAddr::from(([127, 0, 0, 1], 3852)),
            grpc_addr: SocketAddr::from(([127, 0, 0, 1], 3850)),
            http_addr: SocketAddr::from(([127, 0, 0, 1], 3851)),
        };
        write_daemon_state(&state_file, &state).unwrap();

        remove_daemon_state(&state_file).unwrap();

        assert_eq!(read_daemon_state(&state_file).unwrap(), None);
    }

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
}
