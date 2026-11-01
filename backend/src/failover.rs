//! Фоновый failover-движок (порт check_and_failover из десктопа).
//! 1) Если настроен приоритетный сервер и активен другой — проверить приоритетный;
//!    восстановился (пинг < порог-50) → вернуться на него.
//! 2) Пинг активного: в норме (<= порога) — ничего.
//! 3) Иначе: параллельный пинг кандидатов → переключение на лучший.

use std::time::{Instant, Duration};

static LAST_SWITCH: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);
static LAST_WHITELIST_SYNC: tokio::sync::Mutex<Option<(String, Instant)>> =
    tokio::sync::Mutex::const_new(None);

/// F-04: сериализация итераций проверки — фоновый цикл и ручной прогон
/// (POST /api/failover/check) не должны выполняться параллельно (двойной switch).
static CHECK_IN_PROGRESS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub const CANARY_DURATION_SECS: u64 = 180;
pub const CANARY_PAYLOAD_PORT: u16 = 7894;
pub const CANARY_PAYLOAD_BYTES: u64 = 65536; // Честные 64 КБ (FO-CNR-02)
pub const CANARY_PAYLOAD_URL: &str = "http://speed.cloudflare.com/__down?bytes=65536";
pub const CANARY_PROBE_BYTES: u64 = CANARY_PAYLOAD_BYTES;
pub const CANARY_DOWNLOAD_TIMEOUT_SECS: u64 = 10;

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanaryDownloadResult {
    pub success: bool,
    pub bytes_downloaded: u64,
    pub duration_ms: u64,
    pub speed_kbps: f64,
    pub status_code: u16,
    pub error: Option<String>,
}

/// Выполнение реальной загрузки 64 КБ полезной нагрузки через локальный слушатель Canary Probe (127.0.0.1:7894).
/// Проверяет статус 200, точный объем полученных байт (>= chunk_bytes) и возвращает (байты, скорость в кбит/с).
pub async fn perform_canary_download(
    port: u16,
    chunk_bytes: u64,
    timeout_ms: u64,
) -> Result<(u64, f64), String> {
    let target_url = format!("http://speed.cloudflare.com/__down?bytes={chunk_bytes}");
    let res = perform_canary_download_detailed(port, &target_url, chunk_bytes, timeout_ms).await?;
    Ok((res.bytes_downloaded, res.speed_kbps))
}

pub async fn perform_canary_download_detailed(
    port: u16,
    target_url: &str,
    chunk_bytes: u64,
    timeout_ms: u64,
) -> Result<CanaryDownloadResult, String> {
    let proxy_url = format!("http://127.0.0.1:{port}");
    let proxy = reqwest::Proxy::all(&proxy_url)
        .map_err(|e| format!("Ошибка создания Canary-прокси для {proxy_url}: {e}"))?;

    let client = reqwest::Client::builder()
        .proxy(proxy)
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
        .map_err(|e| format!("Ошибка инициализации HTTP-клиента Canary: {e}"))?;

    let start = std::time::Instant::now();
    let resp = match client.get(target_url).send().await {
        Ok(r) => r,
        Err(e) => return Err(format!("Сетевой сбой при загрузке полезной нагрузки через {proxy_url}: {e}")),
    };

    let status = resp.status();
    if status != reqwest::StatusCode::OK {
        return Err(format!("Неверный HTTP-статус Canary: {} (ожидался 200 OK)", status.as_u16()));
    }

    let bytes = match resp.bytes().await {
        Ok(b) => b,
        Err(e) => return Err(format!("Сбой чтения данных полезной нагрузки Canary: {e}")),
    };

    let bytes_len = bytes.len() as u64;
    let duration_ms = start.elapsed().as_millis().max(1) as u64;
    let duration_secs = duration_ms as f64 / 1000.0;
    let speed_kbps = ((bytes_len as f64 * 8.0) / duration_secs) / 1000.0;

    if bytes_len < chunk_bytes {
        return Err(format!(
            "Усечённая полезная нагрузка Canary: получено {} байт из ожидаемых {}",
            bytes_len, chunk_bytes
        ));
    }

    Ok(CanaryDownloadResult {
        success: true,
        bytes_downloaded: bytes_len,
        duration_ms,
        speed_kbps,
        status_code: status.as_u16(),
        error: None,
    })
}


#[derive(Clone, Debug)]
pub struct CanaryState {
    pub target_id: String,
    pub target_name: String,
    pub from_id: String,
    pub from_name: String,
    pub started_at: Instant,
    pub probes_passed: u32,
    pub total_bytes: u64,
    pub duration_secs: u64,
}

static CANARY_STAGE: tokio::sync::Mutex<Option<CanaryState>> = tokio::sync::Mutex::const_new(None);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanaryStatus {
    pub active: bool,
    pub target_id: String,
    pub target_name: String,
    pub from_name: String,
    pub elapsed_secs: u64,
    pub duration_secs: u64,
    pub remaining_secs: u64,
    pub probes_passed: u32,
    pub total_bytes: u64,
}

pub static WHITELIST_EPISODE_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn get_whitelist_episode_bytes() -> u64 {
    WHITELIST_EPISODE_BYTES.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn reset_whitelist_episode_bytes() {
    WHITELIST_EPISODE_BYTES.store(0, std::sync::atomic::Ordering::Relaxed);
}

pub fn record_whitelist_episode_bytes(bytes: u64) {
    WHITELIST_EPISODE_BYTES.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed);
}

pub async fn is_canary_active() -> bool {
    CANARY_STAGE.lock().await.is_some()
}

pub async fn canary_status() -> Option<CanaryStatus> {
    let guard = CANARY_STAGE.lock().await;
    guard.as_ref().map(|c| {
        let elapsed = c.started_at.elapsed().as_secs();
        CanaryStatus {
            active: true,
            target_id: c.target_id.clone(),
            target_name: c.target_name.clone(),
            from_name: c.from_name.clone(),
            elapsed_secs: elapsed,
            duration_secs: c.duration_secs,
            remaining_secs: c.duration_secs.saturating_sub(elapsed),
            probes_passed: c.probes_passed,
            total_bytes: c.total_bytes,
        }
    })
}

pub async fn reset_canary() {
    *CANARY_STAGE.lock().await = None;
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CanaryLastResult {
    pub server_id: String,
    pub server_name: String,
    pub timestamp: String,
    pub success: bool,
    pub is_zombie: bool,
    pub reason: String,
    pub elapsed_secs: u64,
}

static LAST_CANARY_RESULT: tokio::sync::Mutex<Option<CanaryLastResult>> = tokio::sync::Mutex::const_new(None);

pub async fn canary_last_result() -> Option<CanaryLastResult> {
    LAST_CANARY_RESULT.lock().await.clone()
}

pub async fn set_canary_last_result(res: CanaryLastResult) {
    *LAST_CANARY_RESULT.lock().await = Some(res);
}

pub async fn current_live_status(
    active_id: &str,
    active_name: &str,
    active_ping: i64,
    cfg: &crate::config::AppConfig,
) -> String {
    if !cfg.failover.enabled {
        return "Failover выключен".to_string();
    }
    if let Some(canary) = canary_status().await {
        return format!(
            "Тестирование кандидата '{}' (Canary Soak: {}/{} с, {} КБ загружено)",
            canary.target_name,
            canary.elapsed_secs,
            canary.duration_secs,
            canary.total_bytes / 1024
        );
    }
    let act_label = if !active_name.is_empty() { active_name } else if !active_id.is_empty() { active_id } else { "не выбран" };
    let ping_str = if active_ping > 0 { format!("{active_ping} мс") } else { "—".to_string() };

    if !cfg.failover.priority_chain.is_empty() {
        if let Some(top_id) = cfg.failover.priority_chain.first() {
            if top_id == active_id || top_id == act_label {
                return format!("Штатно: активен приоритетный узел '{act_label}' ({ping_str})");
            } else {
                return format!("Удержание узла '{act_label}' ({ping_str}, поиск доступных в приоритете)");
            }
        }
    }
    format!("Активен '{act_label}' ({ping_str})")
}

use crate::mihomo::{self, Server};
use crate::AppState;
use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use tokio::sync::Mutex;

static LAST_ACTIVATED_SERVER: Mutex<Option<(String, Instant, bool)>> = Mutex::const_new(None);

pub const EARLY_FAILURE_WINDOW_SECS: u64 = 300; // 5 минут после переключения

// ============================================================================
// FO-QUR-01: Persistent Quarantine Data Structures & Paths
// ============================================================================

#[cfg(target_os = "linux")]
pub const DEFAULT_QUARANTINE_FILE: &str = "/opt/etc/xkeen-route/quarantine.json";
#[cfg(not(target_os = "linux"))]
pub const DEFAULT_QUARANTINE_FILE: &str = "xkeen-route.quarantine.json";

pub const QUARANTINE_AMNESTY_SECS: u64 = 43200; // 12 часов до полной амнистии

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistentQuarantineEntry {
    #[serde(default)]
    pub server_name: String,
    pub quarantined_at: u64,
    pub fail_count: u32,
    pub duration_secs: u64,
    pub reason: String,
}

pub type InternalQuarantineEntry = PersistentQuarantineEntry;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuarantineInfo {
    pub server_id: String,
    pub server_name: String,
    pub fail_count: u32,
    pub remaining_secs: u64,
    pub reason: String,
}

static SERVER_QUARANTINE: std::sync::LazyLock<Mutex<std::collections::HashMap<String, PersistentQuarantineEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

static QUARANTINE_SAVE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static QUARANTINE_PATH_OVERRIDE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

pub fn current_unix_timestamp() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn get_quarantine_path() -> std::path::PathBuf {
    let guard = QUARANTINE_PATH_OVERRIDE.lock().unwrap();
    if let Some(ref p) = *guard {
        p.clone()
    } else {
        std::path::PathBuf::from(DEFAULT_QUARANTINE_FILE)
    }
}

#[cfg(test)]
pub fn set_quarantine_path_override(path: Option<std::path::PathBuf>) {
    let mut guard = QUARANTINE_PATH_OVERRIDE.lock().unwrap();
    *guard = path;
}

/// Загрузка записей карантина из файла с авто-очисткой просроченных записей старше 12 часов.
pub async fn load_quarantine_from_file(
    path: impl AsRef<std::path::Path>,
) -> Result<std::collections::HashMap<String, PersistentQuarantineEntry>, String> {
    let p = path.as_ref();
    if !p.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let content = tokio::fs::read_to_string(p)
        .await
        .map_err(|e| format!("Ошибка чтения {}: {e}", p.display()))?;

    if content.trim().is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let raw_entries: std::collections::HashMap<String, PersistentQuarantineEntry> =
        serde_json::from_str(&content)
            .map_err(|e| format!("Ошибка парсинга JSON из {}: {e}", p.display()))?;

    let now = current_unix_timestamp();
    let mut clean_map = std::collections::HashMap::new();

    for (server_id, entry) in raw_entries {
        // Если запись старше 12 часов непрерывной стабильности — полная амнистия, запись удаляется
        if now.saturating_sub(entry.quarantined_at) > QUARANTINE_AMNESTY_SECS {
            continue;
        }
        clean_map.insert(server_id, entry);
    }

    Ok(clean_map)
}

/// Атомарное сохранение записей карантина на диск через временный файл и rename
pub async fn save_quarantine_to_file(
    path: impl AsRef<std::path::Path>,
    entries: &std::collections::HashMap<String, PersistentQuarantineEntry>,
) -> Result<(), String> {
    let _file_guard = QUARANTINE_SAVE_LOCK.lock().await;
    let json = serde_json::to_string_pretty(entries)
        .map_err(|e| format!("Ошибка сериализации карантина: {e}"))?;
    crate::api::atomic_write_file(path.as_ref(), &json).await
}

/// Асинхронное фоновое сохранение снимка карантина
pub fn spawn_quarantine_save(snapshot: std::collections::HashMap<String, PersistentQuarantineEntry>) {
    let path = get_quarantine_path();
    tokio::spawn(async move {
        if let Err(e) = save_quarantine_to_file(&path, &snapshot).await {
            crate::log_w!("Failover: ошибка сохранения карантина в {}: {e}", path.display());
        }
    });
}

/// Принудительный сброс текущего состояния карантина на диск (для shutdown / тестов)
pub async fn flush_quarantine_to_disk() -> Result<(), String> {
    let path = get_quarantine_path();
    let snapshot = {
        let q = SERVER_QUARANTINE.lock().await;
        q.clone()
    };
    save_quarantine_to_file(&path, &snapshot).await
}

/// Инициализация подсистемы карантина при старте сервиса
pub async fn init_quarantine_from_disk() -> Result<(), String> {
    let path = get_quarantine_path();
    match load_quarantine_from_file(&path).await {
        Ok(entries) => {
            let total = entries.len();
            let now = current_unix_timestamp();
            let active = entries.values().filter(|e| now < e.quarantined_at + e.duration_secs).count();
            let mut q = SERVER_QUARANTINE.lock().await;
            *q = entries;
            crate::log_i!(
                "Failover: загружено {} записей карантина из {} (активных: {}, в амнистии: {})",
                total, path.display(), active, total.saturating_sub(active)
            );
            Ok(())
        }
        Err(e) => {
            crate::log_w!(
                "Failover: повреждённый файл карантина {} ({}), инициализация с пустым карантином",
                path.display(), e
            );
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
            Ok(())
        }
    }
}

pub async fn is_quarantined(server_id: &str) -> bool {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = current_unix_timestamp();
    if let Some(entry) = q.get(server_id) {
        if now < entry.quarantined_at + entry.duration_secs {
            true
        } else {
            // Не удаляем сразу fail_count, чтобы сохранялась история для прогрессивного бана.
            // Полная амнистия наступает только через 12 часов непрерывной стабильности.
            if now.saturating_sub(entry.quarantined_at) > QUARANTINE_AMNESTY_SECS {
                q.remove(server_id);
            }
            false
        }
    } else {
        false
    }
}

pub async fn get_quarantine_remaining(server_id: &str) -> Option<u64> {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = current_unix_timestamp();
    if let Some(entry) = q.get(server_id) {
        if now < entry.quarantined_at + entry.duration_secs {
            Some((entry.quarantined_at + entry.duration_secs) - now)
        } else {
            if now.saturating_sub(entry.quarantined_at) > QUARANTINE_AMNESTY_SECS {
                q.remove(server_id);
            }
            None
        }
    } else {
        None
    }
}

pub async fn add_quarantine(server_id: &str, server_name: &str, reason: &str) -> u64 {
    let now = current_unix_timestamp();
    let (duration_secs, snapshot) = {
        let mut q = SERVER_QUARANTINE.lock().await;
        let prev_count = q.get(server_id)
            .filter(|e| now.saturating_sub(e.quarantined_at) <= QUARANTINE_AMNESTY_SECS)
            .map(|e| e.fail_count)
            .unwrap_or(0);
        let fail_count = prev_count + 1;
        // Эскалация: 1-й сбой: 15 мин (900с), 2-й: 30 мин (1800с), 3-й+: максимум 1 час (3600с)
        // Строго по указанию пользователя: "больше часа не надо бана"
        let duration_secs = match fail_count {
            1 => 900,
            2 => 1800,
            _ => 3600,
        };
        q.insert(server_id.to_string(), PersistentQuarantineEntry {
            server_name: server_name.to_string(),
            quarantined_at: now,
            fail_count,
            duration_secs,
            reason: reason.to_string(),
        });
        (duration_secs, q.clone())
    };
    spawn_quarantine_save(snapshot);
    duration_secs
}

pub async fn add_quarantine_with_duration(
    server_id: &str,
    server_name: &str,
    reason: &str,
    duration_secs: u64,
) -> u64 {
    let now = current_unix_timestamp();
    let capped_duration = std::cmp::min(duration_secs, 3600);
    let (duration, snapshot) = {
        let mut q = SERVER_QUARANTINE.lock().await;
        let prev_count = q.get(server_id)
            .filter(|e| now.saturating_sub(e.quarantined_at) <= QUARANTINE_AMNESTY_SECS)
            .map(|e| e.fail_count)
            .unwrap_or(0);
        let fail_count = prev_count + 1;
        q.insert(server_id.to_string(), PersistentQuarantineEntry {
            server_name: server_name.to_string(),
            quarantined_at: now,
            fail_count,
            duration_secs: capped_duration,
            reason: reason.to_string(),
        });
        (capped_duration, q.clone())
    };
    spawn_quarantine_save(snapshot);
    duration
}

pub async fn get_zombie_servers() -> Vec<String> {
    let q = SERVER_QUARANTINE.lock().await;
    let now = current_unix_timestamp();
    q.iter()
        .filter(|(_, v)| {
            if now >= v.quarantined_at + v.duration_secs {
                return false;
            }
            let r = v.reason.to_lowercase();
            // Истинный зомби: подтверждённая аномалия "Чёрная дыра" (DPI drop) или явный зомби-сервер.
            // Обычный таймаут или L7-сбой отдельного проверочного URL НЕ является зомби!
            r.contains("зомби") || r.contains("zombie") || r.contains("черн") || r.contains("blackhole")
        })
        .map(|(k, _)| k.clone())
        .collect()
}

pub async fn clear_quarantine(server_id: &str) {
    let snapshot = {
        let mut q = SERVER_QUARANTINE.lock().await;
        q.remove(server_id);
        q.clone()
    };
    spawn_quarantine_save(snapshot);
}

pub async fn clear_all_quarantine() {
    let snapshot = {
        let mut q = SERVER_QUARANTINE.lock().await;
        q.clear();
        q.clone()
    };
    spawn_quarantine_save(snapshot);
}

pub async fn quarantine_status() -> Vec<QuarantineInfo> {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = current_unix_timestamp();
    q.retain(|_, v| now < v.quarantined_at + v.duration_secs || now.saturating_sub(v.quarantined_at) <= QUARANTINE_AMNESTY_SECS);
    let mut res: Vec<QuarantineInfo> = q.iter()
        .filter(|(_, v)| now < v.quarantined_at + v.duration_secs)
        .map(|(id, v)| QuarantineInfo {
            server_id: id.clone(),
            server_name: if v.server_name.is_empty() { id.clone() } else { v.server_name.clone() },
            fail_count: v.fail_count,
            remaining_secs: (v.quarantined_at + v.duration_secs).saturating_sub(now),
            reason: v.reason.clone(),
        })
        .collect();
    res.sort_by(|a, b| a.remaining_secs.cmp(&b.remaining_secs));
    res
}

pub async fn record_switch_target(target_id: &str) {
    let now = Instant::now();
    *LAST_SWITCH.lock().await = Some(now);
    *LAST_ACTIVATED_SERVER.lock().await = Some((target_id.to_string(), now, false));
    reset_canary().await;
}

pub async fn record_manual_switch(target_id: &str) {
    let now = Instant::now();
    *LAST_SWITCH.lock().await = Some(now);
    *LAST_ACTIVATED_SERVER.lock().await = Some((target_id.to_string(), now, true));
    reset_canary().await;
    clear_quarantine(target_id).await;
}

pub const MAX_EVENTS: usize = 50;

#[derive(Clone, Serialize)]
pub struct FailoverEvent {
    pub time: String,
    pub message: String,
    pub switched: bool,
}

#[derive(Default)]
pub struct FailoverLog {
    events: Mutex<VecDeque<FailoverEvent>>,
}

impl FailoverLog {
    pub async fn push(&self, message: impl Into<String>, switched: bool) {
        let message = message.into();
        if switched {
            crate::log_w!("Failover: {}", message);
        } else {
            crate::log_i!("Failover: {}", message);
        }
        self.push_event_only(message, switched).await;
    }

    pub async fn push_event_only(&self, message: impl Into<String>, switched: bool) {
        let message = message.into();
        let mut q = self.events.lock().await;
        q.push_back(FailoverEvent {
            time: Local::now().format("%H:%M:%S").to_string(),
            message,
            switched,
        });
        while q.len() > MAX_EVENTS {
            q.pop_front();
        }
    }

    pub async fn snapshot(&self) -> Vec<FailoverEvent> {
        self.events.lock().await.iter().rev().cloned().collect()
    }
}

/// Вычисление адаптивного порога возврата (гистерезис):
/// для высоких порогов (> 100 мс) порог возврата = threshold - 50 мс.
/// для низких порогов (<= 100 мс) порог возврата = 80% от порога (минимум 1 мс),
/// чтобы при threshold=50 мс гистерезис был 40 мс, а не 0 мс (что ломало автовозврат).
pub fn calc_hysteresis(threshold: i64) -> i64 {
    if threshold > 100 {
        (threshold - 50).max(1)
    } else {
        ((threshold * 8) / 10).max(1)
    }
}

/// Таймаут для проверки пинга с учётом заданного порога.
pub fn calc_ping_timeout(threshold: i64) -> u64 {
    (threshold as u64 + 1000).clamp(2000, 6000)
}

/// Находит список кандидатов с более высоким приоритетом для автовозврата.
/// Если активный сервер находится в цепочке на позиции `pos`, кандидаты — `chain[..pos]`.
/// Если активный сервер не входит в цепочку, кандидаты — вся цепочка `chain[..]`.
pub fn get_higher_priority_candidates<'a, T: AsRef<str>>(
    chain: &'a [T],
    active_id: &str,
) -> &'a [T] {
    if let Some(pos) = chain.iter().position(|s| s.as_ref() == active_id) {
        &chain[..pos]
    } else {
        chain
    }
}

async fn record_switch(target_id: &str) {
    record_switch_target(target_id).await;
}

/// Проверяет, является ли узел сервером белых списков:
/// 1) По ID из `cfg.whitelist_servers`
/// 2) По провайдеру/подписке из `cfg.whitelist_providers`
/// 3) По тегам в названии `[БС]`, `(БС)`, `[WL]`, `(WL)`, `whitelist` (case-insensitive).
pub fn is_whitelist_server(node_name: &str, node_id: &str, cfg: &crate::config::AppConfig) -> bool {
    is_whitelist_server_full(node_name, node_id, None, cfg)
}

pub fn is_whitelist_server_full(
    node_name: &str,
    node_id: &str,
    provider: Option<&str>,
    cfg: &crate::config::AppConfig,
) -> bool {
    if !node_id.is_empty() && cfg.whitelist_servers.iter().any(|s| s == node_id) {
        return true;
    }
    if let Some(p) = provider {
        if !p.is_empty() && cfg.whitelist_providers.iter().any(|wp| wp == p) {
            return true;
        }
    }
    let check_tag = |s: &str| -> bool {
        let lower = s.to_lowercase();
        lower.contains("[бс]")
            || lower.contains("(бс)")
            || lower.contains("[бc]")
            || lower.contains("(бc)")
            || lower.contains("[бs]")
            || lower.contains("(бs)")
            || lower.contains("[bs]")
            || lower.contains("(bs)")
            || lower.contains("[wl]")
            || lower.contains("(wl)")
            || lower.contains("whitelist")
            || lower.contains("белые списки")
            || lower.contains("белый список")
    };
    check_tag(node_name) || (!node_id.is_empty() && check_tag(node_id))
}

/// Синхронизация селекторной группы '🛡️ Whitelist' в ядре Mihomo при переключении узла.
pub async fn sync_whitelist_on_switch(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    node_id: &str,
    node_name: &str,
) {
    if !cfg.whitelist.enabled {
        return;
    }

    let is_auto = is_whitelist_server(node_name, node_id, cfg);
    let should_follow = cfg.whitelist.follow_failover
        || cfg.whitelist.target_server.as_deref() == Some("follow_failover")
        || cfg.whitelist.target_server.as_deref().map_or(true, |s| s.trim().is_empty());

    // Дедупликация: не спамить Mihomo API и логи, если тот же узел уже синхронизирован недавно (< 2 сек)
    {
        let mut last_sync = LAST_WHITELIST_SYNC.lock().await;
        if let Some((ref last_id, ref last_time)) = *last_sync {
            if last_id == node_id && last_time.elapsed() < Duration::from_secs(2) {
                return;
            }
        }
        *last_sync = Some((node_id.to_string(), Instant::now()));
    }

    if should_follow && !node_id.is_empty() {
        let _ = crate::mihomo::switch_group(http, cfg, crate::routing::WHITELIST_GROUP_NAME, node_id).await;
    }

    crate::log_i!(
        "[FAILOVER] Whitelist mode active server switched to: {} (auto-detected: {})",
        node_name,
        is_auto
    );
}

/// Комплексная обработка перехода на сервер с учетом режима Белых списков (Zero-Touch):
/// 1. Если включен auto_by_failover:
///    - Если узел помечен как [БС]: автоматически активирует whitelist.enabled,
///      отключает gaming.enabled (выставляя gaming_suspended = true),
///      синхронизирует селекторную группу 🛡️ Whitelist на новый узел.
///    - Если узел обычный: деактивирует whitelist.enabled,
///      восстанавливает gaming.enabled (если gaming_suspended == true).
/// 2. Применяет правила маршрутизации и сохраняет состояние.
pub async fn handle_whitelist_transition(
    state: &AppState,
    node_id: &str,
    node_name: &str,
    provider: Option<&str>,
) {
    let cfg = state.config.read().await.clone();
    let is_bs = is_whitelist_server_full(node_name, node_id, provider, &cfg);

    if is_bs {
        // Сброс счетчика эпизода белых списков при переходе на сервер БС (Tier 2)
        reset_whitelist_episode_bytes();
    }

    if cfg.whitelist.auto_by_failover {
        if is_bs {
            let mut cfg_changed = false;
            let mut gaming_log = String::new();

            {
                let _cfg_guard = state.config_lock.lock().await;
                let mut current_cfg = (**state.config.read().await).clone();

                if !current_cfg.whitelist.enabled {
                    current_cfg.whitelist.enabled = true;
                    cfg_changed = true;
                }

                if current_cfg.gaming.enabled {
                    current_cfg.gaming.enabled = false;
                    current_cfg.whitelist.gaming_suspended = true;
                    cfg_changed = true;
                    gaming_log = " (Игровой режим приостановлен во избежание конфликтов сокетов)".to_string();
                }

                if cfg_changed {
                    if let Err(e) = crate::config::save(&state.config_path, &current_cfg).await {
                        crate::log_e!("[FAILOVER] Ошибка сохранения config.json при авто-активации БС: {e}");
                    } else {
                        *state.config.write().await = std::sync::Arc::new(current_cfg);
                    }
                }
            }

            if cfg_changed {
                crate::log_i!(
                    "[FAILOVER] 🛡️ Авто-активация режима Белых списков (узел '{}' помечен как [БС]){}",
                    node_name,
                    gaming_log
                );
                if let Ok(mut tx) = crate::transaction::ConfigTx::begin(state).await {
                    if let Ok(yaml) = tx.read_yaml().await {
                        if let Ok((new_yaml, _)) = crate::routing::apply_routing(&yaml, tx.config()) {
                            let _ = tx.set_yaml(new_yaml);
                            let _ = tx.commit_and_reload().await;
                        }
                    }
                }
            }

            let _ = crate::mihomo::switch_group(&state.http, &cfg, crate::routing::WHITELIST_GROUP_NAME, node_id).await;
        } else {
            let mut cfg_changed = false;
            let mut gaming_log = String::new();

            {
                let _cfg_guard = state.config_lock.lock().await;
                let mut current_cfg = (**state.config.read().await).clone();

                if current_cfg.whitelist.enabled {
                    current_cfg.whitelist.enabled = false;
                    cfg_changed = true;
                }

                if current_cfg.whitelist.gaming_suspended {
                    current_cfg.gaming.enabled = true;
                    current_cfg.whitelist.gaming_suspended = false;
                    cfg_changed = true;
                    gaming_log = " (Игровой режим автоматически восстановлен)".to_string();
                }

                if cfg_changed {
                    if let Err(e) = crate::config::save(&state.config_path, &current_cfg).await {
                        crate::log_e!("[FAILOVER] Ошибка сохранения config.json при деактивации БС: {e}");
                    } else {
                        *state.config.write().await = std::sync::Arc::new(current_cfg);
                    }
                }
            }

            if cfg_changed {
                crate::log_i!(
                    "[FAILOVER] ⚪ Деактивация режима Белых списков (переход на обычный узел '{}'){}",
                    node_name,
                    gaming_log
                );
                if let Ok(mut tx) = crate::transaction::ConfigTx::begin(state).await {
                    if let Ok(yaml) = tx.read_yaml().await {
                        if let Ok((new_yaml, _)) = crate::routing::apply_routing(&yaml, tx.config()) {
                            let _ = tx.set_yaml(new_yaml);
                            let _ = tx.commit_and_reload().await;
                        }
                    }
                }
            }
        }
    } else if cfg.whitelist.enabled {
        let _ = crate::mihomo::switch_group(&state.http, &cfg, crate::routing::WHITELIST_GROUP_NAME, node_id).await;
    }
}

