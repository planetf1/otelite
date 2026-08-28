//! Metrics signal handler

use crate::{conversion, Result};
use opentelemetry_proto::tonic::collector::metrics::v1::ExportMetricsServiceRequest;
use otelite_core::storage::StorageBackend;
use std::sync::Arc;
use tracing::{debug, info};

/// Handler for metrics signals
#[derive(Clone)]
pub struct MetricsHandler {
    storage: Arc<dyn StorageBackend>,
}

impl MetricsHandler {
    /// Create a new metrics handler
    pub fn new(storage: Arc<dyn StorageBackend>) -> Self {
        Self { storage }
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

        let metrics = conversion::convert_metrics(request);
        // One atomic transaction for the whole export (see LogsHandler).
        if !metrics.is_empty() {
            self.storage.write_metric_batch(&metrics).await?;
        }

        info!("Stored {} metrics", metric_count);
        Ok(())
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
        assert!(handler.process(request).await.is_ok());
    }
}
