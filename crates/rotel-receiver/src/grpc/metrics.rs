// gRPC MetricsService implementation for OTLP

use crate::signals::MetricsHandler;
use opentelemetry_proto::tonic::collector::metrics::v1::{
    metrics_service_server::{MetricsService, MetricsServiceServer},
    ExportMetricsServiceRequest, ExportMetricsServiceResponse,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{debug, error};

/// Implementation of the OTLP MetricsService
pub struct MetricsServiceImpl {
    handler: Arc<MetricsHandler>,
}

impl MetricsServiceImpl {
    /// Create a new MetricsService implementation
    pub fn new(handler: Arc<MetricsHandler>) -> Self {
        Self { handler }
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

        let req = request.into_inner();

        // Process the metrics through the handler
        match self.handler.process(req).await {
            Ok(_) => {
                debug!("Successfully processed metrics");
                Ok(Response::new(ExportMetricsServiceResponse {
                    partial_success: None,
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
    use rotel_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};

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
