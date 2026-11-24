use axum::response::{IntoResponse, Json, Response};
use serde_json::json;

use crate::{config, AppState};

pub mod backups;
pub use backups::*;
pub mod clash_proxy;
pub use clash_proxy::*;
pub mod dns_adguard;
pub use dns_adguard::*;
pub mod domains;
pub use domains::*;
pub mod multiwan;
pub use multiwan::*;
pub mod router;
pub mod routing;
pub use routing::*;
pub mod servers;
pub use servers::*;
pub mod status;
pub use status::*;
pub mod system_ssh;
pub use system_ssh::*;
pub mod vkturn;
pub use vkturn::*;
pub mod zapret;
pub use zapret::*;

pub use crate::error::{ApiResult, AppError};

/// Вспомогательное: единый формат ответа API.
pub fn api_ok(data: serde_json::Value) -> axum::response::Response {
    Json(json!({ "success": true, "data": data })).into_response()
}

pub fn api_err(error: impl Into<String>) -> axum::response::Response {
    Json(json!({ "success": false, "error": error.into() })).into_response()
}

pub fn api_err_bad_request(error: impl Into<String>) -> axum::response::Response {
    AppError::bad_request(error).into_response()
}

pub fn api_err_detailed(
    code: impl Into<String>,
    error: impl Into<String>,
    details: Option<serde_json::Value>,
    suggested_action: Option<&str>,
) -> axum::response::Response {
    let err_str = error.into();
    Json(json!({
        "success": false,
        "code": code.into(),
        "error": err_str,
        "details": details,
        "suggested_action": suggested_action,
    })).into_response()
}

/// Атомарная запись файла на диск (tmp + rename)
pub async fn atomic_write_file(path: impl AsRef<std::path::Path>, content: &str) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let p = path.as_ref();
    if let Some(parent) = p.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = format!("{}.{}.{}.tmp", p.display(), std::process::id(), nonce);
    // Z-12: пишем через File с sync_all перед rename — на роутерах с внезапной потерей
    // питания rename без fsync оставлял пустой/битый файл (config.json, zapret.conf, config.yaml)
    let write_res: Result<(), String> = async {
        let mut f = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| format!("Ошибка записи {tmp}: {e}"))?;
        f.write_all(content.as_bytes())
            .await
            .map_err(|e| format!("Ошибка записи {tmp}: {e}"))?;
        f.flush().await.map_err(|e| format!("Ошибка записи {tmp}: {e}"))?;
        f.sync_all().await.map_err(|e| format!("Ошибка fsync {tmp}: {e}"))?;
        Ok(())
    }
    .await;
    if let Err(e) = write_res {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(e);
    }
    if let Err(e) = tokio::fs::rename(&tmp, p).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Ошибка атомарного сохранения {}: {e}", p.display()));
    }
    Ok(())
}

/// Обработчик 404 для несуществующих API-эндпоинтов
pub async fn api_not_found() -> Response {
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(json!({
            "success": false,
            "error": "API endpoint not found"
        })),
    )
        .into_response()
}

/// Сохранение zapret.enabled вместе с пересборкой правил Mihomo в одной транзакции (баги 28, 57, 95).
/// Общая для бота, UI-ядра и сторожа. Если ядро не приняло reload, файлы всё равно приводятся
/// в согласованное состояние без reload — следующий старт Mihomo подхватит верные правила.
/// Атомарное обновление флагов AppConfig с одновременным переприменением правил crate::routing::apply_routing
/// и reload ядра Mihomo. Возвращает (новый AppConfig, примененный config.yaml).
pub async fn commit_config_with_routing<F>(
    state: &AppState,
    mutate: F,
) -> Result<(config::AppConfig, String), String>
where
    F: FnOnce(&mut config::AppConfig),
{
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    mutate(tx.config_mut());
    let new_cfg = tx.config().clone();
    let yaml = tx.read_yaml().await?;
    let (new_yaml, _) = crate::routing::apply_routing(&yaml, &new_cfg)?;
    tx.set_yaml(new_yaml.clone())?;
    tx.commit_and_reload().await?;
    Ok((new_cfg, new_yaml))
}

#[cfg(test)]
mod tests;
