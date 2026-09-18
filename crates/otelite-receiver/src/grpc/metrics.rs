// gRPC MetricsService implementation for OTLP

use crate::signals::MetricsHandler;
use opentelemetry_proto::tonic::collector::metrics::v1::{
    metrics_service_server::{MetricsService, MetricsServiceServer},
    ExportMetricsPartialSuccess, ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use std::sync::Arc;
use tokio::sync::Semaphore;
use tonic::{Request, Response, Status};
use tracing::{debug, error};

/// Implementation of the OTLP MetricsService
pub struct MetricsServiceImpl {
    handler: Arc<MetricsHandler>,
    /// Acquired per in-flight export: real backpressure (#256). Once the
    /// server-wide permit pool is exhausted, exports are rejected with
    /// UNAVAILABLE, which OTLP exporters retry with backoff.
    request_semaphore: Arc<Semaphore>,
}

impl MetricsServiceImpl {
    /// Create a new MetricsService implementation
    pub fn new(handler: Arc<MetricsHandler>) -> Self {
        Self::with_concurrency_limit(handler, 1000)
    }

    /// Create a service bounded to `max_concurrent` in-flight exports.
    pub fn with_concurrency_limit(handler: Arc<MetricsHandler>, max_concurrent: usize) -> Self {
        Self {
            handler,
            request_semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    /// Convert into a tonic service
    pub fn into_service(self) -> MetricsServiceServer<Self> {
        MetricsServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl MetricsService for MetricsServiceImpl {
    async fn export(
        &self,
        request: Request<ExportMetricsServiceRequest>,
    ) -> Result<Response<ExportMetricsServiceResponse>, Status> {
        debug!("Received metrics export request");

        // Hold the permit for the whole export; dropped at return.
        let _permit = self
            .request_semaphore
            .clone()
            .try_acquire_owned()
            .map_err(|_| {
                Status::unavailable("otelite receiver at its concurrency limit; retry the export")
            })?;

        let req = request.into_inner();

        // Process the metrics through the handler
        match self.handler.process(req).await {
            Ok(result) => {
                debug!("Successfully processed metrics");
                let rejected = result.dropped.rejected_data_points();
                Ok(Response::new(ExportMetricsServiceResponse {
                    // Partial success reports the data points conversion
                    // dropped entirely (exponential histograms, unset
                    // values); a 0/absent field means fully accepted
                    // (#256).
                    partial_success: (rejected > 0).then(|| ExportMetricsPartialSuccess {
                        rejected_data_points: i64::try_from(rejected).unwrap_or(i64::MAX),
                        error_message: format!(
                            "otelite dropped telemetry it cannot store: {}",
                            result.dropped.summary()
                        ),
                    }),
                }))
            },
            Err(e) => {
                error!("Failed to process metrics: {}", e);
                Err(e.to_grpc_status())
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};

    #[tokio::test]
    async fn test_metrics_service_creation() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let config = StorageConfig {
            data_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut storage = SqliteBackend::new(config);
        storage
            .initialize()
            .await
            .expect("Failed to initialize storage");
        let handler = Arc::new(MetricsHandler::new(Arc::new(storage)));
        let _service = MetricsServiceImpl::new(handler);
    }

    #[tokio::test]
    async fn test_metrics_export_empty() {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let config = StorageConfig {
            data_dir: temp_dir.path().to_path_buf(),
            ..Default::default()
        };
        let mut storage = SqliteBackend::new(config);
        storage
            .initialize()
            .await
            .expect("Failed to initialize storage");
        let handler = Arc::new(MetricsHandler::new(Arc::new(storage)));
        let service = MetricsServiceImpl::new(handler);

        let request = Request::new(ExportMetricsServiceRequest {
            resource_metrics: vec![],
        });

        let response = service.export(request).await;
        assert!(response.is_ok());
    }
}
