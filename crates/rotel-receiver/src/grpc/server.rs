// gRPC server implementation for OTLP receiver

use crate::config::ReceiverConfig;
use crate::error::ReceiverError;
use crate::health::HealthChecker;
use crate::signals::{LogsHandler, MetricsHandler, TracesHandler};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Notify, Semaphore};
use tonic::transport::Server;
use tracing::{info, warn};

/// gRPC server for OTLP protocol with backpressure support
pub struct GrpcServer {
    config: ReceiverConfig,
    health_checker: Arc<HealthChecker>,
    shutdown_notify: Arc<Notify>,
    /// Semaphore for limiting concurrent requests (backpressure)
    request_semaphore: Arc<Semaphore>,
    /// Signal handlers
    metrics_handler: Arc<MetricsHandler>,
    logs_handler: Arc<LogsHandler>,
    traces_handler: Arc<TracesHandler>,
}

impl GrpcServer {
    /// Create a new gRPC server with the given configuration
    pub fn new(config: ReceiverConfig, storage: Arc<dyn rotel_storage::StorageBackend>) -> Self {
        // Default to 1000 concurrent requests for backpressure
        let max_concurrent_requests = 1000;

        Self {
            config,
            health_checker: Arc::new(HealthChecker::new()),
            shutdown_notify: Arc::new(Notify::new()),
            request_semaphore: Arc::new(Semaphore::new(max_concurrent_requests)),
            metrics_handler: Arc::new(MetricsHandler::new(storage.clone())),
            logs_handler: Arc::new(LogsHandler::new(storage.clone())),
            traces_handler: Arc::new(TracesHandler::new(storage)),
        }
    }

    /// Create a new gRPC server with custom concurrency limit
    pub fn with_concurrency_limit(
        config: ReceiverConfig,
        storage: Arc<dyn rotel_storage::StorageBackend>,
        max_concurrent: usize,
    ) -> Self {
        Self {
            config,
            health_checker: Arc::new(HealthChecker::new()),
            shutdown_notify: Arc::new(Notify::new()),
            request_semaphore: Arc::new(Semaphore::new(max_concurrent)),
            metrics_handler: Arc::new(MetricsHandler::new(storage.clone())),
            logs_handler: Arc::new(LogsHandler::new(storage.clone())),
            traces_handler: Arc::new(TracesHandler::new(storage)),
        }
    }

    /// Start the gRPC server
    pub async fn start(&self) -> Result<(), ReceiverError> {
        let addr = self.config.grpc_addr;

        info!("Starting gRPC server on {}", addr);

        // Mark server as ready
        self.health_checker.set_ready(true);

        // Use pre-created signal handlers
        let metrics_handler = self.metrics_handler.clone();
        let logs_handler = self.logs_handler.clone();
        let traces_handler = self.traces_handler.clone();

        // Create gRPC services
        let metrics_service = crate::grpc::metrics::MetricsServiceImpl::new(metrics_handler);
        let logs_service = crate::grpc::logs::LogsServiceImpl::new(logs_handler);
        let traces_service = crate::grpc::traces::TraceServiceImpl::new(traces_handler);

        // Build server with backpressure configuration
        // Note: Compression is configured per-service in tonic 0.11+
        let mut server = Server::builder()
            // Set concurrency limits for backpressure
            .concurrency_limit_per_connection(256)
            // Set timeout for requests
            .timeout(Duration::from_secs(30))
            // Set TCP keepalive
            .tcp_keepalive(Some(Duration::from_secs(60)))
            // Set max frame size (16MB)
            .max_frame_size(Some(16 * 1024 * 1024));

        // Add services and start server
        let shutdown_notify = self.shutdown_notify.clone();
        let health_checker = self.health_checker.clone();

        tokio::spawn(async move {
            let result = server
                .add_service(metrics_service.into_service())
                .add_service(logs_service.into_service())
                .add_service(traces_service.into_service())
                .serve_with_shutdown(addr, async move {
                    shutdown_notify.notified().await;
                    info!("Shutting down gRPC server");
                    health_checker.set_ready(false);
                })
                .await;

            if let Err(e) = result {
                warn!("gRPC server error: {}", e);
            }
        });

        Ok(())
    }

    /// Trigger graceful shutdown
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_one();
    }

    /// Get health checker
    pub fn health_checker(&self) -> Arc<HealthChecker> {
        self.health_checker.clone()
    }

    /// Get request semaphore for backpressure control
    pub fn request_semaphore(&self) -> Arc<Semaphore> {
        self.request_semaphore.clone()
    }

    /// Check if server can accept more requests (backpressure check)
    pub fn can_accept_request(&self) -> bool {
        self.request_semaphore.available_permits() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rotel_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};

    fn create_test_storage() -> Arc<dyn StorageBackend> {
        let storage = SqliteBackend::new(StorageConfig::default());
        Arc::new(storage)
    }

    #[test]
    fn test_grpc_server_creation() {
        let config = ReceiverConfig::new();
        let storage = create_test_storage();
        let server = GrpcServer::new(config, storage);
        assert!(server.health_checker().is_alive());
    }

    #[test]
    fn test_grpc_server_shutdown() {
        let config = ReceiverConfig::new();
        let storage = create_test_storage();
        let server = GrpcServer::new(config, storage);
        server.shutdown();
        // Shutdown notification sent successfully
    }

    #[test]
    fn test_grpc_server_with_concurrency_limit() {
        let config = ReceiverConfig::new();
        let storage = create_test_storage();
        let server = GrpcServer::with_concurrency_limit(config, storage, 100);
        assert!(server.can_accept_request());
        assert_eq!(server.request_semaphore().available_permits(), 100);
    }

    #[test]
    fn test_grpc_server_backpressure_check() {
        let config = ReceiverConfig::new();
        let storage = create_test_storage();
        let server = GrpcServer::new(config, storage);
        // Default limit is 1000
        assert!(server.can_accept_request());
        assert_eq!(server.request_semaphore().available_permits(), 1000);
    }
}

// Made with Bob
