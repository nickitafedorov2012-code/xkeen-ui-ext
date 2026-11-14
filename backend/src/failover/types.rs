use std::time::Instant;
use serde::{Deserialize, Serialize};
use crate::mihomo::Server;

pub const CANARY_DURATION_SECS: u64 = 180;
pub const CANARY_PAYLOAD_PORT: u16 = crate::routing::CANARY_PROBE_PORT;
pub const CANARY_PAYLOAD_BYTES: u64 = 65536; // Честные 64 КБ (FO-CNR-02)
pub const CANARY_PAYLOAD_URL: &str = "http://speed.cloudflare.com/__down?bytes=65536";
pub const CANARY_PROBE_BYTES: u64 = CANARY_PAYLOAD_BYTES;

pub fn canary_payload_port() -> u16 {
    crate::routing::CANARY_PROBE_PORT
}

pub fn canary_payload_url(cfg: &crate::config::AppConfig) -> String {
    let custom = cfg.failover.canary_url.trim();
    if custom.is_empty() {
        CANARY_PAYLOAD_URL.to_string()
    } else {
        custom.to_string()
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CanaryDownloadResult {
    pub success: bool,
    pub bytes_downloaded: u64,
    pub duration_ms: u64,
    pub speed_kbps: f64,
    pub status_code: u16,
    pub error: Option<String>,
}

/// Выполнение реальной загрузки 64 КБ полезной нагрузки через локальный слушатель Canary Probe.
/// Проверяет статус 200, точный объем полученных байт (>= chunk_bytes) и возвращает (байты, скорость в кбит/с).
pub async fn perform_canary_download_with_url(
    port: u16,
    target_url: &str,
    chunk_bytes: u64,
    timeout_ms: u64,
) -> Result<(u64, f64), String> {
    let res = perform_canary_download_detailed(port, target_url, chunk_bytes, timeout_ms).await?;
    Ok((res.bytes_downloaded, res.speed_kbps))
}

pub async fn perform_canary_download(
    port: u16,
    chunk_bytes: u64,
    timeout_ms: u64,
) -> Result<(u64, f64), String> {
    let target_url = format!("http://speed.cloudflare.com/__down?bytes={chunk_bytes}");
    perform_canary_download_with_url(port, &target_url, chunk_bytes, timeout_ms).await
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

pub(crate) static CANARY_STAGE: tokio::sync::Mutex<Option<CanaryState>> = tokio::sync::Mutex::const_new(None);

/// Время последнего цикла скаутинга Smart Restore (для интервала 60-90 сек).
pub(crate) static LAST_SMART_RESTORE_SCOUT: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);
/// Текущее смещение sliding window ротации при скаутинге Tier 1.
pub(crate) static SMART_RESTORE_SCOUT_OFFSET: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
/// Размер окна скаутинга (10-12 серверов).
pub const SMART_RESTORE_WINDOW_SIZE: usize = 12;
/// Интервал между запусками фонового скаутинга Smart Restore (60 сек).
/// В режиме БС обычный VPS часто недоступен, поэтому редкий скаутинг раз в 60с допустим для
/// обнаружения восстановления, но заведомо недоступные кандидаты предварительно отфильтровываются.
pub const SMART_RESTORE_SCOUT_INTERVAL_SECS: u64 = 60;

/// Отфильтровывает из кандидатов скаутинга в БС заведомо недоступные VPS-узлы (в карантине / с отрицательным последним пингом).
pub fn filter_scouting_candidates(
    candidates: &[Server],
    known_unreachable_ids: &std::collections::HashSet<String>,
) -> Vec<Server> {
    candidates
        .iter()
        .filter(|s| !known_unreachable_ids.contains(&s.id))
        .cloned()
        .collect()
}

/// Функция отбора окна скаутинга для Smart Restore.
/// Первыми идут сервера с ранее успешным/низким пингом, затем round-robin скользящее окно.
pub fn select_scouting_window(
    servers: &[Server],
    offset: usize,
    window_size: usize,
) -> (Vec<Server>, usize) {
    if servers.is_empty() {
        return (Vec::new(), 0);
    }
    if servers.len() <= window_size {
        return (servers.to_vec(), 0);
    }

    // Разделяем на ранее успешные (ping > 0) и остальные
    let mut successful: Vec<Server> = Vec::new();
    let mut others: Vec<Server> = Vec::new();

    for s in servers {
        let p = crate::mihomo::cached_ping(&s.id).unwrap_or(s.ping_ms);
        if p > 0 {
            successful.push(s.clone());
        } else {
            others.push(s.clone());
        }
    }
    successful.sort_by_key(|s| crate::mihomo::cached_ping(&s.id).unwrap_or(s.ping_ms));

    let mut result = Vec::with_capacity(window_size);

    // До половины окна (до window_size / 2) берем из ранее успешных нод
    let max_success = (window_size / 2).min(successful.len());
    for s in successful.iter().take(max_success) {
        result.push(s.clone());
    }

    // Оставшиеся ноды: собираем список кандидатов для round-robin
    let mut rr_candidates = Vec::new();
    for s in successful.iter().skip(max_success) {
        rr_candidates.push(s.clone());
    }
    for s in &others {
        rr_candidates.push(s.clone());
    }

    let needed = window_size.saturating_sub(result.len());
    let mut next_offset = offset;
    if !rr_candidates.is_empty() && needed > 0 {
        let start = offset % rr_candidates.len();
        for i in 0..needed {
            let idx = (start + i) % rr_candidates.len();
            let cand = &rr_candidates[idx];
            if !result.iter().any(|r| r.id == cand.id) {
                result.push(cand.clone());
            }
        }
        next_offset = (start + needed) % rr_candidates.len();
    }

    (result, next_offset)
}

/// Сервер, Canary-тест которого нужно продолжить на текущем тике умного возврата.
/// `canary` — (target_id, from_id) активного теста. Тест продолжается, только если он
/// запущен с текущего активного узла и целевой сервер всё ещё существует.
pub fn canary_continuation_candidate(
    canary: Option<(&str, &str)>,
    active_id: &str,
    servers: &[Server],
) -> Option<Server> {
    let (target_id, from_id) = canary?;
    if from_id != active_id || target_id == active_id {
        return None;
    }
    servers.iter().find(|s| s.id == target_id).cloned()
}

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

/// Суммарная дельта трафика за цикл (без переполнения).
pub fn whitelist_cycle_delta(dl_bytes: u64, up_bytes: u64) -> u64 {
    dl_bytes.saturating_add(up_bytes)
}

/// Учёт дельты трафика активного узла [БС] за цикл failover.
/// Пополняет ТОЛЬКО счётчик эпизода БС. Накопительный TOTAL_WHITELIST_BYTES ведёт
/// исключительно поток /traffic в traffic.rs — раньше failover добавлял туда ту же
/// дельту /connections повторно, и объём БС на дашборде завышался примерно вдвое.
pub fn account_whitelist_cycle_delta(dl_bytes: u64, up_bytes: u64) -> u64 {
    let delta = whitelist_cycle_delta(dl_bytes, up_bytes);
    if delta > 0 {
        record_whitelist_episode_bytes(delta);
    }
    delta
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
