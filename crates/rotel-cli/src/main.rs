//! Rotel CLI - OpenTelemetry receiver and dashboard

use clap::{Parser, Subcommand};
use rotel_server::{DashboardConfig, DashboardServer};
use rotel_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, Level};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod api;
pub mod commands;
pub mod config;
pub mod error;
pub mod output;

use config::{Config, OutputFormat};
use error::{Error, Result};

#[derive(Parser, Debug)]
#[command(name = "rotel")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("ROTEL_GIT_SHA"), ")"))]
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

    /// Disable automatic paging of long output
    #[arg(long, global = true)]
    no_pager: bool,

    /// Request timeout in seconds
    #[arg(long, default_value = "30", global = true)]
    timeout: u64,

    /// Log level (trace, debug, info, warn, error)
    #[arg(long, default_value = "info", global = true)]
    log_level: String,

    /// Log output file path (logs to stderr if not specified)
    #[arg(long, global = true)]
    log_file: Option<PathBuf>,

    /// Log format (text or json)
    #[arg(long, default_value = "text", global = true)]
    log_format: String,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the server with OTLP receivers in the foreground (default if no subcommand)
    #[command(
        alias = "dashboard",
        after_help = "Examples:\n  rotel serve\n  rotel serve --addr 0.0.0.0:8080 --storage-path /data/rotel.db"
    )]
    Serve {
        /// Server bind address
        #[arg(long, default_value = "127.0.0.1:3000")]
        addr: SocketAddr,

        /// Storage database path
        #[arg(long, default_value = "rotel.db")]
        storage_path: String,
    },
    /// Run `serve` as a background daemon
    #[command(
        after_help = "Examples:\n  rotel start\n  rotel start --addr 0.0.0.0:3000 --storage-path /data/rotel.db"
    )]
    Start {
        /// Server bind address
        #[arg(long, default_value = "127.0.0.1:3000")]
        addr: String,

        /// Storage database path
        #[arg(long, default_value = "rotel.db")]
        storage_path: String,
    },
    /// Stop the `serve` background daemon
    #[command(after_help = "Examples:\n  rotel stop")]
    Stop,
    /// Stop the running daemon and start a fresh one.
    /// Picks up a freshly compiled binary if you ran `cargo build --release` first.
    #[command(
        after_help = "Examples:\n  rotel restart\n  rotel restart --addr 0.0.0.0:3000 --storage-path /data/rotel.db"
    )]
    Restart {
        /// Server bind address
        #[arg(long, default_value = "127.0.0.1:3000")]
        addr: String,

        /// Storage database path
        #[arg(long, default_value = "rotel.db")]
        storage_path: String,
    },
    /// Show `serve` daemon status
    #[command(after_help = "Examples:\n  rotel status")]
    Status,
    /// Manage system service installation
    #[command(
        after_help = "Examples:\n  rotel service install\n\nThis creates a launchd plist (macOS) or systemd unit (Linux) for auto-start."
    )]
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
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
    /// Show token usage statistics for GenAI/LLM spans
    #[command(
        after_help = "Examples:\n  rotel usage --since 24h\n  rotel usage --model gpt-4 --by-model\n  rotel usage --system openai --since 7d"
    )]
    Usage(commands::usage::UsageCommand),
    /// Launch the Terminal User Interface
    #[command(
        after_help = "Examples:\n  rotel tui\n  rotel tui --api-url http://localhost:3000\n  rotel tui --view traces --refresh-interval 5"
    )]
    Tui {
        /// Rotel API base URL
        #[arg(long, default_value = "http://localhost:3000")]
        api_url: String,

        /// Refresh interval in seconds
        #[arg(long, default_value = "2")]
        refresh_interval: u64,

        /// Initial view (logs, traces, metrics)
        #[arg(long, default_value = "logs")]
        view: String,

        /// Enable debug logging
        #[arg(long)]
        debug: bool,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceCommands {
    /// Install rotel as a system service (launchd on macOS, systemd on Linux)
    #[command(
        after_help = "Examples:\n  rotel service install\n\nCreates a service configuration for auto-start on boot."
    )]
    Install,
}

#[derive(Subcommand, Debug)]
enum LogsCommands {
    /// List recent log entries
    #[command(
        after_help = "Examples:\n  rotel logs list --severity ERROR --since 24h\n  rotel logs list --query 'severity = \"ERROR\" AND body contains \"timeout\"'\n  rotel logs list --format json | jq '.[] | .body'"
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

        /// Structured query filter (e.g., 'severity = "ERROR" AND body contains "timeout"')
        #[arg(long)]
        query: Option<String>,
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
        after_help = "Examples:\n  rotel traces list --status ERROR --min-duration 1s\n  rotel traces list --query 'duration > 500ms AND status = \"ERROR\"'\n  rotel traces list --since 24h --limit 20"
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

        /// Structured query filter (e.g., 'duration > 500ms AND status = "ERROR"')
        #[arg(long)]
        query: Option<String>,
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
        after_help = "Examples:\n  rotel metrics list --name http --since 24h\n  rotel metrics list --query 'name contains \"http\" AND value > 100'\n  rotel metrics list --label method=GET --limit 20"
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

        /// Structured query filter (e.g., 'name contains "http" AND value > 100')
        #[arg(long)]
        query: Option<String>,
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

