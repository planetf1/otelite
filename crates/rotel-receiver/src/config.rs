//! Configuration for the OTLP receiver

use std::net::SocketAddr;

/// Configuration for the OTLP receiver
#[derive(Debug, Clone)]
pub struct ReceiverConfig {
    /// gRPC server address (default: 0.0.0.0:4317)
    pub grpc_addr: SocketAddr,

    /// HTTP server address (default: 0.0.0.0:4318)
    pub http_addr: SocketAddr,

    /// Enable gRPC compression
    pub grpc_compression: bool,

    /// Enable HTTP compression
    pub http_compression: bool,

    /// Maximum message size in bytes (default: 10MB)
    pub max_message_size: usize,
}

impl Default for ReceiverConfig {
    fn default() -> Self {
        Self {
            grpc_addr: "0.0.0.0:4317".parse().expect("valid address"),
            http_addr: "0.0.0.0:4318".parse().expect("valid address"),
            grpc_compression: true,
            http_compression: true,
            max_message_size: 10 * 1024 * 1024, // 10MB
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

    /// Enable or disable gRPC compression
    pub fn with_grpc_compression(mut self, enabled: bool) -> Self {
        self.grpc_compression = enabled;
        self
    }

    /// Enable or disable HTTP compression
    pub fn with_http_compression(mut self, enabled: bool) -> Self {
        self.http_compression = enabled;
        self
    }

    /// Set the maximum message size
    pub fn with_max_message_size(mut self, size: usize) -> Self {
        self.max_message_size = size;
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
        assert!(config.grpc_compression);
        assert!(config.http_compression);
        assert_eq!(config.max_message_size, 10 * 1024 * 1024);
    }

    #[test]
    fn test_builder_pattern() {
        let config = ReceiverConfig::new()
            .with_grpc_addr("127.0.0.1:5317".parse().unwrap())
            .with_http_addr("127.0.0.1:5318".parse().unwrap())
            .with_grpc_compression(false)
            .with_max_message_size(5 * 1024 * 1024);

        assert_eq!(config.grpc_addr.port(), 5317);
        assert_eq!(config.http_addr.port(), 5318);
        assert!(!config.grpc_compression);
        assert_eq!(config.max_message_size, 5 * 1024 * 1024);
    }
}

// Made with Bob
