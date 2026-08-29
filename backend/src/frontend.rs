use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../frontend/dist"]
struct Assets;

/// Раздача встроенного SPA с fallback на index.html (роутинг на стороне клиента).
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response()
        }
        None => match Assets::get("index.html") {
            Some(content) => (
                [(header::CONTENT_TYPE, "text/html")],
                content.data,
            )
                .into_response(),
            None => (
                StatusCode::NOT_FOUND,
                "Фронтенд не собран: выполните `npm run build` в frontend/",
            )
                .into_response(),
        },
    }
}
