//! API client module for communicating with Rotel backend

pub mod client;
pub mod models;

pub use client::ApiClient;
pub use models::{
    LogEntry, LogsResponse, MetricResponse, MetricValue, SpanEntry, TraceDetail, TraceEntry,
    TracesResponse,
};

// Made with Bob
