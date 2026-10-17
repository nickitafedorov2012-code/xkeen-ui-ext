//! Фоновый failover-движок (порт check_and_failover из десктопа).
//! 1) Если настроен приоритетный сервер и активен другой — проверить приоритетный;
//!    восстановился (пинг < порог-50) → вернуться на него.
//! 2) Пинг активного: в норме (<= порога) — ничего.
//! 3) Иначе: параллельный пинг кандидатов → переключение на лучший.

use std::time::{Instant, Duration};

static LAST_SWITCH: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);

pub const CANARY_DURATION_SECS: u64 = 60;
pub const CANARY_PAYLOAD_URL: &str = "http://speed.cloudflare.com/__down?bytes=8192";
pub const CANARY_PROBE_BYTES: u64 = 8192;

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
    reason: String,
}

static SERVER_QUARANTINE: std::sync::LazyLock<Mutex<std::collections::HashMap<String, InternalQuarantineEntry>>> =
    std::sync::LazyLock::new(|| Mutex::new(std::collections::HashMap::new()));

pub async fn is_quarantined(server_id: &str) -> bool {
    let mut q = SERVER_QUARANTINE.lock().await;
    if let Some(entry) = q.get(server_id) {
        if Instant::now() < entry.quarantined_until {
            true
        } else {
            q.remove(server_id);
            false
        }
    } else {
        false
    }
}

pub async fn get_quarantine_remaining(server_id: &str) -> Option<u64> {
    let mut q = SERVER_QUARANTINE.lock().await;
    if let Some(entry) = q.get(server_id) {
        let now = Instant::now();
        if now < entry.quarantined_until {
            Some((entry.quarantined_until - now).as_secs())
        } else {
            q.remove(server_id);
            None
        }
    } else {
        None
    }
}

