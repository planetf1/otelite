//! Otelite Core Library
//!
//! This crate provides core functionality for the Otelite OpenTelemetry receiver,
//! including telemetry data types for metrics, logs, and traces.

// Telemetry data types
pub mod telemetry;

// API response types
pub mod api;

// Query parser
pub mod query;

// Storage abstraction (trait + associated types)
pub mod storage;

// Re-exports for convenience
pub use telemetry::{
    format_attribute_preview, format_attribute_value, LogRecord, Metric, Resource, Span, Trace,
};