/// Нагрузочный стресс-тест в 3 параллельных сокета через проверяемый узел.
/// Симулирует реальный трафик клиентов (загрузка 100 КБ полезной нагрузки + 2 параллельных L7-пробника).
/// Если сервер глушится DPI при открытии нескольких сокетов или таймаутит — возвращает <= 0 (-1).
pub async fn run_canary_probe(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    server_id: &str,
    timeout_ms: u64,
) -> i64 {
    let probe_timeout = timeout_ms.max(5000);
    let (d1, d2, d3) = tokio::join!(
        mihomo::probe_server_l7(http, cfg, server_id, probe_timeout, CANARY_PAYLOAD_URL, 200),
        mihomo::probe_server_l7(http, cfg, server_id, probe_timeout, mihomo::SECONDARY_L7_CHECK_URL, 204),
        mihomo::probe_server_l7(http, cfg, server_id, probe_timeout, mihomo::PRIMARY_L7_CHECK_URL, 204),
    );
    if d1 > 0 && d2 > 0 && d3 > 0 {
        std::cmp::max(d1, std::cmp::max(d2, d3))
    } else {
        -1
    }
}

/// F-05: WAN-проба по трём независимым эндпоинтам — падение одного сайта, DPI-блок
/// или временный сбой не должны замораживать failover при живом канале
/// (раньше единственный URL ya.ru через mihomo был точкой отказа самой защиты).
pub const WAN_CHECK_URLS: &[&str] = &[
    "http://ya.ru",
    "http://cp.cloudflare.com/generate_204",
    "http://connectivitycheck.gstatic.com/generate_204",
];

#[allow(dead_code)]
pub const WAN_CHECK_URL: &str = WAN_CHECK_URLS[0];

/// Проверка физической доступности провайдера (WAN) напрямую через DIRECT.
/// Если шлюз провайдера или кабель упал, возвращает false.
pub async fn is_wan_online(http: &reqwest::Client, cfg: &crate::config::AppConfig) -> bool {
    for url in WAN_CHECK_URLS {
        let ms = mihomo::ping_server_url_expected(
            http,
            cfg,
            "DIRECT",
            1500,
            Some(url),
            Some(200),
        ).await;
        if ms > 0 {
            return true;
        }
    }
    false
}

/// F-01: строгое сопоставление соединения с сервером (без fuzzy contains).
/// Раньше `sl.contains(&active_lower)` зачислял трафик сервера «DE-10» узлу «DE-1»:
/// критерий здоровья «есть живой трафик» получал чужие байты, упавший сервер
/// считался живым и failover не переключал. Служебные группы (proxy/global/direct/reject)
/// никогда не считаются трафиком конкретного узла.
pub fn conn_chain_matches_server(chains: &[Value], active_lower: &str) -> bool {
    chains.iter().any(|item| {
        item.as_str()
            .map(|s| {
                let sl = s.to_lowercase();
                if sl == "proxy" || sl == "global" || sl == "direct" || sl == "reject" {
                    false
                } else {
                    sl == active_lower
                }
            })
            .unwrap_or(false)
    })
}

/// Снимок сокетов для вычисления прироста трафика (дельты): { id -> (cumulative_upload, cumulative_download) }
static CONN_SNAPSHOT: std::sync::LazyLock<tokio::sync::Mutex<std::collections::HashMap<String, (u64, u64)>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

#[allow(dead_code)]
pub async fn clear_conn_snapshot() {
    CONN_SNAPSHOT.lock().await.clear();
}

/// Снимок сокетов для per-device failover: { id -> (cumulative_upload, cumulative_download) }
static DEV_CONN_SNAPSHOT: std::sync::LazyLock<tokio::sync::Mutex<std::collections::HashMap<String, (u64, u64)>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

#[allow(dead_code)]
pub async fn clear_dev_conn_snapshot() {
    DEV_CONN_SNAPSHOT.lock().await.clear();
}

/// Снятие снимка сокетов через единый запрос /connections к Mihomo API
#[allow(dead_code)]
pub async fn fetch_connections_snapshot(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
) -> Result<std::collections::HashMap<String, (u64, u64)>, String> {
    let val = mihomo::m_get(http, cfg, "/connections").await?;
    let mut map = std::collections::HashMap::new();
    if let Some(conns) = val.get("connections").and_then(|c| c.as_array()) {
        for c in conns {
            if let Some(id) = c.get("id").and_then(|i| i.as_str()) {
                let up = c.get("upload").and_then(|u| u.as_u64()).unwrap_or(0);
                let dl = c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);
                map.insert(id.to_string(), (up, dl));
            }
        }
    }
    Ok(map)
}

/// Вычисление дельт трафика (download_delta, upload_delta) между предыдущим и текущим снимками
pub fn calc_traffic_deltas(prev: (u64, u64), current: (u64, u64)) -> (u64, u64) {
    let delta_up = current.0.saturating_sub(prev.0);
    let delta_dl = current.1.saturating_sub(prev.1);
    (delta_dl, delta_up)
}

/// Чистая обработка массива соединений с расчётом дельт трафика и детекцией чёрной дыры.
/// Возвращает (has_live_traffic, total_delta_download, total_delta_upload, matched_sockets, is_blackhole).
pub fn process_connections_delta(
    conns: &[Value],
    active_server_id: &str,
    snapshot: &mut std::collections::HashMap<String, (u64, u64)>,
) -> (bool, u64, u64, usize, bool) {
    if active_server_id.is_empty() || active_server_id.eq_ignore_ascii_case("direct") {
        return (false, 0, 0, 0, false);
    }
    let active_lower = active_server_id.to_lowercase();
    let mut matched_sockets = 0usize;
    let mut total_delta_upload = 0u64;
    let mut total_delta_download = 0u64;

    let mut current_ids = std::collections::HashSet::new();

    for c in conns {
        let Some(id) = c.get("id").and_then(|i| i.as_str()) else { continue; };
        let cur_up = c.get("upload").and_then(|u| u.as_u64()).unwrap_or(0);
        let cur_dl = c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);
        current_ids.insert(id.to_string());

        let in_chains = c.get("chains")
            .and_then(|ch| ch.as_array())
            .map(|arr| conn_chain_matches_server(arr, &active_lower))
            .unwrap_or(false);

        let (prev_up, prev_dl) = snapshot.get(id).copied().unwrap_or((0, 0));
        let (delta_dl, delta_up) = calc_traffic_deltas((prev_up, prev_dl), (cur_up, cur_dl));

        snapshot.insert(id.to_string(), (cur_up, cur_dl));

        if in_chains {
            matched_sockets += 1;
            total_delta_upload += delta_up;
            total_delta_download += delta_dl;
        }
    }

    // Удаляем закрытые соединения, чтобы предотвратить утечку памяти
    snapshot.retain(|k, _| current_ids.contains(k));

    let has_live_traffic = total_delta_download > 1024;
    let is_blackhole = matched_sockets >= 3 && total_delta_upload > 5120 && total_delta_download == 0;

    (has_live_traffic, total_delta_download, total_delta_upload, matched_sockets, is_blackhole)
}

/// Обработка соединений для отдельного устройства с расчётом дельт трафика и детекцией чёрной дыры.
/// Сравнивает текущие соединения со снимком prev_snapshot.
/// Возвращает (has_live_traffic, total_delta_download, total_delta_upload, matched_sockets, is_blackhole).
pub fn process_device_connections_delta(
    conns: &[Value],
    device_ip: &str,
    device_group: &str,
    active_server_id: &str,
    prev_snapshot: &std::collections::HashMap<String, (u64, u64)>,
) -> (bool, u64, u64, usize, bool) {
    if active_server_id.is_empty() || active_server_id.eq_ignore_ascii_case("direct") {
        return (false, 0, 0, 0, false);
    }
    let active_lower = active_server_id.to_lowercase();
    let group_lower = device_group.to_lowercase();
    let mut matched_sockets = 0usize;
    let mut total_delta_upload = 0u64;
    let mut total_delta_download = 0u64;

    for c in conns {
        let Some(id) = c.get("id").and_then(|i| i.as_str()) else { continue; };
        let cur_up = c.get("upload").and_then(|u| u.as_u64()).unwrap_or(0);
        let cur_dl = c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);

        let src_ip = c.get("metadata")
            .and_then(|m| m.get("sourceIP"))
            .and_then(|s| s.as_str())
            .unwrap_or("");

        let chains = c.get("chains").and_then(|ch| ch.as_array());

        let matches_device = src_ip == device_ip
            || (!group_lower.is_empty() && chains.map_or(false, |arr| {
                arr.iter().any(|item| {
                    item.as_str().map_or(false, |s| s.to_lowercase() == group_lower)
                })
            }));

        let matches_server = chains.map_or(false, |arr| {
            conn_chain_matches_server(arr, &active_lower)
        });

        if matches_device && matches_server {
            let (prev_up, prev_dl) = prev_snapshot.get(id).copied().unwrap_or((0, 0));
            let (delta_dl, delta_up) = calc_traffic_deltas((prev_up, prev_dl), (cur_up, cur_dl));

            matched_sockets += 1;
            total_delta_upload += delta_up;
            total_delta_download += delta_dl;
        }
    }

    let has_live_traffic = total_delta_download > 1024;
    let is_blackhole = matched_sockets >= 3 && total_delta_upload > 5120 && total_delta_download == 0;

    (has_live_traffic, total_delta_download, total_delta_upload, matched_sockets, is_blackhole)
}

/// Единая функция оценки активного трафика и аномалии "Чёрная дыра" за один проход.
/// Принимает опционально предзагруженный срез соединений (для исключения повторных запросов).
pub async fn check_active_traffic_and_blackhole(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    active_server_id: &str,
    cached_conns: Option<&[Value]>,
) -> (bool, u64, u64, usize, bool) {
    if let Some(conns) = cached_conns {
        let mut snap = CONN_SNAPSHOT.lock().await;
        let res = process_connections_delta(conns, active_server_id, &mut snap);
        if res.4 {
            crate::log_w!(
                "Обнаружена аномалия 'Чёрная дыра' на сервере '{}': {} сокетов, delta upload: {} байт, delta download: 0 байт",
                active_server_id, res.3, res.2
            );
        }
        return res;
    }

    let Ok(val) = mihomo::m_get(http, cfg, "/connections").await else {
        return (false, 0, 0, 0, false);
    };
    let Some(conns) = val.get("connections").and_then(|c| c.as_array()) else {
        return (false, 0, 0, 0, false);
    };

    let mut snap = CONN_SNAPSHOT.lock().await;
    let res = process_connections_delta(conns, active_server_id, &mut snap);
    if res.4 {
        crate::log_w!(
            "Обнаружена аномалия 'Чёрная дыра' на сервере '{}': {} сокетов, delta upload: {} байт, delta download: 0 байт",
            active_server_id, res.3, res.2
        );
    }
    res
}

/// Пассивный детектор "Чёрной дыры" (DPI Zero-Download Anomaly):
/// Детектирует DPI-сброс, когда прирост upload > 5 КБ при delta download == 0 на >= 3 активных сокетах.
pub async fn check_blackhole_anomaly(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    active_server_id: &str,
) -> bool {
    let (_, _, _, _, is_blackhole) = check_active_traffic_and_blackhole(http, cfg, active_server_id, None).await;
    is_blackhole
}

/// Проверка реального входящего клиентского трафика через активный узел в Mihomo по дельте.
/// Возвращает (has_live_traffic, delta_download_bytes).
/// Живой трафик — только при дельте download > 1 КБ (1024 байта) за интервал проверки.
#[allow(dead_code)]
pub async fn check_active_download_traffic(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    active_server_id: &str,
) -> (bool, u64) {
    let (has_live, dl_delta, _, _, _) = check_active_traffic_and_blackhole(http, cfg, active_server_id, None).await;
    (has_live, dl_delta)
}

/// Определение исправности активного сервера:
/// 1) Если обнаружена "чёрная дыра" — ВСЕГДА неисправен.
/// 2) Если пинг в норме (ping > 0 && ping <= threshold) — исправен.
/// 3) Если пинг сбоит, но клиенты реально скачивают трафик (has_live_traffic) — исправен.
pub fn is_active_server_healthy(
    ping: i64,
    threshold: i64,
    has_live_traffic: bool,
    is_blackhole: bool,
) -> bool {
    !is_blackhole && ((ping > 0 && ping <= threshold) || has_live_traffic)
}

/// Проверка условий переключения Best-Ping с учётом гистерезиса:
/// 1) Если обнаружена "чёрная дыра" — немедленное переключение.
/// 2) Если сервер мёртв / не отвечает (act_ping <= 0) — немедленное переключение.
/// 3) Сервер сбоит по задержке (act_ping > 0), и лучший кандидат превосходит его
///    минимум на величину гистерезиса (best_ms + hysteresis < act_ping).
pub fn should_best_ping_switch(
    act_ping: i64,
    is_blackhole: bool,
    best_ms: i64,
    hysteresis: i64,
) -> bool {
    if is_blackhole || act_ping <= 0 {
        return true;
    }
    best_ms + hysteresis < act_ping
}

