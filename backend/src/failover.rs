//! Фоновый failover-движок (порт check_and_failover из десктопа).
//! 1) Если настроен приоритетный сервер и активен другой — проверить приоритетный;
//!    восстановился (пинг < порог-50) → вернуться на него.
//! 2) Пинг активного: в норме (<= порога) — ничего.
//! 3) Иначе: параллельный пинг кандидатов → переключение на лучший.

use crate::{mihomo, AppState};
use chrono::Local;
use serde::Serialize;
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
        let mut q = self.events.lock().await;
        q.push_back(FailoverEvent {
            time: Local::now().format("%H:%M:%S").to_string(),
            message: message.into(),
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
    let servers = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_server).await?;

    let active = servers.iter().find(|s| s.is_active).cloned();
    let priority = if cfg.failover.priority_server.is_empty() {
        None
    } else {
        servers
            .iter()
            .find(|s| s.id == cfg.failover.priority_server)
            .cloned()
    };

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

    // 3. Поиск лучшего кандидата
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

/// Фоновый цикл: каждые interval_secs (если enabled).
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        loop {
            let interval = {
                let cfg = state.config.read().await;
                cfg.failover.interval_secs.clamp(15, 3600) as u64
            };
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;

            let enabled = state.config.read().await.failover.enabled;
            if !enabled {
                continue;
            }
            let _ = run_check(&state).await;
        }
    });
}
