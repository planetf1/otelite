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
use tokio::sync::Notify;
use tonic::transport::Server;
use tracing::{info, warn};

/// gRPC server for OTLP protocol with backpressure support
pub struct GrpcServer {
    config: ReceiverConfig,
    health_checker: Arc<HealthChecker>,
    shutdown_notify: Arc<Notify>,
    local_addr: Arc<tokio::sync::RwLock<Option<std::net::SocketAddr>>>,
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
        Self::with_health(config, storage, Arc::new(HealthChecker::new()))
    }

    /// Create a server that shares its health state (and therefore its
    /// `/health` view) with another transport, e.g. the HTTP receiver
    /// (#256): a write failing on either transport degrades the combined
    /// health.
    pub fn with_health(
        config: ReceiverConfig,
        storage: Arc<dyn otelite_core::storage::StorageBackend>,
        health_checker: Arc<HealthChecker>,
    ) -> Self {
        Self {
            config,
            health_checker: health_checker.clone(),
            shutdown_notify: Arc::new(Notify::new()),
            local_addr: Arc::new(tokio::sync::RwLock::new(None)),
            metrics_handler: Arc::new(MetricsHandler::with_health(
                storage.clone(),
                health_checker.clone(),
            )),
            logs_handler: Arc::new(LogsHandler::with_health(
                storage.clone(),
                health_checker.clone(),
            )),
            traces_handler: Arc::new(TracesHandler::with_health(storage, health_checker)),
        }
    }

    /// Create a new gRPC server with custom concurrency limit
    pub fn with_concurrency_limit(
        config: ReceiverConfig,
        storage: Arc<dyn otelite_core::storage::StorageBackend>,
        max_concurrent: usize,
    ) -> Self {
        Self::new(config.with_max_concurrent_requests(max_concurrent), storage)
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

        // Create gRPC services. The concurrency limit bounds in-flight
        // exports across all connections: once exhausted, exports fail
        // immediately with UNAVAILABLE (which exporters retry) instead of
        // piling up unbounded memory and blocking-pool occupancy (#256).
        let limit = self.config.max_concurrent_requests;
        let metrics_service = crate::grpc::metrics::MetricsServiceImpl::with_concurrency_limit(
            metrics_handler,
            limit,
        );
        let logs_service =
            crate::grpc::logs::LogsServiceImpl::with_concurrency_limit(logs_handler, limit);
        let traces_service =
            crate::grpc::traces::TraceServiceImpl::with_concurrency_limit(traces_handler, limit);

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

        // Add services and start server.
        //
        // Message-size limits are per-service in tonic 0.14 (the Server
        // builder has no builder-level setting): without these, exports
        // above tonic's 4 MB decode default are rejected even when the
        // configured limit is higher, and a misconfigured exporter could
        // otherwise stream messages the parser was never sized for.
        // Oversized messages come back as RESOURCE_EXHAUSTED.
        let max_message_size = self.config.max_message_size;

        let shutdown_notify = self.shutdown_notify.clone();
        let health_checker = self.health_checker.clone();

        tokio::spawn(async move {
            let result = server
                .add_service(
                    metrics_service
                        .into_service()
                        .max_decoding_message_size(max_message_size)
                        .max_encoding_message_size(max_message_size),
                )
                .add_service(
                    logs_service
                        .into_service()
                        .max_decoding_message_size(max_message_size)
                        .max_encoding_message_size(max_message_size),
                )
                .add_service(
                    traces_service
                        .into_service()
                        .max_decoding_message_size(max_message_size)
                        .max_encoding_message_size(max_message_size),
                )
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
        // The limit now lives in the config (#256) and is applied per
        // service in start(); the server must build with it.
        let server = GrpcServer::with_concurrency_limit(config, storage, 100);
        assert!(server.health_checker().is_alive());
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
