//! Health check endpoints for monitoring

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Health check status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Service is healthy and ready
    Healthy,

    /// Service is unhealthy
    Unhealthy,
}

/// Health checker for the receiver
#[derive(Debug, Clone)]
pub struct HealthChecker {
    /// Is the service ready to accept requests
    ready: Arc<AtomicBool>,

    /// Is the service alive (not deadlocked)
    alive: Arc<AtomicBool>,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
            alive: Arc::new(AtomicBool::new(true)),
        }
    }

    /// Mark the service as ready
    pub fn set_ready(&self, ready: bool) {
        self.ready.store(ready, Ordering::SeqCst);
    }

    /// Mark the service as alive
    pub fn set_alive(&self, alive: bool) {
        self.alive.store(alive, Ordering::SeqCst);
    }

    /// Check if the service is ready
    pub fn is_ready(&self) -> bool {
        self.ready.load(Ordering::SeqCst)
    }

    /// Check if the service is alive
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    /// Get overall health status
    pub fn status(&self) -> HealthStatus {
        if self.is_alive() && self.is_ready() {
            HealthStatus::Healthy
        } else {
            HealthStatus::Unhealthy
        }
    }
}

impl Default for HealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_checker_creation() {
        let checker = HealthChecker::new();
        assert!(!checker.is_ready()); // Not ready initially
        assert!(checker.is_alive()); // Alive by default
        assert_eq!(checker.status(), HealthStatus::Unhealthy);
    }

    #[test]
    fn test_set_ready() {
        let checker = HealthChecker::new();
        checker.set_ready(true);
        assert!(checker.is_ready());
        assert_eq!(checker.status(), HealthStatus::Healthy);
    }

    #[test]
    fn test_set_alive() {
        let checker = HealthChecker::new();
        checker.set_ready(true);
        checker.set_alive(false);
        assert!(!checker.is_alive());
        assert_eq!(checker.status(), HealthStatus::Unhealthy);
    }

    #[test]
    fn test_health_status() {
        let checker = HealthChecker::new();

        // Not ready, alive -> unhealthy
        assert_eq!(checker.status(), HealthStatus::Unhealthy);

        // Ready, alive -> healthy
        checker.set_ready(true);
        assert_eq!(checker.status(), HealthStatus::Healthy);

        // Ready, not alive -> unhealthy
        checker.set_alive(false);
        assert_eq!(checker.status(), HealthStatus::Unhealthy);
    }
}
