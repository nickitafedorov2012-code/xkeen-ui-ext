// Самообновление панели: проверка релиза на GitHub + установка бинаря.
use crate::VERSION;
use axum::extract::State;
use axum::response::Response;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;
use std::time::Duration;

use crate::api::{api_err, api_ok};
use crate::AppState;

const REPO: &str = "nickitafedorov2012-code/xkeen-ui-ext";
const JSDELIVR_RESOLVED: &str =
    "https://data.jsdelivr.com/v1/packages/gh/nickitafedorov2012-code/xkeen-ui-ext/resolved";
const JSDELIVR_CDN: &str = "https://cdn.jsdelivr.net/gh/nickitafedorov2012-code/xkeen-ui-ext";
const GITHUB_RELEASES: &str =
    "https://api.github.com/repos/nickitafedorov2012-code/xkeen-ui-ext/releases?per_page=1";
const GITHUB_RELEASE: &str = "https://github.com/nickitafedorov2012-code/xkeen-ui-ext/releases/download";
const GITHUB_LATEST: &str = "https://github.com/nickitafedorov2012-code/xkeen-ui-ext/releases/latest";
const BIN_PATH: &str = "/opt/sbin/xkeen-route";
const INIT_SCRIPT: &str = "/opt/etc/init.d/S99xkeen-route";

#[derive(Deserialize, Clone)]
struct GhRelease {
    tag_name: String,
    #[serde(default)]
    body: String,
}

static LAST_CHECK: tokio::sync::Mutex<Option<(std::time::Instant, GhRelease)>> =
    tokio::sync::Mutex::const_new(None);

#[derive(Deserialize)]
struct JsDelivrResolved {
    version: String,
}

/// Клиент с проксированием через Mihomo mixed-port (обход блокировок ТСПУ для локальных процессов).
fn proxied_client(proxy_addr: &str) -> Option<reqwest::Client> {
    let proxy = reqwest::Proxy::all(proxy_addr).ok()?;
    reqwest::Client::builder()
        .proxy(proxy)
        .danger_accept_invalid_certs(true)
        .build()
        .ok()
}

fn version_tuple(v: &str) -> (Vec<u64>, bool) {
    let clean = v.trim_start_matches('v');
    let is_prerelease = clean.contains('-');
    let main_part = clean.split('-').next().unwrap_or(clean);
    let parts: Vec<u64> = main_part
        .split('.')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();
    (parts, !is_prerelease)
}

fn is_newer(latest: &str, current: &str) -> bool {
    version_tuple(latest) > version_tuple(current)
}

/// Первые строки release notes (список изменений) для UI.
fn notes_lines(body: &str, max: usize) -> Vec<String> {
    body.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("<!--"))
        // Срезаем только маркер списка; markdown-выделение (**жирный**) не трогаем.
        .map(|l| {
            l.strip_prefix("- ")
                .or_else(|| l.strip_prefix("* "))
                .unwrap_or(l)
                .trim()
                .to_string()
        })
        .filter(|l| !l.is_empty())
        .take(max)
        .collect()
}

async fn fetch_latest(direct: &reqwest::Client, proxy_url: &str) -> Result<GhRelease, String> {
    let proxied = proxied_client(proxy_url);
    let mut clients = Vec::new();
    if let Some(ref p) = proxied {
        clients.push(p);
    }
    clients.push(direct);

    for http in clients {
        // 1. Быстрый редирект GitHub releases/latest (без лимитов API, мгновенно)
        if let Ok(res) = http
            .get(GITHUB_LATEST)
            .timeout(Duration::from_secs(6))
            .send()
            .await
        {
            let final_url = res.url().to_string();
            if let Some(tag) = final_url.split("/releases/tag/").nth(1) {
                let tag = tag.trim_matches('/').to_string();
                if !tag.is_empty() {
                    let notes = fetch_notes(http, &tag).await;
                    return Ok(GhRelease { tag_name: tag, body: notes });
                }
            }
        }

        // 2. jsDelivr CDN
        if let Ok(res) = http
            .get(JSDELIVR_RESOLVED)
            .timeout(Duration::from_secs(8))
            .send()
            .await
        {
            if res.status().is_success() {
                if let Ok(r) = res.json::<JsDelivrResolved>().await {
                    let tag = format!("v{}", r.version.trim_start_matches('v'));
                    let notes = fetch_notes(http, &tag).await;
                    return Ok(GhRelease { tag_name: tag, body: notes });
                }
            }
        }

        // 3. GitHub REST API (список релизов)
        if let Ok(res) = http
            .get(GITHUB_RELEASES)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "xkeen-route")
            .timeout(Duration::from_secs(8))
            .send()
            .await
        {
            if res.status().is_success() {
                if let Ok(list) = res.json::<Vec<GhRelease>>().await {
                    if let Some(rel) = list.into_iter().next() {
                        return Ok(rel);
                    }
                }
            }
        }
    }
    Err("GitHub и jsDelivr недоступны (напрямую и через прокси)".into())
}