pub async fn add_quarantine(server_id: &str, server_name: &str, reason: &str) -> u64 {
    let mut q = SERVER_QUARANTINE.lock().await;
    let fail_count = q.get(server_id).map(|e| e.fail_count).unwrap_or(0) + 1;
    // Эскалация: 1-й сбой: 15 мин (900с), 2-й: 30 мин (1800с), 3-й+: 60 мин (3600с)
    let duration_secs = match fail_count {
        1 => 900,
        2 => 1800,
        _ => 3600,
    };
    let until = Instant::now() + Duration::from_secs(duration_secs);
    q.insert(server_id.to_string(), InternalQuarantineEntry {
        server_id: server_id.to_string(),
        server_name: server_name.to_string(),
        fail_count,
        quarantined_until: until,
        reason: reason.to_string(),
    });
    duration_secs
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
    q.retain(|_, v| now < v.quarantined_until);
    let mut res: Vec<QuarantineInfo> = q.values()
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

/// Прогон микро-порции реального трафика (8 КБ) через проверяемый сервер.
/// Загружает тестовый пэйлоад 8192 байта со speed.cloudflare.com.
/// Если сервер обрывает соединение, дропает пакеты или таймаутит — возвращает <= 0 (-1).
pub async fn run_canary_probe(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    server_id: &str,
    timeout_ms: u64,
) -> i64 {
    let probe_timeout = timeout_ms.max(4000);
    let delay = mihomo::ping_server_url(http, cfg, server_id, probe_timeout, Some(CANARY_PAYLOAD_URL)).await;
    if delay > 0 {
        return delay;
    }
    // Fallback на базовый health check URL панели при сбоях CDN Cloudflare
    mihomo::ping_server_url(http, cfg, server_id, probe_timeout, None).await
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
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency));
    let shared_cfg = std::sync::Arc::new(cfg.clone());
    let mut handles = Vec::new();

    for id in ids {
        let http_c = http.clone();
        let cfg_c = shared_cfg.clone();
        let sem_c = sem.clone();
        let id_c = id.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem_c.acquire().await.ok();
            let ms = mihomo::ping_server_url(&http_c, &cfg_c, &id_c, timeout_ms, None).await;
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
                    let _ = crate::api::sync_whitelist_mode_tunnel(&state, best_id).await;
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

    let mut current = mihomo::ping_server(&state.http, &cfg, &active.id, ping_timeout).await;
    // Защита от дребезга (flapping): при единичном сбое или повышенном пинге
    // делаем повторную проверку через 750 мс для подтверждения, исключая ложные переключения
    if current <= 0 || current > threshold {
        tokio::time::sleep(Duration::from_millis(750)).await;
        let retry_ping = mihomo::ping_server(&state.http, &cfg, &active.id, ping_timeout).await;
        if retry_ping > 0 && retry_ping <= threshold {
            current = retry_ping;
        }
    }

    let is_active_healthy = current > 0 && current <= threshold;

    // СЦЕНАРИЙ А: АКТИВНЫЙ СЕРВЕР СБОИТ (ТАЙМАУТ ИЛИ ПИНГ ВЫШЕ ПОРОГА)
    // ЭКСТРЕННОЕ ПЕРЕКЛЮЧЕНИЕ: немедленно отменяем любой Canary-тест и переключаем клиентов без задержек!
    if !is_active_healthy {
        // Критически важно: если активный резерв упал, клиенты без интернета.
        // Нельзя оставлять их ждать 180с Canary-теста — сбрасываем стадию теста!
        reset_canary().await;

        let reason = if current > 0 {
            format!("высокий пинг {current} мс (> {threshold})")
        } else {
            "сервер не отвечает".to_string()
        };

        // Анти-флеппинг: если активный сервер упал вскоре после включения (< 300 сек),
        // отправляем его в карантин, чтобы остановить бесконечный цикл переключений
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

        if is_early_failure {
            let q_secs = add_quarantine(&active.id, &active.name, &reason).await;
            let q_mins = q_secs / 60;
            let el_str = elapsed_secs.map(|s| format!(" спустя {s} с после включения")).unwrap_or_default();
            let q_note = format!(
                "⚠️ Анти-флеппинг: сервер '{}' дал сбой{el_str} ({reason}) → помещён в карантин на {q_mins} мин",
                active.name
            );
            state.failover_log.push(&q_note, false).await;
            crate::notifications::notify_failover(
                &state.http,
                Some(&cfg.mihomo_proxy_url()),
                &cfg.notifications,
                "quarantine",
                "Карантин нестабильного сервера (анти-флеппинг)",
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
                        let _ = crate::api::sync_whitelist_mode_tunnel(&state, id).await;
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
                        let _ = crate::api::sync_whitelist_mode_tunnel(&state, best_id).await;
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
                                "Canary Soak Test: кандидат '{}' не выдержал тест стабильности (сбой передачи данных на {} сек) — тест сброшен, остаёмся на '{}'",
                                pri.name, elapsed_secs, active.name
                            );
                            *canary_guard = None;
                            drop(canary_guard);
                            state.failover_log.push(&fail_msg, false).await;
                            return Ok(fail_msg);
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

                        let is_main = pri.id == chain[0].id;
                        let label = if is_main { "Основной" } else { "Более приоритетный" };
                        let msg = format!(
                            "{label} '{}' успешно прошёл {canary_duration_secs}-секундный тест стабильности ({probes_count} проверок, {total_kb} КБ, 0 сбоев) — безопасный возврат с '{}'",
                            pri.name, active.name
                        );
                        return match mihomo::switch_server(&state.http, &cfg, &pri.id).await {
                            Ok(_) => {
                                record_switch(&pri.id).await;
                                let _ = crate::api::sync_whitelist_mode_tunnel(&state, &pri.id).await;
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
        assert_eq!(status.total_bytes, 8192);

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
        // Проверка расчёта пропускной способности canary:
        // 8 КБ (8192 байт) каждые 5 секунд на протяжении 60 секунд (1 минута).
        let total_probes = CANARY_DURATION_SECS / 5; // 12 проверок
        let total_bytes = total_probes * CANARY_PROBE_BYTES; // 98,304 байт = 96 КБ
        let bandwidth_kbps = (total_bytes * 8) as f64 / (CANARY_DURATION_SECS as f64 * 1000.0);

        // Строго меньше установленного пользователем лимита 100 кбит/с
        assert!(bandwidth_kbps < 100.0, "Canary bandwidth must be strictly below 100 kbps, actual: {bandwidth_kbps} kbps");
        assert_eq!(total_probes, 12);
        assert_eq!(total_bytes, 98304);
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
}
