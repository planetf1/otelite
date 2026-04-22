// gRPC module for OTLP receiver
//
// This module implements the OpenTelemetry Protocol (OTLP) gRPC receiver,
// supporting metrics, logs, and traces on port 4317.

pub mod logs;
pub mod metrics;
pub mod server;
pub mod traces;

pub use server::GrpcServer;
