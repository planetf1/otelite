//! Service management commands for running otelite as a background daemon

use crate::error::{Error, Result};
use otelite_storage::StorageConfig;
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tracing::{info, warn};

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
    cmd.arg("serve").arg("--addr").arg(&addr);
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
    write_pid(pid)?;

    let storage_display = storage_path
        .as_deref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| StorageConfig::default_data_dir().display().to_string());

    println!("✓ Otelite daemon started with PID {}", pid);
    println!("  Logs: {}", log_file.display());
    println!("  Storage: {}", storage_display);
    println!("  Dashboard: http://{}", addr);
    println!("\nUse 'otelite stop' to stop the daemon");
    println!("Use 'otelite status' to check daemon status");

    Ok(())
}

/// Stop the otelite daemon
pub async fn handle_stop() -> Result<()> {
    let pid = read_pid()?.ok_or_else(|| {
        Error::ConfigError("Otelite daemon is not running (no PID file found)".to_string())
    })?;

    if !is_process_running(pid) {
        warn!("PID file exists but process is not running, cleaning up");
        remove_pid_file()?;
        return Err(Error::ConfigError(
            "Otelite daemon is not running".to_string(),
        ));
    }

    info!("Stopping otelite daemon (PID {})...", pid);

    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;

        // Send SIGTERM for graceful shutdown
        kill(Pid::from_raw(pid as i32), Signal::SIGTERM)
            .map_err(|e| Error::ConfigError(format!("Failed to send SIGTERM to process: {}", e)))?;

        // Wait for process to exit (with timeout)
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(10);

        while is_process_running(pid) {
            if start.elapsed() > timeout {
                warn!("Process did not exit gracefully, sending SIGKILL");
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

    remove_pid_file()?;
    println!("✓ Otelite daemon stopped");

    Ok(())
}

/// Stop the running daemon and start a fresh one
pub async fn handle_restart(storage_path: Option<PathBuf>, addr: String) -> Result<()> {
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
    handle_start(storage_path, addr).await
}

/// Show the status of the otelite daemon
pub async fn handle_status() -> Result<()> {
    let pid = match read_pid()? {
        Some(pid) => pid,
        None => {
            println!("Status: Not running");
            return Ok(());
        },
    };

    if is_process_running(pid) {
        println!("Status: Running");
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

        // Show log file location
        let log_file = get_log_file()?;
        println!("Logs: {}", log_file.display());

        // Show storage location from PID file directory
        let runtime_dir = get_runtime_dir()?;
        println!("Runtime directory: {}", runtime_dir.display());
    } else {
        println!("Status: Not running (stale PID file)");
        warn!("Cleaning up stale PID file");
        remove_pid_file()?;
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
