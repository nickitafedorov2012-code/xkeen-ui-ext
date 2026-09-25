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

// ==================== УПРАВЛЕНИЕ И ОБНОВЛЕНИЕ ЯДРА MIHOMO ====================

const MIHOMO_REPO_RELEASES: &str = "https://api.github.com/repos/MetaCubeX/mihomo/releases?per_page=15";
const MIHOMO_REPO_MIRROR: &str = "https://ghproxy.net/https://api.github.com/repos/MetaCubeX/mihomo/releases?per_page=15";

#[derive(Deserialize, serde::Serialize, Clone, Debug)]
pub struct MihomoReleaseAsset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Deserialize, serde::Serialize, Clone, Debug)]
pub struct MihomoRelease {
    pub tag_name: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub published_at: Option<String>,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub body: String,
    #[serde(default)]
    pub assets: Vec<MihomoReleaseAsset>,
}

static MIHOMO_RELEASES_CACHE: tokio::sync::Mutex<Option<(std::time::Instant, Vec<MihomoRelease>)>> =
    tokio::sync::Mutex::const_new(None);

pub async fn detect_mihomo_arch() -> &'static str {
    #[cfg(unix)]
    {
        if let Ok(out) = tokio::process::Command::new("uname").arg("-m").output().await {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_lowercase();
            if s.contains("aarch64") || s.contains("arm64") {
                return "arm64";
            }
            if s.contains("mips") {
                if s.contains("el") || s.contains("le") {
                    return "mipsle-softfloat";
                }
                return "mips-hardfloat";
            }
            if s.contains("x86_64") || s.contains("amd64") {
                return "amd64";
            }
            if s.contains("armv7") || s.contains("armv6") || s.contains("arm") {
                return "armv7";
            }
        }
    }

    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "mipsel" => "mipsle-softfloat",
        "mips" => "mips-hardfloat",
        "arm" => "armv7",
        "x86_64" => "amd64",
        _ => "arm64",
    }
}

pub fn find_mihomo_asset<'a>(assets: &'a [MihomoReleaseAsset], arch: &str) -> Option<&'a MihomoReleaseAsset> {
    let candidates: Vec<&MihomoReleaseAsset> = assets
        .iter()
        .filter(|a| a.name.ends_with(".gz") && a.name.contains("linux"))
        .collect();

    match arch {
        "arm64" => {
            candidates.iter().find(|a| a.name.contains("arm64") && !a.name.contains("armv")).copied()
        }
        "mipsle" | "mipsle-softfloat" => {
            candidates.iter().find(|a| a.name.contains("mipsle-softfloat") || a.name.contains("mipsle")).copied()
        }
        "mipsle-hardfloat" => {
            candidates.iter().find(|a| a.name.contains("mipsle-hardfloat") || a.name.contains("mipsle")).copied()
        }
        "mips" | "mips-hardfloat" => {
            candidates.iter().find(|a| a.name.contains("mips-hardfloat") || (a.name.contains("linux-mips-") && !a.name.contains("mips64") && !a.name.contains("mipsle"))).copied()
        }
        "mips-softfloat" => {
            candidates.iter().find(|a| a.name.contains("mips-softfloat")).copied()
        }
        "amd64" => {
            candidates.iter().find(|a| a.name.contains("amd64-compatible") || a.name.contains("amd64-v1") || a.name.contains("amd64")).copied()
        }
        "armv7" => {
            candidates.iter().find(|a| a.name.contains("armv7")).copied()
        }
        _ => candidates.into_iter().next(),
    }
}

async fn fetch_mihomo_releases(direct: &reqwest::Client, proxy_url: &str) -> Result<Vec<MihomoRelease>, String> {
    let proxied = proxied_client(proxy_url);
    let mut clients = Vec::new();
    if let Some(ref p) = proxied {
        clients.push(p);
    }
    clients.push(direct);

    let urls = [MIHOMO_REPO_RELEASES, MIHOMO_REPO_MIRROR];

    for http in &clients {
        for url in &urls {
            if let Ok(res) = http
                .get(*url)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "xkeen-route")
                .timeout(Duration::from_secs(10))
                .send()
                .await
            {
                if res.status().is_success() {
                    if let Ok(list) = res.json::<Vec<MihomoRelease>>().await {
                        if !list.is_empty() {
                            return Ok(list);
                        }
                    }
                }
            }
        }
    }
    Err("Не удалось получить список релизов Mihomo с GitHub".into())
}