    // Build the EnvFilter (respects RUST_LOG env var)
    let env_filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level.to_string()));

    // Configure output destination and format
    if let Some(log_file) = &cli.log_file {
        // Log to file with daily rotation
        let file_appender = tracing_appender::rolling::daily(
            log_file
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
            log_file
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("rotel.log")),
        );
        let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);

        // Choose format based on --log-format flag
        if cli.log_format.to_lowercase() == "json" {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().json().with_writer(non_blocking))
                .init();
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().with_writer(non_blocking))
                .init();
        }

        // Keep the guard alive by leaking it - this is intentional for the lifetime of the program
        std::mem::forget(_guard);
    } else {
        // Log to stderr (default)
        if cli.log_format.to_lowercase() == "json" {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer().json())
                .init();
        } else {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(fmt::layer())
                .init();
        }
    }

    // Build config from CLI args
    let config = Config {
        endpoint: cli.endpoint.unwrap_or_else(Config::endpoint_from_env),
        timeout: std::time::Duration::from_secs(cli.timeout),
        format: cli.format.unwrap_or_default(),
        no_color: cli.no_color,
        no_header: cli.no_header,
        no_pager: cli.no_pager,
    };

    // Handle commands
    match cli.command {
        Some(Commands::Serve { addr, storage_path }) => run_dashboard(addr, storage_path).await,
        Some(Commands::Start { addr, storage_path }) => {
            commands::service::handle_start(storage_path, addr).await
        },
        Some(Commands::Stop) => commands::service::handle_stop().await,
        Some(Commands::Restart { addr, storage_path }) => {
            commands::service::handle_restart(storage_path, addr).await
        },
        Some(Commands::Status) => commands::service::handle_status().await,
        Some(Commands::Service { command }) => handle_service_command(command).await,
        Some(Commands::Logs { command }) => handle_logs_command(command, &config).await,
        Some(Commands::Traces { command }) => handle_traces_command(command, &config).await,
        Some(Commands::Metrics { command }) => handle_metrics_command(command, &config).await,
        Some(Commands::Usage(cmd)) => {
            let storage = create_storage(&config)?;
            cmd.execute(storage)?;
            Ok(())
        },
        Some(Commands::Tui {
            api_url,
            refresh_interval,
            view,
            debug,
        }) => handle_tui_command(api_url, refresh_interval, view, debug).await,
        None => {
            // Default: run dashboard
            run_dashboard("127.0.0.1:3000".parse().unwrap(), "rotel.db".to_string()).await
        },
    }
}

/// Create storage backend from config
fn create_storage(_config: &Config) -> Result<Arc<dyn StorageBackend>> {
    let storage_path = "rotel.db";
    let storage_config = StorageConfig::default().with_data_dir(PathBuf::from(storage_path));

    let mut storage = SqliteBackend::new(storage_config);
    // Initialize synchronously using tokio runtime
    tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(storage.initialize())
        .map_err(|e| Error::ApiError(format!("Failed to initialize storage: {}", e)))?;

    Ok(Arc::new(storage))
}

async fn run_dashboard(addr: SocketAddr, storage_path: String) -> Result<()> {
    // Check for first run and show welcome message
    let is_first_run = Config::is_first_run();
    if is_first_run {
        println!("\nWelcome to Rotel! Starting OpenTelemetry collector...\n");
        println!("  Dashboard:  http://{}", addr);
        println!("  OTLP gRPC:  localhost:4317");
        println!("  OTLP HTTP:  localhost:4318");

        // Determine storage path display
        let storage_display = if storage_path == "rotel.db" {
            format!("~/.local/share/rotel/{}", storage_path)
        } else {
            storage_path.clone()
        };
        println!("  Storage:    {}\n", storage_display);

        println!("To send test data:");
        println!("  otel-cli exec --endpoint http://localhost:4318 -- echo \"hello\"\n");

        println!("To view data:");
        println!("  rotel logs list");
        println!("  rotel traces list");
        println!("  rotel tui\n");

        // Create config file
        if let Err(e) = Config::create_default_config() {
            eprintln!("Warning: Failed to create config file: {}", e);
        } else {
            println!(
                "Config file created at: {}\n",
                Config::config_file().display()
            );
        }
    }

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
            query,
        } => {
            logs::handle_list(&client, config, limit, severity, since, query).await?;
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
            query,
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
                query,
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
            query,
        } => {
            metrics::handle_list(&client, config, limit, name, label, since, query).await?;
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

async fn handle_service_command(command: ServiceCommands) -> Result<()> {
    match command {
        ServiceCommands::Install => commands::service::handle_service_install().await,
    }
}

async fn handle_tui_command(
    api_url: String,
    refresh_interval: u64,
    view: String,
    debug: bool,
) -> Result<()> {
    // Create TUI configuration
    let config = rotel_tui::Config {
        api_url,
        refresh_interval: std::time::Duration::from_secs(refresh_interval),
        initial_view: view,
        debug,
        version: concat!(env!("CARGO_PKG_VERSION"), " (", env!("ROTEL_GIT_SHA"), ")").to_string(),
    };

    // Run the TUI application
    rotel_tui::app::run(config)
        .await
        .map_err(|e| Error::ApiError(format!("TUI error: {}", e)))?;

    Ok(())
}
