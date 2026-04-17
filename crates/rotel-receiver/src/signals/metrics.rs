//! Metrics signal handler

use crate::Result;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use tracing::{debug, info};

/// Handler for metrics signals
#[derive(Debug, Clone)]
pub struct MetricsHandler {
    // Future: will contain storage backend reference
}

impl MetricsHandler {
    /// Create a new metrics handler
    pub fn new() -> Self {
        Self {}
    }

    /// Process metrics data from OTLP request
    pub async fn process(&self, request: ExportMetricsServiceRequest) -> Result<()> {
        let metric_count: usize = request
            .resource_metrics
            .iter()
            .map(|rm| {
                rm.scope_metrics
                    .iter()
                    .map(|sm| sm.metrics.len())
                    .sum::<usize>()
            })
            .sum();

        debug!(
            "Processing {} metrics from {} resource metrics",
            metric_count,
            request.resource_metrics.len()
        );

        // TODO: Convert OTLP metrics to internal format
        // TODO: Store metrics in backend
        // For now, just log receipt

        info!("Received {} metrics", metric_count);
        Ok(())
    }

    /// Process raw bytes (for HTTP/protobuf)
    pub fn process_bytes(&self, data: &[u8]) -> Result<()> {
        debug!("Processing metrics data: {} bytes", data.len());

        // TODO: Parse OTLP metrics from protobuf/JSON
        // TODO: Store metrics in backend
        // For now, just log receipt

        info!("Received metrics: {} bytes", data.len());
        Ok(())
    }
}

impl Default for MetricsHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl super::SignalHandler for MetricsHandler {
    fn process(&self, data: &[u8]) -> Result<()> {
        self.process_bytes(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_metrics_handler_creation() {
        let handler = MetricsHandler::new();
        assert!(handler.process_bytes(&[1, 2, 3]).is_ok());
    }

    #[tokio::test]
    async fn test_metrics_handler_process() {
        let handler = MetricsHandler::new();
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![],
        };
        assert!(handler.process(request).await.is_ok());
    }
}

// Made with Bob
