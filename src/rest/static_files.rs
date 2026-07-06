use std::path::{Path, PathBuf};

use bytes::Bytes;
use http_body_util::Full;
use hyper::{
    header::{HeaderValue, CONTENT_TYPE},
    Method, Response, StatusCode,
};

pub(crate) struct AdminUiStaticFiles {
    root: PathBuf,
}

impl AdminUiStaticFiles {
    #[must_use]
    pub(crate) fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub(crate) async fn dispatch(
        &self,
        method: &Method,
        segments: &[&str],
    ) -> Option<Response<Full<Bytes>>> {
        if method != Method::GET {
            return None;
        }

        match segments {
            ["admin"] | ["admin", ..] => Some(self.serve_index().await),
            ["assets"] => Some(not_found_response()),
            ["assets", asset_segments @ ..] => Some(self.serve_asset(asset_segments).await),
            _ => None,
        }
    }

    async fn serve_index(&self) -> Response<Full<Bytes>> {
        self.serve_file(self.root.join("index.html")).await
    }

    async fn serve_asset(&self, asset_segments: &[&str]) -> Response<Full<Bytes>> {
        if !asset_segments.iter().copied().all(is_safe_asset_segment) {
            return not_found_response();
        }

        let mut path = self.root.join("assets");
        for segment in asset_segments {
            path.push(segment);
        }

        self.serve_file(path).await
    }

    async fn serve_file(&self, path: PathBuf) -> Response<Full<Bytes>> {
        let Ok(root) = tokio::fs::canonicalize(&self.root).await else {
            return not_found_response();
        };
        let Ok(file) = tokio::fs::canonicalize(path).await else {
            return not_found_response();
        };

        if !file.starts_with(&root) {
            return not_found_response();
        }

        let Ok(metadata) = tokio::fs::metadata(&file).await else {
            return not_found_response();
        };
        if !metadata.is_file() {
            return not_found_response();
        }

        let Ok(body) = tokio::fs::read(&file).await else {
            return not_found_response();
        };

        file_response(&file, body)
    }
}

fn is_safe_asset_segment(segment: &str) -> bool {
    let lowered = segment.to_ascii_lowercase();
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && !segment.contains('\\')
        && !lowered.contains("%2e")
        && !lowered.contains("%2f")
        && !lowered.contains("%5c")
}

fn file_response(path: &Path, body: Vec<u8>) -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::from(Bytes::from(body)));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static(content_type(path)));
    response
}

fn not_found_response() -> Response<Full<Bytes>> {
    let mut response = Response::new(Full::from(Bytes::from_static(b"not found")));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

fn content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "css" => "text/css; charset=utf-8",
        "gif" => "image/gif",
        "html" => "text/html; charset=utf-8",
        "ico" => "image/x-icon",
        "jpg" | "jpeg" => "image/jpeg",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" | "map" => "application/json; charset=utf-8",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "txt" => "text/plain; charset=utf-8",
        "wasm" => "application/wasm",
        "webp" => "image/webp",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}
