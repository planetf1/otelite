//! Traces signal handler

use crate::conversion::DroppedCounts;
use crate::health::HealthChecker;
use crate::{conversion, Result};
use opentelemetry_proto::tonic::collector::trace::v1::ExportTraceServiceRequest;
use otelite_core::storage::StorageBackend;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

pub struct TraceProcessResult {
    pub accepted_spans: usize,
    pub rejected_spans: usize,
    /// Telemetry dropped from accepted spans (e.g. span links) — reported,
    /// never asserted stored (#256).
    pub dropped: DroppedCounts,
}

/// Handler for traces signals
#[derive(Clone)]
pub struct TracesHandler {
    storage: Arc<dyn StorageBackend>,
    health: Arc<HealthChecker>,
}

impl TracesHandler {
    /// Create a new traces handler
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self::with_health(storage, Arc::new(HealthChecker::new()))
    }

    /// Create a handler that shares write-health state with the server's
    /// `/health` endpoint (#256).
    pub fn with_health(storage: Arc<dyn StorageBackend>, health: Arc<HealthChecker>) -> Self {
        Self { storage, health }
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
            if let Err(e) = self.storage.write_span_batch(&spans).await {
                self.health.record_write_failure(e.is_persistent());
                if e.is_persistent() {
                    // Disk full / corruption will keep failing: this is the
                    // operator-level signal, at error level (#256).
                    error!(
                        "traces write failed with a persistent error (exporter retries will not help until it is fixed): {e}"
                    );
                } else {
                    debug!("traces write failed with a transient error (exporter will retry): {e}");
                }
                return Err(e.into());
            }
            self.health.record_write_success();
        }

        // Dropped links come off ACCEPTED spans, so they are not part of
        // the protocol's rejected_spans count — a warn log is the honest
        // report (#256).
        if !conversion.dropped.is_empty() {
            warn!(
                dropped_total = conversion.dropped.total(),
                "Dropped trace telemetry the internal model cannot store: {}",
                conversion.dropped.summary()
            );
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
            dropped: conversion.dropped,
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
        assert!(result.dropped.is_empty());
    }
}
