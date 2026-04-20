//! Static file serving with embedded assets

use axum::{
    body::Body,
    http::{header, StatusCode, Uri},
    response::{IntoResponse, Response},
};

/// Serve static files embedded in the binary
pub async fn serve_static_file(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    // Default to index.html for root path
    let path = if path.is_empty() { "index.html" } else { path };

    match get_static_file(path) {
        Some((content, content_type)) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(content))
            .unwrap(),
        None => {
            // If file not found, serve index.html for client-side routing
            if let Some((content, content_type)) = get_static_file("index.html") {
                Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, content_type)
                    .body(Body::from(content))
                    .unwrap()
            } else {
                (StatusCode::NOT_FOUND, "404 Not Found").into_response()
            }
        },
    }
}

/// Get embedded static file content and MIME type
fn get_static_file(path: &str) -> Option<(&'static [u8], &'static str)> {
    match path {
        "index.html" => Some((
            include_bytes!("../static/index.html"),
            "text/html; charset=utf-8",
        )),
        "css/styles.css" => Some((
            include_bytes!("../static/css/styles.css"),
            "text/css; charset=utf-8",
        )),
        "js/app.js" => Some((
            include_bytes!("../static/js/app.js"),
            "application/javascript; charset=utf-8",
        )),
        "js/api.js" => Some((
            include_bytes!("../static/js/api.js"),
            "application/javascript; charset=utf-8",
        )),
        "js/logs.js" => Some((
            include_bytes!("../static/js/logs.js"),
            "application/javascript; charset=utf-8",
        )),
        "js/traces.js" => Some((
            include_bytes!("../static/js/traces.js"),
            "application/javascript; charset=utf-8",
        )),
        "js/metrics.js" => Some((
            include_bytes!("../static/js/metrics.js"),
            "application/javascript; charset=utf-8",
        )),
        _ => None,
    }
}

// Made with Bob
