//! HTTP server module for OTLP receiver
//!
//! This module provides HTTP/1.1 and HTTP/2 endpoints for receiving
//! OpenTelemetry Protocol (OTLP) data via HTTP transport.

pub mod handlers;
pub mod middleware;
pub mod routes;
pub mod server;

pub use server::HttpServer;

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_structure() {
        // Verify module compiles and exports are accessible
        // Test passes if module compiles without errors
    }
}
