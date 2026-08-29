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

// GenAI semantic-convention attribute vocabulary
pub mod semconv;

// Model pricing data + cost computation
pub mod pricing;

// Per-session cost analysis helpers (anomaly rule, log-spaced buckets)
pub mod session_cost;

// Generic distribution binning + summary stats (issue #133)
pub mod distribution;

// Agent-framework attribute recognizers (CrewAI, AutoGen, LangGraph, ...)
pub mod agent_frameworks;
pub mod model_performance;

/// Cross-cutting GenAI filter dimensions (global filter bar, #135).
pub mod filters;

// Re-exports for convenience
pub use telemetry::{
    format_attribute_preview, format_attribute_value, LogRecord, Metric, Resource, Span, Trace,
};
