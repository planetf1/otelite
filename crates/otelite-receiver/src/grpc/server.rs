// gRPC server implementation for OTLP receiver

use crate::config::ReceiverConfig;
use crate::error::ReceiverError;
use crate::health::HealthChecker;
use crate::signals::{LogsHandler, MetricsHandler, TracesHandler};
use futures_util::Stream;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;
use tokio::sync::{Notify, Semaphore};
use tonic::transport::Server;
use tracing::{info, warn};

/// gRPC server for OTLP protocol with backpressure support
pub struct GrpcServer {
    config: ReceiverConfig,
    health_checker: Arc<HealthChecker>,
    shutdown_notify: Arc<Notify>,
    local_addr: Arc<tokio::sync::RwLock<Option<std::net::SocketAddr>>>,
    /// Semaphore for limiting concurrent requests (backpressure)
    request_semaphore: Arc<Semaphore>,
    /// Signal handlers
    metrics_handler: Arc<MetricsHandler>,
    logs_handler: Arc<LogsHandler>,
    traces_handler: Arc<TracesHandler>,
}

impl GrpcServer {
    /// Create a new gRPC server with the given configuration
    pub fn new(
        config: ReceiverConfig,
        storage: Arc<dyn otelite_core::storage::StorageBackend>,
    ) -> Self {
        // Default to 1000 concurrent requests for backpressure
        let max_concurrent_requests = 1000;

        Self {
            config,
            health_checker: Arc::new(HealthChecker::new()),
            shutdown_notify: Arc::new(Notify::new()),
            local_addr: Arc::new(tokio::sync::RwLock::new(None)),
            request_semaphore: Arc::new(Semaphore::new(max_concurrent_requests)),
            metrics_handler: Arc::new(MetricsHandler::new(storage.clone())),
            logs_handler: Arc::new(LogsHandler::new(storage.clone())),
            traces_handler: Arc::new(TracesHandler::new(storage)),
        }
    }

    /// Create a new gRPC server with custom concurrency limit
    pub fn with_concurrency_limit(
        config: ReceiverConfig,
        storage: Arc<dyn otelite_core::storage::StorageBackend>,
        max_concurrent: usize,
    ) -> Self {
        Self {
            config,
            health_checker: Arc::new(HealthChecker::new()),
            shutdown_notify: Arc::new(Notify::new()),
            local_addr: Arc::new(tokio::sync::RwLock::new(None)),
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

        // Bind before spawning so a taken port fails `start()` loudly, the
        // same way the HTTP receiver does. Previously the bind happened
        // inside the spawned task and a failure there was only logged while
        // health already reported ready — the daemon would then accept no
        // gRPC telemetry at all with no visible failure.
        let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
            ReceiverError::Internal(format!("Failed to bind gRPC server on {addr}: {e}"))
        })?;
        let bound_addr = listener
            .local_addr()
            .map_err(|e| ReceiverError::Internal(format!("Failed to get local address: {}", e)))?;
        *self.local_addr.write().await = Some(bound_addr);

        info!("gRPC server bound to {}", bound_addr);

        // Mark server as ready only once the socket is actually bound.
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
            // Set max frame size to the HTTP/2 maximum (2^24 - 1 = 16,777,215).
            // 16 * 1024 * 1024 = 16,777,216 exceeds this by 1 and panics in h2.
            // (TCP keepalive is applied per accepted connection below, because
            // serve_with_incoming ignores the builder-level setting.)
            .max_frame_size(Some((1 << 24) - 1));

        // Add services and start server
        let shutdown_notify = self.shutdown_notify.clone();
        let health_checker = self.health_checker.clone();

        tokio::spawn(async move {
            let result = server
                .add_service(metrics_service.into_service())
                .add_service(logs_service.into_service())
                .add_service(traces_service.into_service())
                .serve_with_incoming_shutdown(ConnectionStream(listener), async move {
                    shutdown_notify.notified().await;
                    info!("Shutting down gRPC server");
                    health_checker.set_ready(false);
                })
                .await;

            if let Err(e) = result {
                // The bind already succeeded before spawn, so an error here
                // is an accept-loop or protocol failure, not a bind failure.
                warn!("gRPC server error: {}", e);
            }
        });

