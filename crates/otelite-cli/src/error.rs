//! Error types for the Otelite CLI

use std::fmt;

/// Result type alias for CLI operations
pub type Result<T> = std::result::Result<T, Error>;

/// CLI error types
#[derive(Debug)]
pub enum Error {
    /// API request failed
    ApiError(String),
    /// Connection to backend failed
    ConnectionError(String),
    /// Resource not found
    NotFound(String),
    /// Invalid argument or configuration
    InvalidArgument(String),
    /// Configuration or service management error
    ConfigError(String),
    /// HTTP request error
    HttpError(reqwest::Error),
    /// JSON parsing error
    JsonError(serde_json::Error),
    /// IO error
    IoError(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ApiError(msg) => write!(f, "API error: {}", msg),
            Error::ConnectionError(msg) => write!(f, "Connection error: {}", msg),
            Error::NotFound(msg) => write!(f, "Not found: {}", msg),
            Error::InvalidArgument(msg) => write!(f, "Invalid argument: {}", msg),
            Error::ConfigError(msg) => write!(f, "Configuration error: {}", msg),
            Error::HttpError(err) => write!(f, "HTTP error: {}", err),
            Error::JsonError(err) => write!(f, "JSON error: {}", err),
            Error::IoError(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::HttpError(err) => Some(err),
            Error::JsonError(err) => Some(err),
            Error::IoError(err) => Some(err),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        if err.is_connect() || err.is_timeout() {
            Error::ConnectionError(format!(
                "Failed to connect to Otelite backend. Is the server running? Error: {}",
                err
            ))
        } else if err.is_status() {
            if let Some(status) = err.status() {
                if status.as_u16() == 404 {
                    Error::NotFound("Resource not found".to_string())
                } else {
                    Error::ApiError(format!("HTTP {}: {}", status, err))
                }
            } else {
                Error::ApiError(err.to_string())
            }
        } else {
            Error::HttpError(err)
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::JsonError(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::IoError(err)
    }
}

impl Error {
    /// Get the appropriate exit code for this error
    pub fn exit_code(&self) -> i32 {
        match self {
            Error::ConnectionError(_) => 2,
            Error::NotFound(_) => 3,
            _ => 1,
        }
    }

    /// Get a user-friendly error message with suggestions
    pub fn user_message(&self) -> String {
        match self {
            Error::ConnectionError(msg) => {
                format!(
                    "{}\n\nSuggestions:\n  - Check if Otelite server is running\n  - Verify the endpoint URL with --endpoint flag\n  - Check network connectivity",
                    msg
                )
            },
            Error::NotFound(msg) => {
                format!(
                    "{}\n\nSuggestions:\n  - Verify the ID is correct\n  - Use list command to see available items",
                    msg
                )
            },
            Error::InvalidArgument(msg) => {
                format!(
                    "{}\n\nSuggestion:\n  - Use --help to see valid options",
                    msg
                )
            },
            _ => self.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = Error::ApiError("test error".to_string());
        assert_eq!(err.to_string(), "API error: test error");

        let err = Error::ConnectionError("connection failed".to_string());
        assert_eq!(err.to_string(), "Connection error: connection failed");

        let err = Error::NotFound("item not found".to_string());
        assert_eq!(err.to_string(), "Not found: item not found");
    }

    #[test]
    fn test_exit_codes() {
        assert_eq!(Error::ApiError("test".to_string()).exit_code(), 1);
        assert_eq!(Error::ConnectionError("test".to_string()).exit_code(), 2);
        assert_eq!(Error::NotFound("test".to_string()).exit_code(), 3);
        assert_eq!(Error::InvalidArgument("test".to_string()).exit_code(), 1);
    }

    #[test]
    fn test_user_message() {
        let err = Error::ConnectionError("Failed to connect".to_string());
        let msg = err.user_message();
        assert!(msg.contains("Suggestions"));
        assert!(msg.contains("Check if Otelite server is running"));

        let err = Error::NotFound("Log not found".to_string());
        let msg = err.user_message();
        assert!(msg.contains("Suggestions"));
        assert!(msg.contains("Verify the ID is correct"));
    }
}
