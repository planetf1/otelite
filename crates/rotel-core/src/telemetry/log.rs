//! Log telemetry types

use std::collections::HashMap;

/// Represents a log record
#[derive(Debug, Clone, PartialEq)]
pub struct LogRecord {
    /// Timestamp in nanoseconds since Unix epoch
    pub timestamp: i64,

    /// Observed timestamp (when the event was observed)
    pub observed_timestamp: Option<i64>,

    /// Severity level
    pub severity: SeverityLevel,

    /// Severity text (human-readable)
    pub severity_text: Option<String>,

    /// Log body/message
    pub body: String,

    /// Log attributes
    pub attributes: HashMap<String, String>,

    /// Trace ID (if part of a trace)
    pub trace_id: Option<String>,

    /// Span ID (if part of a span)
    pub span_id: Option<String>,

    /// Associated resource
    pub resource: Option<super::Resource>,
}

/// Log severity levels (aligned with OpenTelemetry)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum SeverityLevel {
    /// Trace level (most verbose)
    Trace = 1,

    /// Debug level
    Debug = 5,

    /// Info level
    Info = 9,

    /// Warn level
    Warn = 13,

    /// Error level
    Error = 17,

    /// Fatal level (most severe)
    Fatal = 21,
}

impl LogRecord {
    /// Create a new log record
    pub fn new(severity: SeverityLevel, body: impl Into<String>, timestamp: i64) -> Self {
        Self {
            timestamp,
            observed_timestamp: None,
            severity,
            severity_text: None,
            body: body.into(),
            attributes: HashMap::new(),
            trace_id: None,
            span_id: None,
            resource: None,
        }
    }

    /// Add an attribute to the log record
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Set the trace context
    pub fn with_trace_context(mut self, trace_id: String, span_id: String) -> Self {
        self.trace_id = Some(trace_id);
        self.span_id = Some(span_id);
        self
    }

    /// Set the resource for the log record
    pub fn with_resource(mut self, resource: super::Resource) -> Self {
        self.resource = Some(resource);
        self
    }
}

impl SeverityLevel {
    /// Convert severity level to string
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
            Self::Fatal => "FATAL",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_record_creation() {
        let log = LogRecord::new(SeverityLevel::Info, "Application started", 1234567890);
        assert_eq!(log.severity, SeverityLevel::Info);
        assert_eq!(log.body, "Application started");
        assert_eq!(log.timestamp, 1234567890);
    }

    #[test]
    fn test_log_with_attributes() {
        let log = LogRecord::new(SeverityLevel::Error, "Connection failed", 1234567890)
            .with_attribute("error.type", "NetworkError")
            .with_attribute("retry.count", "3");

        assert_eq!(log.attributes.len(), 2);
        assert_eq!(
            log.attributes.get("error.type"),
            Some(&"NetworkError".to_string())
        );
    }

    #[test]
    fn test_log_with_trace_context() {
        let log = LogRecord::new(SeverityLevel::Debug, "Processing request", 1234567890)
            .with_trace_context("trace123".to_string(), "span456".to_string());

        assert_eq!(log.trace_id, Some("trace123".to_string()));
        assert_eq!(log.span_id, Some("span456".to_string()));
    }

    #[test]
    fn test_severity_ordering() {
        assert!(SeverityLevel::Trace < SeverityLevel::Debug);
        assert!(SeverityLevel::Debug < SeverityLevel::Info);
        assert!(SeverityLevel::Info < SeverityLevel::Warn);
        assert!(SeverityLevel::Warn < SeverityLevel::Error);
        assert!(SeverityLevel::Error < SeverityLevel::Fatal);
    }

    #[test]
    fn test_severity_as_str() {
        assert_eq!(SeverityLevel::Info.as_str(), "INFO");
        assert_eq!(SeverityLevel::Error.as_str(), "ERROR");
    }
}

// Made with Bob
