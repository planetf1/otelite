//! API client module for communicating with Otelite backend

pub mod client;
pub mod models;

pub use client::ApiClient;
pub use models::{
    LogEntry, LogsResponse, MetricResponse, MetricValue, SpanEntry, TraceDetail, TraceEntry,
    TracesResponse,
};
