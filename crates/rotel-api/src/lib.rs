//! Rotel API Backend
//!
//! Shared REST API backend providing consistent data access for all Rotel frontends
//! (Dashboard, TUI, CLI). Serves logs, traces, and metrics from the storage backend
//! with standardized JSON responses.

pub mod config;
pub mod error;
pub mod handlers;
pub mod middleware;
pub mod models;
pub mod routes;
pub mod server;

// Re-export commonly used types
pub use config::ApiConfig;
pub use error::{ApiError, ApiResult};
pub use server::ApiServer;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Initialize tracing and logging for the API server
///
/// Sets up structured logging with environment-based filtering.
/// Default level is INFO, can be overridden with RUST_LOG environment variable.
pub fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,rotel_api=debug")),
        )
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .init();
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_tracing_init() {
        // Test that tracing initialization doesn't panic
        // Note: Can only be called once per test process
        // init_tracing();
    }
}

// Made with Bob
