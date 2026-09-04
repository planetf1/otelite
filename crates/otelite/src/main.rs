//! Otelite CLI - OpenTelemetry receiver and dashboard

use clap::{Parser, Subcommand};
use otelite_api::{DashboardConfig, DashboardServer};
use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{info, warn, Level};
use tracing_subscriber::{fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

pub mod commands;
pub mod config;
pub mod error;
pub mod output;

use config::{Config, OutputFormat};
use error::{Error, Result};

#[derive(Parser, Debug)]
#[command(name = "otelite")]
#[command(version = concat!(env!("CARGO_PKG_VERSION"), " (", env!("OTELITE_GIT_SHA"), ")"))]
#[command(about = "Lightweight OpenTelemetry receiver and dashboard", long_about = None)]
struct Cli {
    /// Otelite backend endpoint URL
    #[arg(long, env = "OTELITE_ENDPOINT", global = true)]
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

    /// Request timeout in seconds (default: 30, or `timeout` in the config file)
    #[arg(long, global = true)]
    timeout: Option<u64>,

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
        after_help = "Examples:\n  otelite serve\n  otelite serve --addr 0.0.0.0:8080 --storage-path /data/myproject"
    )]
    Serve {
        /// Server bind address
        #[arg(long, default_value = "127.0.0.1:3000")]
        addr: SocketAddr,

        /// Directory for the SQLite database. Defaults to ~/.otelite/data
        /// (override with OTELITE_DATA_DIR).
        #[arg(long)]
        storage_path: Option<PathBuf>,
    },
    /// Run `serve` as a background daemon
    #[command(
        after_help = "Examples:\n  otelite start\n  otelite start --addr 0.0.0.0:3000 --storage-path /data/myproject"
    )]
    Start {
        /// Server bind address
        #[arg(long, default_value = "127.0.0.1:3000")]
        addr: String,

        /// Directory for the SQLite database. Defaults to ~/.otelite/data
        /// (override with OTELITE_DATA_DIR).
        #[arg(long)]
        storage_path: Option<PathBuf>,
    },
    /// Stop the `serve` background daemon
    #[command(after_help = "Examples:\n  otelite stop")]
    Stop,
    /// Stop the running daemon and start a fresh one.
    /// Picks up a freshly compiled binary if you ran `cargo build --release` first.
    #[command(
        after_help = "Examples:\n  otelite restart\n  otelite restart --addr 0.0.0.0:3000 --storage-path /data/myproject"
    )]
    Restart {
        /// Server bind address
        #[arg(long, default_value = "127.0.0.1:3000")]
        addr: String,

        /// Directory for the SQLite database. Defaults to ~/.otelite/data
        /// (override with OTELITE_DATA_DIR).
        #[arg(long)]
        storage_path: Option<PathBuf>,
    },
    /// Show `serve` daemon status
    #[command(after_help = "Examples:\n  otelite status")]
    Status,
    /// Manage system service installation
    #[command(
        after_help = "Examples:\n  otelite service install\n\nThis creates a launchd plist (macOS) or systemd unit (Linux) for auto-start."
    )]
    Service {
        #[command(subcommand)]
        command: ServiceCommands,
    },
    /// Manage log entries
    #[command(
        after_help = "Use 'otelite logs <command> --help' for more information on a specific command."
    )]
    Logs {
        #[command(subcommand)]
        command: LogsCommands,
    },
    /// Manage distributed traces
    #[command(
        after_help = "Use 'otelite traces <command> --help' for more information on a specific command."
    )]
    Traces {
        #[command(subcommand)]
        command: TracesCommands,
    },
    /// Manage time-series metrics
    #[command(
        after_help = "Use 'otelite metrics <command> --help' for more information on a specific command."
    )]
    Metrics {
        #[command(subcommand)]
        command: MetricsCommands,
    },
    /// Show token usage statistics for GenAI/LLM spans
    #[command(
        after_help = "Examples:\n  otelite usage --since 24h\n  otelite usage --model gpt-4 --by-model\n  otelite usage --system openai --since 7d"
    )]
    Usage(commands::usage::UsageCommand),
    /// Show the provider × model mix (tokens, sessions, estimated cost share)
    #[command(
        after_help = "Examples:\n  otelite providers --since 24h\n  otelite providers --since 7d --format json"
    )]
    Providers(commands::providers::ProvidersCommand),
    /// Show GenAI telemetry capability coverage per emitter identity
    #[command(
        after_help = "Examples:\n  otelite capabilities --since 7d\n  otelite capabilities --since 30d --format json"
    )]
    Capabilities(commands::capabilities::CapabilitiesCommand),
    /// Diagnose model performance: current interval vs preceding and rolling
    /// baselines, with deterministic classification and confidence
    #[command(
        after_help = "Examples:\n  otelite model-performance --start 2026-08-22 --end 2026-08-29 --rolling 30d\n  otelite model-performance --start 2026-08-22 --end 2026-08-29 --model gpt-4o --format json-compact"
    )]
    ModelPerformance(commands::model_performance::ModelPerformanceCommand),
    /// Show cache economics (read/write split, hit rate, estimated savings)
    #[command(
        after_help = "Examples:\n  otelite cache --since 24h\n  otelite cache --since 7d --series --bucket-secs 86400"
    )]
    Cache(commands::cache::CacheCommand),
    /// Show reasoning token share by model and effort
    #[command(
        after_help = "Examples:\n  otelite reasoning --since 24h\n  otelite reasoning --since 7d --format json"
    )]
    Reasoning(commands::reasoning::ReasoningCommand),
    /// Show per-harness rollup: sessions, cost, tokens, tool calls, retries
    #[command(
        after_help = "Examples:\n  otelite agents --since 24h\n  otelite agents --since 7d --format json"
    )]
    Agents(commands::agents::AgentsCommand),
    /// Show per-project usage: sessions, cost, tokens, top model
    #[command(
        after_help = "Examples:\n  otelite projects --since 24h\n  otelite projects --since 7d --format json"
    )]
    Projects(commands::projects::ProjectsCommand),
    /// Session cost analysis: top-cost sessions and the cost distribution
    #[command(
        subcommand,
        after_help = "Examples:\n  otelite sessions costs --since 24h --top 20\n  otelite sessions cost-hist --since 7d --buckets 30 --format json\n  otelite sessions context <session-id> --start 1787000000000000000"
    )]
    Sessions(commands::sessions::SessionsCommand),
    /// Distribution of a named metric: session_cost | tool_duration | llm_duration | ttft | output_tokens
    #[command(
        after_help = "Examples:\n  otelite histogram session_cost --since 7d --scale log\n  otelite histogram ttft --since 24h --buckets 30 --format json"
    )]
    Histogram(commands::histogram::HistogramCommand),
    /// Launch the Terminal User Interface
    #[command(
        after_help = "Examples:\n  otelite tui\n  otelite tui --api-url http://localhost:3000\n  otelite tui --view traces --refresh-interval 5"
    )]
    Tui {
        /// Otelite API base URL
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
    /// Recent LLM request health: status, model, tokens, TTFT, trace-id
    #[command(
        after_help = "Examples:\n  otelite llm\n  otelite llm --since 1h --limit 50\n  otelite llm --model claude-sonnet --status error\n  otelite llm --trace <trace-id>"
    )]
    Llm(commands::llm::LlmCommand),
    /// One-shot forensic report for an AI agent session
    #[command(
        after_help = "Examples:\n  otelite diagnose e023c577-8f96-4de8-a86c-6ce3080519b5\n  otelite diagnose <session-id> --suggest"
    )]
    Diagnose {
        /// Session ID (value of the session.id span attribute)
        session_id: String,
        /// Print a remediation suggestion when streaming stalls are detected
        #[arg(long)]
        suggest: bool,
    },
    /// Import telemetry data from a JSONL file into a local database (no server required)
    #[command(
        after_help = "Each line must be a complete OTLP JSON export request (ExportMetricsServiceRequest, ExportLogsServiceRequest, or ExportTraceServiceRequest).\n\nExamples:\n  otelite import ci-metrics.jsonl\n  otelite import --signal-type metrics build.jsonl\n  otelite import --storage-path ./ci-run-42 build.jsonl\n  otelite serve --storage-path ./ci-run-42   # browse imported data"
    )]
    Import {
        /// Path to JSONL file (use '-' to read from stdin)
        file: String,

        /// Signal type to import: metrics, logs, or traces (auto-detected if omitted)
        #[arg(long)]
        signal_type: Option<String>,

        /// Data directory for the target database (default: ~/.otelite/data)
        #[arg(long)]
        storage_path: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum ServiceCommands {
    /// Install otelite as a system service (launchd on macOS, systemd on Linux)
    #[command(
        after_help = "Examples:\n  otelite service install\n\nCreates a service configuration for auto-start on boot, then\nloads and starts it immediately. Any OTELITE_* environment\nvariables set in your shell are carried into the service."
    )]
    Install,

    /// Uninstall the system service (stop it and remove its configuration)
    #[command(
        after_help = "Examples:\n  otelite service uninstall\n\nStops the service and removes its unit file. Your data is\nnot touched."
    )]
    Uninstall,
}