        Ok(())
    }

    /// Trigger graceful shutdown
    pub fn shutdown(&self) {
        self.shutdown_notify.notify_one();
    }

    /// Get the local address the server is bound to
    /// Returns None if the server hasn't been started yet
    pub async fn local_addr(&self) -> Option<std::net::SocketAddr> {
        *self.local_addr.read().await
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

/// Stream of accepted TCP connections from a listener that was bound (and
/// therefore verified) before the server task was spawned.
struct ConnectionStream(tokio::net::TcpListener);

impl Stream for ConnectionStream {
    type Item = std::io::Result<tokio::net::TcpStream>;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<std::io::Result<tokio::net::TcpStream>>> {
        match self.0.poll_accept(cx) {
            Poll::Ready(Ok((stream, _addr))) => {
                // serve_with_incoming ignores builder-level TCP keepalive, so
                // apply it per accepted connection to keep dead-peer
                // detection equivalent to the previous behaviour.
                // serve_with_incoming ignores builder-level TCP keepalive, so
                // apply it per accepted connection (via socket2, since tokio's
                // TcpStream exposes no keepalive setter).
                let sock = socket2::SockRef::from(&stream);
                let _ = sock.set_keepalive(true);
                let keepalive = socket2::TcpKeepalive::new().with_time(Duration::from_secs(60));
                let _ = sock.set_tcp_keepalive(&keepalive);
                Poll::Ready(Some(Ok(stream)))
            },
            Poll::Ready(Err(e)) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use otelite_storage::{sqlite::SqliteBackend, StorageBackend, StorageConfig};
    use tempfile::TempDir;

    fn create_test_storage() -> (Arc<dyn StorageBackend>, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let storage = SqliteBackend::new(config);
        (Arc::new(storage), temp_dir)
    }

    /// Test storage with the database initialized and writes enabled.
    async fn create_initialized_storage() -> (Arc<dyn StorageBackend>, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let config = StorageConfig::default().with_data_dir(temp_dir.path().to_path_buf());
        let mut storage = SqliteBackend::new(config);
        storage
            .initialize()
            .await
            .expect("test storage initialises");
        (Arc::new(storage), temp_dir)
    }

    #[test]
    fn test_grpc_server_creation() {
        let config = ReceiverConfig::new();
        let (storage, _temp_dir) = create_test_storage();
        let server = GrpcServer::new(config, storage);
        assert!(server.health_checker().is_alive());
    }

    #[test]
    fn test_grpc_server_shutdown() {
        let config = ReceiverConfig::new();
        let (storage, _temp_dir) = create_test_storage();
        let server = GrpcServer::new(config, storage);
        server.shutdown();
        // Shutdown notification sent successfully
    }

    #[test]
    fn test_grpc_server_with_concurrency_limit() {
        let config = ReceiverConfig::new();
        let (storage, _temp_dir) = create_test_storage();
        let server = GrpcServer::with_concurrency_limit(config, storage, 100);
        assert!(server.can_accept_request());
        assert_eq!(server.request_semaphore().available_permits(), 100);
    }

    #[test]
    fn test_grpc_server_backpressure_check() {
        let config = ReceiverConfig::new();
        let (storage, _temp_dir) = create_test_storage();
        let server = GrpcServer::new(config, storage);
        // Default limit is 1000
        assert!(server.can_accept_request());
        assert_eq!(server.request_semaphore().available_permits(), 1000);
    }

    /// Get a free port by binding to port 0 and releasing it.
    async fn free_port() -> u16 {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        listener.local_addr().unwrap().port()
    }

    /// Regression test: a taken port must fail `start()` instead of being
    /// logged inside a spawned task while health reports ready. Before the
    /// fix, starting on an occupied port returned `Ok(())` and the gRPC
    /// endpoint silently accepted no telemetry.
    #[tokio::test]
    async fn test_grpc_start_fails_when_port_in_use() {
        let blocker = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = blocker.local_addr().unwrap();

        let (storage, _temp_dir) = create_test_storage();
        let config = ReceiverConfig::new().with_grpc_addr(addr);
        let server = GrpcServer::new(config, storage);

        let err = server
            .start()
            .await
            .expect_err("start must fail when the gRPC port is already bound");
        assert!(
            err.to_string().contains("bind"),
            "error should mention the bind failure, got: {err}"
        );
        assert!(
            !server.health_checker().is_ready(),
            "health must not report ready after a failed bind"
        );
        assert!(
            server.local_addr().await.is_none(),
            "local_addr must stay unset after a failed bind"
        );
        // Keep the blocker alive until here so the port stays occupied.
        drop(blocker);
    }

    #[tokio::test]
    async fn test_grpc_start_binds_and_reports_addr() {
        let (storage, _temp_dir) = create_test_storage();
        let port = free_port().await;
        let config =
            ReceiverConfig::new().with_grpc_addr(format!("127.0.0.1:{port}").parse().unwrap());
        let server = GrpcServer::new(config, storage);

        server
            .start()
            .await
            .expect("start on a free port must succeed");
        assert!(server.health_checker().is_ready());
        assert_eq!(
            server.local_addr().await,
            Some(format!("127.0.0.1:{port}").parse().unwrap())
        );

        server.shutdown();
        // Give the serve task a moment to unwind before dropping the server.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    /// Regression test: an actual OTLP gRPC export succeeds against the
    /// bound listener (proves the pre-bound listener still serves, i.e. the
    /// ConnectionStream adapter works end to end).
    #[tokio::test]
    async fn test_grpc_start_accepts_export() {
        let (storage, _temp_dir) = create_initialized_storage().await;
        let port = free_port().await;
        let config =
            ReceiverConfig::new().with_grpc_addr(format!("127.0.0.1:{port}").parse().unwrap());
        let server = GrpcServer::new(config, storage.clone());

        server
            .start()
            .await
            .expect("start on a free port must succeed");

        let endpoint = tonic::transport::Endpoint::from_shared(format!("http://127.0.0.1:{port}"))
            .expect("valid endpoint")
            .connect_timeout(Duration::from_secs(5));
        let channel = endpoint
            .connect()
            .await
            .expect("client must connect to the bound port");
        use opentelemetry_proto::tonic::collector::trace::v1::{
            trace_service_client::TraceServiceClient, ExportTraceServiceRequest,
        };
        use opentelemetry_proto::tonic::trace::v1::{ResourceSpans, ScopeSpans, Span as ProtoSpan};

        let mut client = TraceServiceClient::new(channel);

        let request = tonic::Request::new(ExportTraceServiceRequest {
            resource_spans: vec![ResourceSpans {
                resource: None,
                scope_spans: vec![ScopeSpans {
                    scope: None,
                    spans: vec![ProtoSpan {
                        trace_id: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16],
                        span_id: vec![1, 2, 3, 4, 5, 6, 7, 8],
                        name: "grpc-bind-check".to_string(),
                        start_time_unix_nano: 1_700_000_000_000_000_000,
                        end_time_unix_nano: 1_700_000_000_000_000_000,
                        ..Default::default()
                    }],
                    schema_url: String::new(),
                }],
                schema_url: String::new(),
            }],
        });

        let response = client
            .export(request)
            .await
            .expect("export to pre-bound listener must succeed")
            .into_inner();

        // No rejections: partial_success is either absent or reports 0.
        assert!(
            response.partial_success.as_ref().map(|p| p.rejected_spans) == Some(0)
                || response.partial_success.is_none(),
            "span must not be rejected"
        );

        // The span must actually be in the test database (fresh TempDir).
        let stats = storage.stats().await.expect("stats query on test storage");
        assert_eq!(stats.span_count, 1);

        server.shutdown();
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
