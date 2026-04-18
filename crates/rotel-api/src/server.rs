//! HTTP server setup and configuration

use axum::{middleware, Router};
use std::net::SocketAddr;
use tokio::net::TcpListener;
use tower_http::{compression::CompressionLayer, trace::TraceLayer};
use tracing::info;

use crate::{
    config::ApiConfig,
    error::ApiResult,
    middleware::{cors, logging},
    routes,
};

/// API server instance
pub struct ApiServer {
    config: ApiConfig,
    router: Router,
}

impl ApiServer {
    /// Create a new API server with the given configuration
    pub fn new(config: ApiConfig) -> Self {
        let router = Self::create_router(&config);

        Self { config, router }
    }

    /// Create the router with all middleware and routes
    fn create_router(config: &ApiConfig) -> Router {
        let mut router = routes::create_router();

        // Add compression middleware
        router = router.layer(CompressionLayer::new());

        // Add CORS middleware if enabled
        if config.enable_cors {
            router = router.layer(cors::create_cors_layer(config.cors_origins.clone()));
        }

        // Add request logging middleware if enabled
        if config.enable_request_logging {
            router = router
                .layer(middleware::from_fn(logging::add_request_id))
                .layer(middleware::from_fn(logging::log_request));
        }

        // Add tracing middleware
        router = router.layer(TraceLayer::new_for_http());

        router
    }

    /// Get the bind address
    pub fn bind_address(&self) -> SocketAddr {
        self.config.bind_address
    }

    /// Start the server and listen for incoming connections
    pub async fn serve(self) -> ApiResult<()> {
        let addr = self.config.bind_address;

        info!("Starting API server on {}", addr);

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| crate::error::ApiError::InternalError(format!("Failed to bind: {}", e)))?;

        info!("API server listening on {}", addr);

        axum::serve(listener, self.router)
            .await
            .map_err(|e| crate::error::ApiError::InternalError(format!("Server error: {}", e)))?;

        Ok(())
    }

    /// Create a test server for integration tests
    #[cfg(test)]
    pub fn into_router(self) -> Router {
        self.router
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_creation() {
        let config = ApiConfig::default();
        let server = ApiServer::new(config);
        assert_eq!(server.bind_address().to_string(), "127.0.0.1:8080");
    }

    #[test]
    fn test_custom_bind_address() {
        let addr: SocketAddr = "0.0.0.0:9090".parse().unwrap();
        let config = ApiConfig::new(addr);
        let server = ApiServer::new(config);
        assert_eq!(server.bind_address().to_string(), "0.0.0.0:9090");
    }
}

// Made with Bob
