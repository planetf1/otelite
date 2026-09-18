//! Health check endpoints for monitoring

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

/// Health check status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthStatus {
    /// Service is healthy and ready
    Healthy,

    /// Service is unhealthy
    Unhealthy,
}

/// Consecutive *persistent* write failures before `/health` reports
/// unhealthy (#256): large enough that a single flaky export cannot trip
/// it, small enough that a genuinely broken write path (full disk,
/// corruption) is visible within seconds, while the exporter is still
/// retrying.
pub const WRITE_FAILURE_THRESHOLD: u32 = 3;

/// Health checker for the receiver
#[derive(Debug, Clone)]
pub struct HealthChecker {
    /// Is the service ready to accept requests
    ready: Arc<AtomicBool>,

    /// Is the service alive (not deadlocked)
    alive: Arc<AtomicBool>,

    /// Has the write path sustained persistent failures (degraded)
    degraded: Arc<AtomicBool>,

    /// Consecutive persistent write failures (reset by any success)
    consecutive_persistent_failures: Arc<AtomicU32>,
}

impl HealthChecker {
    /// Create a new health checker
    pub fn new() -> Self {
        Self {
            ready: Arc::new(AtomicBool::new(false)),
            alive: Arc::new(AtomicBool::new(true)),
            degraded: Arc::new(AtomicBool::new(false)),
            consecutive_persistent_failures: Arc::new(AtomicU32::new(0)),
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

    /// Check if the write path is degraded (sustained persistent failures)
    pub fn is_degraded(&self) -> bool {
        self.degraded.load(Ordering::SeqCst)
    }

    /// Record a successful storage write: clears the failure streak and
    /// re-arms `/health` as healthy.
    pub fn record_write_success(&self) {
        if self
            .consecutive_persistent_failures
            .swap(0, Ordering::SeqCst)
            != 0
        {
            self.degraded.store(false, Ordering::SeqCst);
        }
    }

    /// Record a failed storage write (#256).
    ///
    /// Only *persistent* failures — the ones classified by
    /// `StorageError::is_persistent` (disk full, corruption, permissions) —
    /// count towards degradation. A transient busy/locked timeout is what
    /// an exporter retry clears, and counting it would make `/health` flap
    /// under normal load. Callers log the failure at `error!` level when it
    /// is persistent; this method only tracks state.
    pub fn record_write_failure(&self, persistent: bool) {
        if !persistent {
            return;
        }
        let n = self
            .consecutive_persistent_failures
            .fetch_add(1, Ordering::SeqCst)
            + 1;
        if n == WRITE_FAILURE_THRESHOLD {
            self.degraded.store(true, Ordering::SeqCst);
        }
    }

    /// Get overall health status
    pub fn status(&self) -> HealthStatus {
        if self.is_alive() && self.is_ready() && !self.is_degraded() {
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

    // ── #256: write-path degradation ────────────────────────────────────

    /// Transient (non-persistent) write failures must never degrade health —
    /// a busy timeout is what an exporter retry clears, and counting it
    /// would make /health flap under normal load.
    #[test]
    fn test_transient_failures_do_not_degrade() {
        let checker = HealthChecker::new();
        checker.set_ready(true);

        for _ in 0..WRITE_FAILURE_THRESHOLD * 3 {
            checker.record_write_failure(false);
        }
        assert!(!checker.is_degraded());
        assert_eq!(checker.status(), HealthStatus::Healthy);
    }

    /// Health flips unhealthy exactly at the threshold of consecutive
    /// persistent failures — not one before, not one after.
    #[test]
    fn test_persistent_failures_flip_health_at_threshold() {
        let checker = HealthChecker::new();
        checker.set_ready(true);

        for _ in 0..WRITE_FAILURE_THRESHOLD - 1 {
            checker.record_write_failure(true);
            assert!(!checker.is_degraded(), "below threshold must stay healthy");
        }
        assert_eq!(checker.status(), HealthStatus::Healthy);

        checker.record_write_failure(true);
        assert!(checker.is_degraded());
        assert_eq!(
            checker.status(),
            HealthStatus::Unhealthy,
            "sustained persistent failure must flip /health unhealthy"
        );
    }

    /// A single success anywhere in the streak re-arms health: the counter
    /// resets, so the next degradation needs a full threshold again.
    #[test]
    fn test_success_resets_streak_and_rearms_health() {
        let checker = HealthChecker::new();
        checker.set_ready(true);

        checker.record_write_failure(true);
        checker.record_write_failure(true);
        checker.record_write_success();
        assert!(!checker.is_degraded());

        // Two more persistent failures alone are not enough — the streak
        // restarted from zero, not from two.
        checker.record_write_failure(true);
        checker.record_write_failure(true);
        assert!(!checker.is_degraded());
        assert_eq!(checker.status(), HealthStatus::Healthy);

        // Crossing the threshold again from the fresh streak still works.
        checker.record_write_failure(true);
        assert!(checker.is_degraded());
        // And a success heals it.
        checker.record_write_success();
        assert!(!checker.is_degraded());
        assert_eq!(checker.status(), HealthStatus::Healthy);
    }
}
