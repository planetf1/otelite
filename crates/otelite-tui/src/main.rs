use anyhow::Result;
use clap::Parser;

mod api;
mod app;
mod config;
mod events;
mod state;
mod ui;

/// Otelite TUI - Terminal User Interface for OpenTelemetry data
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Otelite API base URL
    #[arg(short, long, default_value = "http://localhost:3000")]
    api_url: String,

    /// Refresh interval in seconds
    #[arg(short, long, default_value = "2")]
    refresh_interval: u64,

    /// Initial view (logs, traces, metrics)
    #[arg(short, long, default_value = "logs")]
    view: String,

    /// Enable debug logging
    #[arg(short, long)]
    debug: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // Initialize configuration
    let config = config::Config {
        api_url: args.api_url,
        refresh_interval: std::time::Duration::from_secs(args.refresh_interval),
        initial_view: args.view,
        debug: args.debug,
        version: concat!(env!("CARGO_PKG_VERSION"), " (", env!("ROTEL_GIT_SHA"), ")").to_string(),
    };

    // Run the TUI application
    app::run(config).await?;

    Ok(())
}
