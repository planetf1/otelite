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

    /// Disable table headers in output
    #[arg(long, global = true)]
    no_header: bool,

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
    /// Start the dashboard server with OTLP receivers (default if no subcommand)
    #[command(
        after_help = "Examples:\n  rotel dashboard\n  rotel dashboard --addr 0.0.0.0:8080 --storage-path /data/rotel.db"
    )]
    Dashboard {
        /// Dashboard bind address
        #[arg(long, default_value = "127.0.0.1:3000")]
        addr: SocketAddr,

        /// Storage database path
        #[arg(long, default_value = "rotel.db")]
        storage_path: String,
    },
    /// Manage log entries
    #[command(
        after_help = "Use 'rotel logs <command> --help' for more information on a specific command."
    )]
    Logs {
        #[command(subcommand)]
        command: LogsCommands,
    },
    /// Manage distributed traces
    #[command(
        after_help = "Use 'rotel traces <command> --help' for more information on a specific command."
    )]
    Traces {
        #[command(subcommand)]
        command: TracesCommands,
    },
    /// Manage time-series metrics
    #[command(
        after_help = "Use 'rotel metrics <command> --help' for more information on a specific command."
    )]
    Metrics {
        #[command(subcommand)]
        command: MetricsCommands,
    },
}

#[derive(Subcommand, Debug)]
enum LogsCommands {
    /// List recent log entries
    #[command(
        after_help = "Examples:\n  rotel logs list --severity ERROR --since 24h\n  rotel logs list --format json | jq '.[] | .body'"
    )]
    List {
        /// Filter by time range (e.g., 1h, 24h, 7d)
        #[arg(long, default_value = "1h")]
        since: Option<String>,

        /// Filter by severity level (ERROR, WARN, INFO, DEBUG, TRACE)
        #[arg(long)]
        severity: Option<String>,

        /// Maximum number of results
        #[arg(long, short = 'n', default_value = "50")]
        limit: Option<usize>,
    },
    /// Full-text search in log bodies
    #[command(
        after_help = "Examples:\n  rotel logs search \"database error\" --limit 20\n  rotel logs search \"timeout\" --format json"
    )]
    Search {
        /// Search query
        query: String,

        /// Maximum number of results
        #[arg(long, short = 'n', default_value = "50")]
        limit: Option<usize>,
    },
    /// Show a single log entry by ID
    #[command(
        after_help = "Examples:\n  rotel logs show log-12345\n  rotel logs show log-12345 --format json"
    )]
    Show {
        /// Log ID
        id: String,
    },
    /// Export log entries to file or stdout
    #[command(
        after_help = "Examples:\n  rotel logs export --format json --output logs.json\n  rotel logs export --format csv --severity ERROR --since 24h\n  rotel logs export --format json | jq ."
    )]
    Export {
        /// Export format (json or csv)
        #[arg(long, default_value = "json")]
        format: String,

        /// Filter by severity level (ERROR, WARN, INFO, DEBUG, TRACE)
        #[arg(long)]
        severity: Option<String>,

        /// Filter by time range (e.g., 1h, 24h, 7d)
        #[arg(long, default_value = "1h")]
        since: Option<String>,

        /// Output file path (stdout if not specified)
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum TracesCommands {
    /// List recent distributed traces
    #[command(
        after_help = "Examples:\n  rotel traces list --status ERROR --min-duration 1s\n  rotel traces list --since 24h --limit 20"
    )]
    List {
        /// Filter by time range (e.g., 1h, 24h, 7d)
        #[arg(long, default_value = "1h")]
        since: Option<String>,

        /// Filter by status (OK or ERROR)
        #[arg(long)]
        status: Option<String>,

        /// Filter by minimum duration (e.g., 1s, 500ms)
        #[arg(long)]
        min_duration: Option<String>,

        /// Maximum number of results
        #[arg(long, short = 'n', default_value = "50")]
        limit: Option<usize>,
    },
    /// Show a single trace with all spans
    #[command(
        after_help = "Examples:\n  rotel traces show trace-abc123\n  rotel traces show trace-abc123 --format json"
    )]
    Show {
        /// Trace ID
        id: String,
    },
    /// Export traces to file or stdout
    #[command(
        after_help = "Examples:\n  rotel traces export --format json --output traces.json\n  rotel traces export --status ERROR --min-duration 1s\n  rotel traces export --format json | jq ."
    )]
    Export {
        /// Export format (json only)
        #[arg(long, default_value = "json")]
        format: String,

        /// Filter by status (OK or ERROR)
        #[arg(long)]
        status: Option<String>,

        /// Filter by minimum duration (e.g., 1s, 500ms)
        #[arg(long)]
        min_duration: Option<String>,

        /// Filter by time range (e.g., 1h, 24h, 7d)
        #[arg(long, default_value = "1h")]
        since: Option<String>,

        /// Output file path (stdout if not specified)
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum MetricsCommands {
    /// List available metrics
    #[command(
        after_help = "Examples:\n  rotel metrics list --name http --since 24h\n  rotel metrics list --label method=GET --limit 20"
    )]
    List {
        /// Filter by time range (e.g., 1h, 24h, 7d)
        #[arg(long, default_value = "1h")]
        since: Option<String>,

        /// Filter by metric name pattern
        #[arg(long)]
        name: Option<String>,

        /// Filter by label (key=value, can be specified multiple times)
        #[arg(long)]
        label: Vec<String>,

        /// Maximum number of results
        #[arg(long, short = 'n', default_value = "50")]
        limit: Option<u32>,
    },
    /// Show metric values by name
    #[command(
        after_help = "Examples:\n  rotel metrics show http_requests_total\n  rotel metrics show http_requests_total --label method=GET"
    )]
    Show {
        /// Metric name
        name: String,

        /// Filter by time range (e.g., 1h, 24h, 7d)
        #[arg(long, default_value = "1h")]
        since: Option<String>,

        /// Filter by label (key=value, can be specified multiple times)
        #[arg(long)]
        label: Vec<String>,
    },
    /// Export metrics to file or stdout
    #[command(
        after_help = "Examples:\n  rotel metrics export --format json --output metrics.json\n  rotel metrics export --name cpu.usage --since 1h\n  rotel metrics export --format json | jq ."
    )]
    Export {
        /// Export format (json only)
        #[arg(long, default_value = "json")]
        format: String,

        /// Filter by metric name pattern
        #[arg(long)]
        name: Option<String>,

        /// Filter by time range (e.g., 1h, 24h, 7d)
        #[arg(long, default_value = "1h")]
        since: Option<String>,

        /// Output file path (stdout if not specified)
        #[arg(long, short = 'o')]
        output: Option<String>,
    },
}

