use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::Embed;
use std::path::Path;

#[derive(Embed)]
#[folder = "../frontend/dist"]
pub struct Assets;

/// Раздача встроенного SPA с fallback на index.html (роутинг на стороне клиента).
pub async fn serve(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };

    // 1. Прямой поиск в Assets бинарника (приоритет у актуальной встроенной сборки)
    if let Some(content) = Assets::get(path) {
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response();
    }

    // 2. Если запрос был к /assets/..., но файл лежит в корне dist (или наоборот)
    if let Some(rest) = path.strip_prefix("assets/") {
        if let Some(content) = Assets::get(rest) {
            let mime = mime_guess::from_path(rest).first_or_octet_stream();
            return ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response();
        }
    } else {
        let with_assets = format!("assets/{path}");
        if let Some(content) = Assets::get(&with_assets) {
            let mime = mime_guess::from_path(&with_assets).first_or_octet_stream();
            return ([(header::CONTENT_TYPE, mime.as_ref())], content.data).into_response();
        }
    }

    // 3. Fallback: проверяем наличие внешнего файла на диске (если файл не был упакован в Assets)
    let disk_dist = Path::new("/opt/share/xkeen-route/dist");
    if disk_dist.is_dir() {
        let candidate = disk_dist.join(path);
        if candidate.is_file() {
            if let Ok(data) = tokio::fs::read(&candidate).await {
                let mime = mime_guess::from_path(&candidate).first_or_octet_stream();
                return ([(header::CONTENT_TYPE, mime.as_ref())], data).into_response();
            }
        }
    }

    // 4. Для статических ассетов (.js, .css, .ico, .svg, .png и т.д.) отдавать 404, а не index.html!
    // Возврат text/html на запрос .js ломает ES module loader браузера и приводит к чёрному экрану.
    let is_asset = path.contains('.') && !path.ends_with(".html");
    if is_asset {
        return (StatusCode::NOT_FOUND, "Asset not found").into_response();
    }

    // 5. Fallback на index.html для SPA клиентского роутинга
    match Assets::get("index.html") {
        Some(content) => (
            [(header::CONTENT_TYPE, "text/html")],
            content.data,
        )
            .into_response(),
        None => {
            let idx = disk_dist.join("index.html");
            if idx.is_file() {
                if let Ok(data) = tokio::fs::read(&idx).await {
                    return ([(header::CONTENT_TYPE, "text/html")], data).into_response();
                }
            }
            (
                StatusCode::NOT_FOUND,
                "Фронтенд не собран: выполните `npm run build` в frontend/",
            )
                .into_response()
        }
    }
}
