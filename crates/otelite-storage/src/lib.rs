//! Otelite Storage Layer
//!
//! Provides the SQLite-backed implementation of the `StorageBackend` trait.
//! The trait itself, along with all associated types, is defined in
//! `otelite-core::storage` so that downstream crates can program against the
//! abstraction without pulling in a SQLite dependency.

pub mod config;
pub mod error;
pub mod sqlite;

pub use config::StorageConfig;

// Re-export the core storage contract so callers who already depend on
// `otelite-storage` don't need to change their use-paths.
pub use otelite_core::storage::{
    PurgeAllStats, PurgeOptions, QueryParams, Result, SignalType, StorageBackend, StorageError,
    StorageStats,
};
