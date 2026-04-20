//! Rotel Dashboard - Lightweight web UI for OpenTelemetry visualization
//!
//! This crate provides a web-based dashboard for viewing logs, traces, and metrics
//! collected by Rotel. The dashboard is designed to be lightweight, fast, and
//! embedded within the main Rotel binary.

pub mod api;
pub mod cache;
pub mod config;
pub mod server;
pub mod static_files;

pub use config::DashboardConfig;
pub use server::DashboardServer;

/// Dashboard version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// Made with Bob