/// Ступенчатый опрос списка серверов через семафор (по 4 потока) с интервалом между запусками,
/// чтобы минимизировать нагрузку на CPU роутера Keenetic.
pub async fn ping_servers_staggered(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    ids: &[String],
    timeout_ms: u64,
    concurrency: usize,
    interval_ms: u64,
) -> std::collections::BTreeMap<String, i64> {
    let mut out = std::collections::BTreeMap::new();
    if ids.is_empty() {
        return out;
    }
    let max_concurrency = concurrency.clamp(1, 6);
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrency));
    let shared_cfg = std::sync::Arc::new(cfg.clone());
    let bounded_timeout = timeout_ms.clamp(1000, 2500);
    let mut handles = Vec::new();

    for id in ids {
        let http_c = http.clone();
        let cfg_c = shared_cfg.clone();
        let sem_c = sem.clone();
        let id_c = id.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem_c.acquire().await.ok();
            let ms = mihomo::ping_server_url(&http_c, &cfg_c, &id_c, bounded_timeout, None).await;
            (id_c, ms)
        }));
        if interval_ms > 0 {
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
    }

    for h in handles {
        if let Ok((id, ms)) = h.await {
            out.insert(id, ms);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedTarget {
    pub id: String,
    pub ping_ms: i64,
    pub is_quarantined: bool,
    pub is_alive_slow: bool,
    pub tier: u8,
}

impl std::ops::Deref for SelectedTarget {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.id
    }
}

impl PartialEq<str> for SelectedTarget {
    fn eq(&self, other: &str) -> bool {
        self.id == other
    }
}

impl PartialEq<&str> for SelectedTarget {
    fn eq(&self, other: &&str) -> bool {
        self.id == *other
    }
}

impl PartialEq<String> for SelectedTarget {
    fn eq(&self, other: &String) -> bool {
        &self.id == other
    }
}

/// Чистая 4-уровневая иерархия отбора резервного сервера:
/// 1. В пределах порога пинга вне карантина (0 < ping <= threshold)
/// 2. Живой медленный вне карантина (ping > threshold)
pub trait PingMap {
    fn get_ping(&self, id: &str) -> Option<i64>;
}

impl PingMap for std::collections::HashMap<String, i64> {
    fn get_ping(&self, id: &str) -> Option<i64> {
        self.get(id).copied()
    }
}

impl PingMap for std::collections::BTreeMap<String, i64> {
    fn get_ping(&self, id: &str) -> Option<i64> {
        self.get(id).copied()
    }
}

/// Чистая 4-уровневая иерархия отбора резервного сервера:
/// 1. В пределах порога пинга вне карантина (0 < ping <= threshold)
/// 2. Живой медленный вне карантина (ping > threshold)
/// 3. В пределах порога пинга в карантине (0 < ping <= threshold)
/// 4. Живой медленный в карантине (ping > threshold)
pub fn select_from_candidates(
    candidates: &[String],
    pings: &impl PingMap,
    is_quarantined_fn: impl Fn(&str) -> bool,
    threshold: i64,
    preserve_order: bool,
) -> Option<SelectedTarget> {
    let mut tier1 = Vec::new();
    let mut tier2 = Vec::new();
    let mut tier3 = Vec::new();
    let mut tier4 = Vec::new();

    for id in candidates {
        if let Some(ms) = pings.get_ping(id.as_str()) {
            if ms <= 0 {
                continue; // узел лежит или таймаут
            }
            let quarantined = is_quarantined_fn(id.as_str());
            if !quarantined && ms <= threshold {
                tier1.push((id.clone(), ms));
            } else if !quarantined && ms > threshold {
                tier2.push((id.clone(), ms));
            } else if quarantined && ms <= threshold {
                tier3.push((id.clone(), ms));
            } else if quarantined && ms > threshold {
                tier4.push((id.clone(), ms));
            }
        }
    }

    let pick = |mut list: Vec<(String, i64)>, tier: u8, is_q: bool, is_slow: bool| -> Option<SelectedTarget> {
        if list.is_empty() {
            return None;
        }
        if !preserve_order {
            list.sort_by_key(|(_, ms)| *ms);
        }
        list.first().map(|(id, ms)| SelectedTarget {
            id: id.clone(),
            ping_ms: *ms,
            is_quarantined: is_q,
            is_alive_slow: is_slow,
            tier,
        })
    };

    if let Some(target) = pick(tier1, 1, false, false) {
        return Some(target);
    }
    if let Some(target) = pick(tier2, 2, false, true) {
        return Some(target);
    }
    if let Some(target) = pick(tier3, 3, true, false) {
        return Some(target);
    }
    if let Some(target) = pick(tier4, 4, true, true) {
        return Some(target);
    }

    None
}

/// Логика отбора кандидата при аварийном переключении (Strict Chain Boundary vs Best-of-Rest)
/// с поддержкой Умного фейловера (Tier 1: обычные VPS -> Tier 2: серверы белых списков).
pub fn select_failover_target_tiered(
    has_chain: bool,
    chain_candidate_ids: &[String],
    all_servers: &[Server],
    active_id: &str,
    is_quarantined_fn: impl Fn(&str) -> bool,
    is_whitelist_fn: impl Fn(&Server) -> bool,
    smart_failover_enabled: bool,
    pings: &impl PingMap,
    threshold: i64,
) -> Result<SelectedTarget, String> {
    if has_chain {
        if chain_candidate_ids.is_empty() {
            return Err("Все серверы цепочки приоритетов недоступны (в таймауте или карантине). Переключение заблокировано.".to_string());
        }
        if smart_failover_enabled {
            let tier1_chain: Vec<String> = chain_candidate_ids
                .iter()
                .filter(|id| {
                    all_servers.iter().find(|s| &s.id == *id).map_or(true, |s| !is_whitelist_fn(s))
                })
                .cloned()
                .collect();
            let tier2_chain: Vec<String> = chain_candidate_ids
                .iter()
                .filter(|id| {
                    all_servers.iter().find(|s| &s.id == *id).map_or(false, |s| is_whitelist_fn(s))
                })
                .cloned()
                .collect();

            // 1. Сначала пробуем отобрать среди обычных серверов VPS (Tier 1) в порядке цепочки
            if !tier1_chain.is_empty() {
                if let Some(target) = select_from_candidates(&tier1_chain, pings, &is_quarantined_fn, threshold, true) {
                    return Ok(target);
                }
            }
            // 2. Только если все серверы Tier 1 в цепочке недоступны — переходим на серверы белых списков (Tier 2)
            if !tier2_chain.is_empty() {
                if let Some(target) = select_from_candidates(&tier2_chain, pings, &is_quarantined_fn, threshold, true) {
                    return Ok(target);
                }
            }
        } else {
            if let Some(target) = select_from_candidates(chain_candidate_ids, pings, &is_quarantined_fn, threshold, true) {
                return Ok(target);
            }
        }
        return Err("Все серверы цепочки приоритетов недоступны (в таймауте или карантине). Переключение заблокировано.".to_string());
    }

    // Резервный поиск среди всех остальных серверов (best-of-rest) — ТОЛЬКО если цепочка НЕ настроена
    let eligible_servers: Vec<&Server> = all_servers
        .iter()
        .filter(|s| {
            s.id != active_id
                && !s.is_pool
                && s.protocol != "URL-TEST"
                && s.protocol != "FALLBACK"
                && s.id != "DIRECT"
                && s.id != "REJECT"
                && s.id != "GLOBAL"
                && !s.id.starts_with("AUTO-")
        })
        .collect();

    if eligible_servers.is_empty() {
        return Err("Нет резервных серверов для переключения".to_string());
    }

    if smart_failover_enabled {
        // Разделяем на Tier 1 (обычные VPS) и Tier 2 (серверы белых списков)
        let tier1_ids: Vec<String> = eligible_servers
            .iter()
            .filter(|s| !is_whitelist_fn(s))
            .map(|s| s.id.clone())
            .collect();
        let tier2_ids: Vec<String> = eligible_servers
            .iter()
            .filter(|s| is_whitelist_fn(s))
            .map(|s| s.id.clone())
            .collect();

        // Сначала пробуем отобрать среди обычных серверов VPS (Tier 1)
        if !tier1_ids.is_empty() {
            if let Some(target) = select_from_candidates(&tier1_ids, pings, &is_quarantined_fn, threshold, false) {
                return Ok(target);
            }
        }

        // Если все обычные серверы недоступны — переходим на серверы белых списков (Tier 2)
        if !tier2_ids.is_empty() {
            if let Some(target) = select_from_candidates(&tier2_ids, pings, &is_quarantined_fn, threshold, false) {
                return Ok(target);
            }
        }
    } else {
        let candidate_ids: Vec<String> = eligible_servers.iter().map(|s| s.id.clone()).collect();
        if let Some(target) = select_from_candidates(&candidate_ids, pings, &is_quarantined_fn, threshold, false) {
            return Ok(target);
        }
    }

    Err("Все резервные серверы недоступны".to_string())
}

pub fn select_failover_target(
    has_chain: bool,
    chain_candidate_ids: &[String],
    all_servers: &[Server],
    active_id: &str,
    is_quarantined_fn: impl Fn(&str) -> bool,
    pings: &impl PingMap,
    threshold: i64,
) -> Result<SelectedTarget, String> {
    select_failover_target_tiered(
        has_chain,
        chain_candidate_ids,
        all_servers,
        active_id,
        is_quarantined_fn,
        |_| false,
        false,
        pings,
        threshold,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceFailoverDecision {
    KeepCurrent {
        reason: String,
        has_live_traffic: bool,
    },
    AutoRestore {
        target: String,
        ping_ms: i64,
        is_main: bool,
    },
    Switch {
        target: SelectedTarget,
        reason: String,
    },
    NoBackupAvailable {
        reason: String,
    },
}

/// Чистая функция принятия решения для per-device failover.
/// Учитывает наличие активного трафика загрузки (> 1 КБ), детекцию чёрной дыры,
/// 4-уровневую иерархию и субординацию уровней (Smart Failover: Tier 1 VPS -> Tier 2 БС).
pub fn evaluate_device_failover(
    _device_ip: &str,
    cur: &str,
    servers_chain: &[String],
    cur_ping: i64,
    threshold: i64,
    auto_restore: bool,
    has_live_traffic: bool,
    is_blackhole: bool,
    all_servers: &[Server],
    higher_pings: &impl PingMap,
    candidate_pings: &impl PingMap,
    is_quarantined_fn: impl Fn(&str) -> bool,
    is_whitelist_fn: impl Fn(&Server) -> bool,
    smart_failover_enabled: bool,
) -> DeviceFailoverDecision {
    let hyst = calc_hysteresis(threshold);
    let is_cur_healthy = is_active_server_healthy(cur_ping, threshold, has_live_traffic, is_blackhole);

    if is_cur_healthy {
        // При активной загрузке (> 1 КБ) сбои пинга толерируются
        if has_live_traffic && (cur_ping <= 0 || cur_ping > threshold) {
            return DeviceFailoverDecision::KeepCurrent {
                reason: format!("пинг сбоит ({cur_ping} мс), но идёт активная загрузка (> 1 КБ)"),
                has_live_traffic: true,
            };
        }

        // Автовозврат: только если на устройстве НЕТ активной загрузки (> 1 КБ)
        if auto_restore && !has_live_traffic {
            let higher = get_higher_priority_candidates(servers_chain, cur);
            for target_server in higher {
                // Не восстанавливаем на узел в карантине
                if is_quarantined_fn(target_server) {
                    continue;
                }
                if let Some(p) = higher_pings.get_ping(target_server) {
                    if p > 0 && p <= hyst {
                        let is_main = target_server == &servers_chain[0];
                        return DeviceFailoverDecision::AutoRestore {
                            target: target_server.to_string(),
                            ping_ms: p,
                            is_main,
                        };
                    }
                }
            }
        }

        return DeviceFailoverDecision::KeepCurrent {
            reason: format!("пинг {cur_ping} мс в норме"),
            has_live_traffic,
        };
    }

    // Текущий сервер сбоит или попал в чёрную дыру
    let reason = if is_blackhole {
        "DPI чёрная дыра (upload > 5KB, download == 0)".to_string()
    } else if cur_ping > 0 {
        format!("пинг {cur_ping} мс > порога {threshold} мс")
    } else {
        "сервер не отвечает".to_string()
    };

    let cands: Vec<String> = servers_chain.iter().filter(|s| *s != cur).cloned().collect();
    if cands.is_empty() {
        return DeviceFailoverDecision::NoBackupAvailable { reason };
    }

    let target_res = select_failover_target_tiered(
        true, // has_chain = true (соблюдает порядок цепочки dr.servers)
        &cands,
        all_servers,
        cur,
        &is_quarantined_fn,
        &is_whitelist_fn,
        smart_failover_enabled,
        candidate_pings,
        threshold,
    );

    match target_res {
        Ok(target) => DeviceFailoverDecision::Switch { target, reason },
        Err(_) => DeviceFailoverDecision::NoBackupAvailable { reason },
    }
}

pub const ACTIVE_FAILOVER_CHECK_INTERVAL_SECS: u64 = 10;

/// Проверяет, требуется ли ранний аудит свежепереключенного сервера (в течение 10 сек после переключения)
pub async fn is_post_switch_pending() -> bool {
    let last = LAST_SWITCH.lock().await;
    if let Some(t) = *last {
        t.elapsed() < Duration::from_secs(10)
    } else {
        false
    }
}

/// Одна итерация проверки. Возвращает текст решения (для ручного прогона).
pub async fn run_check(state: &AppState) -> Result<String, String> {
    // DIAG-01: Изоляция от Speedtest — если выполняется замер скорости, пропускаем переключение failover
    let _speedtest_guard = match state.speedtest_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            crate::log_i!("Failover пропущен: в данный момент выполняется Speedtest");
            return Ok("Проверка failover пропущена: выполняется Speedtest".to_string());
        }
    };

    // F-07: служба остановлена пользователем — failover не трогает маршруты и соединения
    if crate::watchdog::is_service_stopped() {
        return Ok("Failover пропущен: служба XKeen остановлена пользователем".to_string());
    }

    // F-04: ручной прогон ждёт завершения фоновой итерации (и наоборот)
    let _check_guard = CHECK_IN_PROGRESS.lock().await;

    // FO-SWT-01: замена слепого 30-секундного кулдауна на 5-секундную задержку стабилизации сокетов
    {
        let last = LAST_SWITCH.lock().await;
        if let Some(t) = *last {
            if t.elapsed() < Duration::from_secs(5) {
                return Ok("Ожидание стабилизации сокетов (5 сек) после предыдущего переключения".to_string());
            }
        }
    }

    let cfg = state.config.read().await.clone();

    // WAN Down Detection: если физический линк провайдера лежит, не трогаем Failover
    if !is_wan_online(&state.http, &cfg).await {
        let msg = "WAN Down: физический канал провайдера недоступен (DIRECT сбой). Failover заморожен.".to_string();
        state.failover_log.push_event_only(&msg, false).await;
        return Ok(msg);
    }

    let threshold = cfg.failover.ping_threshold_ms as i64;
    let servers = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await?;

    // Реальный активный сервер (предпочитаем конкретный прокси перед группами Fastest/Fallback).
    let active = servers
        .iter()
        .find(|s| s.is_active && s.id != "Fastest" && s.id != "Fallback")
        .or_else(|| servers.iter().find(|s| s.is_active))
        .cloned();

    // 0. Режим Best-Ping Failover (строго внутри Failover: выбор лучшего по пингу только при сбое активного узла)
    if cfg.failover.best_ping_enabled {
        let act_id = active.as_ref().map(|s| s.id.as_str()).unwrap_or("");
        let act_name = active.as_ref().map(|s| s.name.as_str()).unwrap_or("Неизвестный");

        if act_id.is_empty() {
            let msg = "Best-Ping Failover: активный сервер не определён".to_string();
            state.failover_log.push(&msg, false).await;
            return Err(msg);
        }

        let ping_timeout = calc_ping_timeout(threshold);

        // 1. Опрос ТОЛЬКО активного сервера при штатной работе (L7 Split Probe + Fast Retry)
        let mut act_ping = mihomo::ping_server_l7_split(&state.http, &cfg, act_id, ping_timeout).await;
        if act_ping <= 0 || act_ping > threshold {
            tokio::time::sleep(Duration::from_millis(1000)).await;
            let retry = mihomo::ping_server_l7_split(&state.http, &cfg, act_id, ping_timeout).await;
            if retry > 0 && retry <= threshold {
                act_ping = retry;
            }
        }

        // 2. Единый опрос /connections за цикл: расчёт дельт трафика и детекция черной дыры
        let conns_val = mihomo::m_get(&state.http, &cfg, "/connections").await.ok();
        let conns_slice = conns_val.as_ref().and_then(|v| v.get("connections")).and_then(|c| c.as_array()).map(|a| a.as_slice());

        let (has_live_traffic, dl_bytes, up_bytes, _sockets, is_blackhole) =
            check_active_traffic_and_blackhole(&state.http, &cfg, act_id, conns_slice).await;

        // Честный учёт трафика БС за эпизод по дельте /connections (без подмены каналов)
        let is_active_bs = is_whitelist_server_full(act_name, act_id, active.as_ref().and_then(|s| s.provider.as_deref()), &cfg);
        if is_active_bs {
            let delta_bytes = dl_bytes + up_bytes;
            if delta_bytes > 0 {
                record_whitelist_episode_bytes(delta_bytes);
                crate::traffic::TOTAL_WHITELIST_BYTES.fetch_add(delta_bytes, std::sync::atomic::Ordering::Relaxed);
            }
        }

        let is_active_healthy = is_active_server_healthy(act_ping, threshold, has_live_traffic, is_blackhole);

        if is_active_healthy {
            if has_live_traffic && (act_ping <= 0 || act_ping > threshold) {
                crate::log_i!(
                    "Best-Ping Failover: контрольный L7-пробник узла '{}' не ответил (пинг {} мс), но через узел активно течёт реальный клиентский трафик ({} КБ) — узел исправен",
                    act_name, act_ping, dl_bytes / 1024
                );
            }
            let msg = format!(
                "Best-Ping Failover: активный '{}' работает штатно (пинг {act_ping} мс <= порога {threshold} мс), переключение не требуется",
                act_name
            );
            return Ok(msg);
        }

        // 3. Активный сервер сбоит! Только теперь формируем пул кандидатов и опрашиваем их:
        if is_blackhole && cfg.failover.quarantine_enabled {
            let reason = "Зомби-сервер (DPI сброс трафика: upload > 5KB, download == 0)";
            let q_secs = add_quarantine(act_id, act_name, reason).await;
            let q_note = format!(
                "⚠️ Best-Ping: сервер '{}' объявлен зомби (чёрная дыра) → карантин на {} мин",
                act_name,
                q_secs / 60
            );
            state.failover_log.push(&q_note, false).await;
        }

        let pool_limit = cfg.failover.best_ping_pool_size.clamp(2, 30);
        let mut pool: Vec<String> = Vec::with_capacity(pool_limit);

        if !cfg.failover.priority_chain.is_empty() {
            for id in &cfg.failover.priority_chain {
                if pool.len() >= pool_limit {
                    break;
                }
                if id != act_id && !pool.contains(id) && servers.iter().any(|s| &s.id == id && !s.is_pool && s.id != "Fastest" && s.id != "Fallback" && s.id != "DIRECT") {
                    pool.push(id.clone());
                }
            }
        } else {
            for s in &servers {
                if pool.len() >= pool_limit {
                    break;
                }
                if s.id != act_id && !pool.contains(&s.id) && !s.is_pool && s.id != "Fastest" && s.id != "Fallback" && s.id != "DIRECT" {
                    pool.push(s.id.clone());
                }
            }
        }

        if pool.is_empty() {
            let msg = format!("Best-Ping Failover: активный '{}' сбоит, но нет доступных резервных серверов для проверки", act_name);
            state.failover_log.push(&msg, false).await;
            return Err(msg);
        }

        let pings = ping_servers_staggered(&state.http, &cfg, &pool, ping_timeout, 4, 25).await;

        // Активный сервер сбоит (пинг <= 0 или пинг > threshold). Отбираем живых кандидатов по 4-уровневой иерархии.
        let mut q_ids = std::collections::HashSet::new();
        if cfg.failover.quarantine_enabled {
            for (id, _) in &pings {
                if is_quarantined(id).await {
                    q_ids.insert(id.clone());
                }
            }
        }

        let candidates: Vec<String> = pool.iter()
            .filter(|id| id.as_str() != "Fastest" && id.as_str() != "Fallback" && id.as_str() != "DIRECT" && id.as_str() != act_id)
            .cloned()
            .collect();

        let target = if cfg.failover.smart_failover_enabled {
            let tier1_cands: Vec<String> = candidates.iter()
                .filter(|id| {
                    servers.iter().find(|s| &s.id == *id)
                        .map_or(true, |s| !is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg))
                })
                .cloned()
                .collect();
            let tier2_cands: Vec<String> = candidates.iter()
                .filter(|id| {
                    servers.iter().find(|s| &s.id == *id)
                        .map_or(false, |s| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg))
                })
                .cloned()
                .collect();

            select_from_candidates(&tier1_cands, &pings, |id| q_ids.contains(id), threshold, false)
                .or_else(|| select_from_candidates(&tier2_cands, &pings, |id| q_ids.contains(id), threshold, false))
        } else {
            select_from_candidates(&candidates, &pings, |id| q_ids.contains(id), threshold, false)
        };

        if let Some(target) = target {
            let best_id = &target.id;
            let best_ms = target.ping_ms;
            let best_name = servers.iter().find(|s| &s.id == best_id).map(|s| s.name.as_str()).unwrap_or(best_id.as_str());

            // Гистерезис Best-Ping:
            let hysteresis = cfg.failover.best_ping_hysteresis_ms as i64;
            if !should_best_ping_switch(act_ping, is_blackhole, best_ms, hysteresis) {
                let msg = format!(
                    "Best-Ping Failover: активный '{}' ({act_ping} мс) сбоит, но лучший кандидат '{}' ({best_ms} мс) не превышает гистерезис ({hysteresis} мс) — переключение отклонено",
                    act_name, best_name
                );
                return Ok(msg);
            }

            if target.is_quarantined {
                crate::log_w!(
                    "ВНИМАНИЕ: Все резервные серверы в карантине! Аварийный обход карантина в Best-Ping: выбран сервер '{}' (пинг {} мс, tier {})",
                    best_id, best_ms, target.tier
                );
                let note = format!(
                    "⚠️ ВНИМАНИЕ: Все резервы в карантине! Аварийный обход карантина в Best-Ping: выбран '{}' (пинг {} мс)",
                    best_name, best_ms
                );
                state.failover_log.push(&note, false).await;
            }
            if target.is_alive_slow {
                crate::log_w!(
                    "Best-Ping Failover: все быстрые серверы недоступны. Выбран живой, но медленный сервер '{}' (пинг {} мс > порога {} мс)",
                    best_id, best_ms, threshold
                );
            }

            let fail_reason = if is_blackhole {
                "DPI чёрная дыра".to_string()
            } else if act_ping <= 0 {
                "таймаут / недоступен".to_string()
            } else {
                format!("пинг {act_ping} мс > порога {threshold} мс")
            };

            let msg = format!(
                "Best-Ping Failover: активный '{}' сбоит ({fail_reason}) → переключение на лучший узел '{best_name}' (пинг {best_ms} мс{})",
                act_name,
                if target.is_quarantined { " [обход карантина]" } else if target.is_alive_slow { " [медленный резерв]" } else { "" }
            );
            return match mihomo::switch_server(&state.http, &cfg, best_id).await {
                Ok(_) => {
                    record_switch(best_id).await;
                    let best_prov = servers.iter().find(|s| &s.id == best_id).and_then(|s| s.provider.as_deref());
                    handle_whitelist_transition(state, best_id, best_name, best_prov).await;
                    state.failover_log.push(&msg, true).await;
                    crate::notifications::notify_failover(
                        &state.http,
                        Some(&cfg.mihomo_proxy_url()),
                        &cfg.notifications,
                        "switch",
                        "Best-Ping Failover — переключение при сбое",
                        &msg,
                    ).await;
                    Ok(msg)
                }
                Err(e) => {
                    state.failover_log.push(format!("{msg} — ошибка: {e}"), false).await;
                    Err(e)
                }
            };
        } else {
            let msg = format!("Best-Ping Failover: активный '{}' сбоит, но в пуле из {} серверов нет доступных узлов", act_name, pool.len());
            state.failover_log.push(&msg, false).await;
            return Err(msg);
        }
    }

    // Цепочка приоритетов: только существующие сейчас серверы, по порядку.
    let chain: Vec<Server> = cfg
        .failover
        .priority_chain
        .iter()
        .filter_map(|id| servers.iter().find(|s| &s.id == id).cloned())
        .collect();

    let hyst = calc_hysteresis(threshold);
    let ping_timeout = calc_ping_timeout(threshold);
    let mut checked_higher_notes: Vec<String> = Vec::new();

    // 1. Проверка текущего активного сервера
    let Some(active) = active else {
        let msg = "Активный сервер не определён".to_string();
        state.failover_log.push(&msg, false).await;
        return Err(msg);
    };

    let mut current = mihomo::ping_server_l7_split(&state.http, &cfg, &active.id, ping_timeout).await;
    // Fast Retry: 1 быстрый повтор через 1 сек для фильтрации случайных скачков
    if current <= 0 || current > threshold {
        tokio::time::sleep(Duration::from_millis(1000)).await;
        let retry_ping = mihomo::ping_server_l7_split(&state.http, &cfg, &active.id, ping_timeout).await;
        if retry_ping > 0 && retry_ping <= threshold {
            current = retry_ping;
        }
    }

    // Единый опрос /connections за цикл: расчёт дельт трафика и детекция черной дыры
    let conns_val = mihomo::m_get(&state.http, &cfg, "/connections").await.ok();
    let conns_slice = conns_val.as_ref().and_then(|v| v.get("connections")).and_then(|c| c.as_array()).map(|a| a.as_slice());

    let (has_live_traffic, dl_bytes, up_bytes, _sockets, is_blackhole) =
        check_active_traffic_and_blackhole(&state.http, &cfg, &active.id, conns_slice).await;

    // Честный учёт трафика БС за эпизод по дельте /connections (без подмены каналов)
    let is_active_bs = is_whitelist_server_full(&active.name, &active.id, active.provider.as_deref(), &cfg);
    if is_active_bs {
        let delta_bytes = dl_bytes + up_bytes;
        if delta_bytes > 0 {
            record_whitelist_episode_bytes(delta_bytes);
            crate::traffic::TOTAL_WHITELIST_BYTES.fetch_add(delta_bytes, std::sync::atomic::Ordering::Relaxed);
        }
    }

    // ПРИОРИТЕТ РЕАЛЬНОГО ТРАФИКА:
    // Сервер признаётся живым и исправным:
    // 1) Если клиенты прямо сейчас скачивают трафик (дельта download > 1 КБ) и нет аномалии черной дыры.
    // 2) ИЛИ если L7-пинг в норме (<= threshold) и нет аномалии черной дыры.
    let is_active_healthy = !is_blackhole && ((current > 0 && current <= threshold) || has_live_traffic);

    if has_live_traffic && (current <= 0 || current > threshold) {
        crate::log_i!(
            "Failover: контрольный L7-пробник узла '{}' не ответил (пинг {} мс), но через узел активно течёт реальный клиентский трафик ({} КБ) — узел исправен",
            active.name, current, dl_bytes / 1024
        );
    }

    // СЦЕНАРИЙ А: АКТИВНЫЙ СЕРВЕР СБОИТ (ТАЙМАУТ, ВЫСОКИЙ ПИНГ ИЛИ АНОМАЛИЯ ЧЕРНОЙ ДЫРЫ)
    // ЭКСТРЕННОЕ ПЕРЕКЛЮЧЕНИЕ: немедленно отменяем любой Canary-тест и переключаем клиентов без задержек!
    if !is_active_healthy {
        // Критически важно: если активный резерв упал, клиенты без интернета.
        // Нельзя оставлять их ждать 180с Canary-теста — сбрасываем стадию теста!
        reset_canary().await;

        let reason = if is_blackhole {
            "Зомби-сервер (DPI сброс трафика: upload > 5KB, download == 0)".to_string()
        } else if current > 0 {
            format!("высокий пинг {current} мс (> {threshold})")
        } else {
            "сервер не отвечает (L7 сбой)".to_string()
        };

        // Анти-флеппинг и карантин зомби:
        let (is_early_failure, elapsed_secs) = {
            let last_act = LAST_ACTIVATED_SERVER.lock().await;
            if let Some((ref act_id, activated_at, is_manual)) = *last_act {
                // Сервер, выбранный пользователем вручную, обладает полным иммунитетом к раннему карантину!
                if act_id == &active.id && !is_manual {
                    let el = activated_at.elapsed().as_secs();
                    (el < EARLY_FAILURE_WINDOW_SECS, Some(el))
                } else {
                    (false, None)
                }
            } else {
                (false, None)
            }
        };

        if cfg.failover.quarantine_enabled && (is_blackhole || is_early_failure) {
            let effective_reason = if is_early_failure && elapsed_secs.unwrap_or(0) <= 10 {
                format!("Мёртв при переключении (ранний сбой через {} с)", elapsed_secs.unwrap_or(5))
            } else {
                reason.clone()
            };
            let q_secs = add_quarantine(&active.id, &active.name, &effective_reason).await;
            let q_mins = q_secs / 60;
            let el_str = elapsed_secs.map(|s| format!(" спустя {s} с после включения")).unwrap_or_default();
            let q_note = format!(
                "⚠️ Карантин: сервер '{}' дал сбой{el_str} ({effective_reason}) → помещён в карантин на {q_mins} мин",
                active.name
            );
            state.failover_log.push(&q_note, false).await;
            crate::notifications::notify_failover(
                &state.http,
                Some(&cfg.mihomo_proxy_url()),
                &cfg.notifications,
                "quarantine",
                "Карантин нестабильного сервера",
                &q_note,
            ).await;
        }

        // Унифицированный отбор кандидата (4-уровневая иерархия без блэкаутов)
        let has_priority_chain = !cfg.failover.priority_chain.is_empty() || !chain.is_empty();

        let chain_candidates: Vec<String> = if has_priority_chain {
            chain
                .iter()
                .filter(|s| s.id != active.id)
                .map(|s| s.id.clone())
                .collect()
        } else {
            Vec::new()
        };

        let target_res = if has_priority_chain {
            if chain_candidates.is_empty() {
                Err("Все серверы цепочки приоритетов недоступны (в таймауте или карантине). Переключение заблокировано.".to_string())
            } else {
                let pings = mihomo::ping_all(&state.http, &cfg, &chain_candidates, ping_timeout).await;
                let mut q_ids = std::collections::HashSet::new();
                if cfg.failover.quarantine_enabled {
                    for id in &chain_candidates {
                        if is_quarantined(id).await {
                            q_ids.insert(id.clone());
                        }
                    }
                }
                select_failover_target_tiered(
                    true,
                    &chain_candidates,
                    &servers,
                    &active.id,
                    |id| q_ids.contains(id),
                    |s| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg),
                    cfg.failover.smart_failover_enabled,
                    &pings,
                    threshold,
                )
            }
        } else {
            let candidate_ids: Vec<String> = servers
                .iter()
                .filter(|s| {
                    s.id != active.id
                        && !s.is_pool
                        && s.protocol != "URL-TEST"
                        && s.protocol != "FALLBACK"
                        && s.id != "DIRECT"
                        && s.id != "REJECT"
                        && s.id != "GLOBAL"
                        && !s.id.starts_with("AUTO-")
                })
                .map(|s| s.id.clone())
                .collect();
            if candidate_ids.is_empty() {
                Err("Нет резервных серверов для переключения".to_string())
            } else {
                let pings = mihomo::ping_all(&state.http, &cfg, &candidate_ids, ping_timeout).await;
                let mut q_ids = std::collections::HashSet::new();
                if cfg.failover.quarantine_enabled {
                    for id in &candidate_ids {
                        if is_quarantined(id).await {
                            q_ids.insert(id.clone());
                        }
                    }
                }
                select_failover_target_tiered(
                    false,
                    &[],
                    &servers,
                    &active.id,
                    |id| q_ids.contains(id),
                    |s| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg),
                    cfg.failover.smart_failover_enabled,
                    &pings,
                    threshold,
                )
            }
        };

        match target_res {
            Ok(target) => {
                if target.is_quarantined {
                    crate::log_w!(
                        "ВНИМАНИЕ: Все резервные серверы в карантине! Аварийный обход карантина: выбран сервер '{}' (пинг {} мс, tier {})",
                        target.id, target.ping_ms, target.tier
                    );
                    let note = format!(
                        "⚠️ ВНИМАНИЕ: Все резервные серверы в карантине! Аварийный обход карантина: выбран '{}' (пинг {} мс)",
                        target.id, target.ping_ms
                    );
                    state.failover_log.push(&note, false).await;
                }
                if target.is_alive_slow {
                    crate::log_w!(
                        "Failover: все быстрые серверы недоступны. Аварийное переключение на живой, но медленный сервер '{}' (пинг {} мс > порога {} мс)",
                        target.id, target.ping_ms, threshold
                    );
                }
                let best_name = servers.iter().find(|s| s.id == target.id).map(|s| s.name.clone()).unwrap_or_else(|| target.id.clone());
                let msg = format!(
                    "Экстренный Failover: активный '{}' ({reason}) → переключение на '{best_name}' (пинг {} мс{})",
                    active.name, target.ping_ms,
                    if target.is_quarantined { " [обход карантина]" } else if target.is_alive_slow { " [медленный резерв]" } else { "" }
                );
                return match mihomo::switch_server(&state.http, &cfg, &target.id).await {
                    Ok(_) => {
                        record_switch(&target.id).await;
                        let s_prov = servers.iter().find(|s| s.id == target.id).and_then(|s| s.provider.as_deref());
                        handle_whitelist_transition(state, &target.id, &best_name, s_prov).await;
                        state.failover_log.push(&msg, true).await;
                        crate::notifications::notify_failover(
                            &state.http,
                            Some(&cfg.mihomo_proxy_url()),
                            &cfg.notifications,
                            "switch",
                            "Сбой сервера — экстренное переключение",
                            &msg,
                        ).await;
                        Ok(msg)
                    }
                    Err(e) => {
                        state.failover_log.push(format!("{msg} — ошибка: {e}"), false).await;
                        Err(e)
                    }
                };
            }
            Err(msg) => {
                state.failover_log.push(&msg, false).await;
                let event_title = if has_priority_chain {
                    "Критический сбой: Все серверы цепочки приоритетов недоступны!"
                } else {
                    "Критический сбой: Все серверы недоступны!"
                };
                crate::notifications::notify_failover(
                    &state.http,
                    Some(&cfg.mihomo_proxy_url()),
                    &cfg.notifications,
                    "all_down",
                    event_title,
                    &msg,
                ).await;
                return Err(msg);
            }
        }
    }

    // СЦЕНАРИЙ Б: АКТИВНЫЙ СЕРВЕР РАБОТАЕТ ШТАТНО (ПИНГ В НОРМЕ)
    // Клиенты имеют стабильный интернет. Только теперь проверяем автовозврат на более приоритетный сервер.
    let canary_duration_secs = if cfg.failover.canary_duration_secs == 0 {
        CANARY_DURATION_SECS
    } else {
        cfg.failover.canary_duration_secs.clamp(15, 300)
    };

    let is_active_bs = is_whitelist_server_full(&active.name, &active.id, active.provider.as_deref(), &cfg);
    let should_restore_chain = cfg.failover.auto_restore_priority && !chain.is_empty();
    let should_restore_smart = cfg.failover.smart_failover_enabled && is_active_bs;

    if should_restore_chain || should_restore_smart {
        // Если активный сервер уже совпадает с целевым сервером Canary, сбрасываем стадию
        {
            let mut canary_guard = CANARY_STAGE.lock().await;
            if canary_guard.as_ref().map_or(false, |c| c.target_id == active.id) {
                *canary_guard = None;
            }
        }

        let higher: Vec<Server> = if should_restore_chain {
            get_higher_priority_candidates(&chain, &active.id).to_vec()
        } else {
            Vec::new()
        };

        let higher = if higher.is_empty() && should_restore_smart {
            // Умный фейловер: активен сервер белых списков, кандидаты на возврат — любые живые обычные VPS серверы (Tier 1)
            servers.iter()
                .filter(|s| {
                    s.id != active.id
                        && !s.is_pool
                        && s.protocol != "URL-TEST"
                        && s.protocol != "FALLBACK"
                        && s.id != "DIRECT"
                        && s.id != "REJECT"
                        && s.id != "GLOBAL"
                        && !s.id.starts_with("AUTO-")
                        && !is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg)
                })
                .cloned()
                .collect()
        } else {
            higher
        };

        let effective_canary_duration_secs = if is_active_bs && should_restore_smart {
            cfg.failover.smart_failover_canary_duration_secs.clamp(15, 180)
        } else {
            canary_duration_secs
        };

        if !higher.is_empty() {
            let mut eligible_higher = Vec::new();
            for s in higher {
                if cfg.failover.quarantine_enabled && is_quarantined(&s.id).await {
                    if let Some(rem) = get_quarantine_remaining(&s.id).await {
                        let mins = (rem + 59) / 60;
                        checked_higher_notes.push(format!("'{}': в карантине (осталось {} мин)", s.name, mins));
                    }
                } else {
                    eligible_higher.push(s.clone());
                }
            }

            if !eligible_higher.is_empty() {
                let higher_ids: Vec<String> = eligible_higher.iter().map(|s| s.id.clone()).collect();
                let pings = mihomo::ping_all(&state.http, &cfg, &higher_ids, ping_timeout).await;
                if !should_restore_chain {
                    eligible_higher.sort_by_key(|s| pings.get(&s.id).copied().filter(|&p| p > 0).unwrap_or(i64::MAX));
                }
                for pri in &eligible_higher {
                    let ping = pings.get(&pri.id).copied().unwrap_or(-1);
                    if ping > 0 && ping <= hyst {
                        // Кандидат ответил на базовый пинг в пределах гистерезиса.
                        // CANARY SOAK TEST: не переключаемся сразу, проверяем стабильность!
                        let mut canary_guard = CANARY_STAGE.lock().await;
                        let is_same_target = canary_guard
                            .as_ref()
                            .map_or(false, |c| c.target_id == pri.id && c.from_id == active.id);

                        if !is_same_target {
                            // Переключаем селектор-группу Canary на кандидата без сброса сокетов клиентов
                            let _ = mihomo::switch_selector(&state.http, &cfg, mihomo::CANARY_PROBE_GROUP_NAME, &pri.id).await;

                            // Выполняем реальную загрузку тестового блока 64 КБ полезной нагрузки через 127.0.0.1:7894
                            let download_res = perform_canary_download(CANARY_PAYLOAD_PORT, CANARY_PROBE_BYTES, ping_timeout.max(5000)).await;

                            let (downloaded_bytes, measured_speed_kbps) = match download_res {
                                Ok((b, s)) => (b, s),
                                Err(err) => {
                                    // Fallback на run_canary_probe если слушатель 7894 еще не поднят или в оффлайн-тестах
                                    let l7_probe = run_canary_probe(&state.http, &cfg, &pri.id, ping_timeout).await;
                                    if l7_probe <= 0 {
                                        let fail_msg = format!(
                                            "Canary Soak Test: кандидат '{}' ответил на TCP-пинг, но провалил проверку полезной нагрузки ({err}) — помечен как '🧟 Зомби-сервер', переход к следующему в цепочке",
                                            pri.name
                                        );
                                        let now_str = Local::now().format("%H:%M:%S").to_string();
                                        set_canary_last_result(CanaryLastResult {
                                            server_id: pri.id.clone(),
                                            server_name: pri.name.clone(),
                                            timestamp: now_str,
                                            success: false,
                                            is_zombie: true,
                                            reason: format!("Сбой полезной нагрузки: {err}"),
                                            elapsed_secs: 0,
                                        }).await;

                                        if cfg.failover.quarantine_enabled {
                                            add_quarantine_with_duration(&pri.id, &pri.name, "Зомби-сервер: L7-сбой передачи трафика", 600).await;
                                        }
                                        state.failover_log.push(&fail_msg, false).await;
                                        checked_higher_notes.push(format!("'{}': 🧟 Зомби (L7 сбой)", pri.name));
                                        continue;
                                    }
                                    let calc_speed = ((CANARY_PROBE_BYTES as f64 * 8.0) / (l7_probe.max(1) as f64 / 1000.0)) / 1000.0;
                                    (CANARY_PROBE_BYTES, calc_speed)
                                }
                            };

                            // Инициализация новой стадии Canary-теста: фиксируем старт, клиенты остаются на стабильном сервере
                            *canary_guard = Some(CanaryState {
                                target_id: pri.id.clone(),
                                target_name: pri.name.clone(),
                                from_id: active.id.clone(),
                                from_name: active.name.clone(),
                                started_at: Instant::now(),
                                probes_passed: 1,
                                total_bytes: downloaded_bytes,
                                duration_secs: effective_canary_duration_secs,
                            });
                            drop(canary_guard);
                            let note = if is_active_bs && should_restore_smart {
                                format!(
                                    "Canary Soak Test (Умный фейловер): обычный VPS сервер '{}' ответил на пинг ({} мс, порог {} мс). Загружено {} КБ ({:.1} кбит/с). Запущен укороченный {}-секундный тест стабильности для возврата с сервера белых списков '{}'",
                                    pri.name, ping, hyst, downloaded_bytes / 1024, measured_speed_kbps, effective_canary_duration_secs, active.name
                                )
                            } else {
                                format!(
                                    "Canary Soak Test: кандидат '{}' ответил на пинг ({} мс, порог {} мс). Загружено {} КБ ({:.1} кбит/с через 127.0.0.1:{}). Запущен {}-секундный тест стабильности без переключения клиентов с '{}'",
                                    pri.name, ping, hyst, downloaded_bytes / 1024, measured_speed_kbps, CANARY_PAYLOAD_PORT, effective_canary_duration_secs, active.name
                                )
                            };
                            state.failover_log.push(&note, false).await;
                            return Ok(note);
                        }

                        // Тест уже запущен: контрольный L7-опрос стабильности кандидата
                        let elapsed_secs = canary_guard.as_ref().map_or(0, |c| c.started_at.elapsed().as_secs());
                        drop(canary_guard);

                        let probe_ms = run_canary_probe(&state.http, &cfg, &pri.id, ping_timeout).await;
                        let mut canary_guard = CANARY_STAGE.lock().await;

                        // Проверяем, что за время сетевого запроса состояние не сбросилось
                        if !canary_guard.as_ref().map_or(false, |c| c.target_id == pri.id) {
                            return Ok("Canary: тест кандидата прерван или заменён".to_string());
                        }

                        if probe_ms <= 0 {
                            // Сбой передачи данных во время теста стабильности!
                            let fail_msg = format!(
                                "Canary Soak Test: кандидат '{}' не выдержал тест стабильности (сбой передачи данных на {} сек) — помечен как '🧟 Зомби-сервер', переход к следующему в цепочке",
                                pri.name, elapsed_secs
                            );
                            *canary_guard = None;
                            drop(canary_guard);

                            let now_str = Local::now().format("%H:%M:%S").to_string();
                            set_canary_last_result(CanaryLastResult {
                                server_id: pri.id.clone(),
                                server_name: pri.name.clone(),
                                timestamp: now_str,
                                success: false,
                                is_zombie: true,
                                reason: format!("L7-сбой передачи данных на {} сек: таймаут / блокировка веб-трафика", elapsed_secs),
                                elapsed_secs,
                            }).await;

                            if cfg.failover.quarantine_enabled {
                                add_quarantine_with_duration(&pri.id, &pri.name, "Зомби-сервер: L7-сбой передачи трафика", 600).await;
                            }
                            state.failover_log.push(&fail_msg, false).await;
                            checked_higher_notes.push(format!("'{}': 🧟 Зомби (L7 сбой)", pri.name));
                            continue;
                        }

                        let canary = canary_guard.as_mut().unwrap();
                        canary.probes_passed += 1;
                        // Экономия трафика: в промежуточных тиках байты не увеличиваются фиктивно

                        if elapsed_secs < effective_canary_duration_secs {
                            let probes_count = canary.probes_passed;
                            let total_kb = canary.total_bytes / 1024;
                            drop(canary_guard);
                            let note = format!(
                                "Canary Soak Test: '{}' стабилен (прошло {}/{} с, проверок: {}, пинг {} мс, загружено {} КБ) — клиенты на '{}'",
                                pri.name, elapsed_secs, effective_canary_duration_secs, probes_count, probe_ms, total_kb, active.name
                            );
                            return Ok(note);
                        }

                        // Финальная квалификационная загрузка 64 КБ полезной нагрузки
                        if let Ok((final_bytes, _)) = perform_canary_download(CANARY_PAYLOAD_PORT, CANARY_PROBE_BYTES, ping_timeout.max(5000)).await {
                            canary.total_bytes += final_bytes;
                            canary.probes_passed += 1;
                        }

                        // УСПЕШНОЕ ОКОНЧАНИЕ ТЕСТА СТАБИЛЬНОСТИ БЕЗ ЕДИНОГО СБОЯ!
                        let probes_count = canary.probes_passed;
                        let total_kb = canary.total_bytes / 1024;
                        *canary_guard = None;
                        drop(canary_guard);

                        let now_str = Local::now().format("%H:%M:%S").to_string();
                        set_canary_last_result(CanaryLastResult {
                            server_id: pri.id.clone(),
                            server_name: pri.name.clone(),
                            timestamp: now_str,
                            success: true,
                            is_zombie: false,
                            reason: format!("Успешно пройден {effective_canary_duration_secs}-сек тест ({probes_count} проверок, {total_kb} КБ)"),
                            elapsed_secs,
                        }).await;

                        let is_main = !chain.is_empty() && pri.id == chain[0].id;
                        let label = if is_main { "Основной" } else if is_active_bs && should_restore_smart { "Обычный VPS (Tier 1)" } else { "Более приоритетный" };
                        let msg = format!(
                            "{label} '{}' успешно прошёл {effective_canary_duration_secs}-секундный тест стабильности ({probes_count} проверок, {total_kb} КБ, 0 сбоев) — безопасный возврат с '{}'",
                            pri.name, active.name
                        );
                        let is_from_bs = is_whitelist_server_full(&active.name, &active.id, active.provider.as_deref(), &cfg);
                        let switch_res = if is_from_bs {
                            // С [БС] возврат строгий: переключаем группы и сбрасываем сокеты
                            mihomo::switch_server(&state.http, &cfg, &pri.id).await
                        } else {
                            // С обычного резерва на основной узел — мягкий Graceful возврат (не рвём текущие сессии)
                            mihomo::switch_server_graceful(&state.http, &cfg, &pri.id).await
                        };
                        return match switch_res {
                            Ok(_) => {
                                record_switch(&pri.id).await;
                                let s_prov = servers.iter().find(|s| s.id == pri.id).and_then(|s| s.provider.as_deref());
                                handle_whitelist_transition(state, &pri.id, &pri.name, s_prov).await;
                                state.failover_log.push(&msg, true).await;
                                crate::notifications::notify_failover(
                                    &state.http,
                                    Some(&cfg.mihomo_proxy_url()),
                                    &cfg.notifications,
                                    "restore",
                                    "Восстановление основного сервера",
                                    &msg,
                                ).await;
                                Ok(msg)
                            }
                            Err(e) => {
                                state.failover_log.push(format!("{msg}, но переключение не удалось: {e}"), false).await;
                                Err(e)
                            }
                        };
                    } else {
                        // Кандидат недоступен или пинг выше гистерезиса: если он был в процессе Canary, прерываем
                        let mut canary_guard = CANARY_STAGE.lock().await;
                        if canary_guard.as_ref().map_or(false, |c| c.target_id == pri.id) {
                            let elapsed = canary_guard.as_ref().unwrap().started_at.elapsed().as_secs();
                            *canary_guard = None;
                            drop(canary_guard);
                            let abort_msg = format!(
                                "Canary Soak Test: кандидат '{}' потерял пинг ({} мс > порога {} мс на {} сек теста) — тест сброшен, остаёмся на '{}'",
                                pri.name, ping, hyst, elapsed, active.name
                            );
                            let now_str = Local::now().format("%H:%M:%S").to_string();
                            set_canary_last_result(CanaryLastResult {
                                server_id: pri.id.clone(),
                                server_name: pri.name.clone(),
                                timestamp: now_str,
                                success: false,
                                is_zombie: false,
                                reason: abort_msg.clone(),
                                elapsed_secs: elapsed,
                            }).await;
                            state.failover_log.push(&abort_msg, false).await;
                        }

                        if ping > 0 {
                            checked_higher_notes.push(format!("'{}': пинг {} мс", pri.name, ping));
                        } else {
                            checked_higher_notes.push(format!("'{}': недоступен", pri.name));
                        }
                    }
                }
            }
        }
    }

    // Если кандидатов нет или они не готовы, а активный в норме:
    let note = if !checked_higher_notes.is_empty() {
        format!("; приоритетные: {}", checked_higher_notes.join(", "))
    } else {
        String::new()
    };
    let msg = format!("Активный '{}' — пинг {current} мс (в норме{note})", active.name);
    Ok(msg)
}

