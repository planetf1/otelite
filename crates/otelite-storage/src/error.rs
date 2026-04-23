//! Internal error type for the SQLite storage backend.

use thiserror::Error;

/// Result type for internal SQLite operations.
pub type Result<T> = std::result::Result<T, StorageError>;

/// SQLite-specific error type with `#[from]` conversions for database-layer errors.
///
/// This type is an implementation detail of `otelite-storage`.  External callers
/// should use `otelite_core::storage::StorageError` (re-exported as
/// `otelite_storage::StorageError`).  The `From` impl below converts between
/// the two at the `StorageBackend` trait boundary.
#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Failed to initialize storage: {0}")]
    InitializationError(String),

    #[error("Failed to write data: {0}")]
    WriteError(String),

    #[error("Failed to query data: {0}")]
    QueryError(String),

    #[error("Insufficient disk space: {0}")]
    DiskFullError(String),

    #[error("Storage corruption detected: {0}")]
    CorruptionError(String),

    #[error("Permission denied: {0}")]
    PermissionError(String),

    #[error("Configuration error: {0}")]
    ConfigError(String),

    #[error("Purge operation failed: {0}")]
    PurgeError(String),

    #[error("Database error: {0}")]
    DatabaseError(#[from] rusqlite::Error),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),
}

impl StorageError {
    pub fn is_recoverable(&self) -> bool {
        matches!(
            self,
            StorageError::WriteError(_) | StorageError::QueryError(_) | StorageError::PurgeError(_)
        )
    }

    pub fn is_corruption(&self) -> bool {
        matches!(self, StorageError::CorruptionError(_))
    }

    pub fn is_disk_full(&self) -> bool {
        matches!(self, StorageError::DiskFullError(_))
    }
}

impl From<StorageError> for otelite_core::storage::StorageError {
    fn from(e: StorageError) -> Self {
        match e {
            StorageError::InitializationError(s) => Self::InitializationError(s),
            StorageError::WriteError(s) => Self::WriteError(s),
            StorageError::QueryError(s) => Self::QueryError(s),
            StorageError::DiskFullError(s) => Self::DiskFullError(s),
            StorageError::CorruptionError(s) => Self::CorruptionError(s),
            StorageError::PermissionError(s) => Self::PermissionError(s),
            StorageError::ConfigError(s) => Self::ConfigError(s),
            StorageError::PurgeError(s) => Self::PurgeError(s),
            StorageError::DatabaseError(e) => Self::DatabaseError(e.to_string()),
            StorageError::IoError(e) => Self::IoError(e.to_string()),
            StorageError::SerializationError(e) => Self::SerializationError(e.to_string()),
        }
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
