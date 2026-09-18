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
    /// Classify a raw `rusqlite::Error` by its SQLite result code.
    ///
    /// Without this, SQLITE_FULL and SQLITE_CORRUPT surface as generic
    /// `DatabaseError`s (and, after the writer's `WriteError` wrapping, as
    /// opaque strings): the health checker and the API layer cannot tell a
    /// full disk or a corrupt database apart from a transient busy timeout
    /// (#256).
    pub fn from_rusqlite(e: rusqlite::Error) -> Self {
        let code = match &e {
            rusqlite::Error::SqliteFailure(ffi, _) => ffi.code,
            _ => return StorageError::DatabaseError(e),
        };
        use rusqlite::ffi::ErrorCode;
        match code {
            ErrorCode::DiskFull => StorageError::DiskFullError(e.to_string()),
            ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => {
                StorageError::CorruptionError(e.to_string())
            },
            ErrorCode::PermissionDenied | ErrorCode::ReadOnly => {
                StorageError::PermissionError(e.to_string())
            },
            ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => {
                StorageError::WriteError(format!("database busy (retry the export): {}", e))
            },
            _ => StorageError::DatabaseError(e),
        }
    }

    /// A write failure that will keep failing until a human intervenes
    /// (full disk, corruption, permissions) — as opposed to a transient
    /// busy/locked timeout that an exporter retry will clear. The receiver
    /// uses this to flip `/health` to unhealthy after sustained failures
    /// (#256).
    pub fn is_persistent(&self) -> bool {
        matches!(
            self,
            StorageError::DiskFullError(_)
                | StorageError::CorruptionError(_)
                | StorageError::PermissionError(_)
        )
    }

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

    /// Build a raw `rusqlite::Error::SqliteFailure` for a given result code,
    /// mirroring what the SQLite C layer produces.
    fn sqlite_failure(code: rusqlite::ffi::ErrorCode) -> rusqlite::Error {
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code,
                extended_code: 0,
            },
            Some(format!("simulated sqlite error {code:?}")),
        )
    }

    #[test]
    fn test_from_rusqlite_classifies_disk_full() {
        let err = StorageError::from_rusqlite(sqlite_failure(rusqlite::ffi::ErrorCode::DiskFull));
        assert!(matches!(err, StorageError::DiskFullError(_)));
        assert!(err.is_persistent());
        assert!(err.is_disk_full());
    }

    #[test]
    fn test_from_rusqlite_classifies_corruption() {
        use rusqlite::ffi::ErrorCode;
        for code in [ErrorCode::DatabaseCorrupt, ErrorCode::NotADatabase] {
            let err = StorageError::from_rusqlite(sqlite_failure(code));
            assert!(
                matches!(err, StorageError::CorruptionError(_)),
                "code {code:?} must be corruption"
            );
            assert!(err.is_persistent());
            assert!(err.is_corruption());
        }
    }

    #[test]
    fn test_from_rusqlite_classifies_permission() {
        use rusqlite::ffi::ErrorCode;
        for code in [ErrorCode::PermissionDenied, ErrorCode::ReadOnly] {
            let err = StorageError::from_rusqlite(sqlite_failure(code));
            assert!(
                matches!(err, StorageError::PermissionError(_)),
                "code {code:?} must be a permission error"
            );
            assert!(err.is_persistent());
        }
    }

    #[test]
    fn test_from_rusqlite_classifies_busy_as_transient_write() {
        use rusqlite::ffi::ErrorCode;
        for code in [ErrorCode::DatabaseBusy, ErrorCode::DatabaseLocked] {
            let err = StorageError::from_rusqlite(sqlite_failure(code));
            assert!(
                matches!(err, StorageError::WriteError(_)),
                "code {code:?} must be a transient write error"
            );
            // A busy/locked timeout is retryable — it must NOT flip health.
            assert!(
                !err.is_persistent(),
                "busy must be transient, not persistent"
            );
            assert!(err.to_string().contains("busy"));
        }
    }

    #[test]
    fn test_from_rusqlite_default_is_database_error() {
        // An unrecognised code falls through to the generic DatabaseError,
        // which is not persistent (so it cannot wedge /health unhealthy).
        let err =
            StorageError::from_rusqlite(sqlite_failure(rusqlite::ffi::ErrorCode::SchemaChanged));
        assert!(matches!(err, StorageError::DatabaseError(_)));
        assert!(!err.is_persistent());

        // Non-SqliteFailure variants pass through as DatabaseError too.
        let err = StorageError::from_rusqlite(rusqlite::Error::InvalidPath(
            std::path::PathBuf::from("/nonexistent"),
        ));
        assert!(matches!(err, StorageError::DatabaseError(_)));
        assert!(!err.is_persistent());
    }

    #[test]
    fn test_persistent_covers_the_three_failure_modes() {
        assert!(StorageError::DiskFullError("x".into()).is_persistent());
        assert!(StorageError::CorruptionError("x".into()).is_persistent());
        assert!(StorageError::PermissionError("x".into()).is_persistent());
        assert!(!StorageError::WriteError("x".into()).is_persistent());
        assert!(!StorageError::QueryError("x".into()).is_persistent());
        assert!(!StorageError::InitializationError("x".into()).is_persistent());
        assert!(!StorageError::ConfigError("x".into()).is_persistent());
        assert!(!StorageError::PurgeError("x".into()).is_persistent());
    }
}