async fn fetch_mihomo_releases_cached(direct: &reqwest::Client, proxy_url: &str) -> Result<Vec<MihomoRelease>, String> {
    let mut guard = MIHOMO_RELEASES_CACHE.lock().await;
    if let Some((time, ref list)) = *guard {
        if time.elapsed() < Duration::from_secs(45) {
            return Ok(list.clone());
        }
    }
    let fresh = fetch_mihomo_releases(direct, proxy_url).await?;
    *guard = Some((std::time::Instant::now(), fresh.clone()));
    Ok(fresh)
}

/// GET /api/mihomo/releases — список релизов ядра Mihomo
pub async fn mihomo_releases(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let proxy_url = cfg.mihomo_proxy_url();
    let arch = detect_mihomo_arch().await;

    // Определяем текущую установленную версию Mihomo
    let mut current_ver = crate::mihomo::get_version(&state.http, &cfg).await.unwrap_or_default();
    let mut full_ver = current_ver.clone();

    #[cfg(unix)]
    {
        if let Ok(out) = tokio::process::Command::new("/opt/sbin/mihomo").arg("-v").output().await {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                full_ver = s.clone();
                for part in s.split_whitespace() {
                    if part.starts_with('v') && part.chars().nth(1).map(|c| c.is_ascii_digit()).unwrap_or(false) {
                        current_ver = part.to_string();
                        break;
                    }
                }
            }
        }
    }

    let raw_releases = match fetch_mihomo_releases_cached(&state.http, &proxy_url).await {
        Ok(r) => r,
        Err(e) => return api_err(e),
    };

    let clean_current = current_ver.trim_start_matches('v');
    let mut items = Vec::new();
    let mut latest_stable = String::new();

    for r in &raw_releases {
        let clean_tag = r.tag_name.trim_start_matches('v');
        let is_cur = !clean_current.is_empty() && (clean_tag == clean_current || r.tag_name == current_ver);
        let asset = find_mihomo_asset(&r.assets, arch);

        if latest_stable.is_empty() && !r.prerelease && r.tag_name.starts_with('v') {
            latest_stable = r.tag_name.clone();
        }

        items.push(json!({
            "tag_name": r.tag_name,
            "name": r.name.as_deref().unwrap_or(&r.tag_name),
            "published_at": r.published_at,
            "prerelease": r.prerelease,
            "body": notes_lines(&r.body, 6),
            "is_current": is_cur,
            "download_url": asset.map(|a| a.browser_download_url.clone()),
            "file_size": asset.map(|a| a.size).unwrap_or(0),
        }));
    }

    api_ok(json!({
        "current_version": current_ver,
        "current_full": full_ver,
        "latest_version": latest_stable,
        "arch": arch,
        "releases": items,
    }))
}

#[derive(Deserialize)]
pub struct MihomoInstallReq {
    pub tag: String,
    pub custom_url: Option<String>,
}

