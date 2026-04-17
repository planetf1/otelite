//! Rotel CLI - OpenTelemetry receiver and dashboard

use clap::{Parser, Subcommand};
use rotel_dashboard::{DashboardConfig, DashboardServer};
use rotel_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

pub mod api;
pub mod commands;
pub mod config;
pub mod error;
pub mod output;

use config::{Config, OutputFormat};
use error::{Error, Result};

#[derive(Parser, Debug)]
#[command(name = "rotel")]
#[command(version)]
#[command(about = "Lightweight OpenTelemetry receiver and dashboard", long_about = None)]
struct Cli {
    /// Rotel backend endpoint URL
    #[arg(long, env = "ROTEL_ENDPOINT", global = true)]
    endpoint: Option<String>,

    /// Output format (pretty or json)
    #[arg(long, value_name = "FORMAT", global = true)]
    format: Option<OutputFormat>,

    /// Disable color output
    #[arg(long, global = true)]
    no_color: bool,

    /// Request timeout in seconds
    #[arg(long, default_value = "30", global = true)]
    timeout: u64,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info", global = true)]
    log_level: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the dashboard server (default if no subcommand)
    Dashboard {
        /// Dashboard bind address
        #[arg(long, default_value = "127.0.0.1:3000")]
        addr: SocketAddr,

        /// Storage database path
        #[arg(long, default_value = "rotel.db")]
        storage_path: String,
    },
    /// Query and display logs
    Logs {
        #[command(subcommand)]
        command: LogsCommands,
    },
    /// Query and display traces
    Traces {
        #[command(subcommand)]
        command: TracesCommands,
    },
    /// Query and display metrics
    Metrics {
        #[command(subcommand)]
        command: MetricsCommands,
    },
}

#[derive(Subcommand, Debug)]
enum LogsCommands {
    /// List recent logs
    List {
        /// Maximum number of results
        #[arg(long, short = 'n')]
        limit: Option<usize>,

        /// Filter by severity level
        #[arg(long)]
        severity: Option<String>,

        /// Filter by time range (e.g., 1h, 30m, 5s)
        #[arg(long)]
        since: Option<String>,
    },
    /// Search logs by content
    Search {
        /// Search query
        query: String,

        /// Maximum number of results
        #[arg(long, short = 'n')]
        limit: Option<usize>,
    },
    /// Show detailed log entry
    Show {
        /// Log ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum TracesCommands {
    /// List recent traces
    List {
        /// Maximum number of results
        #[arg(long, short = 'n')]
        limit: Option<usize>,

        /// Filter by minimum duration (e.g., 1s, 500ms)
        #[arg(long)]
        min_duration: Option<String>,

        /// Filter by status (OK or ERROR)
        #[arg(long)]
        status: Option<String>,

        /// Filter by time range (e.g., 1h, 30m, 5s)
        #[arg(long)]
        since: Option<String>,
    },
    /// Show detailed trace with spans
    Show {
        /// Trace ID
        id: String,
    },
}

#[derive(Subcommand, Debug)]
enum MetricsCommands {
    /// List available metrics
    List {
        /// Filter by metric type
        #[arg(long)]
        type_: Option<String>,
    },
    /// Get metric values
    Get {
        /// Metric name
        name: String,

        /// Filter by time range (e.g., 1h, 30m, 5s)
        #[arg(long)]
        since: Option<String>,

        /// Filter by label (key=value)
        #[arg(long)]
        label: Option<String>,

        /// Maximum number of data points
        #[arg(long, short = 'n')]
        limit: Option<usize>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
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
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| Error::InvalidArgument(format!("Failed to set tracing subscriber: {}", e)))?;

    // Build config from CLI args
    let config = Config {
        endpoint: cli.endpoint.unwrap_or_else(Config::endpoint_from_env),
        timeout: std::time::Duration::from_secs(cli.timeout),
        format: cli.format.unwrap_or_default(),
        no_color: cli.no_color,
    };

    // Handle commands
    match cli.command {
        Some(Commands::Dashboard { addr, storage_path }) => run_dashboard(addr, storage_path).await,
        Some(Commands::Logs { command }) => handle_logs_command(command, &config).await,
        Some(Commands::Traces { command }) => handle_traces_command(command, &config).await,
        Some(Commands::Metrics { command }) => handle_metrics_command(command, &config).await,
        None => {
            // Default: run dashboard
            run_dashboard("127.0.0.1:3000".parse().unwrap(), "rotel.db".to_string()).await
        },
    }
}

async fn run_dashboard(addr: SocketAddr, storage_path: String) -> Result<()> {
    info!("Starting Rotel Dashboard...");

    // Initialize storage backend
    info!("Initializing storage at {}", storage_path);
    let storage_config = StorageConfig::default().with_data_dir(PathBuf::from(&storage_path));

    let mut storage = SqliteBackend::new(storage_config);
    storage
        .initialize()
        .await
        .map_err(|e| Error::ApiError(format!("Failed to initialize storage: {}", e)))?;
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    info!("Storage initialized successfully");
    info!("Dashboard enabled at http://{}", addr);

    let config = DashboardConfig::default()
        .with_bind_address(addr)
        .with_storage_path(storage_path);

    let server = DashboardServer::new(config, storage);
    server
        .start()
        .await
        .map_err(|e| Error::ApiError(format!("Dashboard server error: {}", e)))?;

    Ok(())
}

async fn handle_logs_command(command: LogsCommands, config: &Config) -> Result<()> {
    use api::client::ApiClient;
    use commands::logs;

    let client = ApiClient::new(config.endpoint.clone(), config.timeout)?;

    match command {
        LogsCommands::List {
            limit,
            severity,
            since,
        } => {
            logs::handle_list(&client, config, limit, severity, since).await?;
        }
        LogsCommands::Search { query, limit } => {
            logs::handle_search(&client, config, &query, limit, None).await?;
        }
        LogsCommands::Show { id } => {
            logs::handle_show(&client, config, &id).await?;
        }
    }

    Ok(())
}

async fn handle_traces_command(_command: TracesCommands, _config: &Config) -> Result<()> {
    // Placeholder - will be implemented in Phase 4
    eprintln!("Traces commands not yet implemented");
    Err(Error::InvalidArgument(
        "Traces commands not yet implemented".to_string(),
    ))
}

async fn handle_metrics_command(_command: MetricsCommands, _config: &Config) -> Result<()> {
    // Placeholder - will be implemented in Phase 5
    eprintln!("Metrics commands not yet implemented");
    Err(Error::InvalidArgument(
        "Metrics commands not yet implemented".to_string(),
    ))
}

// Made with Bob
