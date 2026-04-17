use std::time::Duration;

/// Application configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Rotel API base URL
    pub api_url: String,

    /// Refresh interval for polling data
    pub refresh_interval: Duration,

    /// Initial view to display
    pub initial_view: String,

    /// Enable debug logging
    pub debug: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:8080".to_string(),
            refresh_interval: Duration::from_secs(2),
            initial_view: "logs".to_string(),
            debug: false,
        }
    }
}

// Made with Bob
