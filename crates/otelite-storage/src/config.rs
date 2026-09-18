//! Storage configuration

use otelite_core::storage::{Result, StorageError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Data directory path
    pub data_dir: PathBuf,

    /// Retention period in days (0-365; 0 disables purging)
    pub retention_days: u32,

    /// Purge schedule (cron-like format)
    pub purge_schedule: String,

    /// Enable automatic purging
    pub auto_purge_enabled: bool,

    /// Batch size for purge operations
    pub purge_batch_size: usize,

    /// Enable background planner-statistics maintenance (ANALYZE). Long-lived
    /// processes (serve/daemon) keep this on; short-lived CLI invocations
    /// disable it — a background task in a CLI process logs to stdout and
    /// pollutes machine-readable output (#191).
    pub stats_maintenance_enabled: bool,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: Self::default_data_dir(),
            retention_days: 90,
            purge_schedule: "0 2 * * *".to_string(), // Daily at 2 AM
            auto_purge_enabled: true,
            purge_batch_size: 1000,
            stats_maintenance_enabled: true,
        }
    }
}

impl StorageConfig {
    pub fn default_data_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".otelite")
            .join("data")
    }

    /// Whether the background purge scheduler should run for this
    /// configuration: explicit opt-out wins over a positive retention, and
    /// `retention_days = 0` means "keep forever" (#253).
    pub fn purge_scheduler_enabled(&self) -> bool {
        self.auto_purge_enabled && self.retention_days > 0
    }

    /// Create configuration from environment variables
    pub fn from_env() -> Result<Self> {
        let mut config = Self::default();

        if let Ok(data_dir) = std::env::var("OTELITE_DATA_DIR") {
            config.data_dir = PathBuf::from(data_dir);
        }

        if let Ok(retention_days) = std::env::var("OTELITE_RETENTION_DAYS") {
            config.retention_days = retention_days
                .parse()
                .map_err(|e| StorageError::ConfigError(format!("Invalid retention_days: {}", e)))?;
        }

        if let Ok(purge_schedule) = std::env::var("OTELITE_PURGE_SCHEDULE") {
            config.purge_schedule = purge_schedule;
        }

        if let Ok(auto_purge) = std::env::var("OTELITE_AUTO_PURGE_ENABLED") {
            config.auto_purge_enabled = auto_purge.parse().unwrap_or(true);
        }

        if let Ok(stats_maintenance) = std::env::var("OTELITE_STATS_MAINTENANCE_ENABLED") {
            config.stats_maintenance_enabled = stats_maintenance.parse().unwrap_or(true);
        }

        config.validate()?;
        Ok(config)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // 0 means "keep forever" (the purge scheduler is gated on
        // retention_days > 0); otherwise 1–365.
        if self.retention_days > 365 {
            return Err(StorageError::ConfigError(
                "Retention days must be between 0 and 365 (0 disables purging)".to_string(),
            ));
        }

        if self.purge_batch_size == 0 {
            return Err(StorageError::ConfigError(
                "Purge batch size must be greater than 0".to_string(),
            ));
        }

        Ok(())
    }

    /// Builder method to set data directory
    pub fn with_data_dir(mut self, data_dir: PathBuf) -> Self {
        self.data_dir = data_dir;
        self
    }

    /// Builder method to set retention days
    pub fn with_retention_days(mut self, days: u32) -> Self {
        self.retention_days = days;
        self
    }

    /// Builder method to set purge schedule
    pub fn with_purge_schedule(mut self, schedule: String) -> Self {
        self.purge_schedule = schedule;
        self
    }

    /// Builder method to enable/disable auto purge
    pub fn with_auto_purge(mut self, enabled: bool) -> Self {
        self.auto_purge_enabled = enabled;
        self
    }

    pub fn with_stats_maintenance(mut self, enabled: bool) -> Self {
        self.stats_maintenance_enabled = enabled;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = StorageConfig::default();
        assert_eq!(config.retention_days, 90);
        assert_eq!(config.purge_schedule, "0 2 * * *");
        assert!(config.auto_purge_enabled);
        assert_eq!(config.purge_batch_size, 1000);
    }

    #[test]
    fn test_default_data_dir_is_dotfile() {
        // Must stay under ~/.otelite/data — not ~/Library or ~/.local/share.
        let dir = StorageConfig::default_data_dir();
        let home = dirs::home_dir().expect("home dir must exist in test env");
        assert!(
            dir.starts_with(&home),
            "data dir {:?} should be under home {:?}",
            dir,
            home
        );
        let rel = dir.strip_prefix(&home).unwrap();
        assert_eq!(
            rel,
            std::path::Path::new(".otelite/data"),
            "data dir must be ~/.otelite/data, got {:?}",
            dir
        );
    }

    #[test]
    fn test_config_validation() {
        let mut config = StorageConfig::default();
        assert!(config.validate().is_ok());

        // 0 is valid: "keep forever" (purging is gated on retention > 0).
        config.retention_days = 0;
        assert!(config.validate().is_ok());

        config.retention_days = 366;
        assert!(config.validate().is_err());

        config.retention_days = 90;
        config.purge_batch_size = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_builder() {
        let config = StorageConfig::default()
            .with_retention_days(30)
            .with_auto_purge(false);

        assert_eq!(config.retention_days, 30);
        assert!(!config.auto_purge_enabled);
    }

    /// The purge scheduler runs only when both the explicit opt-in state
    /// and a finite retention window hold (#253): auto-purge disabled wins
    /// over any retention, and retention 0 ("keep forever") wins over an
    /// enabled auto-purge.
    #[test]
    fn test_purge_scheduler_enabled_matrix() {
        let base = StorageConfig::default(); // enabled, 90d
        assert!(base.purge_scheduler_enabled());

        let disabled = base.clone().with_auto_purge(false);
        assert!(!disabled.purge_scheduler_enabled(), "opt-out must win");

        let keep_forever = base.clone().with_retention_days(0);
        assert!(
            !keep_forever.purge_scheduler_enabled(),
            "0 days keeps forever"
        );

        let both_off = disabled.clone().with_retention_days(0);
        assert!(!both_off.purge_scheduler_enabled());
    }
}
