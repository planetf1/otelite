//! Traces signal handler

use crate::Result;
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use tracing::{debug, info};

/// Handler for traces signals
#[derive(Debug, Clone)]
pub struct TracesHandler {
    // Future: will contain storage backend reference
}

impl TracesHandler {
    /// Create a new traces handler
    pub fn new() -> Self {
        Self {}
    }

    /// Process traces data from OTLP request
    pub async fn process(&self, request: ExportTraceServiceRequest) -> Result<()> {
        let span_count: usize = request
            .resource_spans
            .iter()
            .map(|rs| {
                rs.scope_spans
                    .iter()
                    .map(|ss| ss.spans.len())
                    .sum::<usize>()
            })
            .sum();

        debug!(
            "Processing {} spans from {} resource spans",
            span_count,
            request.resource_spans.len()
        );

        // TODO: Convert OTLP traces to internal format
        // TODO: Store traces in backend
        // For now, just log receipt

        info!("Received {} spans", span_count);
        Ok(())
    }

    /// Process raw bytes (for HTTP/protobuf)
    pub fn process_bytes(&self, data: &[u8]) -> Result<()> {
        debug!("Processing traces data: {} bytes", data.len());

        // TODO: Parse OTLP traces from protobuf/JSON
        // TODO: Store traces in backend
        // For now, just log receipt

        info!("Received traces: {} bytes", data.len());
        Ok(())
    }
}

impl Default for TracesHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl super::SignalHandler for TracesHandler {
    fn process(&self, data: &[u8]) -> Result<()> {
        self.process_bytes(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_traces_handler_creation() {
        let handler = TracesHandler::new();
        assert!(handler.process_bytes(&[1, 2, 3]).is_ok());
    }

    #[tokio::test]
    async fn test_traces_handler_process() {
        let handler = TracesHandler::new();
        let request = ExportTraceServiceRequest {
            resource_spans: vec![],
        };
        assert!(handler.process(request).await.is_ok());
    }
}

// Made with Bob
