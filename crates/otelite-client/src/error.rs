use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("API error: {0}")]
    ApiError(String),
    #[error("Connection error: {0}")]
    ConnectionError(String),
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("HTTP error: {0}")]
    HttpError(#[source] reqwest::Error),
    #[error("JSON error: {0}")]
    ParseError(#[from] serde_json::Error),
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
