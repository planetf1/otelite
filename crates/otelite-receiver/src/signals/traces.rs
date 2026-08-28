//! Traces signal handler

use crate::{conversion, Result};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use otelite_core::storage::StorageBackend;
use std::sync::Arc;
use tracing::{debug, info};

pub struct TraceProcessResult {
    pub accepted_spans: usize,
    pub rejected_spans: usize,
}

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
    pub async fn process(&self, request: ExportTraceServiceRequest) -> Result<TraceProcessResult> {
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

        let conversion = conversion::convert_traces_with_rejections(request);
        let spans: Vec<_> = conversion
            .traces
            .into_iter()
            .flat_map(|trace| trace.spans)
            .collect();
        let accepted_spans = spans.len();
        // One atomic transaction for the whole export: a failure rolls
        // back every span, so the exporter's retry of the rejected
        // export cannot duplicate the spans that already committed.
        if !spans.is_empty() {
            self.storage.write_span_batch(&spans).await?;
        }

        info!(
            accepted_spans,
            rejected_spans = conversion.rejected_spans,
            received_spans = span_count,
            "Processed spans"
        );
        Ok(TraceProcessResult {
            accepted_spans,
            rejected_spans: conversion.rejected_spans,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_traces_handler_process() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut storage = SqliteBackend::new(config);
        storage
            .initialize()
            .await
            .expect("Failed to initialize storage");
        let handler = TracesHandler::new(Arc::new(storage));
        let request = ExportTraceServiceRequest {
            resource_spans: vec![],
        };
        let result = handler
            .process(request)
            .await
            .expect("empty request succeeds");
        assert_eq!(result.accepted_spans, 0);
        assert_eq!(result.rejected_spans, 0);
    }
}
