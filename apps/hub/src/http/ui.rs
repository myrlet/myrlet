//! Same-origin embedded Web UI: static assets with SPA fallback and strict headers.

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, Method, StatusCode, Uri};
use axum::response::{IntoResponse, Response};

use crate::error::ApiError;
use crate::state::HubState;

mod web_assets {
    include!(concat!(env!("OUT_DIR"), "/web_assets.rs"));
}

/// Single-page app shell served for `/` and unknown navigation paths.
const INDEX: &str = "/index.html";

/// Strict same-origin policy: no inline scripts, no framing, no third-party origins.
const CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; \
style-src 'self' 'unsafe-inline'; img-src 'self' data:; font-src 'self'; \
connect-src 'self'; frame-ancestors 'none'; base-uri 'self'; form-action 'self'";

/// `GET`/`HEAD` fallback for every path the API router does not claim.
pub async fn serve_ui(State(state): State<HubState>, method: Method, uri: Uri) -> Response {
    let path = uri.path();
    if is_api_path(path) || !state.config.ui_enabled {
        return not_found();
    }
    if method != Method::GET && method != Method::HEAD {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    match resolve_asset(web_assets::ASSETS, path) {
        Some((_, content_type, bytes)) => asset_response(method, path, content_type, bytes),
        None => not_found(),
    }
}

/// API paths never fall back to HTML, whether or not the UI is enabled.
fn is_api_path(path: &str) -> bool {
    path == "/v1" || path.starts_with("/v1/")
}

fn not_found() -> Response {
    ApiError::new(
        StatusCode::NOT_FOUND,
        "NotFound",
        "the requested resource does not exist",
    )
    .into_response()
}

/// Exact asset match first, then SPA fallback for extension-less navigation paths.
fn resolve_asset<'a>(
    assets: &'a [(&'a str, &'a str, &'a [u8])],
    path: &str,
) -> Option<(&'a str, &'a str, &'a [u8])> {
    let lookup = if path == "/" { INDEX } else { path };
    if let Some(asset) = assets.iter().find(|asset| asset.0 == lookup) {
        return Some(*asset);
    }
    if lookup.starts_with("/assets/") || last_segment_has_extension(lookup) {
        return None;
    }
    assets.iter().find(|asset| asset.0 == INDEX).copied()
}

/// A missing static file (`/assets/x.js`, `/favicon.ico`) must 404, never return HTML.
fn last_segment_has_extension(path: &str) -> bool {
    path.rsplit('/')
        .next()
        .is_some_and(|segment| segment.contains('.'))
}

fn asset_response(
    method: Method,
    path: &str,
    content_type: &str,
    bytes: &'static [u8],
) -> Response {
    let cache_control = if path.starts_with("/assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    let body = if method == Method::HEAD {
        Body::empty()
    } else {
        Body::from(bytes)
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, cache_control)
        .header("content-security-policy", CONTENT_SECURITY_POLICY)
        .header("x-content-type-options", "nosniff")
        .header("referrer-policy", "no-referrer")
        .header("x-frame-options", "DENY")
        .header(header::CONTENT_LENGTH, bytes.len().to_string())
        .body(body)
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

#[cfg(test)]
mod tests {
    use super::resolve_asset;

    const ASSETS: &[(&str, &str, &[u8])] = &[
        ("/index.html", "text/html; charset=utf-8", b"<html></html>"),
        (
            "/assets/app.js",
            "application/javascript; charset=utf-8",
            b"js",
        ),
    ];

    fn resolved(path: &str) -> Option<&'static str> {
        resolve_asset(ASSETS, path).map(|asset| asset.0)
    }

    #[test]
    fn root_serves_the_app_shell() {
        assert_eq!(resolved("/"), Some("/index.html"));
    }

    #[test]
    fn exact_asset_match_wins() {
        assert_eq!(resolved("/assets/app.js"), Some("/assets/app.js"));
    }

    #[test]
    fn missing_hashed_asset_is_not_html() {
        assert_eq!(resolved("/assets/missing.js"), None);
    }

    #[test]
    fn extensionless_navigation_falls_back_to_the_app_shell() {
        assert_eq!(resolved("/tasks/task_1"), Some("/index.html"));
    }

    #[test]
    fn missing_root_static_file_is_not_html() {
        assert_eq!(resolved("/robots.txt"), None);
    }
}
