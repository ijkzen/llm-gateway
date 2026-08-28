use axum::http::{StatusCode, header};
use axum::response::Response;
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/dist/"]
struct FrontendAssets;

pub async fn serve_asset(uri: axum::http::Uri) -> Result<Response, StatusCode> {
    let path = uri.path().trim_start_matches('/');

    if let Some(content) = FrontendAssets::get(path) {
        let mime = mime_guess::from_path(path)
            .first_or_octet_stream()
            .to_string();
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, mime)
            .body(content.data.into())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }

    if let Some(index) = FrontendAssets::get("index.html") {
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html")
            .body(index.data.into())
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR);
    }

    Err(StatusCode::NOT_FOUND)
}