/// POST /api/mihomo/install — загрузка и установка выбранной версии ядра Mihomo
pub async fn mihomo_install(
    State(state): State<AppState>,
    axum::extract::Json(req): axum::extract::Json<MihomoInstallReq>,
) -> Response {
    let tag = req.tag.trim();
    if tag.is_empty() {
        return api_err("Не указана версия ядра Mihomo для установки");
    }

    let cfg = state.config.read().await.clone();
    let proxy_url = cfg.mihomo_proxy_url();
    let arch = detect_mihomo_arch().await;

    // Определяем URL для загрузки
    let download_url = if let Some(ref cu) = req.custom_url {
        cu.clone()
    } else {
        let rels = fetch_mihomo_releases_cached(&state.http, &proxy_url).await.unwrap_or_default();
        let matched = rels.iter()
            .find(|r| r.tag_name.eq_ignore_ascii_case(tag) || r.tag_name.trim_start_matches('v') == tag.trim_start_matches('v'))
            .and_then(|r| find_mihomo_asset(&r.assets, arch));

        if let Some(asset) = matched {
            asset.browser_download_url.clone()
        } else {
            let clean_tag = if tag.starts_with('v') || tag == "Prerelease-Alpha" { tag.to_string() } else { format!("v{tag}") };
            format!("https://github.com/MetaCubeX/mihomo/releases/download/{clean_tag}/mihomo-linux-{arch}-{clean_tag}.gz")
        }
    };

    crate::log_i!("[Mihomo Update] Начало загрузки ядра {} ({arch}) из {download_url}", tag);

    let tmp_dir = Path::new("/opt/tmp");
    let _ = tokio::fs::create_dir_all(tmp_dir).await;
    let gz_path = tmp_dir.join("mihomo_update.gz");
    let bin_path = tmp_dir.join("mihomo_update.bin");
    let _ = tokio::fs::remove_file(&gz_path).await;
    let _ = tokio::fs::remove_file(&bin_path).await;

    let proxied = proxied_client(&proxy_url);
    let http = proxied.as_ref().unwrap_or(&state.http);

    let mut dl_resp = http.get(&download_url)
        .header("User-Agent", "xkeen-route")
        .timeout(Duration::from_secs(180))
        .send()
        .await;

    if dl_resp.is_err() || dl_resp.as_ref().map(|r| !r.status().is_success()).unwrap_or(false) {
        let mirror_url = format!("https://ghproxy.net/{download_url}");
        if let Ok(m_res) = http.get(&mirror_url).header("User-Agent", "xkeen-route").timeout(Duration::from_secs(180)).send().await {
            if m_res.status().is_success() {
                dl_resp = Ok(m_res);
            }
        }
    }

    let mut res = match dl_resp {
        Ok(r) if r.status().is_success() => r,
        Ok(r) => return api_err(format!("Ошибка загрузки ядра: HTTP {}", r.status())),
        Err(e) => return api_err(format!("Ошибка сети при загрузке: {e}")),
    };

    use tokio::io::AsyncWriteExt;
    let mut file = match tokio::fs::File::create(&gz_path).await {
        Ok(f) => f,
        Err(e) => return api_err(format!("Ошибка создания временного файла: {e}")),
    };

    let mut total_bytes = 0usize;
    while let Ok(Some(chunk)) = res.chunk().await {
        total_bytes += chunk.len();
        if let Err(e) = file.write_all(&chunk).await {
            let _ = tokio::fs::remove_file(&gz_path).await;
            return api_err(format!("Ошибка записи архива: {e}"));
        }
    }
    let _ = file.flush().await;
    drop(file);

    if total_bytes < 1024 * 1024 {
        let _ = tokio::fs::remove_file(&gz_path).await;
        return api_err(format!("Загруженный архив слишком мал ({total_bytes} байт)"));
    }

    // Распаковка .gz
    let decompress_status = tokio::process::Command::new("sh")
        .arg("-c")
        .arg(format!("gunzip -c '{}' > '{}' || gzip -dc '{}' > '{}'", gz_path.display(), bin_path.display(), gz_path.display(), bin_path.display()))
        .status()
        .await;

    let _ = tokio::fs::remove_file(&gz_path).await;

    if decompress_status.is_err() || !bin_path.exists() {
        let _ = tokio::fs::remove_file(&bin_path).await;
        return api_err("Ошибка распаковки архива ядра Mihomo (gunzip)");
    }

    let meta = match tokio::fs::metadata(&bin_path).await {
        Ok(m) => m,
        Err(e) => return api_err(format!("Ошибка чтения файла ядра: {e}")),
    };

    if meta.len() < 3 * 1024 * 1024 {
        let _ = tokio::fs::remove_file(&bin_path).await;
        return api_err("Распакованный бинарник слишком мал");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755)).await;
    }

    // Проверка исполнения бинарника
    let test_run = tokio::process::Command::new(&bin_path)
        .arg("-v")
        .output()
        .await;

    let mut new_ver_str = tag.to_string();
    match test_run {
        Ok(out) if out.status.success() => {
            let out_str = String::from_utf8_lossy(&out.stdout);
            crate::log_i!("[Mihomo Update] Проверка бинарника успешна: {}", out_str.trim());
            if let Some(v_line) = out_str.lines().next() {
                new_ver_str = v_line.trim().to_string();
            }
        }
        Ok(out) => {
            let err_str = String::from_utf8_lossy(&out.stderr);
            let _ = tokio::fs::remove_file(&bin_path).await;
            return api_err(format!("Бинарник не совместим с роутером: {err_str}"));
        }
        Err(e) => {
            let _ = tokio::fs::remove_file(&bin_path).await;
            return api_err(format!("Не удалось запустить новый бинарник: {e}"));
        }
    }

    // Бэкап и замена /opt/sbin/mihomo
    let target_bin = Path::new("/opt/sbin/mihomo");
    let bak_bin = Path::new("/opt/sbin/mihomo.bak");

    if target_bin.exists() {
        let _ = tokio::fs::copy(target_bin, bak_bin).await;
    }

    if let Err(e) = tokio::fs::rename(&bin_path, target_bin).await {
        if let Err(copy_err) = tokio::fs::copy(&bin_path, target_bin).await {
            let _ = tokio::fs::remove_file(&bin_path).await;
            return api_err(format!("Ошибка установки бинарника в /opt/sbin/mihomo: {copy_err} (rename: {e})"));
        }
        let _ = tokio::fs::remove_file(&bin_path).await;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = tokio::fs::set_permissions(target_bin, std::fs::Permissions::from_mode(0o755)).await;
    }

    // Перезапуск службы XKeen / Mihomo
    let _ = tokio::process::Command::new("sh")
        .arg(&cfg.system.xkeen_init)
        .arg("restart")
        .status()
        .await;

    crate::log_i!("[Mihomo Update] Служба XKeen перезапущена с новым ядром");

    tokio::time::sleep(Duration::from_millis(2500)).await;

    let check_ok = match crate::mihomo::get_version(&state.http, &cfg).await {
        Some(_) => true,
        None => {
            tokio::time::sleep(Duration::from_millis(2000)).await;
            crate::mihomo::get_version(&state.http, &cfg).await.is_some()
        }
    };

    if !check_ok {
        crate::log_w!("[Mihomo Update] Ядро не ответило после обновления. Выполняется откат...");
        if bak_bin.exists() {
            let _ = tokio::fs::copy(bak_bin, target_bin).await;
            let _ = tokio::process::Command::new("sh")
                .arg(&cfg.system.xkeen_init)
                .arg("restart")
                .status()
                .await;
        }
        return api_err("Ядро Mihomo не запустилось после обновления. Выполнен автоматический откат на предыдущую версию.");
    }

    api_ok(json!({
        "success": true,
        "version": tag,
        "full_version": new_ver_str,
        "message": format!("Ядро Mihomo успешно обновлено до {tag}")
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_newer_versions() {
        assert!(is_newer("v1.2.6", "v1.2.5"));
        assert!(!is_newer("v1.2.5", "v1.2.6"));
        assert!(!is_newer("v1.2.5", "v1.2.5"));
        assert!(is_newer("1.3.0", "1.2.9"));
        assert!(is_newer("v2.0.0", "v1.99.99"));
        assert!(!is_newer("v1.2.4", "v1.2.5"));
    }

    #[test]
    fn test_prerelease_comparison() {
        // Стабильный релиз всегда новее пререлиза с тем же номером
        assert!(is_newer("v1.2.5", "v1.2.5-beta.1"));
        assert!(!is_newer("v1.2.5-beta.1", "v1.2.5"));
        // Более старшая версия даже в виде pre-release новее старого релиза
        assert!(is_newer("v1.2.6-rc1", "v1.2.5"));
    }

    #[test]
    fn test_notes_lines_formatting() {
        let body = "<!-- Comment -->\n- **Первое**: изменение\n* Второе: исправление\nОбычная строка\n\n- Еще пункт";
        let lines = notes_lines(body, 3);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "**Первое**: изменение");
        assert_eq!(lines[1], "Второе: исправление");
        assert_eq!(lines[2], "Обычная строка");
    }

    #[test]
    fn test_find_mihomo_asset() {
        let assets = vec![
            MihomoReleaseAsset {
                name: "mihomo-linux-amd64-v1.19.31.gz".into(),
                browser_download_url: "https://example.com/amd64".into(),
                size: 10000000,
            },
            MihomoReleaseAsset {
                name: "mihomo-linux-arm64-v1.19.31.gz".into(),
                browser_download_url: "https://example.com/arm64".into(),
                size: 10000000,
            },
            MihomoReleaseAsset {
                name: "mihomo-linux-mipsle-softfloat-v1.19.31.gz".into(),
                browser_download_url: "https://example.com/mipsle".into(),
                size: 10000000,
            },
        ];

        let arm = find_mihomo_asset(&assets, "arm64");
        assert!(arm.is_some());
        assert_eq!(arm.unwrap().name, "mihomo-linux-arm64-v1.19.31.gz");

        let mips = find_mihomo_asset(&assets, "mipsle");
        assert!(mips.is_some());
        assert_eq!(mips.unwrap().name, "mihomo-linux-mipsle-softfloat-v1.19.31.gz");
    }
}
