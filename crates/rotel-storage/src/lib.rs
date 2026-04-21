//! Rotel Storage Layer
//!
//! Provides embedded storage for OTLP telemetry data (logs, traces, metrics).
//! Features zero-configuration initialization, automatic retention policies,
//! and pluggable backend architecture.

pub mod config;
pub mod error;
pub mod sqlite;

use async_trait::async_trait;
use rotel_core::query::QueryPredicate;
use rotel_core::telemetry::log::SeverityLevel;
use rotel_core::telemetry::{LogRecord, Metric, Span};

pub use config::StorageConfig;
pub use error::{Result, StorageError};

/// Statistics returned after a purge_all operation
#[derive(Debug, Clone)]
pub struct PurgeAllStats {
    /// Number of log records deleted
    pub logs_deleted: u64,
    /// Number of spans deleted
    pub spans_deleted: u64,
    /// Number of metric data points deleted
    pub metrics_deleted: u64,
}

/// Statistics about stored telemetry data
#[derive(Debug, Clone)]
pub struct StorageStats {
    /// Total number of log records
    pub log_count: u64,
    /// Total number of spans
    pub span_count: u64,
    /// Total number of metric data points
    pub metric_count: u64,
    /// Oldest record timestamp (nanoseconds since Unix epoch)
    pub oldest_timestamp: Option<i64>,
    /// Newest record timestamp (nanoseconds since Unix epoch)
    pub newest_timestamp: Option<i64>,
    /// Total storage size in bytes
    pub storage_size_bytes: u64,
}

/// Query parameters for filtering telemetry data
#[derive(Debug, Clone, Default)]
pub struct QueryParams {
    /// Start time (inclusive, nanoseconds since Unix epoch)
    pub start_time: Option<i64>,
    /// End time (exclusive, nanoseconds since Unix epoch)
    pub end_time: Option<i64>,
    /// Maximum number of results
    pub limit: Option<usize>,
    /// Trace ID filter
    pub trace_id: Option<String>,
    /// Span ID filter
    pub span_id: Option<String>,
    /// Minimum severity level filter (for logs)
    pub min_severity: Option<SeverityLevel>,
    /// Full-text search query (for logs)
    pub search_text: Option<String>,
    /// Structured query predicates parsed from query expressions
    pub predicates: Vec<QueryPredicate>,
}

/// Purge options for manual data cleanup
#[derive(Debug, Clone)]
pub struct PurgeOptions {
    /// Purge data older than this timestamp (nanoseconds since Unix epoch)
    pub older_than: Option<i64>,
    /// Purge specific signal types
    pub signal_types: Vec<SignalType>,
    /// Dry run mode (don't actually delete)
    pub dry_run: bool,
}

/// Signal type for purging
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    Logs,
    Traces,
    Metrics,
}

/// Storage backend trait for pluggable implementations
#[async_trait]
pub trait StorageBackend: Send + Sync {
    /// Initialize the storage backend
    async fn initialize(&mut self) -> Result<()>;

    /// Write a log record
    async fn write_log(&self, log: &LogRecord) -> Result<()>;

    /// Write a span
    async fn write_span(&self, span: &Span) -> Result<()>;

    /// Write a metric
    async fn write_metric(&self, metric: &Metric) -> Result<()>;

    /// Query log records
    async fn query_logs(&self, params: &QueryParams) -> Result<Vec<LogRecord>>;

    /// Query spans
    async fn query_spans(&self, params: &QueryParams) -> Result<Vec<Span>>;

    /// Query metrics (raw time-series rows, latest first)
    async fn query_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>>;

    /// Query metrics returning the single most-recent data point per unique name.
    /// Use this for list views where each metric should appear exactly once.
    async fn query_latest_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>>;

    /// Get storage statistics
    async fn stats(&self) -> Result<StorageStats>;

    /// Purge old data based on retention policy
    async fn purge(&self, options: &PurgeOptions) -> Result<u64>;

    /// Delete all telemetry data immediately
    async fn purge_all(&self) -> Result<PurgeAllStats>;

    /// Close the storage backend
    async fn close(&mut self) -> Result<()>;

    /// Return distinct resource attribute keys for the given signal type.
    /// `signal` must be one of "logs", "spans", or "metrics".
    async fn distinct_resource_keys(&self, signal: &str) -> Result<Vec<String>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_storage_stats_creation() {
        let stats = StorageStats {
            log_count: 100,
            span_count: 50,
            metric_count: 200,
            oldest_timestamp: None,
            newest_timestamp: None,
            storage_size_bytes: 1024,
        };
        assert_eq!(stats.log_count, 100);
        assert_eq!(stats.span_count, 50);
        assert_eq!(stats.metric_count, 200);
    }

    #[test]
    fn test_query_params_default() {
        let params = QueryParams::default();
        assert!(params.start_time.is_none());
        assert!(params.end_time.is_none());
        assert!(params.limit.is_none());
        assert!(params.trace_id.is_none());
        assert!(params.span_id.is_none());
        assert!(params.min_severity.is_none());
        assert!(params.search_text.is_none());
        assert!(params.predicates.is_empty());
    }

    #[test]
    fn test_signal_type_equality() {
        assert_eq!(SignalType::Logs, SignalType::Logs);
        assert_ne!(SignalType::Logs, SignalType::Traces);
    }
}
