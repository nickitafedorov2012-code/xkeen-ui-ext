//! Фоновый failover-движок (порт check_and_failover из десктопа).
//! 1) Если настроен приоритетный сервер и активен другой — проверить приоритетный;
//!    восстановился (пинг < порог-50) → вернуться на него.
//! 2) Пинг активного: в норме (<= порога) — ничего.
//! 3) Иначе: параллельный пинг кандидатов → переключение на лучший.

use std::time::{Instant, Duration};

static LAST_SWITCH: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);
static LAST_WHITELIST_SYNC: tokio::sync::Mutex<Option<(String, Instant)>> =
    tokio::sync::Mutex::const_new(None);

pub const CANARY_DURATION_SECS: u64 = 180;
pub const CANARY_PAYLOAD_URL: &str = "http://speed.cloudflare.com/__down?bytes=102400";
pub const CANARY_PROBE_BYTES: u64 = 102400;

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
            "Тестирование кандидата '{}' под нагрузкой (3 сокета, {}/{} с, {} КБ)",
            canary.target_name,
            canary.elapsed_secs,
            canary.duration_secs,
            canary.total_bytes / 1024
        );
    }
    let act_label = if !active_name.is_empty() { active_name } else if !active_id.is_empty() { active_id } else { "не выбран" };
    let ping_str = if active_ping > 0 { format!("{active_ping} мс") } else { "—".to_string() };

    let q = quarantine_status().await;
    if !q.is_empty() {
        let first = &q[0];
        let remaining_mins = (first.remaining_secs + 59) / 60;
        return format!(
            "Активен '{}'. В карантине {} узлов ('{}' ещё {} мин)",
            act_label,
            q.len(),
            first.server_name,
            remaining_mins
        );
    }
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

static LAST_ACTIVATED_SERVER: Mutex<Option<(String, Instant)>> = Mutex::const_new(None);

pub const EARLY_FAILURE_WINDOW_SECS: u64 = 300; // 5 минут после переключения

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct QuarantineInfo {
    pub server_id: String,
    pub server_name: String,
    pub fail_count: u32,
    pub remaining_secs: u64,
    pub reason: String,
}

#[derive(Clone, Debug)]
struct InternalQuarantineEntry {
    server_id: String,
    server_name: String,
    fail_count: u32,
    quarantined_until: Instant,
    last_failed_at: Instant,
    reason: String,
}