#[derive(Subcommand, Debug)]
enum LogsCommands {
    /// List recent log entries
    #[command(
        after_help = "Examples:\n  otelite logs list --severity ERROR --since 24h\n  otelite logs list --query 'severity = \"ERROR\" AND body contains \"timeout\"'\n  otelite logs list --format json | jq '.[] | .body'"
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
        after_help = "Examples:\n  otelite logs search \"database error\" --limit 20\n  otelite logs search \"timeout\" --format json"
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
        after_help = "Examples:\n  otelite logs show log-12345\n  otelite logs show log-12345 --format json\n  otelite logs show log-12345 --full > body.json"
    )]
    Show {
        /// Log ID
        id: String,
        /// Print raw body only (useful for piping large bodies: otelite logs show <id> --full | jq .)
        #[arg(long)]
        full: bool,
    },
    /// Export log entries to file or stdout
    #[command(
        after_help = "Examples:\n  otelite logs export --format json --output logs.json\n  otelite logs export --format csv --severity ERROR --since 24h\n  otelite logs export --format json | jq ."
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
        after_help = "Examples:\n  otelite traces list --status ERROR --min-duration 1s\n  otelite traces list --model claude-opus-4-7 --status ERROR\n  otelite traces list --session <id>\n  otelite traces list --query 'duration > 500ms AND status = \"ERROR\"'\n  otelite traces list --since 24h --limit 20"
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

        /// Filter by model name (e.g., "claude-opus-4-7")
        #[arg(long)]
        model: Option<String>,

        /// Filter by session ID (session.id attribute)
        #[arg(long)]
        session: Option<String>,
    },
    /// Show a single trace with all spans
    #[command(
        after_help = "Examples:\n  otelite traces show trace-abc123\n  otelite traces show trace-abc123 --format json"
    )]
    Show {
        /// Trace ID
        id: String,
    },
    /// Show logs associated with a trace
    #[command(
        after_help = "Examples:\n  otelite traces logs trace-abc123\n  otelite traces logs trace-abc123 --limit 20"
    )]
    Logs {
        /// Trace ID
        id: String,
        /// Maximum number of results
        #[arg(long, short = 'n', default_value = "100")]
        limit: Option<usize>,
    },
    /// Export traces to file or stdout
    #[command(
        after_help = "Examples:\n  otelite traces export --format json --output traces.json\n  otelite traces export --status ERROR --min-duration 1s\n  otelite traces export --format json | jq ."
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
        after_help = "Examples:\n  otelite metrics list --name http --since 24h\n  otelite metrics list --query 'name contains \"http\" AND value > 100'\n  otelite metrics list --label method=GET --limit 20"
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

        /// Maximum number of results (server enforces a cap of 1000)
        #[arg(long, short = 'n', default_value = "50")]
        limit: Option<u32>,

        /// Structured query filter (e.g., 'name contains "http" AND value > 100')
        #[arg(long)]
        query: Option<String>,
    },
    /// Show metric values by name
    #[command(
        after_help = "Examples:\n  otelite metrics show http_requests_total\n  otelite metrics show http_requests_total --label method=GET"
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
        after_help = "Examples:\n  otelite metrics export --format json --output metrics.json\n  otelite metrics export --name cpu.usage --since 1h\n  otelite metrics export --format json | jq ."
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

    // Configure output destination and format. The non-blocking appender's
    // guard is held for the process lifetime: dropping it at exit signals
    // the worker to flush buffered lines, so a clean shutdown doesn't lose
    // the tail of the log (the previous `mem::forget` skipped that final
    // flush).
    let _appender_guard = if let Some(log_file) = &cli.log_file {
        // Log to file with daily rotation
        let file_appender = tracing_appender::rolling::daily(
            log_file
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
            log_file
                .file_name()
                .unwrap_or_else(|| std::ffi::OsStr::new("otelite.log")),
        );
        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

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

        Some(guard)
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
        None
    };

    // Build config from config file, then apply CLI args on top.
    // Precedence: CLI flags > OTELITE_ENDPOINT > config file > defaults.
    let file_config = Config::load_file()?;
    let config = Config {
        endpoint: cli.endpoint.unwrap_or_else(Config::endpoint_from_env),
        timeout: cli
            .timeout
            .map(std::time::Duration::from_secs)
            .unwrap_or(file_config.timeout),
        format: cli.format.unwrap_or(file_config.format),
        no_color: cli.no_color || file_config.no_color,
        no_header: cli.no_header || file_config.no_header,
        no_pager: cli.no_pager || file_config.no_pager,
    };

    // Validate the endpoint and timeout up front so a bad --endpoint or
    // --timeout fails immediately with a clear message instead of
    // mid-request.
    if let Err(e) = reqwest::Url::parse(&config.endpoint) {
        return Err(Error::InvalidArgument(format!(
            "Invalid --endpoint '{}': {} (expected a URL like http://127.0.0.1:4318)",
            config.endpoint, e
        )));
    }
    if config.timeout.is_zero() {
        return Err(Error::InvalidArgument(
            "Invalid --timeout 0: the timeout must be at least 1 second".to_string(),
        ));
    }

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
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::Providers(cmd)) => {
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::Cache(cmd)) => {
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::Capabilities(cmd)) => {
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::ModelPerformance(cmd)) => {
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::Reasoning(cmd)) => {
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::Agents(cmd)) => {
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::Projects(cmd)) => {
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::Sessions(cmd)) => {
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::Histogram(cmd)) => {
            let storage = create_storage(&config).await?;
            cmd.execute(storage, config.format).await?;
            Ok(())
        },
        Some(Commands::Tui {
            api_url,
            refresh_interval,
            view,
            debug,
        }) => handle_tui_command(api_url, refresh_interval, view, debug).await,
        Some(Commands::Llm(cmd)) => cmd.execute(config).await,
        Some(Commands::Diagnose {
            session_id,
            suggest,
        }) => {
            use otelite_client::ApiClient;
            let client = ApiClient::new(config.endpoint.clone(), config.timeout)?;
            commands::diagnose::handle_diagnose(&client, &config, &session_id, suggest).await
        },
        Some(Commands::Import {
            file,
            signal_type,
            storage_path,
        }) => {
            commands::import::handle_import(&file, signal_type.as_deref(), storage_path.as_deref())
                .await
        },
        None => run_dashboard("127.0.0.1:3000".parse().unwrap(), None).await,
    }
}

