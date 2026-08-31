use axum::http::{HeaderMap, StatusCode, Uri, header};
use axum::response::Response;
use rust_embed::Embed;
use sha2::{Digest, Sha256};

#[derive(Embed)]
#[folder = "web/dist/"]
struct FrontendAssets;

/// vite 产物文件名形如 `index-<hash>.js`（内容哈希即文件名一部分），可无限期强缓存。
fn is_hashed_asset(path: &str) -> bool {
    path.starts_with("assets/")
}

/// 用内容 sha256 前 16 位作 ETag（内容不可变，天然稳定）。
fn etag_for(content: &[u8]) -> String {
    let digest = Sha256::digest(content);
    let hex_digest = hex::encode(digest);
    format!("\"{}\"", &hex_digest[..16])
}

fn build_response(
    content: &[u8],
    mime: &str,
    etag: &str,
    cache_control: &str,
) -> Result<Response, StatusCode> {
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, mime)
        .header(header::ETAG, etag)
        .header(header::CACHE_CONTROL, cache_control)
        .body(content.to_vec().into())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

/// 命中 If-None-Match 时返回 304（响应体为空）。
fn not_modified() -> Response {
    Response::builder()
        .status(StatusCode::NOT_MODIFIED)
        .body(axum::body::Body::empty())
        .expect("304 empty body cannot fail")
}

/// 请求头中的 If-None-Match 是否命中给定 etag（支持逗号分隔多值）。
fn etag_matches(headers: &HeaderMap, etag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.split(',').any(|tag| tag.trim() == etag))
}

pub async fn serve_asset(uri: Uri, headers: HeaderMap) -> Result<Response, StatusCode> {
    let path = uri.path().trim_start_matches('/');

    if let Some(content) = FrontendAssets::get(path) {
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        let etag = etag_for(&content.data);
        if etag_matches(&headers, &etag) {
            return Ok(not_modified());
        }
        let cache_control = if is_hashed_asset(path) {
            "public, max-age=31536000, immutable"
        } else {
            "no-cache"
        };
        return build_response(&content.data, &mime, &etag, cache_control);
    }

    if let Some(index) = FrontendAssets::get("index.html") {
        let etag = etag_for(&index.data);
        if etag_matches(&headers, &etag) {
            return Ok(not_modified());
        }
        return build_response(&index.data, "text/html", &etag, "no-cache");
    }

    Err(StatusCode::NOT_FOUND)
}