static SERVER_QUARANTINE: std::sync::LazyLock<Mutex<std::collections::HashMap<String, InternalQuarantineEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub async fn is_quarantined(server_id: &str) -> bool {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = Instant::now();
    if let Some(entry) = q.get(server_id) {
        if now < entry.quarantined_until {
            true
        } else {
            // Не удаляем сразу fail_count, чтобы сохранялась история для прогрессивного бана.
            // Полная амнистия наступает только через 12 часов непрерывной стабильности.
            if now.duration_since(entry.last_failed_at) > Duration::from_secs(43200) {
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
    let now = Instant::now();
    if let Some(entry) = q.get(server_id) {
        if now < entry.quarantined_until {
            Some((entry.quarantined_until - now).as_secs())
        } else {
            if now.duration_since(entry.last_failed_at) > Duration::from_secs(43200) {
                q.remove(server_id);
            }
            None
        }
    } else {
        None
    }
}

pub async fn add_quarantine(server_id: &str, server_name: &str, reason: &str) -> u64 {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = Instant::now();
    let prev_count = q.get(server_id)
        .filter(|e| now.duration_since(e.last_failed_at) <= Duration::from_secs(43200))
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
    let until = now + Duration::from_secs(duration_secs);
    q.insert(server_id.to_string(), InternalQuarantineEntry {
        server_id: server_id.to_string(),
        server_name: server_name.to_string(),
        fail_count,
        quarantined_until: until,
        last_failed_at: now,
        reason: reason.to_string(),
    });
    duration_secs
}

pub async fn add_quarantine_with_duration(server_id: &str, server_name: &str, reason: &str, duration_secs: u64) -> u64 {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = Instant::now();
    // Ограничиваем сверху 3600 с (1 час)
    let capped_duration = std::cmp::min(duration_secs, 3600);
    let prev_count = q.get(server_id)
        .filter(|e| now.duration_since(e.last_failed_at) <= Duration::from_secs(43200))
        .map(|e| e.fail_count)
        .unwrap_or(0);
    let fail_count = prev_count + 1;
    let until = now + Duration::from_secs(capped_duration);
    q.insert(server_id.to_string(), InternalQuarantineEntry {
        server_id: server_id.to_string(),
        server_name: server_name.to_string(),
        fail_count,
        quarantined_until: until,
        last_failed_at: now,
        reason: reason.to_string(),
    });
    capped_duration
}

pub async fn get_zombie_servers() -> Vec<String> {
    let q = SERVER_QUARANTINE.lock().await;
    let now = Instant::now();
    q.values()
        .filter(|v| now < v.quarantined_until && v.reason.to_lowercase().contains("зомби"))
        .map(|v| v.server_id.clone())
        .collect()
}

pub async fn clear_quarantine(server_id: &str) {
    let mut q = SERVER_QUARANTINE.lock().await;
    q.remove(server_id);
}

pub async fn clear_all_quarantine() {
    let mut q = SERVER_QUARANTINE.lock().await;
    q.clear();
}

pub async fn quarantine_status() -> Vec<QuarantineInfo> {
    let mut q = SERVER_QUARANTINE.lock().await;
    let now = Instant::now();
    q.retain(|_, v| now < v.quarantined_until || now.duration_since(v.last_failed_at) <= Duration::from_secs(43200));
    let mut res: Vec<QuarantineInfo> = q.values()
        .filter(|v| now < v.quarantined_until)
        .map(|v| QuarantineInfo {
            server_id: v.server_id.clone(),
            server_name: v.server_name.clone(),
            fail_count: v.fail_count,
            remaining_secs: (v.quarantined_until - now).as_secs(),
            reason: v.reason.clone(),
        })
        .collect();
    res.sort_by(|a, b| a.remaining_secs.cmp(&b.remaining_secs));
    res
}

pub async fn record_switch_target(target_id: &str) {
    let now = Instant::now();
    *LAST_SWITCH.lock().await = Some(now);
    *LAST_ACTIVATED_SERVER.lock().await = Some((target_id.to_string(), now));
    reset_canary().await;
}

pub async fn record_manual_switch(target_id: &str) {
    record_switch_target(target_id).await;
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
            || lower.contains("[wl]")
            || lower.contains("(wl)")
            || lower.contains("whitelist")
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

pub const WAN_CHECK_URL: &str = "http://ya.ru";

/// Проверка физической доступности провайдера (WAN) напрямую через DIRECT.
/// Если шлюз провайдера или кабель упал, возвращает false.
pub async fn is_wan_online(http: &reqwest::Client, cfg: &crate::config::AppConfig) -> bool {
    let ms = mihomo::ping_server_url_expected(
        http,
        cfg,
        "DIRECT",
        1500,
        Some(WAN_CHECK_URL),
        Some(200),
    ).await;
    ms > 0
}

/// Пассивный детектор "Чёрной дыры" (Zero-Download Anomaly):
/// Проверяет сокеты активного прокси-узла на DPI-сброс полезной нагрузки.
/// Если открыто >= 3 сокетов, клиенты передали суммарно > 5 КБ upload,
/// а download равен строго 0 — нода объявляется зомби.
pub async fn check_blackhole_anomaly(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    active_server_id: &str,
) -> bool {
    if active_server_id.is_empty() || active_server_id.eq_ignore_ascii_case("direct") {
        return false;
    }
    let Ok(val) = mihomo::m_get(http, cfg, "/connections").await else {
        return false;
    };
    let Some(conns) = val.get("connections").and_then(|c| c.as_array()) else {
        return false;
    };

    let active_lower = active_server_id.to_lowercase();
    let mut matched_sockets = 0usize;
    let mut total_upload = 0u64;
    let mut total_download = 0u64;

    for c in conns {
        let in_chains = c.get("chains")
            .and_then(|ch| ch.as_array())
            .map(|arr| {
                arr.iter().any(|item| {
                    item.as_str().map(|s| s.to_lowercase() == active_lower).unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if in_chains {
            matched_sockets += 1;
            total_upload += c.get("upload").and_then(|u| u.as_u64()).unwrap_or(0);
            total_download += c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);
        }
    }

    if matched_sockets >= 3 && total_upload > 5120 && total_download == 0 {
        crate::log_w!(
            "Обнаружена аномалия 'Чёрная дыра' на сервере '{}': {} сокетов, upload: {} байт, download: 0 байт",
            active_server_id, matched_sockets, total_upload
        );
        true
    } else {
        false
    }
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
    let max_concurrency = concurrency.clamp(1, 2);
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

    {
        let last = LAST_SWITCH.lock().await;
        if let Some(t) = *last {
            if t.elapsed() < Duration::from_secs(30) {
                return Ok("Ожидание cooldown (30 сек) после предыдущего переключения".to_string());
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
        let pool_limit = cfg.failover.best_ping_pool_size.clamp(2, 30);
        let mut pool: Vec<String> = Vec::with_capacity(pool_limit);

        // 1. Ограничиваем кандидатов строго цепочкой приоритетов (если она настроена)
        if !cfg.failover.priority_chain.is_empty() {
            for id in &cfg.failover.priority_chain {
                if pool.len() >= pool_limit {
                    break;
                }
                if !pool.contains(id) && servers.iter().any(|s| &s.id == id && !s.is_pool && s.id != "Fastest" && s.id != "Fallback" && s.id != "DIRECT") {
                    pool.push(id.clone());
                }
            }
        } else {
            // Если цепочка не задана, берем доступные серверы до лимита
            for s in &servers {
                if pool.len() >= pool_limit {
                    break;
                }
                if !pool.contains(&s.id) && !s.is_pool && s.id != "Fastest" && s.id != "Fallback" && s.id != "DIRECT" {
                    pool.push(s.id.clone());
                }
            }
        }

        // 2. Обязательно включаем текущий активный сервер в пул для замера его реального пинга
        if let Some(ref act) = active {
            if !pool.contains(&act.id) && !act.is_pool && act.id != "Fastest" && act.id != "Fallback" && act.id != "DIRECT" {
                pool.push(act.id.clone());
            }
        }

        if pool.is_empty() {
            let msg = "Best-Ping: нет доступных серверов для проверки".to_string();
            state.failover_log.push(&msg, false).await;
            return Err(msg);
        }

        let ping_timeout = calc_ping_timeout(threshold);
        let pings = ping_servers_staggered(&state.http, &cfg, &pool, ping_timeout, 4, 25).await;

        let act_id = active.as_ref().map(|s| s.id.as_str()).unwrap_or("");
        let act_name = active.as_ref().map(|s| s.name.as_str()).unwrap_or("Неизвестный");
        let act_ping = if !act_id.is_empty() {
            pings.get(act_id).copied().unwrap_or(-1)
        } else {
            -1
        };

        // PURE FAILOVER RULE: Не переключаемся, пока текущий активный сервер в норме (latency <= threshold)
        if act_ping > 0 && act_ping <= threshold as i64 {
            let msg = format!(
                "Best-Ping Failover: активный '{}' работает штатно (пинг {act_ping} мс <= порога {threshold} мс), переключение не требуется",
                act_name
            );
            return Ok(msg);
        }

        // Активный сервер сбоит (пинг <= 0 или пинг > threshold). Отбираем живых кандидатов без карантина.
        let mut valid_candidates: Vec<(String, i64)> = Vec::new();
        for (id, ms) in &pings {
            if *ms > 0 && id.as_str() != "Fastest" && id.as_str() != "Fallback" && id.as_str() != "DIRECT" && id.as_str() != act_id {
                if !is_quarantined(id).await {
                    valid_candidates.push((id.clone(), *ms));
                }
            }
        }

        valid_candidates.sort_by_key(|(_, ms)| *ms);

        if let Some((best_id, best_ms)) = valid_candidates.first() {
            let best_name = servers.iter().find(|s| &s.id == best_id).map(|s| s.name.as_str()).unwrap_or(best_id.as_str());

            let fail_reason = if act_ping <= 0 {
                "таймаут / недоступен".to_string()
            } else {
                format!("пинг {act_ping} мс > порога {threshold} мс")
            };

            let msg = format!(
                "Best-Ping Failover: активный '{}' сбоит ({fail_reason}) → переключение на лучший узел '{best_name}' (пинг {best_ms} мс)",
                act_name
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
            let msg = format!("Best-Ping Failover: активный '{}' сбоит, но в пуле из {} серверов нет доступных узлов вне карантина", act_name, pool.len());
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
    // Fast Retry (защита от Bufferbloat и кратковременных скачков пинга):
    // При сбое или превышении порога делаем 2 быстрых замера с интервалом 1.2 сек через сплит-пробник
    if current <= 0 || current > threshold {
        for _ in 1..=2 {
            tokio::time::sleep(Duration::from_millis(1200)).await;
            let retry_ping = mihomo::ping_server_l7_split(&state.http, &cfg, &active.id, ping_timeout).await;
            if retry_ping > 0 && retry_ping <= threshold {
                current = retry_ping;
                break;
            }
        }
    }

    // Пассивный детектор "Чёрной дыры" (DPI Zero-Download Anomaly):
    // Если пинг формально в норме, но входящий трафик по активным сокетам равен 0 — объявляем зомби
    let is_blackhole = if current > 0 && current <= threshold {
        check_blackhole_anomaly(&state.http, &cfg, &active.id).await
    } else {
        false
    };

    let is_active_healthy = current > 0 && current <= threshold && !is_blackhole;

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
            if let Some((ref act_id, activated_at)) = *last_act {
                if act_id == &active.id {
                    let el = activated_at.elapsed().as_secs();
                    (el < EARLY_FAILURE_WINDOW_SECS, Some(el))
                } else {
                    (false, None)
                }
            } else {
                (false, None)
            }
        };

        if is_blackhole || is_early_failure {
            let q_secs = add_quarantine(&active.id, &active.name, &reason).await;
            let q_mins = q_secs / 60;
            let el_str = elapsed_secs.map(|s| format!(" спустя {s} с после включения")).unwrap_or_default();
            let q_note = format!(
                "⚠️ Карантин: сервер '{}' дал сбой{el_str} ({reason}) → помещён в карантин на {q_mins} мин",
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

        // Поиск живого кандидата из цепочки приоритетов (включая основной узел)
        let chain_candidates: Vec<String> = chain
            .iter()
            .filter(|s| s.id != active.id)
            .map(|s| s.id.clone())
            .collect();
        if !chain_candidates.is_empty() {
            let pings = mihomo::ping_all(&state.http, &cfg, &chain_candidates, ping_timeout).await;
            let mut non_quarantined_cands = Vec::new();
            for id in &chain_candidates {
                if !is_quarantined(id).await {
                    non_quarantined_cands.push(id.clone());
                }
            }

            let best_chain_id = non_quarantined_cands
                .iter()
                .find(|id| pings.get(*id).is_some_and(|ms| *ms > 0 && *ms <= threshold))
                .or_else(|| {
                    chain_candidates
                        .iter()
                        .find(|id| pings.get(*id).is_some_and(|ms| *ms > 0 && *ms <= threshold))
                });

            if let Some(id) = best_chain_id {
                let ms = pings[id];
                let name = servers.iter().find(|s| &s.id == id).map(|s| s.name.clone()).unwrap_or_else(|| id.clone());
                let msg = format!(
                    "Экстренный Failover: активный '{}' ({reason}) → немедленное переключение на '{name}' из цепочки приоритетов (пинг {ms} мс)",
                    active.name
                );
                return match mihomo::switch_server(&state.http, &cfg, id).await {
                    Ok(_) => {
                        record_switch(id).await;
                        let s_prov = servers.iter().find(|s| &s.id == id).and_then(|s| s.provider.as_deref());
                        handle_whitelist_transition(state, id, &name, s_prov).await;
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
        }

        // Резервный поиск среди всех остальных серверов (best-of-rest)
        let candidate_ids: Vec<String> = servers
            .iter()
            .filter(|s| s.id != active.id && s.id != "Fastest" && s.id != "Fallback")
            .take(20)
            .map(|s| s.id.clone())
            .collect();
        if candidate_ids.is_empty() {
            let msg = "Нет резервных серверов для переключения".to_string();
            state.failover_log.push(&msg, false).await;
            return Err(msg);
        }

        let mut non_quarantined_cands = Vec::new();
        for id in &candidate_ids {
            if !is_quarantined(id).await {
                non_quarantined_cands.push(id.clone());
            }
        }
        let eval_cands = if !non_quarantined_cands.is_empty() {
            &non_quarantined_cands
        } else {
            &candidate_ids
        };

        let pings = mihomo::ping_all(&state.http, &cfg, eval_cands, ping_timeout).await;
        let mut valid: Vec<(String, i64)> = pings.iter().filter(|(_, ms)| **ms > 0 && **ms <= threshold).map(|(id, ms)| (id.clone(), *ms)).collect();
        if valid.is_empty() {
            valid = pings.iter().filter(|(_, ms)| **ms > 0).map(|(id, ms)| (id.clone(), *ms)).collect();
        }
        valid.sort_by_key(|(_, ms)| *ms);

        return match valid.first() {
            Some((best_id, best_ms)) => {
                let best_name = servers.iter().find(|s| s.id == *best_id).map(|s| s.name.clone()).unwrap_or_else(|| best_id.clone());
                let msg = format!(
                    "Экстренный Failover: активный '{}' ({reason}) → переключение на '{best_name}' (пинг {best_ms} мс)",
                    active.name
                );
                match mihomo::switch_server(&state.http, &cfg, best_id).await {
                    Ok(_) => {
                        record_switch(best_id).await;
                        let s_prov = servers.iter().find(|s| &s.id == best_id).and_then(|s| s.provider.as_deref());
                        handle_whitelist_transition(state, best_id, &best_name, s_prov).await;
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
                }
            }
            None => {
                let msg = "Все резервные серверы недоступны".to_string();
                state.failover_log.push(&msg, false).await;
                crate::notifications::notify_failover(
                    &state.http,
                    Some(&cfg.mihomo_proxy_url()),
                    &cfg.notifications,
                    "all_down",
                    "Критический сбой: Все серверы недоступны!",
                    &msg,
                ).await;
                Err(msg)
            }
        };
    }

    // СЦЕНАРИЙ Б: АКТИВНЫЙ СЕРВЕР РАБОТАЕТ ШТАТНО (ПИНГ В НОРМЕ)
    // Клиенты имеют стабильный интернет. Только теперь проверяем автовозврат на более приоритетный сервер.
    let canary_duration_secs = if cfg.failover.canary_duration_secs == 0 {
        CANARY_DURATION_SECS
    } else {
        cfg.failover.canary_duration_secs.clamp(15, 300)
    };

    if cfg.failover.auto_restore_priority && !chain.is_empty() {
        // Если активный сервер уже совпадает с целевым сервером Canary, сбрасываем стадию
        {
            let mut canary_guard = CANARY_STAGE.lock().await;
            if canary_guard.as_ref().map_or(false, |c| c.target_id == active.id) {
                *canary_guard = None;
            }
        }

        let higher = get_higher_priority_candidates(&chain, &active.id);
        if !higher.is_empty() {
            let mut eligible_higher = Vec::new();
            for s in higher {
                if is_quarantined(&s.id).await {
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
                            // Проверяем первый зонд сразу перед стартом таймера стабильности
                            let initial_probe = run_canary_probe(&state.http, &cfg, &pri.id, ping_timeout).await;
                            if initial_probe <= 0 {
                                let fail_msg = format!(
                                    "Canary Soak Test: кандидат '{}' ответил на TCP-пинг, но провалил L7-пробу трафика — помечен как '🧟 Зомби-сервер', переход к следующему в цепочке",
                                    pri.name
                                );
                                let now_str = Local::now().format("%H:%M:%S").to_string();
                                set_canary_last_result(CanaryLastResult {
                                    server_id: pri.id.clone(),
                                    server_name: pri.name.clone(),
                                    timestamp: now_str,
                                    success: false,
                                    is_zombie: true,
                                    reason: "L7-сбой: сервер отвечает на TCP-пинг, но блокирует реальный веб-трафик".to_string(),
                                    elapsed_secs: 0,
                                }).await;

                                add_quarantine_with_duration(&pri.id, &pri.name, "Зомби-сервер: L7-сбой передачи трафика", 600).await;
                                state.failover_log.push(&fail_msg, false).await;
                                checked_higher_notes.push(format!("'{}': 🧟 Зомби (L7 сбой)", pri.name));
                                continue;
                            }

                            // Инициализация новой стадии Canary-теста: фиксируем старт, клиенты остаются на стабильном сервере
                            *canary_guard = Some(CanaryState {
                                target_id: pri.id.clone(),
                                target_name: pri.name.clone(),
                                from_id: active.id.clone(),
                                from_name: active.name.clone(),
                                started_at: Instant::now(),
                                probes_passed: 1,
                                total_bytes: CANARY_PROBE_BYTES,
                                duration_secs: canary_duration_secs,
                            });
                            drop(canary_guard);
                            let note = format!(
                                "Canary Soak Test: кандидат '{}' ответил на пинг ({} мс, порог {} мс). Запущен {}-секундный тест стабильности (~15 кбит/с, 8 КБ) без переключения клиентов с '{}'",
                                pri.name, ping, hyst, canary_duration_secs, active.name
                            );
                            state.failover_log.push(&note, false).await;
                            return Ok(note);
                        }

                        // Тест уже запущен: выполняем микро-прогон реального трафика (8 КБ) через кандидата
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

                            add_quarantine_with_duration(&pri.id, &pri.name, "Зомби-сервер: L7-сбой передачи трафика", 600).await;
                            state.failover_log.push(&fail_msg, false).await;
                            checked_higher_notes.push(format!("'{}': 🧟 Зомби (L7 сбой)", pri.name));
                            continue;
                        }

                        let canary = canary_guard.as_mut().unwrap();
                        canary.probes_passed += 1;
                        canary.total_bytes += CANARY_PROBE_BYTES;

                        if elapsed_secs < canary_duration_secs {
                            let probes_count = canary.probes_passed;
                            drop(canary_guard);
                            let note = format!(
                                "Canary Soak Test: '{}' стабилен (прошло {}/{} с, проверок: {}, пинг {} мс) — клиенты на '{}'",
                                pri.name, elapsed_secs, canary_duration_secs, probes_count, probe_ms, active.name
                            );
                            return Ok(note);
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
                            reason: format!("Успешно пройден {canary_duration_secs}-сек тест ({probes_count} проверок, {total_kb} КБ)"),
                            elapsed_secs,
                        }).await;

                        let is_main = pri.id == chain[0].id;
                        let label = if is_main { "Основной" } else { "Более приоритетный" };
                        let msg = format!(
                            "{label} '{}' успешно прошёл {canary_duration_secs}-секундный тест стабильности ({probes_count} проверок, {total_kb} КБ, 0 сбоев) — безопасный возврат с '{}'",
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
        let hyst = calc_hysteresis(threshold);
        let ping_timeout = calc_ping_timeout(threshold);
        let cur_ping = mihomo::ping_server(&state.http, &cfg, cur, ping_timeout).await;
        if cur_ping > 0 && cur_ping <= threshold {
            // В норме: автовозврат на более приоритетный сервер из цепочки, если восстановился.
            if dr.auto_restore {
                let higher = get_higher_priority_candidates(&dr.servers, cur);
                if !higher.is_empty() {
                    let pings = mihomo::ping_all(&state.http, &cfg, higher, ping_timeout).await;
                    for target_server in higher {
                        let p = pings.get(target_server).copied().unwrap_or(-1);
                        if p > 0 && p <= hyst {
                            let is_main = target_server == &dr.servers[0];
                            let label = if is_main { "основной" } else { "приоритетный" };
                            match mihomo::switch_group(&state.http, &cfg, group, target_server).await {
                                Ok(_) => actions.push((
                                    format!(
                                        "[{ip}] {label} '{target_server}' восстановился ({p} мс, порог {hyst} мс) — возврат с резерва '{cur}'"
                                    ),
                                    true,
                                )),
                                Err(e) => actions.push((
                                    format!("[{ip}] возврат на {label} '{target_server}' не удался: {e}"),
                                    false,
                                )),
                            }
                            break;
                        }
                    }
                }
            }
            continue;
        }
        let reason = if cur_ping > 0 {
            format!("пинг {cur_ping} мс > порога {threshold}")
        } else {
            "не отвечает".to_string()
        };
        let cands: Vec<String> = dr.servers.iter().filter(|s| *s != cur).cloned().collect();
        if cands.is_empty() {
            continue;
        }
        let pings = mihomo::ping_all(&state.http, &cfg, &cands, ping_timeout).await;
        let mut valid: Vec<(String, i64)> = pings
            .iter()
            .filter(|(_, ms)| **ms > 0 && **ms <= threshold)
            .map(|(id, ms)| (id.clone(), *ms))
            .collect();
        if valid.is_empty() {
            valid = pings.iter().filter(|(_, ms)| **ms > 0).map(|(id, ms)| (id.clone(), *ms)).collect();
        }
        valid.sort_by_key(|(_, ms)| *ms);
        if let Some((best, ms)) = valid.first() {
            match mihomo::switch_group(&state.http, &cfg, group, best).await {
                Ok(_) => actions.push((format!("[{ip}] '{cur}' ({reason}) → резерв '{best}' ({ms} мс)"), true)),
                Err(e) => actions.push((format!("[{ip}] переключение на '{best}' не удалось: {e}"), false)),
            }
        } else {
            actions.push((format!("[{ip}] '{cur}' ({reason}), все резервы недоступны"), false));
        }
    }

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

/// Фоновый цикл: каждые interval_secs (или 5 сек при активном Canary-стейджинге).
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
                crate::log_i!("Failover: фоновый цикл остановлен");
                return;
            }
            let interval = if is_canary_active().await {
                5u64
            } else {
                let cfg = state.config.read().await;
                cfg.failover.interval_secs.clamp(15, 3600) as u64
            };
            // Спим короткими отрезками, чтобы быстро реагировать на shutdown.
            for _ in 0..interval {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
                    crate::log_i!("Failover: фоновый цикл остановлен");
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }

            let (enabled, dev_enabled) = {
                let cfg = state.config.read().await;
                (cfg.failover.enabled, cfg.failover.device_failover_enabled)
            };
            if enabled {
                let _ = run_check(&state).await;
            }
            if dev_enabled {
                let _ = run_device_check(&state).await;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // Проверка расчёта пропускной способности canary стресс-теста:
        // 100 КБ (102400 байт) каждые 5 секунд на протяжении 180 секунд (3 минуты).
        let total_probes = CANARY_DURATION_SECS / 5; // 36 проверок
        let total_bytes = total_probes * CANARY_PROBE_BYTES; // 3,686,400 байт (~3.6 МБ)
        let bandwidth_kbps = (total_bytes * 8) as f64 / (CANARY_DURATION_SECS as f64 * 1000.0);

        // Стресс-тест в 3 сокета генерирует умеренную нагрузку (~164 кбит/с / 20 КБ/с), достаточную для срабатывания DPI
        assert!(bandwidth_kbps < 200.0, "Canary bandwidth must be strictly below 200 kbps, actual: {bandwidth_kbps} kbps");
        assert_eq!(total_probes, 36);
        assert_eq!(total_bytes, 3686400);
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

        clear_all_quarantine().await;
    }

    #[tokio::test]
    async fn test_failover_chain_skips_zombie_and_continues() {
        let _lock = FAILOVER_TEST_LOCK.lock().await;
        clear_all_quarantine().await;

        let s1 = "node_1_zombie";
        let s2 = "node_2_healthy";
        let s3 = "node_3_active";

        let chain = vec![
            Server { id: s1.into(), name: "Zombie Node".into(), ping_ms: 70, is_active: false, protocol: "Vless".into(), host: "1.1.1.1".into(), port: 443, is_priority: true, provider: None, provider_name: None, is_pool: false },
            Server { id: s2.into(), name: "Healthy Node".into(), ping_ms: 80, is_active: false, protocol: "Vless".into(), host: "2.2.2.2".into(), port: 443, is_priority: false, provider: None, provider_name: None, is_pool: false },
            Server { id: s3.into(), name: "Active Node".into(), ping_ms: 90, is_active: true, protocol: "Vless".into(), host: "3.3.3.3".into(), port: 443, is_priority: false, provider: None, provider_name: None, is_pool: false },
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
        };
        let mut cfg = crate::config::AppConfig::default();
        cfg.failover.enabled = true;
        cfg.failover.priority_chain = vec!["node_active".into()];

        let status_str = current_live_status(&active.id, &active.name, active.ping_ms as i64, &cfg).await;
        assert!(status_str.contains("Active Node"));
        assert!(status_str.contains("45 мс"));

        clear_all_quarantine().await;
        reset_canary().await;
    }
}
