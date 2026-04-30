//! Storage abstraction layer for otelite.
//!
//! Defines the `StorageBackend` trait and all associated types so that
//! downstream crates (`otelite-receiver`, `otelite-api`) can depend only on
//! `otelite-core` rather than the concrete SQLite implementation.

use async_trait::async_trait;
use thiserror::Error;

use crate::api::{ModelUsage, SystemUsage, TokenUsageSummary};
use crate::query::QueryPredicate;
use crate::telemetry::log::SeverityLevel;
use crate::telemetry::{LogRecord, Metric, Span};

/// Result type for storage operations.
pub type Result<T> = std::result::Result<T, StorageError>;

/// Generic storage errors returned by `StorageBackend` implementations.
///
/// All variants carry string payloads so this type has no dependency on any
/// database library. Backend-specific error types should convert to these via
/// a `From` impl.
#[derive(Error, Debug)]
pub enum StorageError {
    /// Storage initialization failed.
    #[error("Failed to initialize storage: {0}")]
    InitializationError(String),

    /// Write operation failed.
    #[error("Failed to write data: {0}")]
    WriteError(String),

    /// Query operation failed.
    #[error("Failed to query data: {0}")]
    QueryError(String),

    /// Disk is full or insufficient space.
    #[error("Insufficient disk space: {0}")]
    DiskFullError(String),

    /// Storage corruption detected.
    #[error("Storage corruption detected: {0}")]
    CorruptionError(String),

    /// Permission denied.
    #[error("Permission denied: {0}")]
    PermissionError(String),

    /// Configuration error.
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Purge operation failed.
    #[error("Purge operation failed: {0}")]
    PurgeError(String),

    /// Underlying database error (string representation).
    #[error("Database error: {0}")]
    DatabaseError(String),

    /// I/O error (string representation).
    #[error("I/O error: {0}")]
    IoError(String),

    /// Serialization error (string representation).
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

impl StorageError {
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            StorageError::WriteError(_) | StorageError::QueryError(_) | StorageError::PurgeError(_)
        )
    }

    pub fn is_corruption(&self) -> bool {
        matches!(self, StorageError::CorruptionError(_))
    }

    pub fn is_disk_full(&self) -> bool {
        matches!(self, StorageError::DiskFullError(_))
    }
}

/// Statistics returned after a `purge_all` operation.
#[derive(Debug, Clone)]
pub struct PurgeAllStats {
    pub logs_deleted: u64,
    pub spans_deleted: u64,
    pub metrics_deleted: u64,
}

/// Statistics about stored telemetry data.
#[derive(Debug, Clone)]
pub struct StorageStats {
    pub log_count: u64,
    pub span_count: u64,
    pub metric_count: u64,
    /// Oldest record timestamp (nanoseconds since Unix epoch).
    pub oldest_timestamp: Option<i64>,
    /// Newest record timestamp (nanoseconds since Unix epoch).
    pub newest_timestamp: Option<i64>,
    pub storage_size_bytes: u64,
}

/// Query parameters for filtering telemetry data.
#[derive(Debug, Clone, Default)]
pub struct QueryParams {
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
    pub limit: Option<usize>,
    pub trace_id: Option<String>,
    pub span_id: Option<String>,
    pub min_severity: Option<SeverityLevel>,
    pub search_text: Option<String>,
    pub predicates: Vec<QueryPredicate>,
}

/// Options for manual data cleanup.
#[derive(Debug, Clone)]
pub struct PurgeOptions {
    /// Purge data older than this timestamp (nanoseconds since Unix epoch).
    pub older_than: Option<i64>,
    pub signal_types: Vec<SignalType>,
    pub dry_run: bool,
}

/// Signal type discriminator used in purge operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignalType {
    Logs,
    Traces,
    Metrics,
}

/// Pluggable storage backend trait.
///
/// Both `otelite-receiver` (writes) and `otelite-api` (reads) depend only on
/// this trait; neither needs a direct dependency on the SQLite implementation.
#[async_trait]
pub trait StorageBackend: Send + Sync {
    async fn initialize(&mut self) -> Result<()>;
    async fn write_log(&self, log: &LogRecord) -> Result<()>;
    async fn write_span(&self, span: &Span) -> Result<()>;
    async fn write_metric(&self, metric: &Metric) -> Result<()>;
    async fn query_logs(&self, params: &QueryParams) -> Result<Vec<LogRecord>>;
    async fn query_spans(&self, params: &QueryParams) -> Result<Vec<Span>>;
    /// Query all spans for the N most-recent distinct traces matching the filters.
    async fn query_spans_for_trace_list(
        &self,
        params: &QueryParams,
        trace_limit: usize,
    ) -> Result<Vec<Span>>;
    /// Query metrics (raw time-series rows, latest first).
    async fn query_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>>;
    /// Query metrics returning the single most-recent data point per unique name.
    async fn query_latest_metrics(&self, params: &QueryParams) -> Result<Vec<Metric>>;
    async fn stats(&self) -> Result<StorageStats>;
    async fn purge(&self, options: &PurgeOptions) -> Result<u64>;
    async fn purge_all(&self) -> Result<PurgeAllStats>;
    async fn close(&mut self) -> Result<()>;
    /// Return distinct resource attribute keys for the given signal type.
    /// `signal` must be one of `"logs"`, `"spans"`, or `"metrics"`.
    async fn distinct_resource_keys(&self, signal: &str) -> Result<Vec<String>>;
    async fn query_token_usage(
        &self,
        start_time: Option<i64>,
        end_time: Option<i64>,
    ) -> Result<(TokenUsageSummary, Vec<ModelUsage>, Vec<SystemUsage>)>;
}
