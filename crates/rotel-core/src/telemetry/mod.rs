//! Telemetry data types for OTLP signals

pub mod log;
pub mod metric;
pub mod resource;
pub mod trace;

pub use log::LogRecord;
pub use metric::Metric;
pub use resource::Resource;
pub use trace::{Span, Trace};

// Made with Bob
