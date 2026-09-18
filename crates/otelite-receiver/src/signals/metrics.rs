//! Metrics signal handler

use crate::conversion::{self, DroppedCounts};
use crate::health::HealthChecker;
use crate::Result;
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use otelite_core::storage::StorageBackend;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

/// Outcome of a processed metrics export (#256): what was received, what
/// was stored, and what conversion dropped in between. The handler never
/// asserts more was stored than this says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsProcessResult {
    pub metrics_received: usize,
    pub metrics_stored: usize,
    pub dropped: DroppedCounts,
}

/// Handler for metrics signals
#[derive(Clone)]
pub struct MetricsHandler {
    storage: Arc<dyn StorageBackend>,
    health: Arc<HealthChecker>,
}

impl MetricsHandler {
    /// Create a new metrics handler
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self::with_health(storage, Arc::new(HealthChecker::new()))
    }

    /// Create a handler that shares write-health state with the server's
    /// `/health` endpoint (#256).
    pub fn with_health(storage: Arc<dyn StorageBackend>, health: Arc<HealthChecker>) -> Self {
        Self { storage, health }
    }

    /// Process metrics data from OTLP request
    pub async fn process(
        &self,
        request: ExportMetricsServiceRequest,
    ) -> Result<MetricsProcessResult> {
        let metrics_received: usize = request
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
            metrics_received,
            request.resource_metrics.len()
        );

        let conversion = conversion::convert_metrics_with_drops(request);
        // One atomic transaction for the whole export (see LogsHandler).
        if !conversion.metrics.is_empty() {
            if let Err(e) = self.storage.write_metric_batch(&conversion.metrics).await {
                self.health.record_write_failure(e.is_persistent());
                if e.is_persistent() {
                    // Disk full / corruption will keep failing: this is the
                    // operator-level signal, at error level (#256).
                    error!(
                        "metrics write failed with a persistent error (exporter retries will not help until it is fixed): {e}"
                    );
                } else {
                    debug!(
                        "metrics write failed with a transient error (exporter will retry): {e}"
                    );
                }
                return Err(e.into());
            }
            self.health.record_write_success();
        }

        // Conversion drops (exponential histograms, unset values, +Inf
        // overflow buckets) are counted and reported — the "Stored N" line
        // below counts stored rows, not received protos (#256).
        if !conversion.dropped.is_empty() {
            warn!(
                dropped_total = conversion.dropped.total(),
                "Dropped metric telemetry the internal model cannot store: {}",
                conversion.dropped.summary()
            );
        }

        info!(
            "Stored {} of {} metrics",
            conversion.metrics.len(),
            metrics_received
        );
        Ok(MetricsProcessResult {
            metrics_received,
            metrics_stored: conversion.metrics.len(),
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
    async fn test_metrics_handler_process() {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut storage = SqliteBackend::new(config);
        storage
            .initialize()
            .await
            .expect("Failed to initialize storage");
        let handler = MetricsHandler::new(Arc::new(storage));
        let request = ExportMetricsServiceRequest {
            resource_metrics: vec![],
        };
        let result = handler
            .process(request)
            .await
            .expect("empty request succeeds");
        assert_eq!(result.metrics_received, 0);
        assert_eq!(result.metrics_stored, 0);
        assert!(result.dropped.is_empty());
    }
}
