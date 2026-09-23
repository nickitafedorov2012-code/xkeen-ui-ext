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

/// Одна итерация проверки. Возвращает текст решения (для ручного прогона).
pub async fn run_check(state: &AppState) -> Result<String, String> {
    let cfg = state.config.read().await.clone();
    let threshold = cfg.failover.ping_threshold_ms as i64;
    let servers = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await?;

    // Реальный активный сервер (предпочитаем конкретный прокси перед группами Fastest/Fallback).
    let active = servers
        .iter()
        .find(|s| s.is_active && s.id != "Fastest" && s.id != "Fallback")
        .or_else(|| servers.iter().find(|s| s.is_active))
        .cloned();
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

    // 1. Возврат на более приоритетный сервер из цепочки, если восстановился
    if cfg.failover.auto_restore_priority && !chain.is_empty() {
        if let Some(act) = &active {
            let higher = get_higher_priority_candidates(&chain, &act.id);
            if !higher.is_empty() {
                let higher_ids: Vec<String> = higher.iter().map(|s| s.id.clone()).collect();
                let pings = mihomo::ping_all(&state.http, &cfg, &higher_ids, ping_timeout).await;
                for pri in higher {
                    let ping = pings.get(&pri.id).copied().unwrap_or(-1);
                    if ping > 0 && ping <= hyst {
                        let is_main = pri.id == chain[0].id;
                        let label = if is_main { "Основной" } else { "Более приоритетный" };
                        let msg = format!(
                            "{label} '{}' восстановился (пинг {ping} мс, порог {hyst} мс) — возврат с '{}'",
                            pri.name, act.name
                        );
                        match mihomo::switch_server(&state.http, &cfg, &pri.id).await {
                            Ok(_) => {
                                state.failover_log.push(&msg, true).await;
                                crate::notifications::notify_failover(
                                    &state.http,
                                    &cfg.notifications,
                                    "restore",
                                    "Восстановление основного сервера",
                                    &msg,
                                ).await;
                                return Ok(msg);
                            }
                            Err(e) => {
                                state.failover_log.push(format!("{msg}, но переключение не удалось: {e}"), false).await;
                                return Err(e);
                            }
                        }
                    } else if ping > 0 {
                        checked_higher_notes.push(format!("'{}': пинг {} мс", pri.name, ping));
                    } else {
                        checked_higher_notes.push(format!("'{}': недоступен", pri.name));
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
    let current = mihomo::ping_server(&state.http, &cfg, &active.id, ping_timeout).await;
    if current > 0 && current <= threshold {
        let note = if !checked_higher_notes.is_empty() {
            format!("; приоритетные: {}", checked_higher_notes.join(", "))
        } else {
            String::new()
        };
        let msg = format!("Активный '{}' — пинг {current} мс (в норме{note})", active.name);
        // Не засоряем журнал событий при штатном пинге каждые 15 сек.
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
        let pings = mihomo::ping_all(&state.http, &cfg, &chain_candidates, ping_timeout).await;
        // Первый живой в порядке цепочки (пинг в пределах порога).
        if let Some(id) = chain_candidates
            .iter()
            .find(|id| pings.get(*id).is_some_and(|ms| *ms > 0 && *ms <= threshold))
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
                    crate::notifications::notify_failover(
                        &state.http,
                        &cfg.notifications,
                        "switch",
                        "Сбой сервера — переключение на резерв",
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

    let pings = mihomo::ping_all(&state.http, &cfg, &candidate_ids, ping_timeout).await;
    let mut valid: Vec<(String, i64)> = pings.iter().filter(|(_, ms)| **ms > 0 && **ms <= threshold).map(|(id, ms)| (id.clone(), *ms)).collect();
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
                    crate::notifications::notify_failover(
                        &state.http,
                        &cfg.notifications,
                        "switch",
                        "Сбой сервера — переключение на резерв",
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
                &cfg.notifications,
                "all_down",
                "Критический сбой: Все серверы недоступны!",
                &msg,
            ).await;
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
}
