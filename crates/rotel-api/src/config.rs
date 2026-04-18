//! API server configuration

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Configuration for the API server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiConfig {
    /// Address to bind the server to (e.g., "127.0.0.1:8080")
    pub bind_address: SocketAddr,

    /// Maximum number of concurrent connections
    pub max_connections: usize,

    /// Request timeout in seconds
    pub request_timeout_secs: u64,

    /// Enable CORS (Cross-Origin Resource Sharing)
    pub enable_cors: bool,

    /// Allowed CORS origins (e.g., ["http://localhost:3000"])
    pub cors_origins: Vec<String>,

    /// Enable request logging
    pub enable_request_logging: bool,

    /// Enable OpenAPI documentation endpoint
    pub enable_docs: bool,

    /// Path to storage database file
    pub storage_path: String,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:8080".parse().unwrap(),
            max_connections: 1000,
            request_timeout_secs: 30,
            enable_cors: true,
            cors_origins: vec![
                "http://localhost:3000".to_string(),
                "http://localhost:5173".to_string(), // Vite default
            ],
            enable_request_logging: true,
            enable_docs: true,
            storage_path: "rotel.db".to_string(),
        }
    }
}

impl ApiConfig {
    /// Create a new API configuration with custom bind address
    pub fn new(bind_address: SocketAddr) -> Self {
        Self {
            bind_address,
            ..Default::default()
        }
    }

    /// Set the storage path
    pub fn with_storage_path(mut self, path: impl Into<String>) -> Self {
        self.storage_path = path.into();
        self
    }

    /// Set CORS origins
    pub fn with_cors_origins(mut self, origins: Vec<String>) -> Self {
        self.cors_origins = origins;
        self
    }

    /// Disable CORS
    pub fn without_cors(mut self) -> Self {
        self.enable_cors = false;
        self
    }

    /// Disable documentation endpoint
    pub fn without_docs(mut self) -> Self {
        self.enable_docs = false;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ApiConfig::default();
        assert_eq!(config.bind_address.to_string(), "127.0.0.1:8080");
        assert_eq!(config.max_connections, 1000);
        assert!(config.enable_cors);
        assert!(config.enable_docs);
    }

    #[test]
    fn test_custom_config() {
        let addr: SocketAddr = "0.0.0.0:9090".parse().unwrap();
        let config = ApiConfig::new(addr)
            .with_storage_path("/tmp/test.db")
            .without_cors();

        assert_eq!(config.bind_address.to_string(), "0.0.0.0:9090");
        assert_eq!(config.storage_path, "/tmp/test.db");
        assert!(!config.enable_cors);
    }
}

// Made with Bob
