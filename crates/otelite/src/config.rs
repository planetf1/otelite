//! Configuration for the Otelite CLI

use std::path::PathBuf;
use std::time::Duration;

use serde::Deserialize;

/// Output format for CLI commands
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum OutputFormat {
    /// Pretty-printed table format (default)
    #[default]
    Pretty,
    /// JSON format for machine parsing
    Json,
    /// Compact JSON format (single-line, for piping to jq)
    JsonCompact,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pretty" => Ok(Self::Pretty),
            "json" => Ok(Self::Json),
            "json-compact" => Ok(Self::JsonCompact),
            _ => Err(format!(
                "Invalid output format: '{}'. Use 'pretty', 'json', or 'json-compact'",
                s
            )),
        }
    }
}

/// CLI configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Otelite backend endpoint URL
    pub endpoint: String,
    /// Request timeout duration
    pub timeout: Duration,
    /// Output format (pretty or json)
    pub format: OutputFormat,
    /// Disable color output
    pub no_color: bool,
    /// Disable table headers
    pub no_header: bool,
    /// Disable automatic paging
    pub no_pager: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:3000".to_string(),
            timeout: Duration::from_secs(30),
            format: OutputFormat::Pretty,
            no_color: false,
            no_header: false,
            no_pager: false,
        }
    }
}

impl Config {
    /// Create a new configuration with custom values
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        endpoint: String,
        timeout: Duration,
        format: OutputFormat,
        no_color: bool,
        no_header: bool,
        no_pager: bool,
    ) -> Self {
        Self {
            endpoint,
            timeout,
            format,
            no_color,
            no_header,
            no_pager,
        }
    }

    /// Get endpoint from environment variable or use default
    pub fn endpoint_from_env() -> String {
        std::env::var("OTELITE_ENDPOINT").unwrap_or_else(|_| "http://localhost:3000".to_string())
    }

    /// Get the config directory path
    /// ($XDG_CONFIG_HOME/otelite when set, else ~/.config/otelite)
    pub fn config_dir() -> PathBuf {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            if !xdg.is_empty() {
                return PathBuf::from(xdg).join("otelite");
            }
        }
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        PathBuf::from(home).join(".config").join("otelite")
    }

    /// Get the config file path (~/.config/otelite/config.toml)
    pub fn config_file() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    /// Load configuration from the config file, if one exists.
    ///
    /// Precedence (highest first): CLI flags, the `OTELITE_ENDPOINT`
    /// environment variable, this file, built-in defaults. This function
    /// returns only the file layer (falling back to built-in defaults for
    /// keys the file does not set); callers apply flags on top.
    ///
    /// A missing file is not an error — built-in defaults are returned.
    /// A present-but-unparseable file is an error, because silently
    /// ignoring a configuration the user edited would hide the typo.
    pub fn load_file() -> Result<Config, crate::error::Error> {
        let path = Self::config_file();
        if !path.exists() {
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path).map_err(|e| {
            crate::error::Error::ConfigError(format!(
                "Failed to read config file {}: {} — check the file permissions,                  or remove the file to fall back to defaults",
                path.display(),
                e
            ))
        })?;
        let parsed: FileConfig = toml::from_str(&content).map_err(|e| {
            crate::error::Error::ConfigError(format!(
                "Failed to parse config file {}: {} — fix the syntax,                  or remove the file to fall back to defaults",
                path.display(),
                e
            ))
        })?;
        let defaults = Self::default();
        Ok(Config {
            endpoint: parsed.endpoint.unwrap_or(defaults.endpoint),
            timeout: parsed
                .timeout
                .map(Duration::from_secs)
                .unwrap_or(defaults.timeout),
            format: match parsed.format {
                Some(f) => f.parse().map_err(crate::error::Error::ConfigError)?,
                None => defaults.format,
            },
            no_color: parsed.no_color.unwrap_or(defaults.no_color),
            no_header: parsed.no_header.unwrap_or(defaults.no_header),
            no_pager: parsed.no_pager.unwrap_or(defaults.no_pager),
        })
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
        let default_config = r#"# Otelite Configuration
# This file was automatically generated on first run.
#
# Values here are CLI defaults. Precedence: command-line flags >
# OTELITE_ENDPOINT > this file > built-in defaults.
#
# Server behaviour (bind address, OTLP ports, storage location,
# retention) is configured with flags and environment variables,
# not this file: `otelite serve --help`, OTELITE_DATA_DIR,
# OTELITE_OTLP_GRPC_PORT, OTELITE_OTLP_HTTP_PORT, OTELITE_RETENTION_DAYS.

# Backend endpoint URL (default: http://localhost:3000)
# endpoint = "http://localhost:3000"

# Request timeout in seconds (default: 30)
# timeout = 30

# Output format: "pretty", "json" or "json-compact" (default: pretty)
# format = "pretty"

# no_color = false
# no_header = false
# no_pager = false
"#;

        std::fs::write(config_file, default_config)?;
        Ok(())
    }
}

