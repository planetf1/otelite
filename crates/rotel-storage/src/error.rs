//! Error types for the storage layer

use thiserror::Error;

/// Result type for storage operations
pub type Result<T> = std::result::Result<T, StorageError>;

/// Errors that can occur during storage operations
#[derive(Error, Debug)]
pub enum StorageError {
    /// Storage initialization failed
    #[error("Failed to initialize storage: {0}")]
    InitializationError(String),

    /// Write operation failed
    #[error("Failed to write data: {0}")]
    WriteError(String),

    /// Query operation failed
    #[error("Failed to query data: {0}")]
    QueryError(String),

    /// Disk is full or insufficient space
    #[error("Insufficient disk space: {0}")]
    DiskFullError(String),

    /// Storage corruption detected
    #[error("Storage corruption detected: {0}")]
    CorruptionError(String),

    /// Permission denied
    #[error("Permission denied: {0}")]
    PermissionError(String),

    /// Configuration error
    #[error("Configuration error: {0}")]
    ConfigError(String),

    /// Purge operation failed
    #[error("Purge operation failed: {0}")]
    PurgeError(String),

    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    /// I/O error
    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    /// Serialization error
    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl StorageError {
    /// Check if error is recoverable
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            StorageError::WriteError(_) | StorageError::QueryError(_) | StorageError::PurgeError(_)
        )
    }

    /// Check if error indicates corruption
    pub fn is_corruption(&self) -> bool {
        matches!(self, StorageError::CorruptionError(_))
    }

    /// Check if error is due to disk space
    pub fn is_disk_full(&self) -> bool {
        matches!(self, StorageError::DiskFullError(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = StorageError::InitializationError("test error".to_string());
        assert_eq!(err.to_string(), "Failed to initialize storage: test error");
    }

    #[test]
    fn test_error_recoverable() {
        let err = StorageError::WriteError("test".to_string());
        assert!(err.is_recoverable());

        let err = StorageError::CorruptionError("test".to_string());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn test_error_corruption_check() {
        let err = StorageError::CorruptionError("test".to_string());
        assert!(err.is_corruption());

        let err = StorageError::WriteError("test".to_string());
        assert!(!err.is_corruption());
    }

    #[test]
    fn test_error_disk_full_check() {
        let err = StorageError::DiskFullError("test".to_string());
        assert!(err.is_disk_full());

        let err = StorageError::WriteError("test".to_string());
        assert!(!err.is_disk_full());
    }
}

// Made with Bob
