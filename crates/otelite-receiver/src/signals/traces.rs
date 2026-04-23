//! Traces signal handler

use crate::{conversion, Result};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use otelite_core::storage::StorageBackend;
use std::sync::Arc;
use tracing::{debug, info};

/// Handler for traces signals
#[derive(Clone)]
pub struct TracesHandler {
    storage: Arc<dyn StorageBackend>,
}

impl TracesHandler {
    /// Create a new traces handler
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
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

        let traces = conversion::convert_traces(request);
        for trace in traces {
            for span in trace.spans {
                self.storage.write_span(&span).await?;
            }
        }

        info!("Stored {} spans", span_count);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};

    #[tokio::test]
    async fn test_traces_handler_process() {
        let mut storage = SqliteBackend::new(StorageConfig::default());
        storage
            .initialize()
            .await
            .expect("Failed to initialize storage");
        let handler = TracesHandler::new(Arc::new(storage));
        let request = ExportTraceServiceRequest {
            resource_spans: vec![],
        };
        assert!(handler.process(request).await.is_ok());
    }
}
