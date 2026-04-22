//! Otelite TUI Library
//!
//! This library provides the core functionality for the Otelite Terminal User Interface,
//! including state management, API client, and UI components.

// TUI is under development - many components not yet integrated
#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]
#![allow(clippy::unnecessary_map_or)]

pub mod api;
pub mod app;
pub mod config;
pub mod events;
pub mod state;
pub mod ui;

// Re-export commonly used types
pub use app::{App, View};
pub use config::Config;
pub use events::{poll_event, AppEvent};