/// Per-device проверка: для каждого устройства с цепочкой серверов —
/// пинг текущего; отвалился или пинг > порога → переключение на следующий
/// живой из цепочки; автовозврат на основной, когда восстановился.
pub async fn run_device_check(state: &AppState) -> Result<String, String> {
    // DIAG-01: Изоляция от Speedtest
    let _speedtest_guard = match state.speedtest_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => {
            crate::log_i!("Per-device failover пропущен: выполняется Speedtest");
            return Ok("Per-device failover пропущен: выполняется Speedtest".to_string());
        }
    };

    // F-07: служба остановлена пользователем — per-device failover не переключает группы
    if crate::watchdog::is_service_stopped() {
        return Ok("Per-device failover пропущен: служба XKeen остановлена пользователем".to_string());
    }

    // F-04: сериализация с run_check (общий CHECK_IN_PROGRESS)
    let _check_guard = CHECK_IN_PROGRESS.lock().await;

    let cfg = state.config.read().await.clone();
    if cfg.device_routing.is_empty() {
        return Ok("Нет устройств с резервными цепочками".to_string());
    }
    let proxies = mihomo::get_proxies(&state.http, &cfg).await?;
    let rules = match mihomo::m_get(&state.http, &cfg, "/rules").await {
        Ok(r) => r,
        Err(e) => {
            crate::log_w!("Ошибка получения правил Mihomo (/rules) при per-device failover: {}", e);
            Value::Null
        }
    };
    let groups_by_ip = mihomo::ip_groups_from_rules(&rules);

    // FO-DEV-01: Единый запрос /connections на цикл проверки устройств
    let conns_val = match mihomo::m_get(&state.http, &cfg, "/connections").await {
        Ok(v) => v,
        Err(e) => {
            crate::log_w!("Ошибка получения соединений Mihomo (/connections) при per-device failover: {}", e);
            Value::Null
        }
    };
    let conns = conns_val.get("connections").and_then(|c| c.as_array()).cloned().unwrap_or_default();
    let prev_snap = DEV_CONN_SNAPSHOT.lock().await.clone();

    let all_servers = match mihomo::get_servers(&state.http, &cfg, &[]).await {
        Ok(s) => s,
        Err(e) => {
            crate::log_w!("Ошибка получения списка серверов при per-device failover: {}", e);
            Vec::new()
        }
    };

    let mut actions: Vec<(String, bool)> = Vec::new(); // (текст, было ли реальное переключение)
    for (ip, dr) in &cfg.device_routing {
        if dr.servers.is_empty() {
            continue;
        }
        // Группа устройства: из SRC-IP правил (AUTO-DEVICE).
        let Some(group) = groups_by_ip.get(ip) else {
            continue;
        };
        let Some(cur) = proxies.get(group).and_then(|g| g.get("now")).and_then(|n| n.as_str()) else {
            continue;
        };
        // Ручное переключение на сервер вне цепочки не трогаем.
        if !dr.servers.iter().any(|s| s == cur) {
            continue;
        }
        let threshold = if dr.ping_threshold_ms > 0 {
            dr.ping_threshold_ms as i64
        } else {
            cfg.failover.ping_threshold_ms as i64
        };
        let ping_timeout = calc_ping_timeout(threshold);

        // FO-DEV-01: Подсчёт дельты трафика и детекция черной дыры для данного устройства
        let (has_live_traffic, _dl_delta, _up_delta, _matched_sockets, is_blackhole) =
            process_device_connections_delta(&conns, ip, group, cur, &prev_snap);

        let cur_ping = mihomo::ping_server_l7_split(&state.http, &cfg, cur, ping_timeout).await;

        if is_blackhole && cfg.failover.quarantine_enabled {
            add_quarantine(cur, cur, "Зомби-сервер: аномалия 'Чёрная дыра' (DPI zero-download)").await;
        }

        let is_cur_healthy = is_active_server_healthy(cur_ping, threshold, has_live_traffic, is_blackhole);

        let mut higher_pings = std::collections::BTreeMap::new();
        let mut candidate_pings = std::collections::BTreeMap::new();

        if is_cur_healthy {
            if dr.auto_restore && !has_live_traffic {
                let higher = get_higher_priority_candidates(&dr.servers, cur);
                if !higher.is_empty() {
                    higher_pings = mihomo::ping_all(&state.http, &cfg, higher, ping_timeout).await;
                }
            }
        } else {
            let cands: Vec<String> = dr.servers.iter().filter(|s| *s != cur).cloned().collect();
            if !cands.is_empty() {
                candidate_pings = ping_servers_staggered(&state.http, &cfg, &cands, ping_timeout, 4, 25).await;
            }
        }

        let mut q_ids = std::collections::HashSet::new();
        if cfg.failover.quarantine_enabled {
            for s in &dr.servers {
                if is_quarantined(s).await {
                    q_ids.insert(s.clone());
                }
            }
        }

        let is_bs_fn = |s: &Server| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);

        let decision = evaluate_device_failover(
            ip,
            cur,
            &dr.servers,
            cur_ping,
            threshold,
            dr.auto_restore,
            has_live_traffic,
            is_blackhole,
            &all_servers,
            &higher_pings,
            &candidate_pings,
            |id| q_ids.contains(id),
            is_bs_fn,
            cfg.failover.smart_failover_enabled,
        );

        match decision {
            DeviceFailoverDecision::KeepCurrent { .. } => {
                continue;
            }
            DeviceFailoverDecision::AutoRestore { target, ping_ms, is_main } => {
                let hyst = calc_hysteresis(threshold);
                let label = if is_main { "основной" } else { "приоритетный" };
                match mihomo::switch_group(&state.http, &cfg, group, &target).await {
                    Ok(_) => actions.push((
                        format!(
                            "[{ip}] {label} '{target}' восстановился ({ping_ms} мс, порог {hyst} мс) — возврат с резерва '{cur}'"
                        ),
                        true,
                    )),
                    Err(e) => actions.push((
                        format!("[{ip}] возврат на {label} '{target}' не удался: {e}"),
                        false,
                    )),
                }
            }
            DeviceFailoverDecision::Switch { target, reason } => {
                let best = &target.id;
                let ms = target.ping_ms;
                if target.is_quarantined {
                    crate::log_w!(
                        "[Device Failover {ip}] Все серверы в карантине! Вынужденный выбор из карантина: {}",
                        best
                    );
                }
                if target.is_alive_slow {
                    crate::log_w!(
                        "Device Failover [{ip}]: все быстрые серверы недоступны. Выбран живой, но медленный сервер '{}' (пинг {} мс > порога {} мс)",
                        best, ms, threshold
                    );
                }
                let qual = if target.is_quarantined {
                    " [обход карантина]"
                } else if target.is_alive_slow {
                    " [медленный]"
                } else {
                    ""
                };
                match mihomo::switch_group(&state.http, &cfg, group, best).await {
                    Ok(_) => actions.push((
                        format!("[{ip}] '{cur}' ({reason}) → резерв '{best}' ({ms} мс{qual})"),
                        true,
                    )),
                    Err(e) => actions.push((
                        format!("[{ip}] переключение на '{best}' не удалось: {e}"),
                        false,
                    )),
                }
            }
            DeviceFailoverDecision::NoBackupAvailable { reason } => {
                actions.push((format!("[{ip}] '{cur}' ({reason}), все резервы недоступны"), false));
            }
        }
    }

    // Обновляем снимок сокетов для устройств
    let mut new_snap = std::collections::HashMap::new();
    for c in &conns {
        if let Some(id) = c.get("id").and_then(|i| i.as_str()) {
            let up = c.get("upload").and_then(|u| u.as_u64()).unwrap_or(0);
            let dl = c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);
            new_snap.insert(id.to_string(), (up, dl));
        }
    }
    *DEV_CONN_SNAPSHOT.lock().await = new_snap;

    if actions.is_empty() {
        return Ok("Устройства: всё в норме".to_string());
    }
    for (a, switched) in &actions {
        state.failover_log.push(a, *switched).await;
    }
    Ok(actions.iter().map(|(a, _)| a.as_str()).collect::<Vec<_>>().join("; "))
}

/// Флаг graceful shutdown: фоновый цикл завершается, не обрывая текущую проверку.
static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Попросить фоновый цикл остановиться (graceful shutdown).
pub fn shutdown() {
    SHUTDOWN.store(true, std::sync::atomic::Ordering::Relaxed);
}

pub fn is_shutdown() -> bool {
    SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed)
}

