// gRPC TraceService implementation for OTLP

use crate::signals::TracesHandler;
use opentelemetry_proto::tonic::collector::trace::v1::{
    trace_service_server::{TraceService, TraceServiceServer},
    ExportTraceServiceRequest, ExportTraceServiceResponse,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{debug, error};

/// Implementation of the OTLP TraceService
pub struct TraceServiceImpl {
    handler: Arc<TracesHandler>,
}

impl TraceServiceImpl {
    /// Create a new TraceService implementation
    pub fn new(handler: Arc<TracesHandler>) -> Self {
        Self { handler }
    }

    /// Convert into a tonic service
    pub fn into_service(self) -> TraceServiceServer<Self> {
        TraceServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl TraceService for TraceServiceImpl {
    async fn export(
        &self,
        request: Request<ExportTraceServiceRequest>,
    ) -> Result<Response<ExportTraceServiceResponse>, Status> {
        debug!("Received trace export request");

        let req = request.into_inner();

        // Process the traces through the handler
        match self.handler.process(req).await {
            Ok(_) => {
                debug!("Successfully processed traces");
                Ok(Response::new(ExportTraceServiceResponse {
                    partial_success: None,
                }))
            },
            Err(e) => {
                error!("Failed to process traces: {}", e);
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
    async fn test_trace_service_creation() {
        let mut storage = SqliteBackend::new(StorageConfig::default());
        storage
            .initialize()
            .await
            .expect("Failed to initialize storage");
        let handler = Arc::new(TracesHandler::new(Arc::new(storage)));
        let _service = TraceServiceImpl::new(handler);
    }

    #[tokio::test]
    async fn test_trace_export_empty() {
        let mut storage = SqliteBackend::new(StorageConfig::default());
        storage
            .initialize()
            .await
            .expect("Failed to initialize storage");
        let handler = Arc::new(TracesHandler::new(Arc::new(storage)));
        let service = TraceServiceImpl::new(handler);

        let request = Request::new(ExportTraceServiceRequest {
            resource_spans: vec![],
        });

        let response = service.export(request).await;
        assert!(response.is_ok());
    }
}
