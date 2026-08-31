// Самообновление панели: проверка релиза на GitHub + установка бинаря.
use crate::VERSION;
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use std::io::Read;
use std::path::Path;
use std::time::Duration;

use crate::api::{api_err, api_ok};
use crate::AppState;

const GITHUB_API: &str = "https://api.github.com/repos/nickitafedorov2012-code/xkeen-ui-ext/releases/latest";
const GITHUB_RELEASE: &str = "https://github.com/nickitafedorov2012-code/xkeen-ui-ext/releases/download";
const BIN_PATH: &str = "/opt/sbin/xkeen-route";
const INIT_SCRIPT: &str = "/opt/etc/init.d/S99xkeen-route";

#[derive(Deserialize)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
}

fn version_tuple(v: &str) -> Vec<u64> {
    v.trim_start_matches('v')
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u64>().ok())
        .collect()
}

fn is_newer(latest: &str, current: &str) -> bool {
    version_tuple(latest) > version_tuple(current)
}

/// Первые строки release notes (список изменений) для UI.
fn notes_lines(body: &str, max: usize) -> Vec<String> {
    body.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("<!--"))
        .map(|l| l.trim_start_matches(['-', '*', ' ']).trim().to_string())
        .filter(|l| !l.is_empty())
        .take(max)
        .collect()
}

async fn fetch_latest(http: &reqwest::Client) -> Result<GhRelease, String> {
    let res = http
        .get(GITHUB_API)
        .header("Accept", "application/vnd.github+json")
        .timeout(Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("GitHub недоступен: {e}"))?;
    if !res.status().is_success() {
        return Err(format!("GitHub API: HTTP {}", res.status()));
    }
    res.json::<GhRelease>().await.map_err(|e| format!("Ответ GitHub не разобран: {e}"))
}

/// GET /api/update/check — текущая/последняя версия + список изменений.
pub async fn check(State(state): State<AppState>) -> Response {
    let rel = match fetch_latest(&state.http).await {
        Ok(r) => r,
        Err(e) => return api_err(e),
    };
    let latest = rel.tag_name.clone();
    let update_available = is_newer(&latest, VERSION);
    api_ok(json!({
        "current": VERSION,
        "latest": latest,
        "update_available": update_available,
        "notes": notes_lines(&rel.body, 12),
    }))
}

/// POST /api/update/install — скачать бинарь релиза, заменить, перезапустить сервис.
pub async fn install(State(state): State<AppState>) -> Response {
    let rel = match fetch_latest(&state.http).await {
        Ok(r) => r,
        Err(e) => return api_err(e),
    };
    let ver = rel.tag_name.clone();
    if !is_newer(&ver, VERSION) {
        return api_err(format!("Уже установлена актуальная версия {VERSION}"));
    }

    let arch = std::env::consts::ARCH;
    let asset = match arch {
        "aarch64" => "xkeen-route-arm64-v8a",
        _ => return api_err(format!("Архитектура {arch} не поддерживается автообновлением")),
    };
    let url = format!("{GITHUB_RELEASE}/{ver}/{asset}");
    crate::log_i!("[UPDATE] Загрузка {url}");

    let tmp_dir = Path::new("/opt/tmp");
    let _ = tokio::fs::create_dir_all(tmp_dir).await;
    let tmp = tmp_dir.join("xkeen-route.update");
    let tmp_for_check = tmp.clone();

    let res = match state
        .http
        .get(&url)
        .timeout(Duration::from_secs(300))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => return api_err(format!("Загрузка: HTTP {}", r.status())),
        Err(e) => return api_err(format!("Загрузка: {e}")),
    };
    let bytes = match res.bytes().await {
        Ok(b) => b,
        Err(e) => return api_err(format!("Загрузка: {e}")),
    };

    // Проверка целостности: размер и ELF-магия.
    let check = tokio::task::spawn_blocking(move || -> Result<(), String> {
        if bytes.len() < 1024 * 1024 {
            return Err(format!("Файл слишком мал ({} байт) — повреждённый артефакт", bytes.len()));
        }
        let mut magic = [0u8; 4];
        let mut cur = std::io::Cursor::new(&bytes[..]);
        cur.read_exact(&mut magic).map_err(|e| e.to_string())?;
        if magic != [0x7F, b'E', b'L', b'F'] {
            return Err("Файл не является ELF-бинарём — отменено".into());
        }
        std::fs::write(&tmp_for_check, &bytes).map_err(|e| format!("Запись: {e}"))?;
        Ok(())
    })
    .await
    .map_err(|e| format!("Проверка: {e}"))
    .and_then(|r| r);

    if let Err(e) = check {
        let _ = tokio::fs::remove_file(&tmp).await;
        return api_err(e);
    }

    // Замена бинаря.
    if let Err(e) = tokio::fs::rename(&tmp, BIN_PATH).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return api_err(format!("Установка: {e}"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(BIN_PATH, std::fs::Permissions::from_mode(0o755)).await;
    }
    crate::log_i!("[UPDATE] Установлена {ver}, перезапуск сервиса");
    // Перезапуск после ответа клиенту: spawn — панель перезапустится сама.
    if Path::new(INIT_SCRIPT).exists() {
        _ = tokio::process::Command::new(INIT_SCRIPT)
            .arg("restart")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
        api_ok(json!({ "installed": ver, "restarting": true }))
    } else {
        api_err(format!("{ver} установлена, но {INIT_SCRIPT} не найден — перезапустите вручную"))
    }
}