/// Keys accepted in config.toml. Unknown keys are ignored so the file
/// can carry settings for other tools or future releases.
#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    endpoint: Option<String>,
    timeout: Option<u64>,
    format: Option<String>,
    no_color: Option<bool>,
    no_header: Option<bool>,
    no_pager: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serialise tests that mutate HOME / XDG_CONFIG_HOME, and point them
    /// at a TempDir so the real user config is never read or written.
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    fn isolated_home() -> (std::sync::MutexGuard<'static, ()>, tempfile::TempDir) {
        let guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().expect("temp dir");
        std::env::set_var("HOME", tmp.path());
        std::env::remove_var("XDG_CONFIG_HOME");
        (guard, tmp)
    }

    fn write_config(tmp: &tempfile::TempDir, contents: &str) -> std::path::PathBuf {
        let dir = tmp.path().join(".config").join("otelite");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("config.toml");
        std::fs::write(&file, contents).unwrap();
        file
    }

    #[test]
    fn test_output_format_from_str() {
        assert_eq!(
            "pretty".parse::<OutputFormat>().unwrap(),
            OutputFormat::Pretty
        );
        assert_eq!("json".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert_eq!(
            "json-compact".parse::<OutputFormat>().unwrap(),
            OutputFormat::JsonCompact
        );
        assert_eq!(
            "PRETTY".parse::<OutputFormat>().unwrap(),
            OutputFormat::Pretty
        );
        assert_eq!("JSON".parse::<OutputFormat>().unwrap(), OutputFormat::Json);
        assert_eq!(
            "JSON-COMPACT".parse::<OutputFormat>().unwrap(),
            OutputFormat::JsonCompact
        );
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
            true,
        );
        assert_eq!(config.endpoint, "http://example.com:9090");
        assert_eq!(config.timeout, Duration::from_secs(60));
        assert_eq!(config.format, OutputFormat::Json);
        assert!(config.no_color);
        assert!(config.no_header);
        assert!(config.no_pager);
    }

    #[test]
    fn test_load_file_returns_defaults_when_absent() {
        let (_guard, _tmp) = isolated_home();
        let config = Config::load_file().expect("no file means defaults, not an error");
        assert_eq!(config.endpoint, "http://localhost:3000");
        assert_eq!(config.timeout, Duration::from_secs(30));
        assert_eq!(config.format, OutputFormat::Pretty);
        assert!(!config.no_color);
    }

    #[test]
    fn test_load_file_reads_values() {
        let (_guard, tmp) = isolated_home();
        write_config(
            &tmp,
            r#"
endpoint = "http://127.0.0.1:9999"
timeout = 5
format = "json"
no_color = true
"#,
        );

        let config = Config::load_file().expect("valid file parses");
        assert_eq!(config.endpoint, "http://127.0.0.1:9999");
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert_eq!(config.format, OutputFormat::Json);
        assert!(config.no_color);
        // unset keys keep their defaults
        assert!(!config.no_header);
        assert!(!config.no_pager);
    }

    #[test]
    fn test_load_file_malformed_is_config_error() {
        let (_guard, tmp) = isolated_home();
        let path = write_config(&tmp, "endpoint = ");

        let err = Config::load_file().expect_err("broken TOML must be an error");
        let msg = format!("{err}");
        assert!(
            msg.contains("Failed to parse config file"),
            "error names the failure: {msg}"
        );
        assert!(msg.contains("config.toml"), "error names the file: {msg}");
        assert!(path.to_string_lossy().as_ref().contains("config.toml"));
    }

    #[test]
    fn test_load_file_invalid_format_is_config_error() {
        let (_guard, tmp) = isolated_home();
        write_config(&tmp, "format = \"bogus\"");

        let err = Config::load_file().expect_err("unknown format must be an error");
        let msg = format!("{err}");
        assert!(
            msg.contains("Invalid output format"),
            "error explains the valid values: {msg}"
        );
    }

    #[test]
    fn test_load_file_ignores_unknown_keys() {
        let (_guard, tmp) = isolated_home();
        write_config(
            &tmp,
            r#"
endpoint = "http://127.0.0.1:4000"

[server]
addr = "127.0.0.1:3000"
"#,
        );

        let config = Config::load_file().expect("unknown keys are ignored");
        assert_eq!(config.endpoint, "http://127.0.0.1:4000");
    }

    #[test]
    fn test_config_dir_respects_xdg_config_home() {
        let guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let xdg = tmp.path().join("xdg-config");
        std::env::set_var("HOME", tmp.path());
        std::env::set_var("XDG_CONFIG_HOME", &xdg);

        assert_eq!(Config::config_dir(), xdg.join("otelite"));

        std::env::remove_var("XDG_CONFIG_HOME");
        drop(guard);
    }

    #[test]
    fn test_created_default_config_round_trips_to_defaults() {
        let (_guard, tmp) = isolated_home();
        std::env::set_var("HOME", tmp.path());

        Config::create_default_config().expect("first-run write succeeds");
        let file = Config::config_file();
        assert!(file.exists(), "template written");

        // The generated template is all commented-out keys, so loading it
        // must yield exactly the built-in defaults.
        let config = Config::load_file().expect("generated template parses");
        assert_eq!(config, Config::default());
    }
}
