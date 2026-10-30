// src/assets.rs — 嵌入静态资源 + SPA fallback（D-02）
use axum::body::Body;
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::EmbeddedFile;

include!(concat!(env!("OUT_DIR"), "/embed.rs"));

pub fn get(path: &str) -> Option<EmbeddedFile> {
    EmbeddedAssets::get(path)
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit_once('.').map(|(_, ext)| ext) {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json") => "application/json",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("ico") => "image/x-icon",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        _ => "application/octet-stream",
    }
}

pub async fn serve(uri: Uri) -> Response {
    let raw = uri.path().trim_start_matches('/');
    let candidate = if raw.is_empty() { "index.html" } else { raw };
    let (path, file) = match get(candidate) {
        Some(f) => (candidate, f),
        None => match get("index.html") {
            Some(f) => ("index.html", f),
            None => return (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    };
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, content_type(path))],
        Body::from(file.data.into_owned()),
    )
        .into_response()
}
