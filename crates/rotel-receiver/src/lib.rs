//! Rotel OTLP Receiver
//!
//! This crate provides OpenTelemetry Protocol (OTLP) receiver functionality,
//! supporting both gRPC and HTTP transports with Protobuf and JSON encodings.

// Module declarations
pub mod config;
pub mod error;
pub mod grpc;
pub mod health;
pub mod http;
pub mod protocol;
pub mod signals;

// Re-exports for convenience
pub use config::ReceiverConfig;
pub use error::{ReceiverError, Result};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_module_structure() {
        // Basic smoke test to ensure modules compile
        let _config = ReceiverConfig::default();
    }
}

// Made with Bob
