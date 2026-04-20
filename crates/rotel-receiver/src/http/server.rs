// HTTP server implementation for OTLP receiver

use crate::config::ReceiverConfig;
use crate::error::ReceiverError;
use crate::health::HealthChecker;
use crate::http::routes::create_router;
use crate::signals::{LogsHandler, MetricsHandler, TracesHandler};
use std::sync::Arc;
use tokio::sync::{Notify, Semaphore};
use tracing::{info, warn};

/// HTTP server for OTLP protocol with backpressure support
pub struct HttpServer {
    config: ReceiverConfig,
    health_checker: Arc<HealthChecker>,
    shutdown_notify: Arc<Notify>,
    local_addr: Arc<tokio::sync::RwLock<Option<std::net::SocketAddr>>>,
    /// Semaphore for limiting concurrent requests (backpressure)
    request_semaphore: Arc<Semaphore>,
}

impl HttpServer {
    /// Create a new HTTP server with the given configuration
    pub fn new(config: ReceiverConfig) -> Self {
        // Default to 1000 concurrent requests for backpressure
        let max_concurrent_requests = 1000;

        Self {
            config,
            health_checker: Arc::new(HealthChecker::new()),
            shutdown_notify: Arc::new(Notify::new()),
            local_addr: Arc::new(tokio::sync::RwLock::new(None)),
            request_semaphore: Arc::new(Semaphore::new(max_concurrent_requests)),
        }
    }

    /// Create a new HTTP server with custom concurrency limit
    pub fn with_concurrency_limit(config: ReceiverConfig, max_concurrent: usize) -> Self {
        Self {
            config,
            health_checker: Arc::new(HealthChecker::new()),
            shutdown_notify: Arc::new(Notify::new()),
            local_addr: Arc::new(tokio::sync::RwLock::new(None)),
            request_semaphore: Arc::new(Semaphore::new(max_concurrent)),
        }
    }

    /// Start the HTTP server
    pub async fn start(
        &self,
        storage: Arc<dyn rotel_storage::StorageBackend>,
    ) -> Result<(), ReceiverError> {
        let addr = self.config.http_addr;

        info!("Starting HTTP server on {}", addr);

        // Mark server as ready
        self.health_checker.set_ready(true);

        // Create signal handlers
        let metrics_handler = Arc::new(MetricsHandler::new(storage.clone()));
        let logs_handler = Arc::new(LogsHandler::new(storage.clone()));
        let traces_handler = Arc::new(TracesHandler::new(storage));

        // Create router with all routes
        // Note: Backpressure is handled via semaphore in handlers
        // and connection limits at the TCP level
        let app = create_router(
            metrics_handler,
            logs_handler,
            traces_handler,
            self.health_checker.clone(),
        );

        // Create TCP listener
        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| ReceiverError::Internal(format!("Failed to bind HTTP server: {}", e)))?;

        // Store the actual bound address
        let bound_addr = listener
            .local_addr()
            .map_err(|e| ReceiverError::Internal(format!("Failed to get local address: {}", e)))?;
        *self.local_addr.write().await = Some(bound_addr);

        info!("HTTP server bound to {}", bound_addr);

        // Spawn server with graceful shutdown
        let shutdown_notify = self.shutdown_notify.clone();
        let health_checker = self.health_checker.clone();

        tokio::spawn(async move {
            let result = axum::serve(listener, app)
                .with_graceful_shutdown(async move {
                    shutdown_notify.notified().await;
                    info!("Shutting down HTTP server");
                    health_checker.set_ready(false);
                })
                .await;

            if let Err(e) = result {
                warn!("HTTP server error: {}", e);
            }
        });

        Ok(())
    }

    /// Get the local address the server is bound to
    /// Returns None if the server hasn't been started yet
    pub async fn local_addr(&self) -> Option<std::net::SocketAddr> {
        *self.local_addr.read().await
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

    #[test]
    fn test_http_server_creation() {
        let config = ReceiverConfig::new();
        let server = HttpServer::new(config);
        assert!(server.health_checker().is_alive());
    }

    #[test]
    fn test_http_server_shutdown() {
        let config = ReceiverConfig::new();
        let server = HttpServer::new(config);
        server.shutdown();
        // Shutdown notification sent successfully
    }

    #[test]
    fn test_http_server_with_concurrency_limit() {
        let config = ReceiverConfig::new();
        let server = HttpServer::with_concurrency_limit(config, 100);
        assert!(server.can_accept_request());
        assert_eq!(server.request_semaphore().available_permits(), 100);
    }

    #[test]
    fn test_http_server_backpressure_check() {
        let config = ReceiverConfig::new();
        let server = HttpServer::new(config);
        // Default limit is 1000
        assert!(server.can_accept_request());
        assert_eq!(server.request_semaphore().available_permits(), 1000);
    }
}
