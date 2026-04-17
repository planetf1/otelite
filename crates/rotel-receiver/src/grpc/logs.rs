// gRPC LogsService implementation for OTLP

use crate::signals::LogsHandler;
use opentelemetry_proto::tonic::collector::logs::v1::{
    logs_service_server::{LogsService, LogsServiceServer},
    ExportLogsServiceRequest, ExportLogsServiceResponse,
};
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::{debug, error};

/// Implementation of the OTLP LogsService
pub struct LogsServiceImpl {
    handler: Arc<LogsHandler>,
}

impl LogsServiceImpl {
    /// Create a new LogsService implementation
    pub fn new(handler: Arc<LogsHandler>) -> Self {
        Self { handler }
    }

    /// Convert into a tonic service
    pub fn into_service(self) -> LogsServiceServer<Self> {
        LogsServiceServer::new(self)
    }
}

#[tonic::async_trait]
impl LogsService for LogsServiceImpl {
    async fn export(
        &self,
        request: Request<ExportLogsServiceRequest>,
    ) -> Result<Response<ExportLogsServiceResponse>, Status> {
        debug!("Received logs export request");

        let req = request.into_inner();

        // Process the logs through the handler
        match self.handler.process(req).await {
            Ok(_) => {
                debug!("Successfully processed logs");
                Ok(Response::new(ExportLogsServiceResponse {
                    partial_success: None,
                }))
            },
            Err(e) => {
                error!("Failed to process logs: {}", e);
                Err(e.to_grpc_status())
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_logs_service_creation() {
        let handler = Arc::new(LogsHandler::new());
        let _service = LogsServiceImpl::new(handler);
    }

    #[tokio::test]
    async fn test_logs_export_empty() {
        let handler = Arc::new(LogsHandler::new());
        let service = LogsServiceImpl::new(handler);

        let request = Request::new(ExportLogsServiceRequest {
            resource_logs: vec![],
        });

        let response = service.export(request).await;
        assert!(response.is_ok());
    }
}

// Made with Bob
