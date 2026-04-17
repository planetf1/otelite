//! Signal-specific processing for metrics, logs, and traces

pub mod logs;
pub mod metrics;
pub mod traces;

pub use logs::LogsHandler;
pub use metrics::MetricsHandler;
pub use traces::TracesHandler;

/// Trait for signal handlers
pub trait SignalHandler {
    /// Process incoming signal data
    fn process(&self, data: &[u8]) -> crate::Result<()>;
}

// Made with Bob