#[tokio::main]
async fn main() {
    // Run the CLI and handle errors with proper exit codes
    if let Err(e) = run_cli().await {
        // Write error to stderr with user-friendly message
        eprintln!("{}", e.user_message());
        // Exit with appropriate code
        std::process::exit(e.exit_code());
    }
}

async fn run_cli() -> Result<()> {
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
        no_header: cli.no_header,
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
    info!("Starting Rotel Dashboard with OTLP Receiver...");

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

    // Start gRPC receiver on port 4317
    let grpc_addr = "0.0.0.0:4317".parse().unwrap();
    let receiver_config = rotel_receiver::ReceiverConfig::new().with_grpc_addr(grpc_addr);

    let grpc_server =
        rotel_receiver::grpc::GrpcServer::new(receiver_config.clone(), storage.clone());

    grpc_server
        .start()
        .await
        .map_err(|e| Error::ApiError(format!("Failed to start gRPC receiver: {}", e)))?;

    info!("gRPC receiver started on {}", grpc_addr);

    // Start HTTP receiver on port 4318
    let http_addr = "0.0.0.0:4318".parse().unwrap();
    let http_config = receiver_config.with_http_addr(http_addr);

    let http_server = rotel_receiver::http::HttpServer::new(http_config);

    http_server
        .start(storage.clone())
        .await
        .map_err(|e| Error::ApiError(format!("Failed to start HTTP receiver: {}", e)))?;

    info!("HTTP receiver started on {}", http_addr);

    // Start dashboard server
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
        },
        LogsCommands::Search { query, limit } => {
            logs::handle_search(&client, config, &query, limit, None).await?;
        },
        LogsCommands::Show { id } => {
            logs::handle_show(&client, config, &id).await?;
        },
        LogsCommands::Export {
            format,
            severity,
            since,
            output,
        } => {
            logs::handle_export(&client, config, &format, severity, since, output).await?;
        },
    }

    Ok(())
}

async fn handle_traces_command(command: TracesCommands, config: &Config) -> Result<()> {
    use api::client::ApiClient;
    use commands::traces;
    use output::parse_duration;

    let client = ApiClient::new(config.endpoint.clone(), config.timeout)?;

    match command {
        TracesCommands::List {
            limit,
            min_duration,
            status,
            since: _,
        } => {
            // Parse min_duration string to milliseconds if provided
            let min_duration_ms = if let Some(duration_str) = min_duration {
                Some(
                    parse_duration(&duration_str)
                        .map_err(|e| Error::InvalidArgument(format!("Invalid duration: {}", e)))?,
                )
            } else {
                None
            };

            traces::handle_list(
                &client,
                config,
                limit.map(|l| l as u32),
                min_duration_ms,
                status,
            )
            .await?;
        },
        TracesCommands::Show { id } => {
            traces::handle_show(&client, config, &id).await?;
        },
        TracesCommands::Export {
            format,
            status,
            min_duration,
            since,
            output,
        } => {
            // Parse min_duration string to milliseconds if provided
            let min_duration_ms = if let Some(duration_str) = min_duration {
                Some(
                    parse_duration(&duration_str)
                        .map_err(|e| Error::InvalidArgument(format!("Invalid duration: {}", e)))?,
                )
            } else {
                None
            };

            traces::handle_export(
                &client,
                config,
                &format,
                status,
                min_duration_ms,
                since,
                output,
            )
            .await?;
        },
    }

    Ok(())
}

async fn handle_metrics_command(command: MetricsCommands, config: &Config) -> Result<()> {
    use api::client::ApiClient;
    use commands::metrics;

    let client = ApiClient::new(config.endpoint.clone(), config.timeout)?;

    match command {
        MetricsCommands::List {
            limit,
            name,
            label,
            since,
        } => {
            metrics::handle_list(&client, config, limit, name, label, since).await?;
        },
        MetricsCommands::Show { name, label, since } => {
            metrics::handle_show(&client, config, &name, label, since).await?;
        },
        MetricsCommands::Export {
            format,
            name,
            since,
            output,
        } => {
            metrics::handle_export(&client, config, &format, name, since, output).await?;
        },
    }

    Ok(())
}

// Made with Bob
