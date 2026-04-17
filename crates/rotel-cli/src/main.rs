//! Rotel CLI - OpenTelemetry receiver and dashboard

use clap::Parser;
use rotel_dashboard::{DashboardConfig, DashboardServer};
use rotel_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

#[derive(Parser, Debug)]
#[command(name = "rotel")]
#[command(about = "Lightweight OpenTelemetry receiver and dashboard", long_about = None)]
struct Cli {
    /// Enable the dashboard
    #[arg(long, default_value_t = true)]
    dashboard: bool,

    /// Dashboard bind address
    #[arg(long, default_value = "127.0.0.1:3000")]
    dashboard_addr: SocketAddr,

    /// Storage database path
    #[arg(long, default_value = "rotel.db")]
    storage_path: String,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info")]
    log_level: String,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // Initialize tracing
    let level = match cli.log_level.to_lowercase().as_str() {
        "trace" => Level::TRACE,
        "debug" => Level::DEBUG,
        "info" => Level::INFO,
        "warn" => Level::WARN,
        "error" => Level::ERROR,
        _ => Level::INFO,
    };

    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();

    tracing::subscriber::set_global_default(subscriber).expect("Failed to set tracing subscriber");

    info!("Starting Rotel...");

    // Initialize storage backend
    info!("Initializing storage at {}", cli.storage_path);
    let storage_config = StorageConfig::default().with_data_dir(PathBuf::from(&cli.storage_path));

    let mut storage = SqliteBackend::new(storage_config);
    storage.initialize().await?;
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    info!("Storage initialized successfully");

    // Start dashboard if enabled
    if cli.dashboard {
        info!("Dashboard enabled at http://{}", cli.dashboard_addr);

        let config = DashboardConfig::default()
            .with_bind_address(cli.dashboard_addr)
            .with_storage_path(cli.storage_path);

        let server = DashboardServer::new(config, storage);
        server.start().await?;
    } else {
        info!("Dashboard disabled");
        // Keep the process running
        tokio::signal::ctrl_c().await?;
    }

    Ok(())
}

// Made with Bob