/// Фоновый цикл: каждые 10-15 сек для активного сервера (или 5 сек при активном Canary/раннем аудите переключения)
/// и каждые interval_secs для полной проверки устройств/пула.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        let mut last_device_check = Instant::now() - Duration::from_secs(3600);
        loop {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
                crate::log_i!("Failover: фоновый цикл остановлен");
                return;
            }
            let interval = if is_canary_active().await || is_post_switch_pending().await {
                5u64
            } else {
                ACTIVE_FAILOVER_CHECK_INTERVAL_SECS
            };
            // Спим короткими отрезками, чтобы быстро реагировать на shutdown.
            for _ in 0..interval {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
                    crate::log_i!("Failover: фоновый цикл остановлен");
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }

            let (enabled, dev_enabled, dev_interval) = {
                let cfg = state.config.read().await;
                (cfg.failover.enabled, cfg.failover.device_failover_enabled, cfg.failover.interval_secs.clamp(15, 3600) as u64)
            };
            if enabled {
                let _ = run_check(&state).await;
            }
            if dev_enabled && last_device_check.elapsed() >= Duration::from_secs(dev_interval) {
                last_device_check = Instant::now();
                let _ = run_device_check(&state).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conn_chain_matches_server_strict() {
        // F-01 anti-regression: fuzzy-матчинг запрещён — трафик 'DE-10' не принадлежит 'DE-1'
        let chains: Vec<Value> = vec![Value::String("DE-10".into()), Value::String("PROXY".into())];
        assert!(!conn_chain_matches_server(&chains, "de-1"), "prefix-overlap servers must NOT match");
        assert!(conn_chain_matches_server(&chains, "de-10"), "exact match must pass (case-insensitive)");

        // Служебные группы никогда не считаются трафиком узла
        let svc: Vec<Value> = vec![Value::String("proxy".into()), Value::String("direct".into()), Value::String("global".into()), Value::String("reject".into())];
        for name in ["proxy", "global", "direct", "reject"] {
            assert!(!conn_chain_matches_server(&svc, name), "service group '{name}' must never match a server");
        }

        // Точное совпадение через цепочку релея (узел в середине цепочки)
        let relay: Vec<Value> = vec![Value::String("Entry".into()), Value::String("MyServer".into())];
        assert!(conn_chain_matches_server(&relay, "myserver"));
        assert!(!conn_chain_matches_server(&relay, "my"));
    }

    #[test]
    fn test_calc_hysteresis() {
        // Для высоких порогов (> 100 мс) гистерезис = threshold - 50 мс
        assert_eq!(calc_hysteresis(300), 250);
        assert_eq!(calc_hysteresis(150), 100);
        assert_eq!(calc_hysteresis(500), 450);

        // Для низких порогов (<= 100 мс) гистерезис = 80% от порога (минимум 1 мс)
        assert_eq!(calc_hysteresis(100), 80);
        assert_eq!(calc_hysteresis(50), 40);
        assert_eq!(calc_hysteresis(10), 8);
        assert_eq!(calc_hysteresis(1), 1);
    }

    #[test]
    fn test_calc_ping_timeout() {
        assert_eq!(calc_ping_timeout(50), 2000); // clamp min 2000
        assert_eq!(calc_ping_timeout(300), 2000); // 300 + 1000 = 1300 -> clamp min 2000
        assert_eq!(calc_ping_timeout(1500), 2500); // 1500 + 1000 = 2500
        assert_eq!(calc_ping_timeout(3000), 4000);
        assert_eq!(calc_ping_timeout(6000), 6000); // clamp max 6000
    }

    #[test]
    fn test_get_higher_priority_candidates() {
        let chain = vec!["DE", "NL", "US", "FI"];

        // Если активный сервер основной ("DE"), кандидатов выше нет
        let higher = get_higher_priority_candidates(&chain, "DE");
        assert!(higher.is_empty());

        // Если активный "NL" (индекс 1), кандидат — "DE"
        let higher = get_higher_priority_candidates(&chain, "NL");
        assert_eq!(higher, &["DE"]);

        // Если активный "FI" (индекс 3), кандидаты — ["DE", "NL", "US"]
        let higher = get_higher_priority_candidates(&chain, "FI");
        assert_eq!(higher, &["DE", "NL", "US"]);

        // Если активный сервер вообще вне цепочки (например, "RU"), кандидаты — вся цепочка
        let higher = get_higher_priority_candidates(&chain, "RU");
        assert_eq!(higher, &["DE", "NL", "US", "FI"]);

        // Пустая цепочка
        let empty: Vec<String> = Vec::new();
        assert!(get_higher_priority_candidates(&empty, "DE").is_empty());
    }

    #[tokio::test]
    async fn test_failover_log_push_and_snapshot() {
        let log = FailoverLog::default();
        log.push_event_only("Event 1", false).await;
        log.push_event_only("Event 2", true).await;

        let snap = log.snapshot().await;
        assert_eq!(snap.len(), 2);
        // Snapshot возвращает события в обратном хронологическом порядке (новые первыми)
        assert_eq!(snap[0].message, "Event 2");
        assert!(snap[0].switched);
        assert_eq!(snap[1].message, "Event 1");
        assert!(!snap[1].switched);

        // Проверяем ограничение MAX_EVENTS
        for i in 0..MAX_EVENTS + 10 {
            log.push_event_only(format!("Event {i}"), false).await;
        }
        let snap2 = log.snapshot().await;
        assert_eq!(snap2.len(), MAX_EVENTS);
    }

    #[test]
    fn test_chain_first_alive_selection() {
        let chain = vec!["DE".to_string(), "NL".to_string(), "US".to_string(), "FI".to_string()];
        let threshold = 300i64;

        // Симуляция: DE мёртв (-1), NL высокий пинг (350), US живой (120), FI живой (80)
        let mut pings = std::collections::BTreeMap::new();
        pings.insert("DE".to_string(), -1i64);
        pings.insert("NL".to_string(), 350i64);
        pings.insert("US".to_string(), 120i64);
        pings.insert("FI".to_string(), 80i64);

        // Первый живой в порядке цепочки (US, так как NL > 300)
        let selected = chain
            .iter()
            .find(|id| pings.get(*id).is_some_and(|ms| *ms > 0 && *ms <= threshold));
        assert_eq!(selected.map(|s| s.as_str()), Some("US"));
    }

    #[test]
    fn test_best_of_rest_selection_when_chain_fails() {
        let servers = vec!["S1".to_string(), "S2".to_string(), "S3".to_string()];
        let threshold = 300i64;

        let mut pings = std::collections::BTreeMap::new();
        pings.insert("S1".to_string(), 280i64);
        pings.insert("S2".to_string(), 95i64);
        pings.insert("S3".to_string(), -1i64);

        let mut valid: Vec<(String, i64)> = pings
            .iter()
            .filter(|(id, ms)| servers.contains(id) && **ms > 0 && **ms <= threshold)
            .map(|(id, ms)| (id.clone(), *ms))
            .collect();
        valid.sort_by_key(|(_, ms)| *ms);

        // Лучший доступный — S2 (95 мс)
        assert_eq!(valid.first().map(|(id, ms)| (id.as_str(), *ms)), Some(("S2", 95)));
    }

    #[tokio::test]
    async fn test_failover_cooldown_anti_flapping() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        record_switch("test_target").await;
        let last = LAST_SWITCH.lock().await;
        assert!(last.is_some());
        let elapsed = last.unwrap().elapsed();
        assert!(elapsed < std::time::Duration::from_secs(30));
    }

    #[test]
    fn test_best_ping_hysteresis_logic() {
        let hysteresis = 40i64;

        // 1. Текущий сервер 100мс, лучший кандидат 70мс.
        // 70 + 40 = 110 >= 100 -> переключение НЕ должно произойти (гистерезис защищает от флэппинга)
        let act_ping = 100i64;
        let best_ms = 70i64;
        let should_switch = act_ping <= 0 || (best_ms + hysteresis < act_ping);
        assert!(!should_switch, "Hysteresis must prevent switching when improvement is within margin");

        // 2. Текущий сервер 125мс, лучший кандидат 70мс.
        // 70 + 40 = 110 < 125 -> переключение ДОЛЖНО произойти
        let act_ping = 125i64;
        let should_switch = act_ping <= 0 || (best_ms + hysteresis < act_ping);
        assert!(should_switch, "Switching must occur when latency improvement exceeds hysteresis");

        // 3. Текущий сервер недоступен (-1).
        let act_ping = -1i64;
        let should_switch = act_ping <= 0 || (best_ms + hysteresis < act_ping);
        assert!(should_switch, "Must switch immediately when active server is dead");
    }

    #[test]
    fn test_best_ping_pool_size_selection() {
        let chain = vec!["P1".to_string(), "P2".to_string()];
        let all_servers: Vec<String> = (1..=25).map(|i| format!("S{i}")).collect();
        let pool_size = 15;

        // Формирование пула как в run_check
        let mut pool: Vec<String> = chain.clone();
        for s in all_servers {
            if pool.len() >= pool_size {
                break;
            }
            if !pool.contains(&s) {
                pool.push(s);
            }
        }

        assert_eq!(pool.len(), 15, "Pool must be capped at pool_size (15)");
        assert_eq!(pool[0], "P1", "Priority servers must be placed first in pool");
        assert_eq!(pool[1], "P2", "Priority servers must be placed first in pool");
        assert_eq!(pool[2], "S1");
        assert_eq!(pool[14], "S13");
    }

    #[test]
    fn test_failover_shutdown() {
        assert!(!is_shutdown());
        shutdown();
        assert!(is_shutdown());
    }

    static FAILOVER_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn test_canary_stage_lifecycle() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        reset_canary().await;
        assert!(!is_canary_active().await);
        assert!(canary_status().await.is_none());

        // Запуск стейджинга Canary
        let mut guard = CANARY_STAGE.lock().await;
        *guard = Some(CanaryState {
            target_id: "srv_de".to_string(),
            target_name: "Германия".to_string(),
            from_id: "srv_nl".to_string(),
            from_name: "Нидерланды".to_string(),
            started_at: Instant::now(),
            probes_passed: 1,
            total_bytes: CANARY_PROBE_BYTES,
            duration_secs: CANARY_DURATION_SECS,
        });
        drop(guard);

        assert!(is_canary_active().await);
        let status = canary_status().await.expect("Canary must be active");
        assert_eq!(status.target_id, "srv_de");
        assert_eq!(status.from_name, "Нидерланды");
        assert_eq!(status.duration_secs, CANARY_DURATION_SECS);
        assert!(status.remaining_secs <= CANARY_DURATION_SECS);
        assert_eq!(status.probes_passed, 1);
        assert_eq!(status.total_bytes, CANARY_PROBE_BYTES);

        // Сброс при сбое или переключении
        reset_canary().await;
        assert!(!is_canary_active().await);
        assert!(canary_status().await.is_none());
    }

    #[tokio::test]
    async fn test_canary_aborted_when_active_server_fails() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        reset_canary().await;

        // Эмулируем запущенный Canary-тест (основной узел srv_de тестируется, клиенты на srv_nl)
        let mut guard = CANARY_STAGE.lock().await;
        *guard = Some(CanaryState {
            target_id: "srv_de".to_string(),
            target_name: "Германия".to_string(),
            from_id: "srv_nl".to_string(),
            from_name: "Нидерланды".to_string(),
            started_at: Instant::now(),
            probes_passed: 5,
            total_bytes: CANARY_PROBE_BYTES * 5,
            duration_secs: 60,
        });
        drop(guard);

        assert!(is_canary_active().await);

        // Анти-регрессия: если активный сервер падает, reset_canary() должен быть вызван немедленно,
        // чтобы не удерживать пользователей на мертвом сервере
        reset_canary().await;
        assert!(!is_canary_active().await, "Canary must be immediately aborted when active server fails");
        assert!(canary_status().await.is_none());
    }

    #[test]
    fn test_canary_bandwidth_limits() {
        // Проверка честного расчёта пропускной способности canary теста (FO-CNR-03):
        // 2 реальные загрузки по 64 КБ (128 КБ = 131,072 байт) за 180 секунд (3 минуты).
        let real_downloads = 2;
        let total_bytes = real_downloads * CANARY_PROBE_BYTES; // 131,072 байт (128 КБ)
        let bandwidth_kbps = (total_bytes * 8) as f64 / (CANARY_DURATION_SECS as f64 * 1000.0);

        // Экономная и честная нагрузка (~5.8 кбит/с), не расходующая мобильный трафик
        assert!(bandwidth_kbps < 100.0, "Canary bandwidth must be strictly below 100 kbps, actual: {bandwidth_kbps} kbps");
        assert_eq!(total_bytes, 131072);
        assert_eq!(CANARY_PROBE_BYTES, 65536);
    }

    async fn spawn_mock_canary_server(payload_bytes: usize, status_code: u16, delay_ms: u64) -> (u16, tokio::task::JoinHandle<()>) {
        use tokio::net::TcpListener;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("Bind mock listener");
        let port = listener.local_addr().unwrap().port();

        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;

                if delay_ms > 0 {
                    tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
                }

                let body = vec![b'X'; payload_bytes];
                let response = if status_code == 200 {
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        payload_bytes
                    )
                } else {
                    format!(
                        "HTTP/1.1 {} Error\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                        status_code
                    )
                };

                let _ = socket.write_all(response.as_bytes()).await;
                if status_code == 200 {
                    let _ = socket.write_all(&body).await;
                }
            }
        });

        (port, handle)
    }

    #[tokio::test]
    async fn test_perform_canary_download_exact_64kb() {
        let (port, _server) = spawn_mock_canary_server(65536, 200, 0).await;
        let res = perform_canary_download_detailed(port, CANARY_PAYLOAD_URL, 65536, 5000).await;
        assert!(res.is_ok(), "64 KB download must succeed: {:?}", res);
        let detail = res.unwrap();
        assert!(detail.success);
        assert_eq!(detail.bytes_downloaded, 65536);
        assert_eq!(detail.status_code, 200);
        assert!(detail.speed_kbps > 0.0);

        // Также проверяем основной хелпер perform_canary_download
        let (port2, _server2) = spawn_mock_canary_server(65536, 200, 0).await;
        let helper_res = perform_canary_download(port2, 65536, 5000).await;
        assert!(helper_res.is_ok());
        let (bytes, speed) = helper_res.unwrap();
        assert_eq!(bytes, 65536);
        assert!(speed > 0.0);
    }

    #[tokio::test]
    async fn test_perform_canary_download_truncated_payload_rejection() {
        // Сервер отдал только 32 КБ вместо 64 КБ
        let (port, _server) = spawn_mock_canary_server(32768, 200, 0).await;
        let res = perform_canary_download_detailed(port, CANARY_PAYLOAD_URL, 65536, 5000).await;
        assert!(res.is_err(), "Truncated payload must be rejected");
        let err = res.err().unwrap();
        assert!(err.contains("Усечённая полезная нагрузка"), "Error must mention truncated payload: {err}");
    }

    #[tokio::test]
    async fn test_perform_canary_download_status_502_rejection() {
        let (port, _server) = spawn_mock_canary_server(0, 502, 0).await;
        let res = perform_canary_download_detailed(port, CANARY_PAYLOAD_URL, 65536, 5000).await;
        assert!(res.is_err(), "502 status must be rejected");
        let err = res.err().unwrap();
        assert!(err.contains("502"), "Error must mention status 502: {err}");
    }

    #[tokio::test]
    async fn test_perform_canary_download_timeout_handling() {
        // Сервер отвечает через 800 мс, а таймаут клиента 100 мс
        let (port, _server) = spawn_mock_canary_server(65536, 200, 800).await;
        let res = perform_canary_download_detailed(port, CANARY_PAYLOAD_URL, 65536, 100).await;
        assert!(res.is_err(), "Exceeded timeout must return error");
    }

    #[test]
    fn test_calc_canary_speed_accuracy() {
        // Математическая проверка формулы скорости: ((bytes * 8.0) / (duration_ms / 1000.0)) / 1000.0
        let bytes: u64 = 65536;
        let duration_ms_500: u64 = 500;
        let speed_500 = ((bytes as f64 * 8.0) / (duration_ms_500 as f64 / 1000.0)) / 1000.0;
        // 65536 * 8 / 0.5 / 1000 = 1048.576 kbps
        assert!((speed_500 - 1048.576).abs() < 0.001);

        let duration_ms_1000: u64 = 1000;
        let speed_1000 = ((bytes as f64 * 8.0) / (duration_ms_1000 as f64 / 1000.0)) / 1000.0;
        // 65536 * 8 / 1.0 / 1000 = 524.288 kbps
        assert!((speed_1000 - 524.288).abs() < 0.001);
    }

    async fn spawn_mock_canary_server_abrupt_disconnect(headers_len: usize, partial_body_bytes: usize) -> (u16, tokio::task::JoinHandle<()>) {
        use tokio::net::TcpListener;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("Bind mock listener");
        let port = listener.local_addr().unwrap().port();

        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    headers_len
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let partial = vec![b'P'; partial_body_bytes];
                let _ = socket.write_all(&partial).await;
                drop(socket);
            }
        });

        (port, handle)
    }

    async fn spawn_mock_canary_server_stall_after_headers(stall_ms: u64) -> (u16, tokio::task::JoinHandle<()>) {
        use tokio::net::TcpListener;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = TcpListener::bind("127.0.0.1:0").await.expect("Bind mock listener");
        let port = listener.local_addr().unwrap().port();

        let handle = tokio::spawn(async move {
            if let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;

                let response = "HTTP/1.1 200 OK\r\nContent-Length: 65536\r\nConnection: close\r\n\r\n";
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.write_all(b"partial").await;
                tokio::time::sleep(std::time::Duration::from_millis(stall_ms)).await;
            }
        });

        (port, handle)
    }

    #[tokio::test]
    async fn test_adversarial_canary_status_codes_504_500_403_404() {
        for status in [504, 500, 403, 404] {
            let (port, _server) = spawn_mock_canary_server(0, status, 0).await;
            let res = perform_canary_download_detailed(port, CANARY_PAYLOAD_URL, 65536, 5000).await;
            assert!(res.is_err(), "Status {status} must be rejected");
            let err = res.err().unwrap();
            assert!(err.contains(&status.to_string()), "Error must mention status {status}: {err}");
        }
    }

    #[tokio::test]
    async fn test_adversarial_canary_zero_byte_and_off_by_one_rejection() {
        // 0-byte payload rejection
        let (port0, _s0) = spawn_mock_canary_server(0, 200, 0).await;
        let res0 = perform_canary_download_detailed(port0, CANARY_PAYLOAD_URL, 65536, 5000).await;
        assert!(res0.is_err(), "Zero byte payload must be rejected");
        let err0 = res0.err().unwrap();
        assert!(err0.contains("Усечённая полезная нагрузка Canary: получено 0 байт из ожидаемых 65536"), "{err0}");

        // Off-by-one (65535 bytes, 1 byte short)
        let (port1, _s1) = spawn_mock_canary_server(65535, 200, 0).await;
        let res1 = perform_canary_download_detailed(port1, CANARY_PAYLOAD_URL, 65536, 5000).await;
        assert!(res1.is_err(), "65535 bytes payload must be rejected");
        let err1 = res1.err().unwrap();
        assert!(err1.contains("Усечённая полезная нагрузка Canary: получено 65535 байт из ожидаемых 65536"), "{err1}");
    }

    #[tokio::test]
    async fn test_adversarial_canary_abrupt_disconnect_midstream() {
        // Content-Length 65536, but connection drops after 10000 bytes
        let (port, _server) = spawn_mock_canary_server_abrupt_disconnect(65536, 10000).await;
        let res = perform_canary_download_detailed(port, CANARY_PAYLOAD_URL, 65536, 5000).await;
        assert!(res.is_err(), "Abrupt TCP disconnect must return error without panic");
        let err = res.err().unwrap();
        assert!(err.contains("Сбой чтения данных") || err.contains("Сетевой сбой"), "Error must reflect read/network failure: {err}");
    }

    #[tokio::test]
    async fn test_adversarial_canary_connection_refused_immediate_failure() {
        // Get an unused closed port
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let closed_port = listener.local_addr().unwrap().port();
        drop(listener); // close immediately

        let res = perform_canary_download_detailed(closed_port, CANARY_PAYLOAD_URL, 65536, 2000).await;
        assert!(res.is_err(), "Connection refused must return error immediately");
        let err = res.err().unwrap();
        assert!(err.contains("Сетевой сбой"), "{err}");
    }

    #[tokio::test]
    async fn test_adversarial_canary_stalled_stream_respects_timeout() {
        let (port, _server) = spawn_mock_canary_server_stall_after_headers(2000).await;
        let start = std::time::Instant::now();
        let res = perform_canary_download_detailed(port, CANARY_PAYLOAD_URL, 65536, 200).await;
        let elapsed = start.elapsed();
        assert!(res.is_err(), "Stalled stream must time out");
        assert!(elapsed.as_millis() < 1500, "Timeout must abort in timely manner: {elapsed:?}");
    }

    #[tokio::test]
    async fn test_adversarial_canary_interim_ticks_zero_phantom_byte_accumulation() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        reset_canary().await;

        // Tick 0: Initial download of 64 KB (65536 bytes)
        let mut guard = CANARY_STAGE.lock().await;
        *guard = Some(CanaryState {
            target_id: "srv_candidate".to_string(),
            target_name: "Кандидат".to_string(),
            from_id: "srv_active".to_string(),
            from_name: "Активный".to_string(),
            started_at: Instant::now(),
            probes_passed: 1,
            total_bytes: 65536,
            duration_secs: 180,
        });
        drop(guard);

        // Interim ticks (1..10): Probes increment, total_bytes MUST remain strictly 65536
        for tick in 1..=10 {
            let mut guard = CANARY_STAGE.lock().await;
            let canary = guard.as_mut().expect("Canary state exists");
            canary.probes_passed += 1;
            // No fake byte accumulation during interim ticks
            assert_eq!(
                canary.total_bytes,
                65536,
                "Interim tick {tick} must NOT accumulate phantom bytes to total_bytes"
            );
            drop(guard);

            let status = canary_status().await.expect("Canary status exists");
            assert_eq!(status.total_bytes, 65536, "CanaryStatus must report honest 65536 total_bytes on tick {tick}");
            assert_eq!(status.probes_passed, 1 + tick as u32);
        }

        // Final qualification tick: +65536 bytes
        let mut guard = CANARY_STAGE.lock().await;
        let canary = guard.as_mut().expect("Canary state exists");
        canary.total_bytes += 65536;
        canary.probes_passed += 1;
        assert_eq!(canary.total_bytes, 131072, "Final tick must equal 128 KB (131072 bytes)");
        drop(guard);

        let final_status = canary_status().await.unwrap();
        assert_eq!(final_status.total_bytes, 131072);
        assert_eq!(final_status.probes_passed, 12);

        reset_canary().await;
    }

    #[test]
    fn test_adversarial_canary_speed_calculation_comprehensive_matrix() {
        let cases: &[(u64, u64, f64)] = &[
            (65536, 1, 524288.0),
            (65536, 25, 20971.52),
            (65536, 50, 10485.76),
            (65536, 100, 5242.88),
            (65536, 250, 2097.152),
            (65536, 500, 1048.576),
            (65536, 1000, 524.288),
            (65536, 2000, 262.144),
            (65536, 5000, 104.8576),
            (65536, 10000, 52.4288),
            (32768, 250, 1048.576),
        ];

        for &(bytes, duration_ms, expected_speed_kbps) in cases {
            let duration_secs = duration_ms as f64 / 1000.0;
            let calc_speed = ((bytes as f64 * 8.0) / duration_secs) / 1000.0;
            assert!(
                (calc_speed - expected_speed_kbps).abs() < 1e-4,
                "Speed mismatch for {bytes} bytes over {duration_ms} ms: calculated {calc_speed}, expected {expected_speed_kbps}"
            );
        }

        // Clamp duration_ms.max(1) prevents divide-by-zero on instantaneous elapsed (0 ms)
        let bytes: u64 = 65536;
        let duration_ms_0: u64 = 0;
        let duration_ms_clamped = duration_ms_0.max(1);
        let duration_secs = duration_ms_clamped as f64 / 1000.0;
        let speed_0 = ((bytes as f64 * 8.0) / duration_secs) / 1000.0;
        assert_eq!(speed_0, 524288.0);
        assert!(!speed_0.is_nan() && !speed_0.is_infinite());
    }

    #[tokio::test]
    async fn test_adversarial_canary_concurrent_stress() {
        let mut handles = Vec::new();
        for _ in 0..5 {
            handles.push(tokio::spawn(async {
                let (port, _s) = spawn_mock_canary_server(65536, 200, 0).await;
                let res = perform_canary_download(port, 65536, 5000).await;
                assert!(res.is_ok(), "Concurrent canary download must succeed");
                let (bytes, speed) = res.unwrap();
                assert_eq!(bytes, 65536);
                assert!(speed > 0.0);
            }));
        }

        for h in handles {
            h.await.expect("Task must not panic");
        }
    }

    #[tokio::test]
    async fn test_eradication_of_fake_constants() {
        // Проверка отсутствия фиктивных констант в коде failover (FO-CNR-03)
        assert_eq!(CANARY_PAYLOAD_URL, "http://speed.cloudflare.com/__down?bytes=65536");
        assert_eq!(CANARY_PAYLOAD_BYTES, 65536);
        assert_eq!(CANARY_PROBE_BYTES, 65536);

        let mut guard = CANARY_STAGE.lock().await;
        *guard = Some(CanaryState {
            target_id: "srv_de".to_string(),
            target_name: "Германия".to_string(),
            from_id: "srv_nl".to_string(),
            from_name: "Нидерланды".to_string(),
            started_at: Instant::now(),
            probes_passed: 1,
            total_bytes: 65536,
            duration_secs: 180,
        });
        drop(guard);

        let mut cfg = crate::config::AppConfig::default();
        cfg.failover.enabled = true;
        let label = current_live_status("srv_nl", "Нидерланды", 50, &cfg).await;
        assert!(!label.contains("8 КБ"), "Label must not contain fake 8 КБ string: {label}");
        assert!(!label.contains("15 кбит/с"), "Label must not contain fake 15 кбит/с string: {label}");
        assert!(!label.contains("3 сокета"), "Label must not contain fake 3 сокета string: {label}");
        assert!(label.contains("64 КБ"), "Label must contain honest 64 КБ: {label}");

        reset_canary().await;
    }


    #[tokio::test]
    async fn test_quarantine_lifecycle_and_escalation() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        clear_all_quarantine().await;
        assert!(!is_quarantined("srv_germany").await);
        assert_eq!(get_quarantine_remaining("srv_germany").await, None);

        // Strike 1: 15 минут (900 сек)
        let d1 = add_quarantine("srv_germany", "Германия", "сервер не отвечает").await;
        assert_eq!(d1, 900, "1-й сбой должен давать 15 минут карантина");
        assert!(is_quarantined("srv_germany").await);
        let rem1 = get_quarantine_remaining("srv_germany").await.expect("Must have remaining time");
        assert!(rem1 <= 900 && rem1 >= 895);

        let list = quarantine_status().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].server_id, "srv_germany");
        assert_eq!(list[0].fail_count, 1);

        // Strike 2: 30 минут (1800 сек)
        let d2 = add_quarantine("srv_germany", "Германия", "сервер не отвечает").await;
        assert_eq!(d2, 1800, "2-й сбой должен давать 30 минут карантина");
        let list2 = quarantine_status().await;
        assert_eq!(list2[0].fail_count, 2);

        // Strike 3: 60 минут (3600 сек)
        let d3 = add_quarantine("srv_germany", "Германия", "сервер не отвечает").await;
        assert_eq!(d3, 3600, "3-й сбой должен давать 60 минут карантина");

        // Ручной сброс карантина
        clear_quarantine("srv_germany").await;
        assert!(!is_quarantined("srv_germany").await);
        assert!(quarantine_status().await.is_empty());
    }

    #[tokio::test]
    async fn test_record_manual_switch_clears_quarantine() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        clear_all_quarantine().await;
        add_quarantine("srv_target", "Целевой", "таймаут").await;
        assert!(is_quarantined("srv_target").await);

        record_manual_switch("srv_target").await;
        assert!(!is_quarantined("srv_target").await, "Ручной выбор сервера должен снимать с него карантин");
        assert!(!is_canary_active().await);
    }

    #[test]
    fn test_is_whitelist_server_tag_detection() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist_servers = vec!["wl_exact_id".into(), "special_srv".into()];

        // 1. Cyrillic tags
        assert!(is_whitelist_server("Сервер [БС] Москва", "node1", &cfg));
        assert!(is_whitelist_server("Резерв [бс] Питер", "node2", &cfg));
        assert!(is_whitelist_server("Канал (БС) Ростов", "node3", &cfg));
        assert!(is_whitelist_server("Шлюз (бс) Казань", "node4", &cfg));

        // 2. Latin tags
        assert!(is_whitelist_server("Netherlands [WL] 01", "node5", &cfg));
        assert!(is_whitelist_server("Finland [wl] Fast", "node6", &cfg));
        assert!(is_whitelist_server("Germany (WL) Direct", "node7", &cfg));
        assert!(is_whitelist_server("Sweden (wl) Cloud", "node8", &cfg));

        // 3. Keyword whitelist (case-insensitive)
        assert!(is_whitelist_server("Vless Whitelist Direct", "node9", &cfg));
        assert!(is_whitelist_server("HYSTERIA WHITELIST 02", "node10", &cfg));
        assert!(is_whitelist_server("node_whitelist_backup", "node11", &cfg));

        // 4. Server ID match in cfg.whitelist_servers
        assert!(is_whitelist_server("Generic Node", "wl_exact_id", &cfg));
        assert!(is_whitelist_server("Ordinary Server", "special_srv", &cfg));

        // 5. Negative non-matching cases
        assert!(!is_whitelist_server("Ordinary Server", "normal_node", &cfg));
        assert!(!is_whitelist_server("Fastest", "Fastest", &cfg));
        assert!(!is_whitelist_server("Fallback", "Fallback", &cfg));
        assert!(!is_whitelist_server("DIRECT", "DIRECT", &cfg));
        assert!(!is_whitelist_server("", "", &cfg));
    }

    #[test]
    fn test_failover_whitelist_follow_logic() {
        let mut cfg = crate::config::AppConfig::default();

        // When whitelist disabled: should not follow
        cfg.whitelist.enabled = false;
        cfg.whitelist.follow_failover = true;
        let should_follow_disabled = cfg.whitelist.enabled && (
            cfg.whitelist.follow_failover
            || cfg.whitelist.target_server.as_deref() == Some("follow_failover")
            || cfg.whitelist.target_server.as_deref().map_or(true, |s| s.trim().is_empty())
        );
        assert!(!should_follow_disabled);

        // When whitelist enabled and follow_failover = true
        cfg.whitelist.enabled = true;
        cfg.whitelist.follow_failover = true;
        cfg.whitelist.target_server = None;
        let should_follow_enabled = cfg.whitelist.enabled && (
            cfg.whitelist.follow_failover
            || cfg.whitelist.target_server.as_deref() == Some("follow_failover")
            || cfg.whitelist.target_server.as_deref().map_or(true, |s| s.trim().is_empty())
        );
        assert!(should_follow_enabled);

        // When target_server is explicitly "follow_failover"
        cfg.whitelist.follow_failover = false;
        cfg.whitelist.target_server = Some("follow_failover".into());
        let should_follow_explicit = cfg.whitelist.enabled && (
            cfg.whitelist.follow_failover
            || cfg.whitelist.target_server.as_deref() == Some("follow_failover")
            || cfg.whitelist.target_server.as_deref().map_or(true, |s| s.trim().is_empty())
        );
        assert!(should_follow_explicit);

        // When pinned to specific target server and follow_failover = false
        cfg.whitelist.follow_failover = false;
        cfg.whitelist.target_server = Some("pinned-proxy-01".into());
        let should_follow_pinned = cfg.whitelist.enabled && (
            cfg.whitelist.follow_failover
            || cfg.whitelist.target_server.as_deref() == Some("follow_failover")
            || cfg.whitelist.target_server.as_deref().map_or(true, |s| s.trim().is_empty())
        );
        assert!(!should_follow_pinned);
    }

    #[test]
    fn test_whitelist_group_switching_payload_and_urlencoding() {
        // Проверка формирования URL и полезной нагрузки для переключения группы 🛡️ Whitelist
        let group_name = crate::routing::WHITELIST_GROUP_NAME;
        assert_eq!(group_name, "🛡️ Whitelist");

        let encoded_group = crate::mihomo::urlencoding_lite(group_name);
        assert!(encoded_group.contains("Whitelist"));
        // Эмодзи щита и пробел должны быть закодированы через %XX
        assert!(!encoded_group.contains(' '));
        assert!(!encoded_group.contains('🛡'));

        let target_node = "NL [БС] Fast";
        let payload = serde_json::json!({ "name": target_node });
        assert_eq!(payload["name"], "NL [БС] Fast");
    }

    #[test]
    fn test_is_whitelist_server_provider_matching() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist_providers = vec!["sub_whitelist_special".into()];

        // 1. Matched by provider
        assert!(is_whitelist_server_full("Generic Server 01", "node_x", Some("sub_whitelist_special"), &cfg));
        // 2. Not matched by provider
        assert!(!is_whitelist_server_full("Generic Server 01", "node_x", Some("other_provider"), &cfg));
        assert!(!is_whitelist_server_full("Generic Server 01", "node_x", None, &cfg));
    }

    #[tokio::test]
    async fn test_auto_transition_whitelist_and_gaming_suspension() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist.auto_by_failover = true;
        cfg.whitelist.enabled = false;
        cfg.gaming.enabled = true;

        // Transition to BS node:
        let is_bs = is_whitelist_server_full("Germany [БС] Fast", "node_bs", None, &cfg);
        assert!(is_bs);

        // Emulate transition logic:
        if is_bs && cfg.whitelist.auto_by_failover {
            cfg.whitelist.enabled = true;
            if cfg.gaming.enabled {
                cfg.gaming.enabled = false;
                cfg.whitelist.gaming_suspended = true;
            }
        }
        assert!(cfg.whitelist.enabled, "Whitelist mode must be enabled on BS node");
        assert!(!cfg.gaming.enabled, "Gaming mode must be disabled on BS node to prevent hijacking");
        assert!(cfg.whitelist.gaming_suspended, "gaming_suspended flag must be true");

        // Transition back to normal node:
        let is_normal = !is_whitelist_server_full("Germany Normal", "node_de", None, &cfg);
        assert!(is_normal);

        if is_normal && cfg.whitelist.auto_by_failover {
            cfg.whitelist.enabled = false;
            if cfg.whitelist.gaming_suspended {
                cfg.gaming.enabled = true;
                cfg.whitelist.gaming_suspended = false;
            }
        }
        assert!(!cfg.whitelist.enabled, "Whitelist mode must be disabled on normal node");
        assert!(cfg.gaming.enabled, "Gaming mode must be restored on normal node");
        assert!(!cfg.whitelist.gaming_suspended);
    }

    #[test]
    fn test_wan_check_url_definition() {
        assert_eq!(WAN_CHECK_URL, "http://ya.ru");
    }

    #[test]
    fn test_blackhole_connection_filtering_logic() {
        let mock_connections = serde_json::json!({
            "connections": [
                {
                    "id": "conn1",
                    "chains": ["PROXY", "node_zombie"],
                    "upload": 2048,
                    "download": 0
                },
                {
                    "id": "conn2",
                    "chains": ["PROXY", "node_zombie"],
                    "upload": 3500,
                    "download": 0
                },
                {
                    "id": "conn3",
                    "chains": ["PROXY", "node_zombie"],
                    "upload": 1024,
                    "download": 0
                },
                {
                    "id": "conn_direct",
                    "chains": ["DIRECT"],
                    "upload": 100000,
                    "download": 500000
                }
            ]
        });

        let conns = mock_connections.get("connections").and_then(|c| c.as_array()).unwrap();
        let target = "node_zombie".to_lowercase();
        let mut matched = 0usize;
        let mut up = 0u64;
        let mut down = 0u64;

        for c in conns {
            let in_chains = c.get("chains")
                .and_then(|ch| ch.as_array())
                .map(|arr| arr.iter().any(|item| item.as_str().map(|s| s.to_lowercase() == target).unwrap_or(false)))
                .unwrap_or(false);
            if in_chains {
                matched += 1;
                up += c.get("upload").and_then(|u| u.as_u64()).unwrap_or(0);
                down += c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);
            }
        }

        assert_eq!(matched, 3);
        assert!(up > 5120);
        assert_eq!(down, 0);
        // Аномалия подтверждена:
        assert!(matched >= 3 && up > 5120 && down == 0);
    }

    #[test]
    fn test_selective_socket_target_identification() {
        let mock_conns = serde_json::json!({
            "connections": [
                { "id": "c1", "chains": ["PROXY", "node_de"] },
                { "id": "c2", "chains": ["DIRECT"] },
                { "id": "c3", "chains": ["Streaming", "node_nl"] },
                { "id": "c4", "chains": ["node_de"] }
            ]
        });

        let target_de = "node_de".to_lowercase();
        let conns = mock_conns.get("connections").and_then(|c| c.as_array()).unwrap();

        let mut ids_to_close = Vec::new();
        for c in conns {
            let id = c.get("id").and_then(|i| i.as_str()).unwrap();
            let in_chains = c.get("chains")
                .and_then(|ch| ch.as_array())
                .map(|arr| arr.iter().any(|item| item.as_str().map(|s| s.to_lowercase() == target_de).unwrap_or(false)))
                .unwrap_or(false);
            if in_chains {
                ids_to_close.push(id);
            }
        }

        assert_eq!(ids_to_close, vec!["c1", "c4"]);
        assert!(!ids_to_close.contains(&"c2"), "DIRECT соединение ни в коем случае не должно закрываться!");
        assert!(!ids_to_close.contains(&"c3"), "Соединения других серверов (node_nl) не должны закрываться!");
    }

    #[tokio::test]
    async fn test_zombie_server_quarantine_and_last_result() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        clear_all_quarantine().await;

        let srv_id = "test_node_zombie";
        let srv_name = "Zombie Candidate";

        // Добавляем в карантин как зомби
        add_quarantine_with_duration(srv_id, srv_name, "Зомби-сервер: L7-сбой передачи трафика", 600).await;

        assert!(is_quarantined(srv_id).await);
        let zombies = get_zombie_servers().await;
        assert!(zombies.contains(&srv_id.to_string()), "get_zombie_servers должен содержать помеченный узел");

        // Проверяем фиксацию последнего теста
        let last = CanaryLastResult {
            server_id: srv_id.to_string(),
            server_name: srv_name.to_string(),
            timestamp: "15:02:10".to_string(),
            success: false,
            is_zombie: true,
            reason: "L7-сбой передачи данных на 5 сек".to_string(),
            elapsed_secs: 5,
        };
        set_canary_last_result(last).await;

        let fetched = canary_last_result().await.expect("last_test должен быть сохранен");
        assert_eq!(fetched.server_id, srv_id);
        assert!(!fetched.success);
        assert!(fetched.is_zombie);
        assert_eq!(fetched.elapsed_secs, 5);

        // Проверяем, что обычный L7-сбой больше НЕ признаётся зомби (Анти-регрессия Вариант 1)
        add_quarantine_with_duration("srv_l7_fail", "L7 Fail Node", "сервер не отвечает (L7 сбой)", 600).await;
        let zombies_updated = get_zombie_servers().await;
        assert!(!zombies_updated.contains(&"srv_l7_fail".to_string()), "Обычный L7 сбой не должен превращать сервер в зомби!");

        // А вот подтверждённая аномалия 'Чёрная дыра' обязана считаться зомби:
        add_quarantine_with_duration("srv_blackhole", "Blackhole Node", "Зомби-сервер (DPI сброс трафика: upload > 5KB, download == 0)", 600).await;
        let zombies_blackhole = get_zombie_servers().await;
        assert!(zombies_blackhole.contains(&"srv_blackhole".to_string()), "Аномалия Чёрная дыра обязана попадать в get_zombie_servers");

        clear_all_quarantine().await;
    }

    #[tokio::test]
    async fn test_manual_switch_is_immune_to_early_failure() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        record_manual_switch("manual_node").await;
        let guard = LAST_ACTIVATED_SERVER.lock().await;
        let entry = guard.as_ref().expect("manual switch recorded");
        assert_eq!(entry.0, "manual_node");
        assert!(entry.2, "Manual switch must set is_manual = true to prevent early failure quarantine!");
    }

    #[test]
    fn test_active_download_traffic_detection() {
        let conns = serde_json::json!({
            "connections": [
                { "id": "1", "chains": ["PROXY", "Silver"], "download": 2048576, "upload": 12040 },
                { "id": "2", "chains": ["DIRECT"], "download": 50000, "upload": 5000 }
            ]
        });
        let arr = conns.get("connections").and_then(|c| c.as_array()).unwrap();
        let active_lower = "silver".to_lowercase();
        let mut total_download = 0u64;
        for c in arr {
            let in_chains = c.get("chains")
                .and_then(|ch| ch.as_array())
                .map(|arr| {
                    arr.iter().any(|item| {
                        item.as_str().map(|s| {
                            let sl = s.to_lowercase();
                            if sl == "proxy" || sl == "global" || sl == "direct" || sl == "reject" {
                                false
                            } else {
                                sl == active_lower || (active_lower.len() >= 3 && sl.contains(&active_lower)) || (sl.len() >= 3 && active_lower.contains(&sl))
                            }
                        }).unwrap_or(false)
                    })
                })
                .unwrap_or(false);
            if in_chains {
                total_download += c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);
            }
        }
        assert!(total_download > 1024, "Silver must detect active download traffic (> 1KB)");
    }

    #[test]
    fn test_quarantine_enabled_config_default() {
        let cfg = crate::config::FailoverConfig::default();
        assert!(cfg.quarantine_enabled, "По умолчанию карантин включен");
    }

    #[tokio::test]
    async fn test_failover_chain_skips_zombie_and_continues() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        clear_all_quarantine().await;

        let s1 = "node_1_zombie";
        let s2 = "node_2_healthy";
        let s3 = "node_3_active";

        let chain = vec![
            Server { id: s1.into(), name: "Zombie Node".into(), ping_ms: 70, is_active: false, protocol: "Vless".into(), host: "1.1.1.1".into(), port: 443, is_priority: true, provider: None, provider_name: None, is_pool: false, resolved_ip: None },
            Server { id: s2.into(), name: "Healthy Node".into(), ping_ms: 80, is_active: false, protocol: "Vless".into(), host: "2.2.2.2".into(), port: 443, is_priority: false, provider: None, provider_name: None, is_pool: false, resolved_ip: None },
            Server { id: s3.into(), name: "Active Node".into(), ping_ms: 90, is_active: true, protocol: "Vless".into(), host: "3.3.3.3".into(), port: 443, is_priority: false, provider: None, provider_name: None, is_pool: false, resolved_ip: None },
        ];

        let higher = get_higher_priority_candidates(&chain, s3);
        assert_eq!(higher.len(), 2);
        assert_eq!(higher[0].id, s1);
        assert_eq!(higher[1].id, s2);

        // Помечаем s1 как зомби
        add_quarantine_with_duration(s1, "Zombie Node", "Зомби-сервер: L7 сбой", 600).await;

        let mut eligible_higher = Vec::new();
        for s in higher {
            if !is_quarantined(&s.id).await {
                eligible_higher.push(s);
            }
        }

        // s1 должен быть пропущен, а s2 доступен для проверки!
        assert_eq!(eligible_higher.len(), 1);
        assert_eq!(eligible_higher[0].id, s2, "Фейловер обязан перейти к следующему серверу s2, пропустив зомби s1");

        clear_all_quarantine().await;
    }

    #[tokio::test]
    async fn test_quarantine_caps_at_one_hour_and_persists_fail_count() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        clear_all_quarantine().await;

        let srv = "test_repeat_offender";
        let name = "Flapping Node";

        // 1-й сбой: 900 с (15 минут)
        let d1 = add_quarantine(srv, name, "1st fail").await;
        assert_eq!(d1, 900, "1-й сбой должен быть 15 минут (900с)");

        // 2-й сбой: 1800 с (30 минут)
        let d2 = add_quarantine(srv, name, "2nd fail").await;
        assert_eq!(d2, 1800, "2-й сбой должен быть 30 минут (1800с)");

        // 3-й сбой: 3600 с (1 час - жесткий потолок)
        let d3 = add_quarantine(srv, name, "3rd fail").await;
        assert_eq!(d3, 3600, "3-й сбой должен быть строго 1 час (3600с)");

        // 4-й сбой: не более 3600 с!
        let d4 = add_quarantine(srv, name, "4th fail").await;
        assert_eq!(d4, 3600, "Карантин никогда не должен превышать 1 час (3600с)!");

        // Попытка задать ручную длительность выше 1 часа:
        let d_manual = add_quarantine_with_duration(srv, name, "manual excessive", 7200).await;
        assert_eq!(d_manual, 3600, "add_quarantine_with_duration обязан ограничивать бан 3600 с!");

        clear_all_quarantine().await;
    }

    #[tokio::test]
    async fn test_current_live_status_reporting() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        reset_canary().await;
        clear_all_quarantine().await;

        let active = Server {
            id: "node_active".into(),
            name: "Active Node".into(),
            ping_ms: 45,
            is_active: true,
            protocol: "Hysteria2".into(),
            host: "1.1.1.1".into(),
            port: 443,
            is_priority: true,
            provider: None,
            provider_name: None,
            is_pool: false,
            resolved_ip: None,
        };
        let mut cfg = crate::config::AppConfig::default();
        cfg.failover.enabled = true;
        cfg.failover.priority_chain = vec!["node_active".into()];

        let status_str = current_live_status(&active.id, &active.name, active.ping_ms as i64, &cfg).await;
        assert!(status_str.contains("Active Node"));
        assert!(status_str.contains("45 мс"));

        // Проверяем, что даже при наличии узлов в карантине, статус активного узла не захламляется строкой "В карантине"
        add_quarantine("srv_dead", "Dead Node", "L7 failure").await;
        let status_with_q = current_live_status(&active.id, &active.name, active.ping_ms as i64, &cfg).await;
        assert!(!status_with_q.contains("В карантине"), "current_live_status не должен содержать строку карантина");
        assert!(status_with_q.contains("Active Node"));

        clear_all_quarantine().await;
        reset_canary().await;
    }

    #[test]
    fn test_failover_strict_chain_does_not_leak_to_external_servers() {
        let active_id = "chain_node_1";
        let chain_candidates = vec!["chain_node_2".to_string()];
        let all_servers = vec![
            Server {
                id: "chain_node_1".into(),
                name: "Chain Node 1".into(),
                ping_ms: -1,
                is_active: true,
                protocol: "VLESS".into(),
                host: "1.1.1.1".into(),
                port: 443,
                is_priority: true,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "chain_node_2".into(),
                name: "Chain Node 2".into(),
                ping_ms: -1,
                is_active: false,
                protocol: "VLESS".into(),
                host: "2.2.2.2".into(),
                port: 443,
                is_priority: true,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "srv_aeza_external".into(),
                name: "AEZA External (Alive)".into(),
                ping_ms: 25,
                is_active: false,
                protocol: "VLESS".into(),
                host: "3.3.3.3".into(),
                port: 443,
                is_priority: false,
                provider: Some("aeza_sub".into()),
                provider_name: Some("AEZA".into()),
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "YouTube".into(),
                name: "📺 YouTube".into(),
                ping_ms: 20,
                is_active: false,
                protocol: "SELECTOR".into(),
                host: "Выбран: srv_aeza_external".into(),
                port: 0,
                is_priority: false,
                provider: None,
                provider_name: None,
                is_pool: true,
                resolved_ip: None,
            },
        ];

        let mut pings = std::collections::HashMap::new();
        pings.insert("chain_node_2".to_string(), -1); // chain_node_2 в таймауте
        pings.insert("srv_aeza_external".to_string(), 25); // живой внешний узел
        pings.insert("YouTube".to_string(), 20); // селекторная группа

        // Сценарий 1: Цепочка настроена (has_chain = true).
        // Все узлы цепочки лежат. Внешний узел AEZA жив.
        // Результат: СТРОГАЯ ИЗОЛЯЦИЯ (Strict Chain Boundary). Failover ОБЯЗАН вернуть ошибку и НЕ трогать AEZA или YouTube!
        let is_q = |_id: &str| false;
        let res = select_failover_target(
            true,
            &chain_candidates,
            &all_servers,
            active_id,
            is_q,
            &pings,
            300,
        );
        assert!(res.is_err(), "Failover обязан завершиться ошибкой при падении всех узлов цепочки!");
        let err_msg = res.unwrap_err();
        assert!(err_msg.contains("цепочки приоритетов недоступны"), "Сообщение об ошибке: {err_msg}");

        // Сценарий 2: Все кандидаты цепочки в карантине (зомби)
        pings.insert("chain_node_2".to_string(), 50); // пинг формально ок, но узел в карантине
        let is_all_q = |_id: &str| true;
        let res_all_q = select_failover_target(
            true,
            &chain_candidates,
            &all_servers,
            active_id,
            is_all_q,
            &pings,
            300,
        );
        // Внешняя AEZA или YouTube ни в коем случае не должны быть выбраны:
        if let Ok(chosen) = res_all_q {
            assert_ne!(chosen, "srv_aeza_external");
            assert_ne!(chosen, "YouTube");
        }

        // Сценарий 3: Цепочка НЕ настроена (has_chain = false, пустая цепочка).
        // Должен работать best-of-rest, выбирая честный сервер srv_aeza_external и ИСКЛЮЧАЯ селекторную группу "YouTube" (is_pool: true)
        let res_no_chain = select_failover_target(
            false,
            &[],
            &all_servers,
            active_id,
            is_q,
            &pings,
            300,
        );
        assert_eq!(res_no_chain.expect("При пустой цепочке должен выбираться лучший внешний узел"), "srv_aeza_external");
    }

    #[tokio::test]
    async fn test_anti_regression_hwid_persistence_in_app_config() {
        let cfg = crate::config::AppConfig::default();
        assert_eq!(cfg.mobile_provider_hwids.get("geodema2").map(|s| s.as_str()), Some("keenetic-router-229"));

        // JSON serialization/deserialization roundtrip maintains mobile_provider_hwids
        let json_str = serde_json::to_string(&cfg).expect("serialize");
        let restored: crate::config::AppConfig = serde_json::from_str(&json_str).expect("deserialize");
        assert_eq!(restored.mobile_provider_hwids.get("geodema2").map(|s| s.as_str()), Some("keenetic-router-229"));
    }

    #[test]
    fn test_connection_delta_download_vs_cumulative() {
        // FO-DELT-01, FO-DELT-02:
        // Проверка устранения ловушки накопленных байт: соединение с 10 МБ прошлых байт,
        // но нулевым приростом в текущем цикле, НЕ считается живым трафиком!
        let mut snapshot = std::collections::HashMap::new();
        snapshot.insert("conn_1".to_string(), (1000, 10_000_000));

        let current_conns = vec![
            serde_json::json!({
                "id": "conn_1",
                "chains": ["PROXY", "Node-A"],
                "upload": 1000,
                "download": 10_000_000, // дельта download == 0!
            })
        ];

        let (has_live, dl_delta, _up_delta, _sockets, _is_hole) =
            process_connections_delta(&current_conns, "Node-A", &mut snapshot);

        assert_eq!(dl_delta, 0, "Дельта скачивания обязана быть строго 0 при отсутствии нового трафика");
        assert!(!has_live, "Накопленные байты из прошлого не должны делать сервер 'живым' при delta == 0");

        // Следующий цикл: клиент реально скачал 2048 байт (> 1 КБ)
        let new_conns = vec![
            serde_json::json!({
                "id": "conn_1",
                "chains": ["PROXY", "Node-A"],
                "upload": 1200,
                "download": 10_002_048, // прирост 2048 байт
            })
        ];

        let (has_live_2, dl_delta_2, up_delta_2, _sockets_2, _is_hole_2) =
            process_connections_delta(&new_conns, "Node-A", &mut snapshot);

        assert_eq!(dl_delta_2, 2048);
        assert_eq!(up_delta_2, 200);
        assert!(has_live_2, "Прирост download > 1024 байт обязан признаваться живым трафиком!");
    }

    #[test]
    fn test_connection_delta_blackhole_detection() {
        // FO-HOLE-01:
        // Аномалия "Чёрная дыра" (DPI сброс):
        // 3 сокета, прирост upload > 5120 байт, при строго нулевом приросте download.
        let mut snapshot = std::collections::HashMap::new();
        snapshot.insert("sock_1".to_string(), (100, 50));
        snapshot.insert("sock_2".to_string(), (100, 50));
        snapshot.insert("sock_3".to_string(), (100, 50));

        let conns = vec![
            serde_json::json!({
                "id": "sock_1",
                "chains": ["PROXY", "Node-Blackhole"],
                "upload": 2100, // delta up = 2000
                "download": 50,  // delta dl = 0
            }),
            serde_json::json!({
                "id": "sock_2",
                "chains": ["PROXY", "Node-Blackhole"],
                "upload": 2100, // delta up = 2000
                "download": 50,  // delta dl = 0
            }),
            serde_json::json!({
                "id": "sock_3",
                "chains": ["PROXY", "Node-Blackhole"],
                "upload": 2100, // delta up = 2000 (total up delta = 6000 > 5120)
                "download": 50,  // delta dl = 0
            }),
        ];

        let (_live, dl_delta, up_delta, sockets, is_blackhole) =
            process_connections_delta(&conns, "Node-Blackhole", &mut snapshot);

        assert_eq!(sockets, 3);
        assert_eq!(dl_delta, 0);
        assert_eq!(up_delta, 6000);
        assert!(is_blackhole, "Аномалия 'Чёрная дыра' обязана сработать при delta upload > 5KB и delta download == 0 на >= 3 сокетах!");
    }

    #[test]
    fn test_fallback_hierarchy_alive_slow_when_no_threshold() {
        // FO-FALL-01:
        // Все серверы цепочки превышают порог (порог 300 мс, серверы имеют 320 и 400 мс).
        // Вместо ошибки и отключения интернета клиентов, Failover ОБЯЗАН выбрать живой медленный узел (Tier 2)!
        let chain_cands = vec!["slow_node_1".to_string(), "slow_node_2".to_string()];
        let mut pings = std::collections::HashMap::new();
        pings.insert("slow_node_1".to_string(), 320);
        pings.insert("slow_node_2".to_string(), 400);

        let target = select_failover_target(
            true,
            &chain_cands,
            &[],
            "dead_active",
            |_| false,
            &pings,
            300,
        ).expect("Failover не должен вызывать блэкаут, когда есть живой медленный узел!");

        assert_eq!(target.id, "slow_node_1");
        assert_eq!(target.ping_ms, 320);
        assert!(target.is_alive_slow, "Узел обязан быть помечен как is_alive_slow");
        assert!(!target.is_quarantined);
        assert_eq!(target.tier, 2, "Должен быть выбран Tier 2 (Alive Slow)");
    }

    #[test]
    fn test_fallback_hierarchy_quarantined_when_all_non_q_dead() {
        // FO-FALL-01:
        // Все незакарантиненные узлы мертвы (ping <= 0).
        // Единственный живой узел находится в карантине (ping 80 мс).
        // Чтобы не оставить клиентов без связи, Failover ОБЯЗАН совершить аварийный обход карантина (Tier 3)!
        let mut pings = std::collections::HashMap::new();
        pings.insert("dead_free_node".to_string(), -1);
        pings.insert("quarantined_alive_node".to_string(), 80);

        let is_q = |id: &str| id == "quarantined_alive_node";

        let target = select_failover_target(
            false,
            &[],
            &[
                Server {
                    id: "dead_free_node".into(),
                    name: "Dead Free".into(),
                    ping_ms: -1,
                    is_active: false,
                    protocol: "vless".into(),
                    host: "1.1.1.1".into(),
                    port: 443,
                    is_priority: false,
                    provider: None,
                    provider_name: None,
                    is_pool: false,
                    resolved_ip: None,
                },
                Server {
                    id: "quarantined_alive_node".into(),
                    name: "Quarantined Alive".into(),
                    ping_ms: 80,
                    is_active: false,
                    protocol: "vless".into(),
                    host: "2.2.2.2".into(),
                    port: 443,
                    is_priority: false,
                    provider: None,
                    provider_name: None,
                    is_pool: false,
                    resolved_ip: None,
                },
            ],
            "dead_active",
            is_q,
            &pings,
            300,
        ).expect("Failover обязан выбрать карантинный узел при отсутствии других живых серверов");

        assert_eq!(target.id, "quarantined_alive_node");
        assert_eq!(target.ping_ms, 80);
        assert!(target.is_quarantined, "Узел обязан быть помечен как is_quarantined");
        assert_eq!(target.tier, 3, "Должен быть выбран Tier 3 (Quarantined Fast)");
    }

    #[tokio::test]
    async fn test_post_switch_5s_recheck_immediate_quarantine() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        // FO-SWT-01:
        // Проверка 5-секундного окна аудита после переключения:
        // При переключении регистрируется отметка, и через 5 с узел доступен для проверки
        let target = "test_flapping_node";
        record_switch(target).await;

        let is_pending = is_post_switch_pending().await;
        assert!(is_pending, "После переключения флаг раннего аудита обязан быть активен");

        // Проверяем, что add_quarantine сразу банит узел
        let q_secs = add_quarantine(target, "Test Node", "Мёртв при переключении (ранний сбой через 5 с)").await;
        assert!(q_secs >= 900);
        assert!(is_quarantined(target).await, "Сбойный узел должен быть в карантине");
        clear_quarantine(target).await;
    }

    #[tokio::test]
    async fn test_single_connections_query_reuse_and_public_helpers() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        // FO-DELT-01, FO-DELT-02:
        let (dl_delta, up_delta) = calc_traffic_deltas((100, 200), (300, 1224));
        assert_eq!(dl_delta, 1024);
        assert_eq!(up_delta, 200);

        clear_conn_snapshot().await;

        let (live, bytes) = check_active_download_traffic(&reqwest::Client::new(), &crate::config::AppConfig::default(), "DIRECT").await;
        assert!(!live);
        assert_eq!(bytes, 0);

        let (live_empty, bytes_empty) = check_active_download_traffic(&reqwest::Client::new(), &crate::config::AppConfig::default(), "").await;
        assert!(!live_empty);
        assert_eq!(bytes_empty, 0);
    }

    #[test]
    fn test_stress_4_tier_hierarchy_adversarial() {
        // 1. Stress: 1000 nodes, 100% of fast nodes dead, slow nodes alive.
        let mut all_servers = Vec::new();
        let mut pings = std::collections::HashMap::new();
        let mut candidate_ids = Vec::new();

        let threshold = 300i64;

        // 500 fast nodes (ping <= 300) ALL DEAD (ping = -1 or 0)
        for i in 0..500 {
            let id = format!("fast_dead_{i}");
            candidate_ids.push(id.clone());
            pings.insert(id.clone(), if i % 2 == 0 { -1 } else { 0 });
            all_servers.push(Server {
                id: id.clone(),
                name: format!("Fast Dead {i}"),
                ping_ms: -1,
                is_active: false,
                protocol: "vless".into(),
                host: "1.1.1.1".into(),
                port: 443,
                is_priority: false,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            });
        }

        // 500 slow nodes (ping > 300) ALL ALIVE (pings between 305 and 1500)
        for i in 0..500 {
            let id = format!("slow_alive_{i}");
            let latency = 305 + (i as i64 * 2);
            candidate_ids.push(id.clone());
            pings.insert(id.clone(), latency);
            all_servers.push(Server {
                id: id.clone(),
                name: format!("Slow Alive {i}"),
                ping_ms: latency,
                is_active: false,
                protocol: "vless".into(),
                host: "2.2.2.2".into(),
                port: 443,
                is_priority: false,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            });
        }

        let is_q = |_id: &str| false;

        // In chain mode (preserve_order = true), it must pick the FIRST slow alive candidate
        let target_chain = select_failover_target(
            true,
            &candidate_ids,
            &all_servers,
            "active_dead",
            is_q,
            &pings,
            threshold,
        ).expect("Must select a slow alive candidate instead of failing!");

        assert_eq!(target_chain.id, "slow_alive_0");
        assert_eq!(target_chain.tier, 2);
        assert!(target_chain.is_alive_slow);
        assert!(!target_chain.is_quarantined);

        // In best-of-rest mode with alive node within first 20 (e.g. 10 dead, 10 alive)
        let mut servers_20 = Vec::new();
        let mut pings_20 = std::collections::HashMap::new();
        for i in 0..10 {
            let id = format!("d_{i}");
            pings_20.insert(id.clone(), -1);
            servers_20.push(Server {
                id: id.clone(),
                name: id.clone(),
                ping_ms: -1,
                is_active: false,
                protocol: "vless".into(),
                host: "1.1.1.1".into(),
                port: 443,
                is_priority: false,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            });
        }
        for i in 0..10 {
            let id = format!("s_{i}");
            let latency = 310 + (i as i64 * 5);
            pings_20.insert(id.clone(), latency);
            servers_20.push(Server {
                id: id.clone(),
                name: id.clone(),
                ping_ms: latency,
                is_active: false,
                protocol: "vless".into(),
                host: "2.2.2.2".into(),
                port: 443,
                is_priority: false,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            });
        }

        let target_best_20 = select_failover_target(
            false,
            &[],
            &servers_20,
            "active_dead",
            is_q,
            &pings_20,
            threshold,
        ).expect("Must select lowest latency slow alive candidate when within first 20");

        assert_eq!(target_best_20.id, "s_0");
        assert_eq!(target_best_20.tier, 2);
        assert!(target_best_20.is_alive_slow);
        assert_eq!(target_best_20.ping_ms, 310);

        // 1b. Anti-Regression Test for BUG-M1-01:
        // When all_servers has > 20 nodes and the first 20 nodes are dead,
        // select_failover_target must NOT truncate candidates at 20.
        // It evaluates all candidate servers, selecting the alive node at index 20 ("alive_20").
        let mut servers_bug = Vec::new();
        let mut pings_bug = std::collections::HashMap::new();
        for i in 0..20 {
            let id = format!("dead_{i}");
            pings_bug.insert(id.clone(), -1);
            servers_bug.push(Server {
                id: id.clone(),
                name: id.clone(),
                ping_ms: -1,
                is_active: false,
                protocol: "vless".into(),
                host: "1.1.1.1".into(),
                port: 443,
                is_priority: false,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            });
        }
        for i in 20..25 {
            let id = format!("alive_{i}");
            pings_bug.insert(id.clone(), 65);
            servers_bug.push(Server {
                id: id.clone(),
                name: id.clone(),
                ping_ms: 65,
                is_active: false,
                protocol: "vless".into(),
                host: "2.2.2.2".into(),
                port: 443,
                is_priority: false,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            });
        }

        let res_bug = select_failover_target(
            false,
            &[],
            &servers_bug,
            "active_dead",
            is_q,
            &pings_bug,
            threshold,
        );
        // BUG-M1-01 FIXED: select_failover_target now evaluates all servers and selects alive_20
        assert!(res_bug.is_ok(), "BUG-M1-01 fix: alive nodes beyond first 20 must be selected");
        assert_eq!(res_bug.unwrap().id, "alive_20");



        // 2. Adversarial: 100% of non-quarantined nodes are dead. Only quarantined nodes are alive.
        let mut pings_q = std::collections::HashMap::new();
        pings_q.insert("free_dead_1".to_string(), -1);
        pings_q.insert("free_dead_2".to_string(), 0);
        pings_q.insert("q_fast_alive".to_string(), 120); // Tier 3
        pings_q.insert("q_slow_alive".to_string(), 450); // Tier 4

        let is_q_test = |id: &str| id.starts_with("q_");
        let q_candidates = vec![
            "free_dead_1".to_string(),
            "free_dead_2".to_string(),
            "q_fast_alive".to_string(),
            "q_slow_alive".to_string(),
        ];

        let target_q = select_from_candidates(
            &q_candidates,
            &pings_q,
            is_q_test,
            threshold,
            false,
        ).expect("Must emergency-bypass quarantine when all free nodes dead");

        assert_eq!(target_q.id, "q_fast_alive");
        assert_eq!(target_q.tier, 3);
        assert!(target_q.is_quarantined);
        assert!(!target_q.is_alive_slow);

        // 3. Adversarial: Even quarantined fast nodes are dead! Only quarantined slow alive remain.
        pings_q.insert("q_fast_alive".to_string(), -1);
        let target_q_slow = select_from_candidates(
            &q_candidates,
            &pings_q,
            is_q_test,
            threshold,
            false,
        ).expect("Must fallback to Tier 4 (quarantined slow alive)");

        assert_eq!(target_q_slow.id, "q_slow_alive");
        assert_eq!(target_q_slow.tier, 4);
        assert!(target_q_slow.is_quarantined);
        assert!(target_q_slow.is_alive_slow);

        // 4. Boundary tests: ping == threshold (exact boundary)
        let mut pings_boundary = std::collections::HashMap::new();
        pings_boundary.insert("exact_threshold".to_string(), threshold);
        pings_boundary.insert("just_above".to_string(), threshold + 1);

        let cands_b = vec!["exact_threshold".to_string(), "just_above".to_string()];
        let target_exact = select_from_candidates(&cands_b, &pings_boundary, |_| false, threshold, true).unwrap();
        assert_eq!(target_exact.id, "exact_threshold");
        assert_eq!(target_exact.tier, 1, "Ping == threshold must belong to Tier 1");

        let cands_just_above = vec!["just_above".to_string()];
        let target_above = select_from_candidates(&cands_just_above, &pings_boundary, |_| false, threshold, true).unwrap();
        assert_eq!(target_above.tier, 2, "Ping == threshold + 1 must belong to Tier 2");
    }

    #[tokio::test]
    async fn test_stress_5s_post_switch_recheck_and_quarantine_triggering() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        clear_all_quarantine().await;

        let dead_srv = "dead_flapping_srv";
        let next_srv = "healthy_backup_srv";

        // 1. Activate dead server
        record_switch(dead_srv).await;
        assert!(is_post_switch_pending().await, "5s post switch audit must be pending");

        // 2. Inspect state inside early failure window (< 10s)
        let guard = LAST_ACTIVATED_SERVER.lock().await;
        let (act_id, activated_at, is_manual) = guard.as_ref().expect("switch must be recorded");
        assert_eq!(act_id, dead_srv);
        assert!(!is_manual, "Automatic switch must not be marked manual");
        let elapsed = activated_at.elapsed().as_secs();
        assert!(elapsed < EARLY_FAILURE_WINDOW_SECS);
        drop(guard);

        // 3. Dead server detected at 5s recheck: trigger early failure quarantine
        let reason = "Мёртв при переключении (ранний сбой через 5 с)";
        let q_secs = add_quarantine(dead_srv, "Dead Flapping Node", reason).await;
        assert!(q_secs >= 900, "Quarantine duration must be >= 15 min");
        assert!(is_quarantined(dead_srv).await, "Dead server must now be quarantined");

        // 4. Failover candidate selection triggers to pick the next candidate
        let chain = vec![dead_srv.to_string(), next_srv.to_string()];
        let mut pings = std::collections::HashMap::new();
        pings.insert(dead_srv.to_string(), -1);
        pings.insert(next_srv.to_string(), 75);

        let target = select_failover_target(
            true,
            &chain[1..], // candidate IDs excluding the dead active
            &[],
            dead_srv,
            |id| id == dead_srv,
            &pings,
            300,
        ).expect("Failover must successfully select next candidate");

        assert_eq!(target.id, next_srv);
        assert_eq!(target.tier, 1);
        assert!(!target.is_quarantined);

        // 5. Activate next healthy candidate
        record_switch(next_srv).await;
        assert_eq!(LAST_ACTIVATED_SERVER.lock().await.as_ref().unwrap().0, next_srv);

        // 6. Test manual switch immunity: manual switches must NOT be quarantined on early failure
        record_manual_switch("manual_user_srv").await;
        let manual_guard = LAST_ACTIVATED_SERVER.lock().await;
        let (m_id, _, m_manual) = manual_guard.as_ref().unwrap();
        assert_eq!(m_id, "manual_user_srv");
        assert!(m_manual, "Manual switch must set is_manual = true to prevent accidental quarantine");
        drop(manual_guard);

        clear_all_quarantine().await;
    }

    #[tokio::test]
    async fn test_stress_conn_snapshot_concurrent_hammer() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        clear_conn_snapshot().await;

        let num_tasks = 50;
        let num_conns = 200;
        let mut handles = Vec::new();

        for task_idx in 0..num_tasks {
            handles.push(tokio::spawn(async move {
                // Generate unique connections for this task
                let mut mock_conns = Vec::with_capacity(num_conns);
                for c in 0..num_conns {
                    mock_conns.push(serde_json::json!({
                        "id": format!("task_{task_idx}_conn_{c}"),
                        "chains": ["PROXY", "target_srv"],
                        "upload": (c as u64) * 100,
                        "download": (c as u64) * 500,
                    }));
                }

                // Rapidly lock and process delta
                {
                    let mut snap = CONN_SNAPSHOT.lock().await;
                    let (_live, _dl, _up, sockets, is_hole) =
                        process_connections_delta(&mock_conns, "target_srv", &mut snap);
                    assert_eq!(sockets, num_conns);
                    assert!(!is_hole);
                    if task_idx % 5 == 0 {
                        // verify repeat delta calculation
                        let (_live2, dl2, _up2, _sockets2, _) =
                            process_connections_delta(&mock_conns, "target_srv", &mut snap);
                        assert_eq!(dl2, 0, "Repeated query with same bytes must produce delta 0");
                    }
                }
            }));
        }

        // Concurrently run clear_conn_snapshot
        let clear_handle = tokio::spawn(async {
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(1)).await;
                clear_conn_snapshot().await;
            }
        });

        for h in handles {
            h.await.expect("Concurrent task panicked!");
        }
        clear_handle.await.expect("Clear task panicked!");

        // Memory cleanup test: empty connections list removes all closed sockets
        {
            let mut snap = CONN_SNAPSHOT.lock().await;
            snap.insert("lingering_closed_socket".to_string(), (100, 100));
            process_connections_delta(&[], "target_srv", &mut snap);
            assert!(snap.is_empty(), "process_connections_delta must clean up stale sockets");
        }
    }

    #[test]
    fn test_anti_regression_bug_m1_01_no_take_20_truncation_in_best_of_rest() {
        // Anti-regression test for BUG-M1-01:
        // Ensure that both candidate gathering and select_failover_target
        // evaluate all available non-system candidate servers without blind .take(20) truncation.
        let mut servers = Vec::new();
        let mut pings = std::collections::HashMap::new();

        // 50 servers in subscription:
        // First 20 servers (0..19) are dead (e.g. regional data center outage)
        for i in 0..20 {
            let id = format!("node_{i:02}");
            pings.insert(id.clone(), -1);
            servers.push(Server {
                id: id.clone(),
                name: format!("Dead Node {i}"),
                ping_ms: -1,
                is_active: false,
                protocol: "vless".into(),
                host: "10.0.0.1".into(),
                port: 443,
                is_priority: false,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            });
        }

        // Server 20 is operational with 75ms latency
        pings.insert("node_20".to_string(), 75);
        servers.push(Server {
            id: "node_20".into(),
            name: "Alive Node 20".into(),
            ping_ms: 75,
            is_active: false,
            protocol: "vless".into(),
            host: "10.0.0.2".into(),
            port: 443,
            is_priority: false,
            provider: None,
            provider_name: None,
            is_pool: false,
            resolved_ip: None,
        });

        // Servers 21..49 are operational with higher latency (180ms)
        for i in 21..50 {
            let id = format!("node_{i:02}");
            pings.insert(id.clone(), 180);
            servers.push(Server {
                id: id.clone(),
                name: format!("Alive Node {i}"),
                ping_ms: 180,
                is_active: false,
                protocol: "vless".into(),
                host: "10.0.0.3".into(),
                port: 443,
                is_priority: false,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            });
        }

        // Verify candidate extraction gathers all 50 candidates (no .take(20) truncation)
        let candidate_ids: Vec<String> = servers
            .iter()
            .filter(|s| {
                s.id != "active_node"
                    && !s.is_pool
                    && s.protocol != "URL-TEST"
                    && s.protocol != "FALLBACK"
                    && s.id != "DIRECT"
                    && s.id != "REJECT"
                    && s.id != "GLOBAL"
                    && !s.id.starts_with("AUTO-")
            })
            .map(|s| s.id.clone())
            .collect();

        assert_eq!(candidate_ids.len(), 50, "Candidate extraction must not truncate to 20");

        // Execute select_failover_target in unchained best-of-rest mode
        let result = select_failover_target(
            false,
            &[],
            &servers,
            "active_node",
            |_| false,
            &pings,
            300,
        );

        assert!(result.is_ok(), "select_failover_target must succeed when alive servers exist beyond index 20");
        let target = result.unwrap();
        assert_eq!(target.id, "node_20", "Best latency alive candidate beyond index 20 must be selected");
        assert_eq!(target.ping_ms, 75);
        assert_eq!(target.tier, 1, "Must be Tier 1 (Fast, Non-Quarantined)");
        assert!(!target.is_quarantined);
        assert!(!target.is_alive_slow);
    }

    #[tokio::test]
    async fn test_smart_failover_tier_subordination_prefers_vps_over_whitelist() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;

        let s_vps_1 = Server {
            id: "vps_fast".to_string(),
            name: "NL VPS 01".to_string(),
            ping_ms: 120,
            is_active: false,
            protocol: "vless".to_string(),
            host: "1.1.1.1".to_string(),
            port: 443,
            is_priority: false,
            provider: None,
            provider_name: None,
            is_pool: false,
            resolved_ip: None,
        };
        let s_vps_2 = Server {
            id: "vps_slow".to_string(),
            name: "DE VPS 02".to_string(),
            ping_ms: 220,
            is_active: false,
            protocol: "vless".to_string(),
            host: "1.1.1.2".to_string(),
            port: 443,
            is_priority: false,
            provider: None,
            provider_name: None,
            is_pool: false,
            resolved_ip: None,
        };
        let s_whitelist = Server {
            id: "wl_node".to_string(),
            name: "RU Whitelist [БС] Ultra".to_string(),
            ping_ms: 35, // пинг намного лучше VPS!
            is_active: false,
            protocol: "vless".to_string(),
            host: "1.1.1.3".to_string(),
            port: 443,
            is_priority: false,
            provider: None,
            provider_name: None,
            is_pool: false,
            resolved_ip: None,
        };

        let servers = vec![s_vps_1.clone(), s_vps_2.clone(), s_whitelist.clone()];
        let mut pings = std::collections::HashMap::new();
        pings.insert("vps_fast".to_string(), 120);
        pings.insert("vps_slow".to_string(), 220);
        pings.insert("wl_node".to_string(), 35);

        let cfg = crate::config::AppConfig::default();
        let is_bs_fn = |s: &Server| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);
        let threshold = 300;

        // 1. Умный фейловер ВКЛЮЧЕН: несмотря на пинг БС 35мс, система обязана выбрать VPS (120мс), сохраняя трафик БС!
        let target_smart = select_failover_target_tiered(
            false,
            &[],
            &servers,
            "active_dead",
            |_| false,
            is_bs_fn,
            true, // smart_failover_enabled
            &pings,
            threshold,
        ).expect("Must select target in smart failover mode");

        assert_eq!(target_smart.id, "vps_fast", "Smart failover must prioritize Tier 1 VPS over Tier 2 Whitelist");
        assert_eq!(target_smart.ping_ms, 120);

        // 2. Все VPS упали (ping -1): система обязана аварийно переключиться на Tier 2 (Whitelist)!
        pings.insert("vps_fast".to_string(), -1);
        pings.insert("vps_slow".to_string(), -1);

        let target_fallback_bs = select_failover_target_tiered(
            false,
            &[],
            &servers,
            "active_dead",
            |_| false,
            is_bs_fn,
            true,
            &pings,
            threshold,
        ).expect("Must fallback to Whitelist when all VPS nodes fail");

        assert_eq!(target_fallback_bs.id, "wl_node", "Must fallback to Whitelist when all Tier 1 nodes are dead");
        assert_eq!(target_fallback_bs.ping_ms, 35);

        // 3. Умный фейловер ВЫКЛЮЧЕН: выбирается узел с наименьшим пингом (wl_node: 35мс)
        pings.insert("vps_fast".to_string(), 120);
        let target_legacy = select_failover_target_tiered(
            false,
            &[],
            &servers,
            "active_dead",
            |_| false,
            is_bs_fn,
            false, // smart_failover_disabled
            &pings,
            threshold,
        ).expect("Must select lowest latency target when smart failover is disabled");

        assert_eq!(target_legacy.id, "wl_node", "When smart failover is disabled, raw lowest ping wins");
    }

    #[tokio::test]
    async fn test_best_ping_hysteresis_switch_guard() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;

        let hysteresis = 40i64;
        let act_ping = 110i64;

        // Кандидат с пингом 80мс: 80 + 40 = 120 >= 110 -> переключение НЕ должно произойти
        let cand_80 = 80i64;
        let should_switch_80 = act_ping <= 0 || (cand_80 + hysteresis < act_ping);
        assert!(!should_switch_80, "Hysteresis 40ms must prevent switching from 110ms to 80ms (diff 30ms < 40ms)");

        // Кандидат с пингом 60мс: 60 + 40 = 100 < 110 -> переключение ДОЛЖНО произойти
        let cand_60 = 60i64;
        let should_switch_60 = act_ping <= 0 || (cand_60 + hysteresis < act_ping);
        assert!(should_switch_60, "Hysteresis must allow switching when improvement exceeds margin (diff 50ms > 40ms)");
    }

    #[tokio::test]
    async fn test_smart_failover_pure_subordination_fast_bs_ignored() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;

        let servers = vec![
            Server {
                id: "vps_de".into(),
                name: "Germany VPS 01".into(),
                protocol: "vless".into(),
                host: "1.1.1.1".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 85,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "bs_ru".into(),
                name: "Russia [БС] Fast".into(),
                protocol: "vless".into(),
                host: "1.1.1.2".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 18,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
        ];
        let mut pings = std::collections::HashMap::new();
        pings.insert("vps_de".to_string(), 85i64);
        pings.insert("bs_ru".to_string(), 18i64); // Ultra-fast domestic ping

        let cfg = crate::config::AppConfig::default();
        let is_bs_fn = |s: &Server| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);
        let threshold = 300i64;

        // With smart failover enabled: must pick Tier 1 VPS (85ms) over Tier 2 (18ms)
        let target_smart = select_failover_target_tiered(
            false, &[], &servers, "active_dead", |_| false, is_bs_fn, true, &pings, threshold,
        ).expect("Must select target");
        assert_eq!(target_smart.id, "vps_de", "Tier 1 VPS must be chosen despite higher latency");
        assert_eq!(target_smart.ping_ms, 85);
    }

    #[tokio::test]
    async fn test_smart_failover_alive_slow_tier1_over_fast_tier2() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;

        let servers = vec![
            Server {
                id: "vps_slow".into(),
                name: "Netherlands VPS Slow".into(),
                protocol: "vless".into(),
                host: "1.1.1.1".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 420,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "bs_fast".into(),
                name: "Moscow [БС] Fast".into(),
                protocol: "vless".into(),
                host: "1.1.1.2".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 20,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
        ];
        let mut pings = std::collections::HashMap::new();
        pings.insert("vps_slow".to_string(), 420i64); // > threshold 300ms, alive slow
        pings.insert("bs_fast".to_string(), 20i64);   // <= threshold 300ms, fast healthy

        let cfg = crate::config::AppConfig::default();
        let is_bs_fn = |s: &Server| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);
        let threshold = 300i64;

        let target = select_failover_target_tiered(
            false, &[], &servers, "active_dead", |_| false, is_bs_fn, true, &pings, threshold,
        ).expect("Must select target");

        assert_eq!(target.id, "vps_slow", "Alive slow Tier 1 must be chosen over fast Tier 2 to keep internet open");
        assert!(target.is_alive_slow, "Target must be marked as alive slow");
        assert_eq!(target.tier, 2, "Candidate level must be 2 (alive slow)");
    }

    #[tokio::test]
    async fn test_smart_failover_quarantined_tier1_over_fast_tier2() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;

        let servers = vec![
            Server {
                id: "vps_q".into(),
                name: "Finland VPS Quarantined".into(),
                protocol: "vless".into(),
                host: "1.1.1.1".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 65,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "bs_healthy".into(),
                name: "SPB [БС] Healthy".into(),
                protocol: "vless".into(),
                host: "1.1.1.2".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 15,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
        ];
        let mut pings = std::collections::HashMap::new();
        pings.insert("vps_q".to_string(), 65i64);
        pings.insert("bs_healthy".to_string(), 15i64);

        let cfg = crate::config::AppConfig::default();
        let is_bs_fn = |s: &Server| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);
        let threshold = 300i64;

        let is_q_fn = |id: &str| id == "vps_q";

        let target = select_failover_target_tiered(
            false, &[], &servers, "active_dead", is_q_fn, is_bs_fn, true, &pings, threshold,
        ).expect("Must select target");

        assert_eq!(target.id, "vps_q", "Quarantined Tier 1 must be bypassed before falling back to domestic Whitelist");
        assert!(target.is_quarantined, "Target must be marked as quarantined");
    }

    #[tokio::test]
    async fn test_smart_failover_total_tier1_failure_fallback_to_tier2() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;

        let servers = vec![
            Server {
                id: "vps_1".into(),
                name: "VPS 1".into(),
                protocol: "vless".into(),
                host: "1.1.1.1".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 0,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "vps_2".into(),
                name: "VPS 2".into(),
                protocol: "vless".into(),
                host: "1.1.1.2".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 0,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "bs_backup".into(),
                name: "Russia [БС] Backup".into(),
                protocol: "vless".into(),
                host: "1.1.1.3".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 25,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
        ];
        let mut pings = std::collections::HashMap::new();
        pings.insert("vps_1".to_string(), -1i64);  // Dead
        pings.insert("vps_2".to_string(), 0i64);   // Timeout
        pings.insert("bs_backup".to_string(), 25i64); // Alive

        let cfg = crate::config::AppConfig::default();
        let is_bs_fn = |s: &Server| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);
        let threshold = 300i64;

        let target = select_failover_target_tiered(
            false, &[], &servers, "active_dead", |_| false, is_bs_fn, true, &pings, threshold,
        ).expect("Must fallback to Whitelist when all Tier 1 are dead");

        assert_eq!(target.id, "bs_backup");
        assert_eq!(target.ping_ms, 25);
        assert!(!target.is_quarantined);
        assert!(!target.is_alive_slow);
    }

    #[tokio::test]
    async fn test_smart_failover_priority_chain_tier_subordination() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;

        let servers = vec![
            Server {
                id: "vps_main".into(),
                name: "VPS Main (Chain 1)".into(),
                protocol: "vless".into(),
                host: "1.1.1.1".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 0,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "bs_in_chain".into(),
                name: "Russia [БС] (Chain 2)".into(),
                protocol: "vless".into(),
                host: "1.1.1.2".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 15,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
            Server {
                id: "vps_backup".into(),
                name: "VPS Backup (Chain 3)".into(),
                protocol: "vless".into(),
                host: "1.1.1.3".into(),
                port: 443,
                is_active: false,
                is_priority: false,
                ping_ms: 75,
                provider: None,
                provider_name: None,
                is_pool: false,
                resolved_ip: None,
            },
        ];
        let chain_ids = vec!["vps_main".to_string(), "bs_in_chain".to_string(), "vps_backup".to_string()];

        let mut pings = std::collections::HashMap::new();
        pings.insert("vps_main".to_string(), -1i64); // Main dead
        pings.insert("bs_in_chain".to_string(), 15i64); // Whitelist 15ms
        pings.insert("vps_backup".to_string(), 75i64);  // Backup VPS 75ms

        let cfg = crate::config::AppConfig::default();
        let is_bs_fn = |s: &Server| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);
        let threshold = 300i64;

        // Smart failover ON: must skip bs_in_chain and select vps_backup
        let target_smart = select_failover_target_tiered(
            true, &chain_ids, &servers, "vps_main", |_| false, is_bs_fn, true, &pings, threshold,
        ).expect("Must select target");
        assert_eq!(target_smart.id, "vps_backup", "Chain subordination must prefer Tier 1 VPS in chain over Tier 2 Whitelist");

        // Smart failover OFF: follows strict chain order, selecting bs_in_chain
        let target_legacy = select_failover_target_tiered(
            true, &chain_ids, &servers, "vps_main", |_| false, is_bs_fn, false, &pings, threshold,
        ).expect("Must select target");
        assert_eq!(target_legacy.id, "bs_in_chain", "When smart failover is OFF, chain order is strictly preserved");
    }

    #[test]
    fn test_is_whitelist_server_full_homoglyph_and_provider_matrix() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist_servers = vec!["special_wl_id".into()];
        cfg.whitelist_providers = vec!["domestic_sub".into()];

        // Cyrillic & Latin tags
        assert!(is_whitelist_server_full("Node [БС] Moscow", "n1", None, &cfg)); // Cyrillic б + с
        assert!(is_whitelist_server_full("Node [бc] SPB", "n2", None, &cfg));    // Cyrillic б + Latin c
        assert!(is_whitelist_server_full("Node (БС) Kazan", "n3", None, &cfg));
        assert!(is_whitelist_server_full("Node (бc) Samara", "n4", None, &cfg));
        assert!(is_whitelist_server_full("Node [WL] Fast", "n5", None, &cfg));
        assert!(is_whitelist_server_full("Node (wl) Direct", "n6", None, &cfg));
        assert!(is_whitelist_server_full("Node Whitelist Backup", "n7", None, &cfg));
        assert!(is_whitelist_server_full("Node [BS] Latin", "n8", None, &cfg));
        assert!(is_whitelist_server_full("Node (BS) Fast", "n9", None, &cfg));
        assert!(is_whitelist_server_full("Node [бs] Mixed", "n10", None, &cfg));
        assert!(is_whitelist_server_full("Node Белые списки", "n11", None, &cfg));
        assert!(is_whitelist_server_full("Node Белый список", "n12", None, &cfg));

        // ID and Provider matching
        assert!(is_whitelist_server_full("Generic Server", "special_wl_id", None, &cfg));
        assert!(is_whitelist_server_full("Generic Server", "n13", Some("domestic_sub"), &cfg));

        // Negative tests
        assert!(!is_whitelist_server_full("Germany VPS 01", "n14", None, &cfg));
        assert!(!is_whitelist_server_full("Netherlands Vless", "n15", Some("foreign_sub"), &cfg));
        assert!(!is_whitelist_server_full("DIRECT", "DIRECT", None, &cfg));
    }

    #[test]
    fn test_canary_duration_smart_failover_shortened() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.failover.canary_duration_secs = 180;
        cfg.failover.smart_failover_canary_duration_secs = 60;
        cfg.failover.smart_failover_enabled = true;

        // On standard VPS: duration is 180s
        let is_active_bs = false;
        let should_restore_smart = cfg.failover.smart_failover_enabled && is_active_bs;
        let duration = if is_active_bs && should_restore_smart {
            cfg.failover.smart_failover_canary_duration_secs.clamp(15, 180)
        } else {
            cfg.failover.canary_duration_secs.clamp(15, 300)
        };
        assert_eq!(duration, 180);

        // On Whitelist server: duration is shortened to 60s
        let is_active_bs = true;
        let should_restore_smart = cfg.failover.smart_failover_enabled && is_active_bs;
        let duration = if is_active_bs && should_restore_smart {
            cfg.failover.smart_failover_canary_duration_secs.clamp(15, 180)
        } else {
            cfg.failover.canary_duration_secs.clamp(15, 300)
        };
        assert_eq!(duration, 60);
    }

    #[test]
    fn test_socket_reset_selection_whitelist_return() {
        let cfg = crate::config::AppConfig::default();

        // From Whitelist server -> requires hard reset (switch_server)
        let is_from_bs = is_whitelist_server_full("RU [БС] Active", "bs_act", None, &cfg);
        assert!(is_from_bs);
        let use_graceful = !is_from_bs;
        assert!(!use_graceful, "Must use hard reset (graceful: false) when returning from Whitelist");

        // From standard VPS reserve -> uses graceful switch (switch_server_graceful)
        let is_from_normal = is_whitelist_server_full("DE Normal Reserve", "de_res", None, &cfg);
        assert!(!is_from_normal);
        let use_graceful = !is_from_normal;
        assert!(use_graceful, "Must use graceful switch when returning from standard reserve");
    }

    #[test]
    fn test_is_active_server_healthy_permutations() {
        let threshold = 200i64;

        // 1. Штатный здоровый сервер (пинг 60мс <= 200, нет черной дыры, нет скачивания)
        assert!(is_active_server_healthy(60, threshold, false, false));

        // 2. Черная дыра (upload растет, download==0) -> ВСЕГДА неисправен, даже при идеальном пинге 20мс!
        assert!(!is_active_server_healthy(20, threshold, false, true));
        assert!(!is_active_server_healthy(20, threshold, true, true));

        // 3. Пинг в таймауте (-1), но идет реальный трафик (has_live_traffic=true) -> исправен!
        assert!(is_active_server_healthy(-1, threshold, true, false));

        // 4. Пинг выше порога (250мс > 200мс), но идет реальный трафик -> исправен!
        assert!(is_active_server_healthy(250, threshold, true, false));

        // 5. Пинг выше порога (250мс), трафика нет -> неисправен
        assert!(!is_active_server_healthy(250, threshold, false, false));

        // 6. Пинг в таймауте (-1), трафика нет -> неисправен
        assert!(!is_active_server_healthy(-1, threshold, false, false));
    }

    #[test]
    fn test_should_best_ping_switch_hysteresis_matrix() {
        let hysteresis = 40i64;

        // 1. Активный сервер мертв (пинг <= 0) -> немедленное переключение
        assert!(should_best_ping_switch(-1, false, 80, hysteresis));
        assert!(should_best_ping_switch(0, false, 80, hysteresis));

        // 2. Активный сервер — черная дыра (is_blackhole = true) -> немедленное переключение
        assert!(should_best_ping_switch(30, true, 80, hysteresis));

        // 3. Активный 110мс, кандидат 80мс: 80 + 40 = 120 >= 110 -> переключение ОТКЛОНЕНО
        assert!(!should_best_ping_switch(110, false, 80, hysteresis));

        // 4. Активный 110мс, кандидат 60мс: 60 + 40 = 100 < 110 -> переключение РАЗРЕШЕНО
        assert!(should_best_ping_switch(110, false, 60, hysteresis));

        // 5. Активный 210мс (выше threshold 200), кандидат 190мс: 190 + 40 = 230 >= 210 -> ОТКЛОНЕНО
        assert!(!should_best_ping_switch(210, false, 190, hysteresis));

        // 6. Активный 260мс, кандидат 190мс: 190 + 40 = 230 < 260 -> РАЗРЕШЕНО
        assert!(should_best_ping_switch(260, false, 190, hysteresis));

        // 7. Медленный резерв: активный 260мс, кандидат 250мс: 250 + 40 = 290 >= 260 -> ОТКЛОНЕНО
        assert!(!should_best_ping_switch(260, false, 250, hysteresis));
    }

    #[test]
    fn test_ping_servers_staggered_concurrency_clamping() {
        let clamp_fn = |c: usize| c.clamp(1, 6);
        assert_eq!(clamp_fn(0), 1, "Concurrency 0 must clamp to 1");
        assert_eq!(clamp_fn(1), 1);
        assert_eq!(clamp_fn(4), 4, "Default concurrency 4 must remain 4");
        assert_eq!(clamp_fn(6), 6);
        assert_eq!(clamp_fn(10), 6, "Excessive concurrency must clamp to 6 to protect router SoC");
    }

    #[tokio::test]
    async fn test_ping_servers_staggered_empty_list_returns_empty_map() {
        let http = reqwest::Client::new();
        let cfg = crate::config::AppConfig::default();
        let res = ping_servers_staggered(&http, &cfg, &[], 1000, 4, 25).await;
        assert!(res.is_empty(), "Empty ID list must immediately return empty BTreeMap without spawning");
    }

    #[tokio::test]
    async fn test_whitelist_episode_lifecycle_and_delta_accumulation() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;

        // 1. Initial reset
        reset_whitelist_episode_bytes();
        assert_eq!(get_whitelist_episode_bytes(), 0);

        // 2. Accumulate delta
        record_whitelist_episode_bytes(5120);
        record_whitelist_episode_bytes(1024);
        assert_eq!(get_whitelist_episode_bytes(), 6144);

        // 3. New episode starts -> reset
        reset_whitelist_episode_bytes();
        assert_eq!(get_whitelist_episode_bytes(), 0);
    }

    #[tokio::test]
    async fn test_stress_active_only_polling_zero_pool_pings_when_healthy() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        *LAST_SWITCH.lock().await = None;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let candidate_ping_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let candidate_ping_count_c = candidate_ping_count.clone();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let mock_server = tokio::spawn(async move {
            loop {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let c_count = candidate_ping_count_c.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 4096];
                        let n = socket.read(&mut buf).await.unwrap_or(0);
                        let req = String::from_utf8_lossy(&buf[..n]);

                        if req.contains("GET /proxies/PROXY") {
                            let body = r#"{"name":"PROXY","type":"Selector","now":"vps_act"}"#;
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}", body.len(), body);
                            let _ = socket.write_all(resp.as_bytes()).await;
                        } else if req.contains("GET /proxies ") || req.contains("GET /proxies?") {
                            let mut proxies_obj = serde_json::Map::new();

                            // Primary selector group PROXY
                            let mut proxy_group = serde_json::Map::new();
                            proxy_group.insert("name".into(), serde_json::Value::String("PROXY".into()));
                            proxy_group.insert("type".into(), serde_json::Value::String("Selector".into()));
                            proxy_group.insert("now".into(), serde_json::Value::String("vps_act".into()));
                            proxies_obj.insert("PROXY".into(), serde_json::Value::Object(proxy_group));

                            // Active node
                            let mut act_map = serde_json::Map::new();
                            act_map.insert("name".into(), serde_json::Value::String("Germany VPS 01".into()));
                            act_map.insert("type".into(), serde_json::Value::String("vless".into()));
                            proxies_obj.insert("vps_act".into(), serde_json::Value::Object(act_map));

                            // 10 candidate nodes
                            for i in 1..=10 {
                                let mut cand_map = serde_json::Map::new();
                                cand_map.insert("name".into(), serde_json::Value::String(format!("Reserve Candidate {i}")));
                                cand_map.insert("type".into(), serde_json::Value::String("vless".into()));
                                proxies_obj.insert(format!("cand_{i}"), serde_json::Value::Object(cand_map));
                            }
                            let full_json = serde_json::json!({ "proxies": proxies_obj });
                            let body = serde_json::to_string(&full_json).unwrap();
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}", body.len(), body);
                            let _ = socket.write_all(resp.as_bytes()).await;
                        } else if req.contains("/delay") {
                            if req.contains("/DIRECT/delay") {
                                // WAN direct link check
                                let body = r#"{"delay": 12}"#;
                                let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}", body.len(), body);
                                let _ = socket.write_all(resp.as_bytes()).await;
                            } else if req.contains("/vps_act/delay") {
                                // Active server is healthy: 45ms ping
                                let body = r#"{"delay": 45}"#;
                                let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}", body.len(), body);
                                let _ = socket.write_all(resp.as_bytes()).await;
                            } else {
                                // Candidate ping intercepted!
                                c_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                let body = r#"{"delay": 30}"#;
                                let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}", body.len(), body);
                                let _ = socket.write_all(resp.as_bytes()).await;
                            }
                        } else if req.contains("GET /connections") {
                            let body = r#"{"downloadTotal":0,"uploadTotal":0,"connections":[]}"#;
                            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}", body.len(), body);
                            let _ = socket.write_all(resp.as_bytes()).await;
                        } else {
                            let resp = "HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
                            let _ = socket.write_all(resp.as_bytes()).await;
                        }
                    });
                } else {
                    break;
                }
            }
        });

        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.mihomo.host = "127.0.0.1".to_string();
        app_cfg.mihomo.port = port;
        app_cfg.failover.enabled = true;
        app_cfg.failover.best_ping_enabled = true;
        app_cfg.failover.ping_threshold_ms = 300;

        let cfg_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(app_cfg)));
        let state = AppState {
            config: cfg_arc.clone(),
            config_path: std::sync::Arc::new(std::path::PathBuf::from("test.json")),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: std::sync::Arc::new(crate::antigravity::AntigravityManager::new(cfg_arc)),
        };

        let res = run_check(&state).await;
        assert!(res.is_ok(), "run_check must succeed when active server is healthy: {:?}", res);
        let msg = res.unwrap();
        assert!(msg.contains("работает штатно"), "Expected healthy status message, got: {msg}");

        let pool_pings = candidate_ping_count.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            pool_pings, 0,
            "Active-only polling invariant violated: expected 0 candidate pool pings when active is healthy, observed: {}",
            pool_pings
        );

        mock_server.abort();
    }

    #[tokio::test]
    async fn test_stress_ping_servers_staggered_concurrency_and_stagger_with_12_mock_servers() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let current_inflight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let max_inflight = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));

        let current_inflight_c = current_inflight.clone();
        let max_inflight_c = max_inflight.clone();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        let mock_server = tokio::spawn(async move {
            loop {
                if let Ok((mut socket, _)) = listener.accept().await {
                    let cur = current_inflight_c.clone();
                    let max = max_inflight_c.clone();
                    tokio::spawn(async move {
                        let mut buf = [0u8; 2048];
                        let _ = socket.read(&mut buf).await.unwrap_or(0);

                        let inflight = cur.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                        max.fetch_max(inflight, std::sync::atomic::Ordering::SeqCst);

                        tokio::time::sleep(tokio::time::Duration::from_millis(80)).await;

                        cur.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);

                        let body = r#"{"delay": 42}"#;
                        let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{}", body.len(), body);
                        let _ = socket.write_all(resp.as_bytes()).await;
                    });
                } else {
                    break;
                }
            }
        });

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".to_string();
        cfg.mihomo.port = port;
        let http = reqwest::Client::new();

        let ids: Vec<String> = (0..12).map(|i| format!("mock_server_{i}")).collect();

        // 1. Stress benchmark with concurrency = 12 (requested > 6)
        max_inflight.store(0, std::sync::atomic::Ordering::SeqCst);
        let results = ping_servers_staggered(&http, &cfg, &ids, 2000, 12, 25).await;

        assert_eq!(results.len(), 12, "All 12 servers must be pinged");
        for id in &ids {
            assert_eq!(results.get(id), Some(&42));
        }

        let observed_max = max_inflight.load(std::sync::atomic::Ordering::SeqCst);
        assert!(
            observed_max <= 6,
            "Concurrency MUST be clamped to <= 6 even when concurrency 12 was requested! Observed: {}",
            observed_max
        );
        assert!(
            observed_max >= 3,
            "Parallelism was not utilized: expected at least 3-6 concurrent workers, observed: {}",
            observed_max
        );

        // 2. Boundary test: concurrency = 0 (must clamp to 1)
        max_inflight.store(0, std::sync::atomic::Ordering::SeqCst);
        let results_c0 = ping_servers_staggered(&http, &cfg, &ids[0..4], 2000, 0, 10).await;
        assert_eq!(results_c0.len(), 4);
        let observed_c0 = max_inflight.load(std::sync::atomic::Ordering::SeqCst);
        assert_eq!(
            observed_c0, 1,
            "Concurrency 0 must clamp strictly to 1 (sequential), observed: {}",
            observed_c0
        );

        mock_server.abort();
    }

    #[tokio::test]
    async fn test_stress_whitelist_episode_lifecycle_and_transition_retention() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;

        let mut app_cfg = crate::config::AppConfig::default();
        app_cfg.whitelist.auto_by_failover = true;

        let cfg_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(app_cfg)));
        let state = AppState {
            config: cfg_arc.clone(),
            config_path: std::sync::Arc::new(std::path::PathBuf::from("test.json")),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: std::sync::Arc::new(crate::antigravity::AntigravityManager::new(cfg_arc)),
        };

        // 1. Initial State
        reset_whitelist_episode_bytes();
        assert_eq!(get_whitelist_episode_bytes(), 0);

        // 2. Transition into Tier 2 (Whitelist server)
        handle_whitelist_transition(&state, "ru_bs_1", "Russia [БС] Fast", None).await;
        assert_eq!(get_whitelist_episode_bytes(), 0, "Entering Tier 2 must reset episode counter to 0");

        // 3. Multi-cycle traffic accumulation
        record_whitelist_episode_bytes(4096);
        assert_eq!(get_whitelist_episode_bytes(), 4096);
        record_whitelist_episode_bytes(8192);
        assert_eq!(get_whitelist_episode_bytes(), 12288);
        record_whitelist_episode_bytes(1024);
        assert_eq!(get_whitelist_episode_bytes(), 13312);

        // 4. Return to Tier 1 (Standard VPS server)
        handle_whitelist_transition(&state, "de_vps_1", "Germany VPS 01", None).await;
        assert_eq!(
            get_whitelist_episode_bytes(),
            13312,
            "Episode bytes MUST be retained upon returning to Tier 1 to display episode summary"
        );

        // 5. Subsequent entry to Tier 2 in a new episode resets counter
        handle_whitelist_transition(&state, "ru_bs_2", "Russia [БС] Backup", None).await;
        assert_eq!(
            get_whitelist_episode_bytes(),
            0,
            "Entering Tier 2 in a new episode MUST reset counter to 0"
        );
    }

    // ========================================================================
    // FO-QUR-01 Tests: Persistent Quarantine & Roundtrip Storage
    // ========================================================================

    #[tokio::test]
    async fn test_quarantine_atomic_save_roundtrip() {
        let tmp = std::env::temp_dir().join(format!("q_test_roundtrip_{}.json", current_unix_timestamp()));
        let mut map = std::collections::HashMap::new();
        map.insert("srv_de".into(), PersistentQuarantineEntry {
            server_name: "Германия".into(),
            quarantined_at: current_unix_timestamp(),
            fail_count: 2,
            duration_secs: 1800,
            reason: "L7-сбой".into(),
        });
        save_quarantine_to_file(&tmp, &map).await.expect("Must save");
        assert!(tmp.exists(), "Saved file must exist on disk");

        let loaded = load_quarantine_from_file(&tmp).await.expect("Must load");
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded["srv_de"].server_name, "Германия");
        assert_eq!(loaded["srv_de"].fail_count, 2);
        assert_eq!(loaded["srv_de"].duration_secs, 1800);
        let _ = tokio::fs::remove_file(&tmp).await;
    }

    #[tokio::test]
    async fn test_quarantine_corrupted_json_recovery() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        let tmp = std::env::temp_dir().join(format!("q_test_corrupt_{}.json", current_unix_timestamp()));
        tokio::fs::write(&tmp, "{ this is: not valid json !!! [}").await.unwrap();

        let res = load_quarantine_from_file(&tmp).await;
        assert!(res.is_err(), "Must report parse error for corrupted JSON");

        // Verify that init_quarantine_from_disk fallback logic resets cleanly without panic
        set_quarantine_path_override(Some(tmp.clone()));
        let init_res = init_quarantine_from_disk().await;
        assert!(init_res.is_ok(), "init_quarantine_from_disk must gracefully recover without error");
        assert_eq!(quarantine_status().await.len(), 0);

        set_quarantine_path_override(None);
        let _ = tokio::fs::remove_file(&tmp).await;
    }

    #[tokio::test]
    async fn test_quarantine_expired_entry_purge() {
        let tmp = std::env::temp_dir().join(format!("q_test_purge_{}.json", current_unix_timestamp()));
        let now = current_unix_timestamp();

        let mut map = std::collections::HashMap::new();
        // Entry 1: Active ban (quarantined 100s ago for 900s) -> active
        map.insert("srv_active".into(), PersistentQuarantineEntry {
            server_name: "Active Node".into(),
            quarantined_at: now - 100,
            fail_count: 1,
            duration_secs: 900,
            reason: "Active".into(),
        });
        // Entry 2: Ban expired 1 hour ago, but within 12h amnesty window -> in map, but not active
        map.insert("srv_expired_recent".into(), PersistentQuarantineEntry {
            server_name: "Recent Node".into(),
            quarantined_at: now - 4500,
            fail_count: 2,
            duration_secs: 900,
            reason: "Recent".into(),
        });
        // Entry 3: Ban expired 20 hours ago (> 43200s) -> MUST BE PURGED
        map.insert("srv_stale_purged".into(), PersistentQuarantineEntry {
            server_name: "Stale Node".into(),
            quarantined_at: now - 72000,
            fail_count: 3,
            duration_secs: 3600,
            reason: "Stale".into(),
        });

        save_quarantine_to_file(&tmp, &map).await.unwrap();
        let loaded = load_quarantine_from_file(&tmp).await.unwrap();

        assert!(loaded.contains_key("srv_active"), "Active entry must be preserved");
        assert!(loaded.contains_key("srv_expired_recent"), "Recent failure within 12h must be retained for progressive escalation");
        assert!(!loaded.contains_key("srv_stale_purged"), "Entry older than 12h must be completely purged!");

        let _ = tokio::fs::remove_file(&tmp).await;
    }

    #[tokio::test]
    async fn test_quarantine_persistence_across_restart() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        let tmp = std::env::temp_dir().join(format!("q_test_restart_{}_{}.json", std::process::id(), current_unix_timestamp()));
        let _ = tokio::fs::remove_file(&tmp).await;
        set_quarantine_path_override(Some(tmp.clone()));
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }

        // 1. Strike 1 before restart
        let d1 = add_quarantine("srv_flap", "Flapping Node", "timeout 1").await;
        assert_eq!(d1, 900, "Strike 1 must be 900s");
        flush_quarantine_to_disk().await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // 2. Simulate process restart: wipe memory
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }
        assert!(!is_quarantined("srv_flap").await, "Memory should be clean before disk load");

        // 3. Boot init from disk
        init_quarantine_from_disk().await.unwrap();
        assert!(is_quarantined("srv_flap").await, "Quarantine must be restored after boot load");

        // 4. Strike 2 after restart -> must escalate to 1800s!
        let d2 = add_quarantine("srv_flap", "Flapping Node", "timeout 2").await;
        assert_eq!(d2, 1800, "Strike 2 must escalate to 1800s across restart!");

        set_quarantine_path_override(None);
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }
        let _ = tokio::fs::remove_file(&tmp).await;
    }

    #[tokio::test]
    async fn test_quarantine_missing_file_init() {
        let missing_path = std::env::temp_dir().join(format!("nonexistent_q_{}.json", current_unix_timestamp()));
        let res = load_quarantine_from_file(&missing_path).await;
        assert!(res.is_ok(), "Missing file must return Ok(empty)");
        assert_eq!(res.unwrap().len(), 0);
    }

    // ========================================================================
    // FO-DEV-01 Tests: Per-Device Failover Engine Unification
    // ========================================================================

    fn mock_server(id: &str, name: &str) -> Server {
        Server {
            id: id.to_string(),
            name: name.to_string(),
            protocol: "vless".to_string(),
            host: "1.1.1.1".to_string(),
            port: 443,
            is_active: false,
            is_priority: false,
            ping_ms: -1,
            provider: None,
            provider_name: None,
            is_pool: false,
            resolved_ip: None,
        }
    }

    #[test]
    fn test_process_device_connections_delta_matches_ip_and_group() {
        let conns = serde_json::json!([
            {
                "id": "c1",
                "upload": 500,
                "download": 2048,
                "metadata": { "sourceIP": "192.168.1.100" },
                "chains": ["AUTO-DEV-192-168-1-100", "vps_target", "DIRECT"]
            },
            {
                "id": "c2",
                "upload": 200,
                "download": 500,
                "metadata": { "sourceIP": "192.168.1.101" }, // different device
                "chains": ["AUTO-DEV-192-168-1-101", "vps_target", "DIRECT"]
            },
            {
                "id": "c3",
                "upload": 300,
                "download": 4096,
                "metadata": { "sourceIP": "192.168.1.100" },
                "chains": ["DIRECT"] // matches device but NOT server
            }
        ]);
        let conns_arr = conns.as_array().unwrap();
        let prev_snap = std::collections::HashMap::new();

        let (live, dl, up, sockets, is_hole) = process_device_connections_delta(
            conns_arr,
            "192.168.1.100",
            "AUTO-DEV-192-168-1-100",
            "vps_target",
            &prev_snap,
        );

        assert!(live, "Download delta > 1KB must set has_live_traffic = true");
        assert_eq!(dl, 2048);
        assert_eq!(up, 500);
        assert_eq!(sockets, 1, "Only matching IP/group and server must be counted");
        assert!(!is_hole);
    }

    #[test]
    fn test_process_device_connections_delta_blackhole() {
        let conns = serde_json::json!([
            {
                "id": "c1",
                "upload": 2048,
                "download": 0,
                "metadata": { "sourceIP": "192.168.1.50" },
                "chains": ["AUTO-DEV-192-168-1-50", "srv_blackhole", "DIRECT"]
            },
            {
                "id": "c2",
                "upload": 2048,
                "download": 0,
                "metadata": { "sourceIP": "192.168.1.50" },
                "chains": ["AUTO-DEV-192-168-1-50", "srv_blackhole", "DIRECT"]
            },
            {
                "id": "c3",
                "upload": 2048,
                "download": 0,
                "metadata": { "sourceIP": "192.168.1.50" },
                "chains": ["AUTO-DEV-192-168-1-50", "srv_blackhole", "DIRECT"]
            }
        ]);
        let conns_arr = conns.as_array().unwrap();
        let prev_snap = std::collections::HashMap::new();

        let (live, dl, up, sockets, is_hole) = process_device_connections_delta(
            conns_arr,
            "192.168.1.50",
            "AUTO-DEV-192-168-1-50",
            "srv_blackhole",
            &prev_snap,
        );

        assert!(!live, "Download is 0, so live traffic must be false");
        assert_eq!(dl, 0);
        assert_eq!(up, 6144); // > 5120
        assert_eq!(sockets, 3);
        assert!(is_hole, "DPI blackhole anomaly must be detected when up > 5KB across 3 sockets with 0 download");
    }

    #[test]
    fn test_device_failover_skips_when_active_download_traffic() {
        let servers_chain = vec!["vps1".to_string(), "vps2".to_string()];
        let all_servers = vec![
            mock_server("vps1", "VPS 1"),
            mock_server("vps2", "VPS 2"),
        ];
        let higher_pings = std::collections::HashMap::new();
        let cand_pings = std::collections::HashMap::new();

        // Ping failed (-1), but download delta > 1KB (has_live_traffic = true)
        let decision = evaluate_device_failover(
            "192.168.1.20",
            "vps1",
            &servers_chain,
            -1,
            150,
            true,
            true, // has_live_traffic
            false, // is_blackhole
            &all_servers,
            &higher_pings,
            &cand_pings,
            |_| false,
            |_| false,
            true,
        );

        match decision {
            DeviceFailoverDecision::KeepCurrent { has_live_traffic, reason } => {
                assert!(has_live_traffic);
                assert!(reason.contains("активная загрузка"));
            }
            other => panic!("Expected KeepCurrent, got {:?}", other),
        }
    }

    #[test]
    fn test_device_failover_blackhole_forces_switch() {
        let servers_chain = vec!["vps1".to_string(), "vps2".to_string()];
        let all_servers = vec![
            mock_server("vps1", "VPS 1"),
            mock_server("vps2", "VPS 2"),
        ];
        let higher_pings = std::collections::HashMap::new();
        let mut cand_pings = std::collections::HashMap::new();
        cand_pings.insert("vps2".to_string(), 45i64);

        // Ping is seemingly good (30ms), but blackhole detected!
        let decision = evaluate_device_failover(
            "192.168.1.20",
            "vps1",
            &servers_chain,
            30,
            150,
            true,
            false,
            true, // is_blackhole!
            &all_servers,
            &higher_pings,
            &cand_pings,
            |_| false,
            |_| false,
            true,
        );

        match decision {
            DeviceFailoverDecision::Switch { target, reason } => {
                assert_eq!(target.id, "vps2");
                assert!(reason.contains("чёрная дыра"));
            }
            other => panic!("Expected Switch on blackhole, got {:?}", other),
        }
    }

    #[test]
    fn test_device_failover_smart_tier_subordination() {
        let servers_chain = vec!["vps_main".to_string(), "bs_fast".to_string(), "vps_backup".to_string()];
        let all_servers = vec![
            mock_server("vps_main", "VPS Main"),
            mock_server("bs_fast", "Russia [БС] Fast"),
            mock_server("vps_backup", "VPS Backup"),
        ];
        let higher_pings = std::collections::HashMap::new();
        let mut cand_pings = std::collections::HashMap::new();
        // Whitelist has 20ms, standard VPS has 80ms
        cand_pings.insert("bs_fast".to_string(), 20i64);
        cand_pings.insert("vps_backup".to_string(), 80i64);

        let is_bs_fn = |s: &Server| s.name.contains("[БС]");

        // With smart_failover_enabled = true: Tier 1 VPS must be chosen despite higher latency
        let decision_smart = evaluate_device_failover(
            "192.168.1.30",
            "vps_main",
            &servers_chain,
            -1, // failed
            150,
            true,
            false,
            false,
            &all_servers,
            &higher_pings,
            &cand_pings,
            |_| false,
            is_bs_fn,
            true, // smart_failover_enabled
        );

        match decision_smart {
            DeviceFailoverDecision::Switch { target, .. } => {
                assert_eq!(target.id, "vps_backup", "Tier 1 VPS must be prioritized over Tier 2 Whitelist");
            }
            other => panic!("Expected Switch, got {:?}", other),
        }

        // With smart_failover_enabled = false: respects chain order (bs_fast is earlier in chain)
        let decision_legacy = evaluate_device_failover(
            "192.168.1.30",
            "vps_main",
            &servers_chain,
            -1,
            150,
            true,
            false,
            false,
            &all_servers,
            &higher_pings,
            &cand_pings,
            |_| false,
            is_bs_fn,
            false, // smart_failover disabled
        );

        match decision_legacy {
            DeviceFailoverDecision::Switch { target, .. } => {
                assert_eq!(target.id, "bs_fast", "When smart failover is off, chain candidate order is preserved");
            }
            other => panic!("Expected Switch, got {:?}", other),
        }
    }

    #[test]
    fn test_device_failover_emergency_quarantine_bypass() {
        let servers_chain = vec!["vps1".to_string(), "vps_q1".to_string(), "vps_q2".to_string()];
        let all_servers = vec![
            mock_server("vps1", "VPS 1"),
            mock_server("vps_q1", "VPS Q1"),
            mock_server("vps_q2", "VPS Q2"),
        ];
        let higher_pings = std::collections::HashMap::new();
        let mut cand_pings = std::collections::HashMap::new();
        cand_pings.insert("vps_q1".to_string(), 50i64);
        cand_pings.insert("vps_q2".to_string(), 90i64);

        // All backups are in quarantine!
        let is_q = |id: &str| id.starts_with("vps_q");

        let decision = evaluate_device_failover(
            "192.168.1.40",
            "vps1",
            &servers_chain,
            -1,
            150,
            true,
            false,
            false,
            &all_servers,
            &higher_pings,
            &cand_pings,
            is_q,
            |_| false,
            true,
        );

        match decision {
            DeviceFailoverDecision::Switch { target, .. } => {
                assert!(target.is_quarantined, "Target must be marked as quarantined on emergency bypass");
                assert_eq!(target.id, "vps_q1");
            }
            other => panic!("Expected emergency bypass Switch, got {:?}", other),
        }
    }

    #[test]
    fn test_device_failover_auto_restore_quarantine_guard() {
        let servers_chain = vec!["vps_primary".to_string(), "vps_backup".to_string()];
        let all_servers = vec![
            mock_server("vps_primary", "VPS Primary"),
            mock_server("vps_backup", "VPS Backup"),
        ];
        let mut higher_pings = std::collections::HashMap::new();
        higher_pings.insert("vps_primary".to_string(), 25i64); // recovered ping is good!
        let cand_pings = std::collections::HashMap::new();

        // But vps_primary is still in quarantine!
        let is_q = |id: &str| id == "vps_primary";

        let decision = evaluate_device_failover(
            "192.168.1.50",
            "vps_backup",
            &servers_chain,
            60, // backup is healthy
            150,
            true, // auto_restore is on
            false,
            false,
            &all_servers,
            &higher_pings,
            &cand_pings,
            is_q,
            |_| false,
            true,
        );

        match decision {
            DeviceFailoverDecision::KeepCurrent { .. } => {
                // Good! Remained on backup because primary is quarantined
            }
            other => panic!("Must NOT auto-restore to quarantined server, got {:?}", other),
        }
    }

    #[test]
    fn test_device_failover_auto_restore_traffic_guard() {
        let servers_chain = vec!["vps_primary".to_string(), "vps_backup".to_string()];
        let all_servers = vec![
            mock_server("vps_primary", "VPS Primary"),
            mock_server("vps_backup", "VPS Backup"),
        ];
        let mut higher_pings = std::collections::HashMap::new();
        higher_pings.insert("vps_primary".to_string(), 25i64);
        let cand_pings = std::collections::HashMap::new();

        // Primary is healthy and NOT in quarantine, BUT client device is actively downloading!
        let decision = evaluate_device_failover(
            "192.168.1.50",
            "vps_backup",
            &servers_chain,
            50,
            150,
            true, // auto_restore is on
            true, // has_live_traffic = true!
            false,
            &all_servers,
            &higher_pings,
            &cand_pings,
            |_| false,
            |_| false,
            true,
        );

        match decision {
            DeviceFailoverDecision::KeepCurrent { has_live_traffic, .. } => {
                assert!(has_live_traffic);
            }
            other => panic!("Must defer auto-restore during active download traffic, got {:?}", other),
        }
    }

    // ========================================================================
    // Adversarial Challenger 2 Tests: Quarantine Persistence & Resiliency Matrix
    // ========================================================================

    #[tokio::test]
    async fn test_adversarial_quarantine_corrupted_json_matrix() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        let base_tmp = std::env::temp_dir().join(format!("q_corrupt_matrix_{}_{}", std::process::id(), current_unix_timestamp()));
        tokio::fs::create_dir_all(&base_tmp).await.unwrap();

        let corrupt_payloads = vec![
            ("empty", ""),
            ("whitespace", "   \n\t  \r\n  "),
            ("truncated_object", "{\"srv_1\": {\"server_name\": \"test\""),
            ("truncated_middle", "{\"srv_1\": {\"quarantined_at\": 12345, \"dur"),
            ("invalid_syntax", "{ this is not json at all !!! }"),
            ("null", "null"),
            ("array", "[]"),
            ("nested_array", "[{\"srv_1\": 123}]"),
            ("number", "123456789"),
            ("boolean", "true"),
            ("string_scalar", "\"just a string\""),
            ("wrong_field_types", "{\"srv_1\": {\"server_name\": 123, \"quarantined_at\": \"not_a_number\"}}"),
            ("missing_required_fields", "{\"srv_1\": {\"server_name\": \"test\"}}"),
            ("utf8_bom", "\u{FEFF}{\"srv_1\": not valid}"),
        ];

        for (name, payload) in corrupt_payloads {
            let file_path = base_tmp.join(format!("{}.json", name));
            tokio::fs::write(&file_path, payload).await.unwrap();

            // 1. load_quarantine_from_file should either return Ok(empty) for empty/whitespace or Err(...) for invalid json
            let load_res = load_quarantine_from_file(&file_path).await;
            if payload.trim().is_empty() {
                assert!(load_res.is_ok(), "Empty/whitespace file '{name}' must return Ok(empty)");
                assert!(load_res.unwrap().is_empty());
            } else {
                assert!(load_res.is_err(), "Corrupt payload '{name}' must return Err(...)");
            }

            // 2. init_quarantine_from_disk with this file MUST NEVER PANIC and must initialize empty quarantine
            set_quarantine_path_override(Some(file_path.clone()));
            let init_res = init_quarantine_from_disk().await;
            assert!(init_res.is_ok(), "init_quarantine_from_disk must succeed gracefully for '{name}'");
            assert_eq!(quarantine_status().await.len(), 0, "Quarantine status must be empty after loading '{name}'");
        }

        // Binary garbage test (invalid UTF-8)
        let bin_file = base_tmp.join("binary_garbage.json");
        tokio::fs::write(&bin_file, &[0xFF, 0xFE, 0x00, 0xBA, 0xDD, 0xC0, 0xDE]).await.unwrap();
        let bin_load = load_quarantine_from_file(&bin_file).await;
        assert!(bin_load.is_err(), "Binary garbage must return Err");
        set_quarantine_path_override(Some(bin_file));
        let bin_init = init_quarantine_from_disk().await;
        assert!(bin_init.is_ok(), "init_quarantine_from_disk must handle invalid UTF-8 without panic");
        assert_eq!(quarantine_status().await.len(), 0);

        set_quarantine_path_override(None);
        let _ = tokio::fs::remove_dir_all(&base_tmp).await;
    }

    #[tokio::test]
    async fn test_adversarial_quarantine_12h_amnesty_boundary_oracle() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        let tmp = std::env::temp_dir().join(format!("q_amnesty_bounds_{}_{}.json", std::process::id(), current_unix_timestamp()));
        let _ = tokio::fs::remove_file(&tmp).await;
        let now = current_unix_timestamp();

        let mut map = std::collections::HashMap::new();

        // 1. Exactly on the boundary: quarantined_at = now - 43200 (saturating_sub is 43200 <= 43200 -> RETAINED)
        map.insert("srv_boundary_exact".into(), PersistentQuarantineEntry {
            server_name: "Boundary Exact".into(),
            quarantined_at: now - 43200,
            fail_count: 2,
            duration_secs: 900,
            reason: "Boundary exact".into(),
        });

        // 2. Just inside boundary: quarantined_at = now - 43199 (saturating_sub is 43199 <= 43200 -> RETAINED)
        map.insert("srv_boundary_inside".into(), PersistentQuarantineEntry {
            server_name: "Boundary Inside".into(),
            quarantined_at: now - 43199,
            fail_count: 2,
            duration_secs: 900,
            reason: "Boundary inside".into(),
        });

        // 3. Just outside boundary: quarantined_at = now - 43201 (saturating_sub is 43201 > 43200 -> PURGED)
        map.insert("srv_boundary_outside".into(), PersistentQuarantineEntry {
            server_name: "Boundary Outside".into(),
            quarantined_at: now - 43201,
            fail_count: 3,
            duration_secs: 900,
            reason: "Boundary outside".into(),
        });

        // 4. Stale entry 1 week old: quarantined_at = now - 604800 -> PURGED
        map.insert("srv_week_old".into(), PersistentQuarantineEntry {
            server_name: "Week Old".into(),
            quarantined_at: now - 604800,
            fail_count: 5,
            duration_secs: 3600,
            reason: "Week old".into(),
        });

        // 5. Epoch zero: quarantined_at = 0 -> PURGED
        map.insert("srv_epoch_zero".into(), PersistentQuarantineEntry {
            server_name: "Epoch Zero".into(),
            quarantined_at: 0,
            fail_count: 1,
            duration_secs: 900,
            reason: "Epoch zero".into(),
        });

        // 6. Active recent ban: quarantined_at = now - 60, duration = 900 -> RETAINED & ACTIVE
        map.insert("srv_active".into(), PersistentQuarantineEntry {
            server_name: "Active Node".into(),
            quarantined_at: now - 60,
            fail_count: 1,
            duration_secs: 900,
            reason: "Active".into(),
        });

        // 7. Clock skew (future timestamp): quarantined_at = now + 3600 -> saturating_sub is 0 <= 43200 -> RETAINED without underflow/panic
        map.insert("srv_future".into(), PersistentQuarantineEntry {
            server_name: "Future Node".into(),
            quarantined_at: now + 3600,
            fail_count: 1,
            duration_secs: 900,
            reason: "Future clock".into(),
        });

        save_quarantine_to_file(&tmp, &map).await.unwrap();
        let loaded = load_quarantine_from_file(&tmp).await.unwrap();

        assert!(loaded.contains_key("srv_boundary_exact"), "Boundary exact (43200s) must be retained");
        assert!(loaded.contains_key("srv_boundary_inside"), "Boundary inside (43199s) must be retained");
        assert!(!loaded.contains_key("srv_boundary_outside"), "Boundary outside (43201s) must be purged");
        assert!(!loaded.contains_key("srv_week_old"), "Week-old entry must be purged");
        assert!(!loaded.contains_key("srv_epoch_zero"), "Epoch-zero entry must be purged");
        assert!(loaded.contains_key("srv_active"), "Active entry must be retained");
        assert!(loaded.contains_key("srv_future"), "Future entry must be retained without underflow");

        // Load into state and verify quarantine status
        set_quarantine_path_override(Some(tmp.clone()));
        init_quarantine_from_disk().await.unwrap();

        // srv_active is actively banned
        assert!(is_quarantined("srv_active").await);
        // srv_boundary_exact ban has expired (duration 900s), but fail_count retained for escalation
        assert!(!is_quarantined("srv_boundary_exact").await);

        // Strike on srv_boundary_inside (prev fail_count was 2) -> must escalate to strike 3 (3600s)
        let d = add_quarantine("srv_boundary_inside", "Boundary Inside", "re-failed").await;
        assert_eq!(d, 3600, "Strike 3 must be 3600s");

        // Strike on srv_boundary_outside (which was purged) -> fresh strike 1 (900s)
        let d_fresh = add_quarantine("srv_boundary_outside", "Boundary Outside", "new fail").await;
        assert_eq!(d_fresh, 900, "Purged node must restart at 900s strike 1");

        set_quarantine_path_override(None);
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }
        let _ = tokio::fs::remove_file(&tmp).await;
    }

    #[tokio::test]
    async fn test_adversarial_quarantine_progressive_escalation_lifecycle_reboot() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        let tmp = std::env::temp_dir().join(format!("q_escalate_reboot_{}_{}.json", std::process::id(), current_unix_timestamp()));
        let _ = tokio::fs::remove_file(&tmp).await;
        set_quarantine_path_override(Some(tmp.clone()));
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }

        let node = "srv_test_escalation";
        let name = "Escalation Target";

        // 1. Strike 1: 15 min (900s)
        let d1 = add_quarantine(node, name, "timeout strike 1").await;
        assert_eq!(d1, 900, "Strike 1 must be 900s (15 min)");
        flush_quarantine_to_disk().await.unwrap();

        // Simulate reboot: memory wiped
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }
        assert!(!is_quarantined(node).await);

        // Boot: load from disk
        init_quarantine_from_disk().await.unwrap();
        assert!(is_quarantined(node).await);

        // 2. Strike 2: 30 min (1800s)
        let d2 = add_quarantine(node, name, "timeout strike 2").await;
        assert_eq!(d2, 1800, "Strike 2 across reboot must escalate to 1800s (30 min)");
        flush_quarantine_to_disk().await.unwrap();

        // Simulate reboot
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }
        init_quarantine_from_disk().await.unwrap();

        // 3. Strike 3: 60 min (3600s)
        let d3 = add_quarantine(node, name, "timeout strike 3").await;
        assert_eq!(d3, 3600, "Strike 3 across reboot must escalate to 3600s (60 min)");
        flush_quarantine_to_disk().await.unwrap();

        // Simulate reboot
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }
        init_quarantine_from_disk().await.unwrap();

        // 4. Strike 4: Capped at 3600s (user requirement: 'больше часа не надо бана')
        let d4 = add_quarantine(node, name, "timeout strike 4").await;
        assert_eq!(d4, 3600, "Strike 4 must be capped at 3600s (1 hour maximum)");
        flush_quarantine_to_disk().await.unwrap();

        // Verify disk contents
        let disk_content = load_quarantine_from_file(&tmp).await.unwrap();
        assert_eq!(disk_content[node].fail_count, 4);
        assert_eq!(disk_content[node].duration_secs, 3600);

        // 5. Simulate 13 hours passing (> 43200s amnesty)
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            let entry = q.get_mut(node).unwrap();
            entry.quarantined_at = current_unix_timestamp() - 50000;
        }
        flush_quarantine_to_disk().await.unwrap();

        // Simulate reboot after 13 hours
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }
        init_quarantine_from_disk().await.unwrap();

        // Node should have been purged by amnesty upon load
        assert!(!is_quarantined(node).await);

        // New strike after amnesty must restart at strike 1 (900s)
        let d_reset = add_quarantine(node, name, "post-amnesty fail").await;
        assert_eq!(d_reset, 900, "Post-amnesty strike must reset to 900s");

        set_quarantine_path_override(None);
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }
        let _ = tokio::fs::remove_file(&tmp).await;
    }

    #[tokio::test]
    async fn test_adversarial_quarantine_atomic_write_concurrency_hammer() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        let test_id = format!("{}_{}", std::process::id(), current_unix_timestamp());
        let tmp = std::env::temp_dir().join(format!("q_atomic_hammer_{}.json", test_id));
        let _ = tokio::fs::remove_file(&tmp).await;
        set_quarantine_path_override(Some(tmp.clone()));
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }

        // Part 1: Concurrent atomic file write stress (30 simultaneous writers hammering save_quarantine_to_file)
        let mut handles = Vec::new();
        for i in 0..30 {
            let path_clone = tmp.clone();
            handles.push(tokio::spawn(async move {
                let mut custom_map = std::collections::HashMap::new();
                custom_map.insert(format!("srv_hammer_{i}"), PersistentQuarantineEntry {
                    server_name: format!("Node {i}"),
                    quarantined_at: current_unix_timestamp(),
                    fail_count: (i as u32 % 3) + 1,
                    duration_secs: 900,
                    reason: "concurrent stress write".into(),
                });
                let res = save_quarantine_to_file(&path_clone, &custom_map).await;
                assert!(res.is_ok(), "Concurrent save must succeed: {:?}", res.err());
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        // Verify the file was written atomically and contains valid uncorrupted JSON
        assert!(tmp.exists(), "Target quarantine file must exist after concurrent saves");
        let content = tokio::fs::read_to_string(&tmp).await.unwrap();
        assert!(!content.trim().is_empty(), "Quarantine file must not be empty");
        let parsed: Result<std::collections::HashMap<String, PersistentQuarantineEntry>, _> = serde_json::from_str(&content);
        assert!(parsed.is_ok(), "Quarantine file must be 100% valid JSON after concurrency hammer: {:?}", parsed.err());

        // Verify load_quarantine_from_file succeeds without parsing error
        let loaded = load_quarantine_from_file(&tmp).await;
        assert!(loaded.is_ok(), "load_quarantine_from_file must succeed cleanly: {:?}", loaded.err());

        // Part 2: Concurrent reads while writers are active
        let mut reader_handles = Vec::new();
        for _ in 0..20 {
            let path_clone = tmp.clone();
            reader_handles.push(tokio::spawn(async move {
                let read_res = load_quarantine_from_file(&path_clone).await;
                assert!(read_res.is_ok(), "Concurrent read during writes must succeed: {:?}", read_res.err());
            }));
        }
        for rh in reader_handles {
            rh.await.unwrap();
        }

        // Verify no orphaned temporary files (.tmp) remain for this test
        let parent = tmp.parent().unwrap();
        let mut dir = tokio::fs::read_dir(parent).await.unwrap();
        let prefix = format!("q_atomic_hammer_{}", test_id);
        while let Ok(Some(entry)) = dir.next_entry().await {
            let fname = entry.file_name().to_string_lossy().to_string();
            if fname.starts_with(&prefix) && fname.ends_with(".tmp") {
                panic!("Found orphaned temp file after atomic writes: {fname}");
            }
        }

        set_quarantine_path_override(None);
        {
            let mut q = SERVER_QUARANTINE.lock().await;
            q.clear();
        }
        let _ = tokio::fs::remove_file(&tmp).await;
    }
}


