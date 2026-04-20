//! Configuration for the Rotel CLI

use std::path::PathBuf;
use std::time::Duration;

/// Output format for CLI commands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Pretty-printed table format (default)
    #[default]
    Pretty,
    /// JSON format for machine parsing
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pretty" => Ok(Self::Pretty),
            "json" => Ok(Self::Json),
            _ => Err(format!(
                "Invalid output format: '{}'. Use 'pretty' or 'json'",
                s
            )),
        }
    }
}

/// CLI configuration
#[derive(Debug, Clone)]
pub struct Config {
    /// Rotel backend endpoint URL
    pub endpoint: String,
    /// Request timeout duration
    pub timeout: Duration,
    /// Output format (pretty or json)
    pub format: OutputFormat,
    /// Disable color output
    pub no_color: bool,
    /// Disable table headers
    pub no_header: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:3000".to_string(),
            timeout: Duration::from_secs(30),
            format: OutputFormat::Pretty,
            no_color: false,
            no_header: false,
        }
    }
}

impl Config {
    /// Create a new configuration with custom values
    pub fn new(
        endpoint: String,
        timeout: Duration,
        format: OutputFormat,
        no_color: bool,
        no_header: bool,
    ) -> Self {
        Self {
            endpoint,
            timeout,
            format,
            no_color,
            no_header,
        }
    }

    /// Get endpoint from environment variable or use default
    pub fn endpoint_from_env() -> String {
        std::env::var("ROTEL_ENDPOINT").unwrap_or_else(|_| "http://localhost:3000".to_string())
    }

    /// Get the config directory path (~/.config/rotel)
    pub fn config_dir() -> PathBuf {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config").join("rotel")
    }

    /// Get the config file path (~/.config/rotel/config.toml)
    pub fn config_file() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Check if this is the first run (config file doesn't exist)
    pub fn is_first_run() -> bool {
        !Self::config_file().exists()
    }

    /// Create the config directory and file with default settings
    pub fn create_default_config() -> std::io::Result<()> {
        let config_dir = Self::config_dir();
        std::fs::create_dir_all(&config_dir)?;

        let config_file = Self::config_file();
        let default_config = r#"# Rotel Configuration
# This file was automatically generated on first run

[server]
# Dashboard bind address
addr = "127.0.0.1:3000"

# Storage database path
storage_path = "rotel.db"

[otlp]
# OTLP gRPC receiver address
grpc_addr = "0.0.0.0:4317"

# OTLP HTTP receiver address
http_addr = "0.0.0.0:4318"

[cli]
# Default output format (pretty or json)
format = "pretty"

# Request timeout in seconds
timeout = 30
"#;

        std::fs::write(config_file, default_config)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(
            "pretty".parse::<OutputFormat>().unwrap(),
            OutputFormat::Pretty
        );
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert_eq!(
            "PRETTY".parse::<OutputFormat>().unwrap(),
            OutputFormat::Pretty
        );
        assert_eq!("JSON".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert!("invalid".parse::<OutputFormat>().is_err());
    }

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.endpoint, "http://localhost:3000");
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.format, OutputFormat::Pretty);
        assert!(!config.no_color);
        assert!(!config.no_header);
    }

    #[test]
    fn test_config_new() {
        let config = Config::new(
            "http://example.com:9090".to_string(),
            Duration::from_secs(60),
            OutputFormat::Json,
            true,
            true,
        );
        assert_eq!(config.endpoint, "http://example.com:9090");
        assert_eq!(config.timeout, Duration::from_secs(60));
        assert_eq!(config.format, OutputFormat::Json);
        assert!(config.no_color);
        assert!(config.no_header);
    }
}
