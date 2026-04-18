//! CORS middleware configuration

use axum::http::{header, Method};
use tower_http::cors::{Any, CorsLayer};

/// Create CORS middleware layer
///
/// Configures Cross-Origin Resource Sharing to allow frontend applications
/// to access the API from different origins.
pub fn create_cors_layer(allowed_origins: Vec<String>) -> CorsLayer {
    let origins: Vec<_> = allowed_origins
        .iter()
        .filter_map(|origin| origin.parse().ok())
        .collect();

    CorsLayer::new()
        .allow_origin(origins)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT])
        .allow_credentials(false)
        .max_age(std::time::Duration::from_secs(3600))
}

/// Create permissive CORS layer for development
///
/// Allows all origins, methods, and headers. Should only be used in development.
pub fn create_permissive_cors_layer() -> CorsLayer {
    CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any)
        .max_age(std::time::Duration::from_secs(3600))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cors_layer_creation() {
        let origins = vec![
            "http://localhost:3000".to_string(),
            "http://localhost:5173".to_string(),
        ];
        let _layer = create_cors_layer(origins);
        // Layer creation should not panic
    }

    #[test]
    fn test_permissive_cors_layer() {
        let _layer = create_permissive_cors_layer();
        // Layer creation should not panic
    }
}

// Made with Bob
