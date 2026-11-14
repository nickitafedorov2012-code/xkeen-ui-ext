use std::collections::VecDeque;
use chrono::Local;
use serde::Serialize;
use tokio::sync::Mutex;
use super::*;

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

pub(crate) async fn record_switch(target_id: &str) {
    record_switch_target(target_id).await;
}
