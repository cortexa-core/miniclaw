use axum::{
    http::{header, StatusCode, Uri},
    response::{Html, IntoResponse, Response},
};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "web/dist"]
struct Assets;

pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');

    if let Some(file) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime.as_ref())],
            file.data,
        )
            .into_response();
    }

    // SPA fallback
    match Assets::get("index.html") {
        Some(file) => {
            let html = std::str::from_utf8(&file.data).unwrap_or("").to_owned();
            Html(html).into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}
