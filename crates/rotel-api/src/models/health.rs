//! Health check and system status models

use serde::{Deserialize, Serialize};
use utoipa::ToSchema;

/// Health check response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct HealthResponse {
    /// Overall health status
    pub status: HealthStatus,

    /// API version
    pub version: String,

    /// Uptime in seconds
    pub uptime_seconds: u64,

    /// System statistics
    pub system: SystemStats,

    /// Component health checks
    pub components: ComponentHealth,
}

/// Health status enum
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    /// All systems operational
    Healthy,

    /// Some non-critical issues
    Degraded,

    /// Critical issues present
    Unhealthy,
}

/// System resource statistics
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct SystemStats {
    /// Memory usage in bytes
    pub memory_used_bytes: u64,

    /// Total memory in bytes
    pub memory_total_bytes: u64,

    /// Memory usage percentage (0.0 to 100.0)
    pub memory_usage_percent: f64,

    /// CPU usage percentage (0.0 to 100.0)
    pub cpu_usage_percent: f64,

    /// Number of active connections
    pub active_connections: u32,

    /// Total requests processed
    pub total_requests: u64,

    /// Current requests per second
    pub requests_per_second: f64,
}

/// Component health status
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ComponentHealth {
    /// Storage backend health
    pub storage: ComponentStatus,

    /// API server health
    pub api: ComponentStatus,

    /// Metrics collection health
    pub metrics: ComponentStatus,
}

/// Individual component status
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ComponentStatus {
    /// Component health status
    pub status: HealthStatus,

    /// Status message
    pub message: String,

    /// Last check timestamp (Unix timestamp in milliseconds)
    pub last_check: i64,

    /// Response time in milliseconds
    pub response_time_ms: Option<f64>,
}

/// Readiness check response
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReadinessResponse {
    /// Whether the service is ready to accept traffic
    pub ready: bool,

    /// Readiness checks
    pub checks: ReadinessChecks,
}

/// Readiness check details
#[derive(Debug, Clone, Serialize, Deserialize, ToSchema)]
pub struct ReadinessChecks {
    /// Storage backend is ready
    pub storage_ready: bool,

    /// API server is ready
    pub api_ready: bool,

    /// Configuration is valid
    pub config_valid: bool,
}

impl HealthResponse {
    /// Create a mock health response for testing
    pub fn mock() -> Self {
        Self {
            status: HealthStatus::Healthy,
            version: env!("CARGO_PKG_VERSION").to_string(),
            uptime_seconds: 3600, // 1 hour
            system: SystemStats {
                memory_used_bytes: 850_000_000,
                memory_total_bytes: 16_000_000_000,
                memory_usage_percent: 5.3,
                cpu_usage_percent: 12.5,
                active_connections: 42,
                total_requests: 15000,
                requests_per_second: 25.5,
            },
            components: ComponentHealth {
                storage: ComponentStatus {
                    status: HealthStatus::Healthy,
                    message: "Storage backend operational".to_string(),
                    last_check: chrono::Utc::now().timestamp_millis(),
                    response_time_ms: Some(2.5),
                },
                api: ComponentStatus {
                    status: HealthStatus::Healthy,
                    message: "API server operational".to_string(),
                    last_check: chrono::Utc::now().timestamp_millis(),
                    response_time_ms: Some(1.2),
                },
                metrics: ComponentStatus {
                    status: HealthStatus::Healthy,
                    message: "Metrics collection operational".to_string(),
                    last_check: chrono::Utc::now().timestamp_millis(),
                    response_time_ms: Some(0.8),
                },
            },
        }
    }
}

impl ReadinessResponse {
    /// Create a mock readiness response for testing
    pub fn mock() -> Self {
        Self {
            ready: true,
            checks: ReadinessChecks {
                storage_ready: true,
                api_ready: true,
                config_valid: true,
            },
        }
    }
}

// Made with Bob