/// Create storage backend from config
///
/// `StorageConfig::from_env` honours `OTELITE_DATA_DIR` so the read-only CLI
/// commands can be pointed at a specific database (used by the parity tests,
/// issue #144); with the variable unset it resolves to the same default
/// directory as before.
async fn create_storage(_config: &Config) -> Result<Arc<dyn StorageBackend>> {
    let storage_config = StorageConfig::from_env()
        .map_err(|e| Error::ApiError(format!("Failed to build storage configuration: {}", e)))?;

    let mut storage = SqliteBackend::new(storage_config);
    storage
        .initialize()
        .await
        .map_err(|e| Error::ApiError(format!("Failed to initialize storage: {}", e)))?;

    Ok(Arc::new(storage))
}

async fn run_dashboard(addr: SocketAddr, storage_path: Option<PathBuf>) -> Result<()> {
    let storage_config = match storage_path {
        Some(path) => StorageConfig::default().with_data_dir(path),
        None => StorageConfig::default(),
    };

    let is_first_run = Config::is_first_run();
    if is_first_run {
        println!("\nWelcome to Otelite! Starting OpenTelemetry collector...\n");
        let banner_grpc = otlp_port_from_env("OTELITE_OTLP_GRPC_PORT", 4317).unwrap_or(4317);
        let banner_http = otlp_port_from_env("OTELITE_OTLP_HTTP_PORT", 4318).unwrap_or(4318);
        println!("  Dashboard:  http://{}", addr);
        println!("  OTLP gRPC:  {}:{}", addr.ip(), banner_grpc);
        println!("  OTLP HTTP:  {}:{}", addr.ip(), banner_http);
        println!("  Storage:    {}\n", storage_config.data_dir.display());

        println!("To send test data:");
        println!(
            "  otel-cli exec --endpoint http://{}:{banner_http} -- echo \"hello\"\n",
            addr.ip()
        );

        println!("To view data:");
        println!("  otelite logs list");
        println!("  otelite traces list");
        println!("  otelite tui\n");

        println!("GitHub: https://github.com/planetf1/otelite\n");

        if let Err(e) = Config::create_default_config() {
            eprintln!("Warning: Failed to create config file: {}", e);
        } else {
            println!(
                "Config file created at: {}\n",
                Config::config_file().display()
            );
        }
    }

    info!("Starting Otelite Dashboard with OTLP Receiver...");
    info!(
        "Initializing storage at {}",
        storage_config.data_dir.display()
    );
    let data_dir_str = storage_config.data_dir.to_string_lossy().into_owned();

    let mut storage = SqliteBackend::new(storage_config);
    storage
        .initialize()
        .await
        .map_err(|e| Error::ApiError(format!("Failed to initialize storage: {}", e)))?;
    let storage: Arc<dyn StorageBackend> = Arc::new(storage);

    info!("Storage initialized successfully");

    // The OTLP receivers follow the dashboard's IP — a localhost-only
    // dashboard should not publish OTLP on every interface — and the
    // ports can be re-pointed via OTELITE_OTLP_GRPC_PORT /
    // OTELITE_OTLP_HTTP_PORT (defaults keep the standard 4317/4318).
    let grpc_port = otlp_port_from_env("OTELITE_OTLP_GRPC_PORT", 4317)?;
    let http_port = otlp_port_from_env("OTELITE_OTLP_HTTP_PORT", 4318)?;
    let grpc_addr = SocketAddr::new(addr.ip(), grpc_port);
    let http_addr = SocketAddr::new(addr.ip(), http_port);
    let receiver_config = otelite_receiver::ReceiverConfig::new().with_grpc_addr(grpc_addr);

    let grpc_server =
        otelite_receiver::grpc::GrpcServer::new(receiver_config.clone(), storage.clone());

    grpc_server
        .start()
        .await
        .map_err(|e| Error::ApiError(format!("Failed to start gRPC receiver: {}", e)))?;

    info!("gRPC receiver started on {}", grpc_addr);

    // Start HTTP receiver
    let http_config = receiver_config.with_http_addr(http_addr);

    let http_server = otelite_receiver::http::HttpServer::new(http_config);

    http_server
        .start(storage.clone())
        .await
        .map_err(|e| Error::ApiError(format!("Failed to start HTTP receiver: {}", e)))?;

    info!("HTTP receiver started on {}", http_addr);

    // Start dashboard server
    info!("Dashboard enabled at http://{}", addr);

    let config = DashboardConfig::default()
        .with_bind_address(addr)
        .with_storage_path(data_dir_str)
        .with_otlp_ports(grpc_addr.port(), http_addr.port());

    let server = DashboardServer::new(config, storage);
    let dashboard_shutdown = server.shutdown_handle();
    // The serve future's error type is not `Send`, so stringify it before
    // crossing the spawn boundary.
    let serve_result = tokio::spawn(async move { server.start().await.map_err(|e| e.to_string()) });

    // SIGTERM/SIGINT used to be ignored: `axum::serve` blocks forever, so
    // the daemon only died when `otelite stop` escalated to SIGKILL after
    // 10 s. On a signal, shut the receivers down first (they finish
    // in-flight exports), then the dashboard, and give the spawned serve
    // tasks a bounded drain before the runtime exits.
    enum ServeOutcome {
        DashboardError(String),
        Signal,
    }
    let outcome = tokio::select! {
        res = serve_result => {
            // The dashboard stopped on its own (listener/serve error):
            // tear the receivers down and surface the error as before.
            grpc_server.shutdown();
            http_server.shutdown();
            match res {
                Ok(Ok(())) => unreachable!("dashboard serve returns only on error or shutdown"),
                Ok(Err(e)) => ServeOutcome::DashboardError(e),
                Err(e) => ServeOutcome::DashboardError(format!("dashboard task failed: {e}")),
            }
        },
        _ = await_shutdown_signal() => {
            info!("Shutdown signal received; stopping gRPC, HTTP and dashboard servers");
            grpc_server.shutdown();
            http_server.shutdown();
            dashboard_shutdown.trigger();
            ServeOutcome::Signal
        },
    };

    match outcome {
        ServeOutcome::Signal => {
            // Bounded drain for the receivers' spawned serve tasks
            // (tonic/axum stop accepting immediately on shutdown; in-flight
            // OTLP exports get a connection error the exporter retries).
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            Ok(())
        },
        ServeOutcome::DashboardError(e) => {
            Err(Error::ApiError(format!("Dashboard server error: {e}")))
        },
    }
}

