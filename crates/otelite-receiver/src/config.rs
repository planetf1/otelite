//! Configuration for the OTLP receiver

use std::net::SocketAddr;

/// Configuration for the OTLP receiver
#[derive(Debug, Clone)]
pub struct ReceiverConfig {
    /// gRPC server address (default: 0.0.0.0:4317)
    pub grpc_addr: SocketAddr,

    /// HTTP server address (default: 0.0.0.0:4318)
    pub http_addr: SocketAddr,

    /// Maximum message size in bytes (default: 10MB)
    pub max_message_size: usize,

    /// Maximum concurrent in-flight exports per transport (#256).
    /// Beyond this, HTTP exports get 503 and gRPC exports get
    /// UNAVAILABLE — both retried by OTLP exporters. Default 1000 keeps
    /// headroom for bursty multi-agent workloads while bounding memory
    /// (10 MB body + conversion + write per in-flight export).
    pub max_concurrent_requests: usize,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self {
            grpc_addr: "0.0.0.0:4317".parse().expect("valid address"),
            http_addr: "0.0.0.0:4318".parse().expect("valid address"),
            max_message_size: 10 * 1024 * 1024, // 10MB
            max_concurrent_requests: 1000,
        }
    }
}

impl ReceiverConfig {
    /// Create a new configuration with default values
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the gRPC server address
    pub fn with_grpc_addr(mut self, addr: SocketAddr) -> Self {
        self.grpc_addr = addr;
        self
    }

    /// Set the HTTP server address
    pub fn with_http_addr(mut self, addr: SocketAddr) -> Self {
        self.http_addr = addr;
        self
    }

    /// Set the maximum message size
    pub fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
        self
    }

    /// Set the maximum concurrent in-flight exports (backpressure, #256)
    pub fn with_max_concurrent_requests(mut self, max_concurrent: usize) -> Self {
        self.max_concurrent_requests = max_concurrent;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ReceiverConfig::default();
        assert_eq!(config.grpc_addr.port(), 4317);
        assert_eq!(config.http_addr.port(), 4318);
        assert_eq!(config.max_message_size, 10 * 1024 * 1024);
        assert_eq!(config.max_concurrent_requests, 1000);
    }

    #[test]
    fn test_builder_pattern() {
        let config = ReceiverConfig::new()
            .with_grpc_addr("127.0.0.1:5317".parse().unwrap())
            .with_http_addr("127.0.0.1:5318".parse().unwrap())
            .with_max_message_size(5 * 1024 * 1024)
            .with_max_concurrent_requests(7);

        assert_eq!(config.grpc_addr.port(), 5317);
        assert_eq!(config.http_addr.port(), 5318);
        assert_eq!(config.max_message_size, 5 * 1024 * 1024);
        assert_eq!(config.max_concurrent_requests, 7);
    }
}
