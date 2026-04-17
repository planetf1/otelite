//! Rotel Core Library
//!
//! This crate provides core functionality for the Rotel OpenTelemetry receiver.
//!
//! # Examples
//!
//! Basic usage of the core library:
//!
//! ```
//! use rotel_core::{Config, add, divide};
//!
//! // Simple arithmetic
//! let sum = add(2, 3);
//! assert_eq!(sum, 5);
//!
//! // Error handling
//! let result = divide(10.0, 2.0);
//! assert!(result.is_ok());
//! assert_eq!(result.unwrap(), 5.0);
//!
//! // Configuration
//! let config = Config::new("my-service".to_string(), 8080);
//! assert!(config.is_valid());
//! ```
//!
//! # Error Handling
//!
//! Functions that can fail return `Result` types:
//!
//! ```
//! use rotel_core::divide;
//!
//! // This will return an error
//! let result = divide(10.0, 0.0);
//! assert!(result.is_err());
//! assert_eq!(result.unwrap_err(), "Division by zero");
//! ```

pub fn add(left: u64, right: u64) -> u64 {
    left + right
}

/// Example function to demonstrate error handling
pub fn divide(numerator: f64, denominator: f64) -> Result<f64, String> {
    if denominator == 0.0 {
        Err("Division by zero".to_string())
    } else {
        Ok(numerator / denominator)
    }
}

/// Example struct to demonstrate testing patterns
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub name: String,
    pub port: u16,
}

impl Config {
    pub fn new(name: String, port: u16) -> Self {
        Self { name, port }
    }
    
    pub fn is_valid(&self) -> bool {
        !self.name.is_empty() && self.port > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn test_add() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }

    #[test]
    fn test_add_large_numbers() {
        let result = add(u64::MAX - 1, 1);
        assert_eq!(result, u64::MAX);
    }

    #[test]
    fn test_divide_success() {
        let result = divide(10.0, 2.0);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 5.0);
    }

    #[test]
    fn test_divide_by_zero() {
        let result = divide(10.0, 0.0);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Division by zero");
    }

    #[test]
    fn test_config_creation() {
        let config = Config::new("test".to_string(), 8080);
        assert_eq!(config.name, "test");
        assert_eq!(config.port, 8080);
    }

    #[test]
    fn test_config_validation() {
        let valid_config = Config::new("test".to_string(), 8080);
        assert!(valid_config.is_valid());

        let invalid_config = Config::new("".to_string(), 8080);
        assert!(!invalid_config.is_valid());

        let invalid_port = Config::new("test".to_string(), 0);
        assert!(!invalid_port.is_valid());
    }

    #[test]
    fn test_config_equality() {
        let config1 = Config::new("test".to_string(), 8080);
        let config2 = Config::new("test".to_string(), 8080);
        let config3 = Config::new("other".to_string(), 8080);

        // Using pretty_assertions for better diff output
        assert_eq!(config1, config2);
        assert_ne!(config1, config3);
    }

    #[test]
    fn test_config_clone() {
        let config = Config::new("test".to_string(), 8080);
        let cloned = config.clone();
        assert_eq!(config, cloned);
    }
}
