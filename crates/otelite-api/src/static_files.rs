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
        "js/analytics.js" => Some((
            include_bytes!("../static/js/analytics.js"),
            "application/javascript; charset=utf-8",
        )),
        "js/lazy.js" => Some((
            include_bytes!("../static/js/lazy.js"),
            "application/javascript; charset=utf-8",
        )),
        "js/filters.js" => Some((
            include_bytes!("../static/js/filters.js"),
            "application/javascript; charset=utf-8",
        )),
        "js/sessions.js" => Some((
            include_bytes!("../static/js/sessions.js"),
            "application/javascript; charset=utf-8",
        )),
        "js/overview.js" => Some((
            include_bytes!("../static/js/overview.js"),
            "application/javascript; charset=utf-8",
        )),
        "js/palette.js" => Some((
            include_bytes!("../static/js/palette.js"),
            "application/javascript; charset=utf-8",
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// Every file under static/ must have a match arm. An unregistered
    /// file is served as index.html with a 200 by the SPA fallback — a
    /// silent, hard-to-spot breakage (a .js URL returning HTML).
    #[test]
    fn every_static_asset_is_registered() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("static");
        let mut files = BTreeSet::new();
        fn walk(dir: &std::path::Path, out: &mut BTreeSet<String>) {
            let entries = std::fs::read_dir(dir).expect("static tree readable");
            for entry in entries {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    walk(&path, out);
                } else {
                    let rel = path
                        .strip_prefix(env!("CARGO_MANIFEST_DIR"))
                        .expect("path under manifest dir")
                        .to_string_lossy()
                        .replace('\\', "/");
                    out.insert(rel);
                }
            }
        }
        walk(&root, &mut files);
        assert!(
            files.len() >= 10,
            "static tree unexpectedly small: {files:?}"
        );
        for f in &files {
            let key = f.strip_prefix("static/").unwrap_or(f.as_str());
            assert!(
                get_static_file(key).is_some(),
                "static file not registered in get_static_file: {key}"
            );
        }
    }

    #[test]
    fn palette_js_served_as_javascript() {
        let (content, ctype) =
            get_static_file("js/palette.js").expect("palette.js must be registered");
        assert!(ctype.starts_with("application/javascript"));
        // The keybinding hint must survive embedding (catches a stale copy).
        let text = String::from_utf8_lossy(content);
        assert!(text.contains("cmd-palette"));
    }

    #[test]
    fn unknown_path_is_none() {
        assert!(get_static_file("js/does-not-exist.js").is_none());
        assert!(get_static_file("").is_none());
    }
}
