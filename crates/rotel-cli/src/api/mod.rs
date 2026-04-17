//! API client module for communicating with Rotel backend

pub mod client;
pub mod models;

pub use client::ApiClient;
pub use models::{LogEntry, Metric, Span, SpanNode, Trace};

// Made with Bob