/// Кэшированное получение последнего релиза (кэш 30 секунд).
async fn get_latest_cached(http: &reqwest::Client, proxy_url: &str) -> Result<GhRelease, String> {
    let mut guard = LAST_CHECK.lock().await;
    if let Some((time, ref rel)) = *guard {
        if time.elapsed() < Duration::from_secs(30) {
            return Ok(rel.clone());
        }
    }
    let fresh = fetch_latest(http, proxy_url).await?;
    *guard = Some((std::time::Instant::now(), fresh.clone()));
    Ok(fresh)
}

/// Список изменений: секция `### {tag}` из DEVELOPMENT.md через jsDelivr CDN.
async fn fetch_notes(http: &reqwest::Client, tag: &str) -> String {
    let url = format!("{JSDELIVR_CDN}@{tag}/DEVELOPMENT.md");
    let Ok(res) = http.get(&url).timeout(Duration::from_secs(15)).send().await else {
        return String::new();
    };
    let Ok(text) = res.text().await else {
        return String::new();
    };
    let mut lines = Vec::new();
    let mut in_section = false;
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with("### ") {
            if in_section {
                break;
            }
            in_section = t.contains(tag);
            continue;
        }
        if in_section && (t.starts_with("- ") || t.starts_with("* ")) {
            lines.push(t[2..].trim().to_string());
        }
    }
    lines.join("\n")
}

/// GET /api/update/check — текущая/последняя версия + список изменений.
pub async fn check(State(state): State<AppState>) -> Response {
    let proxy_url = {
        let cfg = state.config.read().await;
        cfg.mihomo_proxy_url()
    };
    let rel = match get_latest_cached(&state.http, &proxy_url).await {
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
    let proxy_url = {
        let cfg = state.config.read().await;
        cfg.mihomo_proxy_url()
    };
    let rel = match fetch_latest(&state.http, &proxy_url).await {
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
        "mipsel" => "xkeen-route-mipsel",
        "mips" => "xkeen-route-mips",
        "arm" => "xkeen-route-armv7",
        _ => return api_err(format!("Архитектура {arch} не поддерживается автообновлением")),
    };
    let url = format!("{GITHUB_RELEASE}/{ver}/{asset}");
    crate::log_i!("[UPDATE] Загрузка {url}");

    let tmp_dir = Path::new("/opt/tmp");
    let _ = tokio::fs::create_dir_all(tmp_dir).await;
    let tmp = tmp_dir.join("xkeen-route.update");

    let proxied = proxied_client(&proxy_url);
    let http = proxied.as_ref().unwrap_or(&state.http);
    let mut res = match http
        .get(&url)
        .header("User-Agent", "xkeen-route")
        .timeout(Duration::from_secs(300))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => return api_err(format!("Загрузка: HTTP {}", r.status())),
        Err(e) => return api_err(format!("Загрузка: {e}")),
    };

    use tokio::io::AsyncWriteExt;
    let mut file = match tokio::fs::File::create(&tmp).await {
        Ok(f) => f,
        Err(e) => return api_err(format!("Создание временного файла: {e}")),
    };

    let mut total_bytes = 0usize;
    let mut magic = [0u8; 4];
    let mut magic_len = 0usize;

    loop {
        match res.chunk().await {
            Ok(Some(chunk)) => {
                if magic_len < 4 {
                    let needed = 4 - magic_len;
                    let take = chunk.len().min(needed);
                    magic[magic_len..magic_len + take].copy_from_slice(&chunk[..take]);
                    magic_len += take;
                }
                total_bytes += chunk.len();
                if let Err(e) = file.write_all(&chunk).await {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return api_err(format!("Запись: {e}"));
                }
            }
            Ok(None) => break,
            Err(e) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return api_err(format!("Загрузка: {e}"));
            }
        }
    }

    if let Err(e) = file.flush().await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return api_err(format!("Сброс буфера: {e}"));
    }
    drop(file);

    // Проверка целостности: размер и ELF-магия
    if total_bytes < 1024 * 1024 {
        let _ = tokio::fs::remove_file(&tmp).await;
        return api_err(format!("Файл слишком мал ({total_bytes} байт) — повреждённый артефакт"));
    }
    if magic != [0x7F, b'E', b'L', b'F'] {
        let _ = tokio::fs::remove_file(&tmp).await;
        return api_err("Файл не является ELF-бинарём — отменено");
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
