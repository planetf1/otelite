//! Rotel Core Library
//!
//! This crate provides core functionality for the Rotel OpenTelemetry receiver,
//! including telemetry data types for metrics, logs, and traces.

// Telemetry data types
pub mod telemetry;

// API response types
pub mod api;

// Re-exports for convenience
pub use telemetry::{
    format_attribute_preview, format_attribute_value, LogRecord, Metric, Resource, Span, Trace,
};
