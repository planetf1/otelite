//! Dashboard configuration

use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

/// Configuration for the dashboard server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardConfig {
    /// Address to bind the dashboard server to
    pub bind_address: SocketAddr,

    /// Enable CORS (for development)
    pub enable_cors: bool,

    /// Path to storage database
    pub storage_path: String,

    /// Maximum number of items to return per page
    pub max_page_size: usize,

    /// Default page size for queries
    pub default_page_size: usize,

    /// Cache size in MB for query results
    pub cache_size_mb: usize,

    /// Auto-refresh interval in seconds (0 = disabled)
    pub auto_refresh_interval_secs: u64,
}

impl Default for DashboardConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:3000".parse().unwrap(),
            enable_cors: false,
            storage_path: "otelite.db".to_string(),
            max_page_size: 1000,
            default_page_size: 100,
            cache_size_mb: 10,
            auto_refresh_interval_secs: 1,
        }
    }
}

impl DashboardConfig {
    /// Create a new configuration with custom bind address
    pub fn with_bind_address(mut self, addr: SocketAddr) -> Self {
        self.bind_address = addr;
        self
    }

    /// Enable CORS for development
    pub fn with_cors(mut self, enable: bool) -> Self {
        self.enable_cors = enable;
        self
    }

    /// Set storage path
    pub fn with_storage_path(mut self, path: impl Into<String>) -> Self {
        self.storage_path = path.into();
        self
    }
}
