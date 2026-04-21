//! Trace and span telemetry types

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a distributed trace
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    /// Trace ID (unique identifier for the trace)
    pub trace_id: String,

    /// Spans in this trace
    pub spans: Vec<Span>,

    /// Associated resource
    pub resource: Option<super::Resource>,
}

/// Represents a span within a trace
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Span {
    /// Trace ID this span belongs to
    pub trace_id: String,

    /// Span ID (unique within the trace)
    pub span_id: String,

    /// Parent span ID (if this is a child span)
    pub parent_span_id: Option<String>,

    /// Span name
    pub name: String,

    /// Span kind
    pub kind: SpanKind,

    /// Start time in nanoseconds since Unix epoch
    pub start_time: i64,

    /// End time in nanoseconds since Unix epoch
    pub end_time: i64,

    /// Span attributes
    pub attributes: HashMap<String, String>,

    /// Span events
    pub events: Vec<SpanEvent>,

    /// Span status
    pub status: SpanStatus,

    /// Associated resource
    pub resource: Option<super::Resource>,
}

/// Span kind (type of operation)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SpanKind {
    /// Internal operation
    Internal,

    /// Server-side operation
    Server,

    /// Client-side operation
    Client,

    /// Producer operation (e.g., message queue)
    Producer,

    /// Consumer operation (e.g., message queue)
    Consumer,
}

impl SpanKind {
    /// Convert from integer representation
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Internal),
            1 => Some(Self::Server),
            2 => Some(Self::Client),
            3 => Some(Self::Producer),
            4 => Some(Self::Consumer),
            _ => None,
        }
    }

    /// Convert to integer representation
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}

/// Span event (point-in-time occurrence during a span)
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanEvent {
    /// Event name
    pub name: String,

    /// Timestamp in nanoseconds since Unix epoch
    pub timestamp: i64,

    /// Event attributes
    pub attributes: HashMap<String, String>,
}

/// Span status
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpanStatus {
    /// Status code
    pub code: StatusCode,

    /// Status message (optional)
    pub message: Option<String>,
}

/// Status code for spans
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StatusCode {
    /// Unset (default)
    Unset,

    /// Ok (success)
    Ok,

    /// Error (failure)
    Error,
}

impl StatusCode {
    /// Convert from integer representation
    pub fn from_i32(value: i32) -> Option<Self> {
        match value {
            0 => Some(Self::Unset),
            1 => Some(Self::Ok),
            2 => Some(Self::Error),
            _ => None,
        }
    }

    /// Convert to integer representation
    pub fn to_i32(self) -> i32 {
        self as i32
    }
}

impl Span {
    /// Create a new span
    pub fn new(
        trace_id: impl Into<String>,
        span_id: impl Into<String>,
        name: impl Into<String>,
        start_time: i64,
        end_time: i64,
    ) -> Self {
        Self {
            trace_id: trace_id.into(),
            span_id: span_id.into(),
            parent_span_id: None,
            name: name.into(),
            kind: SpanKind::Internal,
            start_time,
            end_time,
            attributes: HashMap::new(),
            events: Vec::new(),
            status: SpanStatus {
                code: StatusCode::Unset,
                message: None,
            },
            resource: None,
        }
    }

    /// Set the parent span ID
    pub fn with_parent(mut self, parent_span_id: impl Into<String>) -> Self {
        self.parent_span_id = Some(parent_span_id.into());
        self
    }

    /// Set the span kind
    pub fn with_kind(mut self, kind: SpanKind) -> Self {
        self.kind = kind;
        self
    }

    /// Add an attribute to the span
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Add an event to the span
    pub fn with_event(mut self, event: SpanEvent) -> Self {
        self.events.push(event);
        self
    }

    /// Set the span status
    pub fn with_status(mut self, code: StatusCode, message: Option<String>) -> Self {
        self.status = SpanStatus { code, message };
        self
    }

    /// Calculate span duration in nanoseconds
    pub fn duration_ns(&self) -> i64 {
        self.end_time - self.start_time
    }
}

impl Trace {
    /// Create a new trace
    pub fn new(trace_id: impl Into<String>) -> Self {
        Self {
            trace_id: trace_id.into(),
            spans: Vec::new(),
            resource: None,
        }
    }

    /// Add a span to the trace
    pub fn with_span(mut self, span: Span) -> Self {
        self.spans.push(span);
        self
    }

    /// Set the resource for the trace
    pub fn with_resource(mut self, resource: super::Resource) -> Self {
        self.resource = Some(resource);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_span_creation() {
        let span = Span::new("trace123", "span456", "http.request", 1000, 2000);
        assert_eq!(span.trace_id, "trace123");
        assert_eq!(span.span_id, "span456");
        assert_eq!(span.name, "http.request");
        assert_eq!(span.start_time, 1000);
        assert_eq!(span.end_time, 2000);
    }

    #[test]
    fn test_span_with_parent() {
        let span = Span::new("trace123", "span456", "db.query", 1000, 2000).with_parent("span123");

        assert_eq!(span.parent_span_id, Some("span123".to_string()));
    }

    #[test]
    fn test_span_duration() {
        let span = Span::new("trace123", "span456", "operation", 1000, 3500);
        assert_eq!(span.duration_ns(), 2500);
    }

    #[test]
    fn test_span_with_attributes() {
        let span = Span::new("trace123", "span456", "http.request", 1000, 2000)
            .with_attribute("http.method", "GET")
            .with_attribute("http.url", "/api/users");

        assert_eq!(span.attributes.len(), 2);
        assert_eq!(span.attributes.get("http.method"), Some(&"GET".to_string()));
    }

    #[test]
    fn test_trace_with_spans() {
        let span1 = Span::new("trace123", "span1", "parent", 1000, 3000);
        let span2 = Span::new("trace123", "span2", "child", 1500, 2500).with_parent("span1");

        let trace = Trace::new("trace123").with_span(span1).with_span(span2);

        assert_eq!(trace.spans.len(), 2);
        assert_eq!(trace.trace_id, "trace123");
    }
}
