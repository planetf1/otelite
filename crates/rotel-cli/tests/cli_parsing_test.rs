//! Tests for CLI argument parsing and configuration

use rotel_cli::config::{Config, OutputFormat};
use std::time::Duration;

#[test]
fn test_default_config() {
    let config = Config::default();
    assert_eq!(config.endpoint, Config::endpoint_from_env());
    assert_eq!(config.timeout, Duration::from_secs(30));
    assert_eq!(config.format, OutputFormat::Pretty);
    assert!(!config.no_color);
    assert!(!config.no_header);
}

#[test]
fn test_config_with_custom_endpoint() {
    let config = Config {
        endpoint: "http://custom:3000".to_string(),
        timeout: Duration::from_secs(30),
        format: OutputFormat::Pretty,
        no_color: false,
        no_header: false,
        no_pager: false,
    };
    assert_eq!(config.endpoint, "http://custom:3000");
}

#[test]
fn test_config_with_json_format() {
    let config = Config {
        endpoint: Config::endpoint_from_env(),
        timeout: Duration::from_secs(30),
        format: OutputFormat::Json,
        no_color: false,
        no_header: false,
        no_pager: false,
    };
    assert_eq!(config.format, OutputFormat::Json);
}

#[test]
fn test_config_with_no_color() {
    let config = Config {
        endpoint: Config::endpoint_from_env(),
        timeout: Duration::from_secs(30),
        format: OutputFormat::Pretty,
        no_color: true,
        no_header: false,
        no_pager: false,
    };
    assert!(config.no_color);
}

#[test]
fn test_config_with_no_header() {
    let config = Config {
        endpoint: Config::endpoint_from_env(),
        timeout: Duration::from_secs(30),
        format: OutputFormat::Pretty,
        no_color: false,
        no_header: true,
        no_pager: false,
    };
    assert!(config.no_header);
}

#[test]
fn test_config_with_custom_timeout() {
    let config = Config {
        endpoint: Config::endpoint_from_env(),
        timeout: Duration::from_secs(60),
        format: OutputFormat::Pretty,
        no_color: false,
        no_header: false,
        no_pager: false,
    };
    assert_eq!(config.timeout, Duration::from_secs(60));
}

#[test]
fn test_output_format_pretty_default() {
    let format = OutputFormat::default();
    assert_eq!(format, OutputFormat::Pretty);
}

#[test]
fn test_output_format_from_string_pretty() {
    let format: OutputFormat = "pretty".parse().unwrap();
    assert_eq!(format, OutputFormat::Pretty);
}

#[test]
fn test_output_format_from_string_json() {
    let format: OutputFormat = "json".parse().unwrap();
    assert_eq!(format, OutputFormat::Json);
}

#[test]
fn test_output_format_from_string_invalid() {
    let result: Result<OutputFormat, _> = "invalid".parse();
    assert!(result.is_err());
}

#[test]
fn test_config_endpoint_from_env_default() {
    // When ROTEL_ENDPOINT is not set, should return default
    std::env::remove_var("ROTEL_ENDPOINT");
    let endpoint = Config::endpoint_from_env();
    assert_eq!(endpoint, "http://localhost:3000");
}

#[test]
fn test_config_builder_pattern() {
    let config = Config {
        endpoint: "http://test:8080".to_string(),
        timeout: Duration::from_secs(45),
        format: OutputFormat::Json,
        no_color: true,
        no_header: true,
        no_pager: false,
    };

    assert_eq!(config.endpoint, "http://test:8080");
    assert_eq!(config.timeout, Duration::from_secs(45));
    assert_eq!(config.format, OutputFormat::Json);
    assert!(config.no_color);
    assert!(config.no_header);
}

#[test]
fn test_config_all_options_combined() {
    let config = Config {
        endpoint: "http://prod:9000".to_string(),
        timeout: Duration::from_secs(120),
        format: OutputFormat::Json,
        no_color: true,
        no_header: true,
        no_pager: false,
    };

    assert_eq!(config.endpoint, "http://prod:9000");
    assert_eq!(config.timeout, Duration::from_secs(120));
    assert_eq!(config.format, OutputFormat::Json);
    assert!(config.no_color);
    assert!(config.no_header);
}
