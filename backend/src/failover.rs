//! Фоновый failover-движок (порт check_and_failover из десктопа).
//! 1) Если настроен приоритетный сервер и активен другой — проверить приоритетный;
//!    восстановился (пинг < порог-50) → вернуться на него.
//! 2) Пинг активного: в норме (<= порога) — ничего.
//! 3) Иначе: параллельный пинг кандидатов → переключение на лучший.

use crate::mihomo::{self, Server};
use crate::AppState;
use chrono::Local;
use serde::Serialize;
use serde_json::Value;
use std::collections::VecDeque;
use tokio::sync::Mutex;

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

/// Одна итерация проверки. Возвращает текст решения (для ручного прогона).
pub async fn run_check(state: &AppState) -> Result<String, String> {
    let cfg = state.config.read().await.clone();
    let threshold = cfg.failover.ping_threshold_ms as i64;
    let servers = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await?;

    let active = servers.iter().find(|s| s.is_active).cloned();
    // Цепочка приоритетов: только существующие сейчас серверы, по порядку.
    let chain: Vec<Server> = cfg
        .failover
        .priority_chain
        .iter()
        .filter_map(|id| servers.iter().find(|s| &s.id == id).cloned())
        .collect();
    let priority = chain.first().cloned();

    // 1. Возврат на приоритетный, если восстановился
    if let (Some(pri), Some(act)) = (&priority, &active) {
        if pri.id != act.id && cfg.failover.auto_restore_priority {
            let ping = mihomo::ping_server(&state.http, &cfg, &pri.id, 1500).await;
            if ping > 0 && ping < threshold - 50 {
                let msg = format!(
                    "Приоритетный '{}' восстановился (пинг {ping} мс) — возврат",
                    pri.name
                );
                match mihomo::switch_server(&state.http, &cfg, &pri.id).await {
                    Ok(_) => {
                        state.failover_log.push(&msg, true).await;
                        return Ok(msg);
                    }
                    Err(e) => {
                        state.failover_log.push(format!("{msg}, но переключение не удалось: {e}"), false).await;
                        return Err(e);
                    }
                }
            }
        }
    }

    // 2. Проверка активного
    let Some(active) = active else {
        let msg = "Активный сервер не определён".to_string();
        state.failover_log.push(&msg, false).await;
        return Err(msg);
    };
    let current = mihomo::ping_server(&state.http, &cfg, &active.id, 1500).await;
    if current > 0 && current <= threshold {
        let msg = format!("Активный '{}' — пинг {current} мс (в норме)", active.name);
        state.failover_log.push(&msg, false).await;
        return Ok(msg);
    }
    let reason = if current > 0 {
        format!("высокий пинг {current} мс (> {threshold})")
    } else {
        "сервер не отвечает".to_string()
    };

    // 3. Кандидат: сначала цепочка приоритетов по порядку, затем best-of-rest.
    let chain_candidates: Vec<String> = chain
        .iter()
        .filter(|s| s.id != active.id)
        .map(|s| s.id.clone())
        .collect();
    if !chain_candidates.is_empty() {
        let pings = mihomo::ping_all(&state.http, &cfg, &chain_candidates, 1500).await;
        // Первый живой в порядке цепочки (пинг в пределах порога).
        if let Some(id) = chain_candidates
            .iter()
            .find(|id| pings.get(*id).is_some_and(|ms| *ms > 0 && *ms < threshold))
        {
            let ms = pings[id];
            let name = servers.iter().find(|s| &s.id == id).map(|s| s.name.clone()).unwrap_or_else(|| id.clone());
            let msg = format!(
                "Активный '{}' ({reason}) → переключение на '{name}' из цепочки приоритетов (пинг {ms} мс)",
                active.name
            );
            return match mihomo::switch_server(&state.http, &cfg, id).await {
                Ok(_) => {
                    state.failover_log.push(&msg, true).await;
                    Ok(msg)
                }
                Err(e) => {
                    state.failover_log.push(format!("{msg} — ошибка: {e}"), false).await;
                    Err(e)
                }
            };
        }
    }

    let candidate_ids: Vec<String> = servers
        .iter()
        .filter(|s| s.id != active.id)
        .take(20)
        .map(|s| s.id.clone())
        .collect();
    if candidate_ids.is_empty() {
        let msg = "Нет резервных серверов для переключения".to_string();
        state.failover_log.push(&msg, false).await;
        return Err(msg);
    }

    let pings = mihomo::ping_all(&state.http, &cfg, &candidate_ids, 1500).await;
    let mut valid: Vec<(String, i64)> = pings.iter().filter(|(_, ms)| **ms > 0 && **ms < threshold).map(|(id, ms)| (id.clone(), *ms)).collect();
    if valid.is_empty() {
        valid = pings.iter().filter(|(_, ms)| **ms > 0).map(|(id, ms)| (id.clone(), *ms)).collect();
    }
    valid.sort_by_key(|(_, ms)| *ms);

    match valid.first() {
        Some((best_id, best_ms)) => {
            let best_name = servers.iter().find(|s| s.id == *best_id).map(|s| s.name.clone()).unwrap_or_else(|| best_id.clone());
            let msg = format!(
                "Активный '{}' ({reason}) → переключение на '{best_name}' (пинг {best_ms} мс)",
                active.name
            );
            match mihomo::switch_server(&state.http, &cfg, best_id).await {
                Ok(_) => {
                    state.failover_log.push(&msg, true).await;
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
            Err(msg)
        }
    }
}

/// Per-device проверка: для каждого устройства с цепочкой серверов —
/// пинг текущего; отвалился или пинг > порога → переключение на следующий
/// живой из цепочки; автовозврат на основной, когда восстановился.
pub async fn run_device_check(state: &AppState) -> Result<String, String> {
    let cfg = state.config.read().await.clone();
    if cfg.device_routing.is_empty() {
        return Ok("Нет устройств с резервными цепочками".to_string());
    }
    let proxies = mihomo::get_proxies(&state.http, &cfg).await?;
    let rules = mihomo::m_get(&state.http, &cfg, "/rules").await.unwrap_or(Value::Null);
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
        let cur_ping = mihomo::ping_server(&state.http, &cfg, cur, 1500).await;
        if cur_ping > 0 && cur_ping <= threshold {
            // В норме: автовозврат на основной, если сейчас на резерве и основной ожил.
            if dr.auto_restore && cur != dr.servers[0] {
                let p = mihomo::ping_server(&state.http, &cfg, &dr.servers[0], 1500).await;
                // Гистерезис как в run_check: возвращаемся только при уверенном
                // восстановлении (порог - 50 мс), иначе флаппинг на границе порога.
                let hyst = (threshold - 50).max(1);
                if p > 0 && p <= hyst {
                    match mihomo::switch_group(&state.http, &cfg, group, &dr.servers[0]).await {
                        Ok(_) => actions.push((
                            format!(
                                "[{ip}] основной '{}' восстановился ({p} мс) — возврат с резерва '{cur}'",
                                dr.servers[0]
                            ),
                            true,
                        )),
                        Err(e) => actions.push((format!("[{ip}] возврат на основной не удался: {e}"), false)),
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
        let pings = mihomo::ping_all(&state.http, &cfg, &cands, 1500).await;
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

/// Фоновый цикл: каждые interval_secs (если enabled).
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Relaxed) {
                crate::log_i!("Failover: фоновый цикл остановлен");
                return;
            }
            let interval = {
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