/// Read an OTLP port override from the environment. A set-but-invalid
/// value is a user error and fails loudly rather than silently binding
/// the wrong port.
fn otlp_port_from_env(var: &str, default: u16) -> Result<u16> {
    match std::env::var(var) {
        Ok(v) if !v.trim().is_empty() => v
            .trim()
            .parse::<u16>()
            .map_err(|_| Error::ConfigError(format!("Invalid {var}: {v:?} (expected a TCP port)"))),
        _ => Ok(default),
    }
}

/// Wait for SIGTERM or SIGINT (the two signals a service manager or a
/// user's Ctrl-C deliver).
async fn await_shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut sigterm = match signal(SignalKind::terminate()) {
            Ok(handler) => handler,
            Err(e) => {
                warn!(
                    "Failed to install SIGTERM handler: {}; waiting on SIGINT only",
                    e
                );
                let _ = tokio::signal::ctrl_c().await;
                return;
            },
        };

        tokio::select! {
            _ = sigterm.recv() => {},
            _ = tokio::signal::ctrl_c() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

async fn handle_logs_command(command: LogsCommands, config: &Config) -> Result<()> {
    use commands::logs;
    use otelite_client::ApiClient;

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
        LogsCommands::Show { id, full } => {
            logs::handle_show(&client, config, &id, full).await?;
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
    use commands::traces;
    use otelite_client::ApiClient;
    use output::parse_duration;

    let client = ApiClient::new(config.endpoint.clone(), config.timeout)?;

    match command {
        TracesCommands::List {
            limit,
            min_duration,
            status,
            since: _,
            query,
            model,
            session,
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
                model,
                session,
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
        TracesCommands::Logs { id, limit } => {
            traces::handle_logs_for_trace(&client, config, &id, limit).await?;
        },
    }

    Ok(())
}

async fn handle_metrics_command(command: MetricsCommands, config: &Config) -> Result<()> {
    use commands::metrics;
    use otelite_client::ApiClient;

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
        ServiceCommands::Uninstall => commands::service::handle_service_uninstall().await,
    }
}

async fn handle_tui_command(
    api_url: String,
    refresh_interval: u64,
    view: String,
    debug: bool,
) -> Result<()> {
    // Create TUI configuration
    let config = otelite_tui::Config {
        api_url,
        refresh_interval: std::time::Duration::from_secs(refresh_interval),
        initial_view: view,
        debug,
        version: concat!(
            env!("CARGO_PKG_VERSION"),
            " (",
            env!("OTELITE_GIT_SHA"),
            ")"
        )
        .to_string(),
    };

    // Run the TUI application
    otelite_tui::run_tui(config)
        .await
        .map_err(|e| Error::ApiError(format!("TUI error: {}", e)))?;

    Ok(())
}

#[cfg(test)]
mod otlp_port_tests {
    use super::*;

    #[test]
    fn test_otlp_port_from_env_unset_uses_default() {
        std::env::remove_var("OTELITE_TEST_PORT_UNSET");
        assert_eq!(
            otlp_port_from_env("OTELITE_TEST_PORT_UNSET", 4317).unwrap(),
            4317
        );
    }

    #[test]
    fn test_otlp_port_from_env_valid_and_invalid() {
        std::env::set_var("OTELITE_TEST_PORT_VALID", "12345");
        assert_eq!(
            otlp_port_from_env("OTELITE_TEST_PORT_VALID", 4317).unwrap(),
            12345
        );
        std::env::set_var("OTELITE_TEST_PORT_INVALID", "not-a-port");
        assert!(otlp_port_from_env("OTELITE_TEST_PORT_INVALID", 4317).is_err());
        std::env::set_var("OTELITE_TEST_PORT_INVALID", "99999");
        assert!(otlp_port_from_env("OTELITE_TEST_PORT_INVALID", 4317).is_err());
    }
}
