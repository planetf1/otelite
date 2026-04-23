//! Storage configuration

use otelite_core::storage::{Result, StorageError};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Storage configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    /// Data directory path
    pub data_dir: PathBuf,

    /// Retention period in days (1-365)
    pub retention_days: u32,

    /// Purge schedule (cron-like format)
    pub purge_schedule: String,

    /// Enable automatic purging
    pub auto_purge_enabled: bool,

    /// Batch size for purge operations
    pub purge_batch_size: usize,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            data_dir: Self::default_data_dir(),
            retention_days: 90,
            purge_schedule: "0 2 * * *".to_string(), // Daily at 2 AM
            auto_purge_enabled: true,
            purge_batch_size: 1000,
        }
    }
}

impl StorageConfig {
    /// Get default data directory (~/.otelite/data)
    fn default_data_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".otelite")
            .join("data")
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

        config.validate()?;
        Ok(config)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        if self.retention_days < 1 || self.retention_days > 365 {
            return Err(StorageError::ConfigError(
                "Retention days must be between 1 and 365".to_string(),
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
    fn test_config_validation() {
        let mut config = StorageConfig::default();
        assert!(config.validate().is_ok());

        config.retention_days = 0;
        assert!(config.validate().is_err());

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
}
