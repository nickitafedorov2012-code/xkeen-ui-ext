//! Фоновый failover-движок (порт check_and_failover из десктопа).
//! 1) Если настроен приоритетный сервер и активен другой — проверить приоритетный;
//!    восстановился (пинг < порог-50) → вернуться на него.
//! 2) Пинг активного: в норме (<= порога) — ничего.
//! 3) Иначе: параллельный пинг кандидатов → переключение на лучший.

use std::time::{Instant, Duration};

pub(crate) static LAST_SWITCH: tokio::sync::Mutex<Option<Instant>> = tokio::sync::Mutex::const_new(None);
pub(crate) static LAST_WHITELIST_SYNC: tokio::sync::Mutex<Option<(String, Instant)>> =
    tokio::sync::Mutex::const_new(None);

/// F-04: сериализация итераций проверки — фоновый цикл и ручной прогон
/// (POST /api/failover/check) не должны выполняться параллельно (двойной switch).
pub(crate) static CHECK_IN_PROGRESS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

use crate::mihomo::{self, Server};
use crate::AppState;
use chrono::Local;
use serde_json::Value;

pub mod types;
pub mod probe;
pub mod quarantine;
pub mod history;
pub mod switcher;
pub mod scoring;

pub use types::*;
pub use probe::*;
pub use quarantine::*;
pub use history::*;
pub use switcher::*;
pub use scoring::*;

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

/// Причина пропуска сканирования Failover / детектора БС (Баг 104)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkipReason {
    FailoverDisabled,
    ServiceStopped,
    SpeedtestRunning,
    CheckInProgress,
}

impl SkipReason {
    pub fn message(&self, prefix: &str) -> String {
        match self {
            SkipReason::FailoverDisabled => format!("{prefix} пропущен: Failover выключен"),
            SkipReason::ServiceStopped => format!("{prefix} пропущен: служба XKeen остановлена пользователем"),
            SkipReason::SpeedtestRunning => format!("{prefix} пропущен: выполняется Speedtest"),
            SkipReason::CheckInProgress => format!("{prefix} пропущен: проверка уже выполняется"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ScanGuardFlags {
    pub failover_enabled: bool,
    pub service_stopped: bool,
    pub speedtest_running: bool,
    pub check_in_progress: bool,
}

/// Единая проверка охранных флагов для run_check, run_device_check и run_whitelist_detector_check (Баг 104)
pub fn scan_allowed(flags: ScanGuardFlags) -> Result<(), SkipReason> {
    if !flags.failover_enabled {
        return Err(SkipReason::FailoverDisabled);
    }
    if flags.service_stopped {
        return Err(SkipReason::ServiceStopped);
    }
    if flags.speedtest_running {
        return Err(SkipReason::SpeedtestRunning);
    }
    if flags.check_in_progress {
        return Err(SkipReason::CheckInProgress);
    }
    Ok(())
}

/// Одна итерация проверки. Возвращает текст решения (для ручного прогона).
pub async fn run_check(state: &AppState) -> Result<String, String> {
    // DIAG-01 / F-07 / Баг 104: единая проверка через scan_allowed
    let speedtest_res = state.speedtest_lock.try_lock();
    if let Err(reason) = scan_allowed(ScanGuardFlags {
        failover_enabled: true,
        service_stopped: crate::watchdog::is_service_stopped(),
        speedtest_running: speedtest_res.is_err(),
        check_in_progress: false,
    }) {
        if reason == SkipReason::SpeedtestRunning {
            crate::log_i!("Failover пропущен: в данный момент выполняется Speedtest");
        }
        return Ok(reason.message("Failover"));
    }
    let Ok(_speedtest_guard) = speedtest_res else {
        return Ok(SkipReason::SpeedtestRunning.message("Failover"));
    };

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
    let vkturn_ready = state.vkturn.rt.read().await.tunnel_ready();
    let servers = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await?;

    // Реальный активный сервер (предпочитаем конкретный прокси перед группами Fastest/Fallback).
    let active = servers
        .iter()
        .find(|s| s.is_active && s.id != "Fastest" && s.id != "Fallback")
        .or_else(|| servers.iter().find(|s| s.is_active))
        .cloned();

    // Баг 62: замеры здорового активного узла из Best-Ping передаются в общий путь, чтобы дойти
    // до «Сценария Б» (автовозврат) без повторного пинга и двойного учёта трафика БС.
    let mut best_ping_measured: Option<(i64, bool, u64, u64, bool)> = None;
    let chain_has_servers = cfg.failover.priority_chain.iter().any(|id| servers.iter().any(|s| &s.id == id));

    // 0. Режим Best-Ping Failover (строго внутри Failover: выбор лучшего по пингу только при сбое активного узла)
    'best_ping: {
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
            account_whitelist_cycle_delta(dl_bytes, up_bytes);
        }

        let is_active_vkturn = act_id == crate::vkturn::PROXY_NAME || act_name == crate::vkturn::PROXY_NAME;
        let is_manual_active = {
            let last_act = LAST_ACTIVATED_SERVER.lock().await;
            last_act.as_ref().map_or(false, |(id, _, manual)| (id == act_id || (is_active_vkturn && id == crate::vkturn::PROXY_NAME)) && *manual)
        };
        let is_manual_vkturn = is_active_vkturn && is_manual_active;
        let vkturn_tunnel_ready = if is_active_vkturn {
            state.vkturn.rt.read().await.tunnel_ready()
        } else {
            false
        };

        let is_active_healthy = is_active_server_healthy(act_ping, threshold, has_live_traffic, is_blackhole) || (is_manual_vkturn && vkturn_tunnel_ready);

        if is_active_healthy {
            if has_live_traffic && (act_ping <= 0 || act_ping > threshold) {
                crate::log_i!(
                    "Best-Ping Failover: контрольный L7-пробник узла '{}' не ответил (пинг {} мс), но через узел активно течёт реальный клиентский трафик ({} КБ) — узел исправен",
                    act_name, act_ping, dl_bytes / 1024
                );
            }
            if !is_manual_vkturn && best_ping_should_continue_to_restore(
                cfg.failover.auto_restore_priority,
                chain_has_servers,
                cfg.failover.smart_failover_enabled,
                is_active_bs,
            ) {
                best_ping_measured = Some((act_ping, has_live_traffic, dl_bytes, up_bytes, is_blackhole));
                break 'best_ping;
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

        // СЦЕНАРИЙ А: Детектор белых списков (если включен)
        let det_cfg = &cfg.failover.whitelist_detector;
        if det_cfg.enabled && get_detector_state().await == WhitelistDetectorState::Normal {
            // Запускаем детекцию блокировок зарубежных узлов обычной подписки
            match evaluate_and_apply_whitelist_detector(state, &cfg, &servers, "Падение активного VPS").await {
                Ok(Some(detector_msg)) => {
                    return Ok(detector_msg);
                }
                Ok(None) => {}
                Err(e) => {
                    crate::log_e!("[DETECTOR] Ошибка работы детектора БС: {e}");
                }
            }
        }

        let pool_limit = cfg.failover.best_ping_pool_size.clamp(2, 30);
        let mut pool: Vec<String> = Vec::with_capacity(pool_limit);

        if !cfg.failover.priority_chain.is_empty() {
            for id in &cfg.failover.priority_chain {
                if pool.len() >= pool_limit {
                    break;
                }
                if !vkturn_ready && id == crate::vkturn::PROXY_NAME {
                    continue;
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
                if !vkturn_ready
                    && (s.id == crate::vkturn::PROXY_NAME
                        || s.provider.as_deref() == Some(crate::vkturn::PROVIDER_ID))
                {
                    continue;
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
            if !should_best_ping_switch(act_ping, threshold, is_blackhole, best_ms, hysteresis) {
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

    let (current, has_live_traffic, dl_bytes, up_bytes, is_blackhole) = if let Some(m) = best_ping_measured {
        m
    } else {
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
        (current, has_live_traffic, dl_bytes, up_bytes, is_blackhole)
    };

    // Честный учёт трафика БС за эпизод по дельте /connections (без подмены каналов);
    // в ветке Best-Ping он уже выполнен.
    let is_active_bs = is_whitelist_server_full(&active.name, &active.id, active.provider.as_deref(), &cfg);
    if is_active_bs && best_ping_measured.is_none() {
        account_whitelist_cycle_delta(dl_bytes, up_bytes);
    }

    let is_active_vkturn = active.id == crate::vkturn::PROXY_NAME || active.name == crate::vkturn::PROXY_NAME;
    let is_manual_active = {
        let last_act = LAST_ACTIVATED_SERVER.lock().await;
        last_act.as_ref().map_or(false, |(id, _, manual)| (id == &active.id || (is_active_vkturn && id == crate::vkturn::PROXY_NAME)) && *manual)
    };
    let is_manual_vkturn = is_active_vkturn && is_manual_active;
    let vkturn_tunnel_ready = if is_active_vkturn {
        state.vkturn.rt.read().await.tunnel_ready()
    } else {
        false
    };

    // ПРИОРИТЕТ РЕАЛЬНОГО ТРАФИКА:
    // Сервер признаётся живым и исправным:
    // 1) Если клиенты прямо сейчас скачивают трафик (дельта download > 1 КБ) и нет аномалии черной дыры.
    // 2) ИЛИ если L7-пинг в норме (<= threshold) и нет аномалии черной дыры.
    // 3) ИЛИ если активен ручной выбор VK-туннеля и tunnel_ready() == true.
    let is_active_healthy = !is_blackhole && ((current > 0 && current <= threshold) || has_live_traffic || (is_manual_vkturn && vkturn_tunnel_ready));

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

        // Детектор белых списков (если включен) при падении активного VPS в режиме Normal
        let det_cfg = &cfg.failover.whitelist_detector;
        if det_cfg.enabled && get_detector_state().await == WhitelistDetectorState::Normal {
            match evaluate_and_apply_whitelist_detector(state, &cfg, &servers, "Падение активного VPS").await {
                Ok(Some(detector_msg)) => {
                    return Ok(detector_msg);
                }
                Ok(None) => {}
                Err(e) => {
                    crate::log_e!("[DETECTOR] Ошибка работы детектора БС: {e}");
                }
            }
        }

        // Унифицированный отбор кандидата (4-уровневая иерархия без блэкаутов)
        // Баг 103: цепочка «настроена», только если в ней есть живые сейчас серверы кроме активного.
        // Устаревшие после обновления подписки ID больше не блокируют переключение — работает best-of-rest.
        let cand_servers: Vec<Server> = servers
            .iter()
            .filter(|s| {
                vkturn_ready
                    || (s.id != crate::vkturn::PROXY_NAME
                        && s.provider.as_deref() != Some(crate::vkturn::PROVIDER_ID))
            })
            .cloned()
            .collect();
        let chain_candidates: Vec<String> = priority_chain_candidates(&cfg.failover.priority_chain, &cand_servers, &active.id);
        let has_priority_chain = !chain_candidates.is_empty();
        if !has_priority_chain && !cfg.failover.priority_chain.is_empty() {
            state
                .failover_log
                .push("Цепочка приоритетов не содержит доступных сейчас серверов (ID устарели?) — выбираю из остальных", false)
                .await;
        }

        let target_res = if has_priority_chain {
            if chain_candidates.is_empty() {
                Err("Все серверы цепочки приоритетов недоступны (в таймауте или карантине). Переключение заблокировано.".to_string())
            } else {
                let fast_timeout = ping_timeout.clamp(1200, 1500);
                let pings = mihomo::ping_all(&state.http, &cfg, &chain_candidates, fast_timeout).await;
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
                    &cand_servers,
                    &active.id,
                    |id| q_ids.contains(id),
                    |s| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg),
                    cfg.failover.smart_failover_enabled,
                    &pings,
                    threshold,
                )
            }
        } else {
            let effective_providers = cfg.failover.auto_build_provider_chain(
                &crate::routing::parse_provider_names(&tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default()),
                &cfg.whitelist_providers,
            );
            let candidate_shortlist = build_fast_failover_shortlist(&cand_servers, &active.id, &cfg);
            if candidate_shortlist.is_empty() {
                Err("Нет резервных серверов для переключения".to_string())
            } else {
                let fast_timeout = ping_timeout.clamp(1200, 1500);
                let pings = mihomo::ping_all(&state.http, &cfg, &candidate_shortlist, fast_timeout).await;
                let mut q_ids = std::collections::HashSet::new();
                if cfg.failover.quarantine_enabled {
                    for id in &candidate_shortlist {
                        if is_quarantined(id).await {
                            q_ids.insert(id.clone());
                        }
                    }
                }
                select_failover_target_tiered_with_provider_chain(
                    false,
                    &[],
                    &cand_servers,
                    &active.id,
                    |id| q_ids.contains(id),
                    |s| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg),
                    cfg.failover.smart_failover_enabled,
                    &effective_providers,
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
    let should_restore_chain = !is_manual_vkturn && cfg.failover.auto_restore_priority && !chain.is_empty();
    let should_restore_smart = !is_manual_vkturn && cfg.failover.smart_failover_enabled && is_active_bs;

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

        let is_smart_restore_mode = higher.is_empty() && should_restore_smart;
        let canary_snapshot = CANARY_STAGE
            .lock()
            .await
            .as_ref()
            .map(|c| (c.target_id.clone(), c.from_id.clone()));
        let canary_continuation = canary_continuation_candidate(
            canary_snapshot.as_ref().map(|(t, f)| (t.as_str(), f.as_str())),
            &active.id,
            &servers,
        );
        let higher = if is_smart_restore_mode && canary_continuation.is_some() {
            // Идёт Canary-тест возврата с БС: продолжаем именно его на каждом тике.
            // Раньше на тиках без скаутинга (60 с) кандидатов не было — тест стоял, а на
            // следующем скаутинге окно сдвигалось, и тест перезапускался на другом узле:
            // при пуле > 12 VPS возврат с [БС] на обычный VPS не завершался никогда.
            canary_continuation.into_iter().collect()
        } else if is_smart_restore_mode {
            // Умный фейловер: активен сервер белых списков, кандидаты на возврат — живые обычные VPS серверы (Tier 1).
            // Защита от пинг-шторма: скаутинг запускается не чаще раза в 60 сек и проверяет компактное скользящее окно (12 узлов).
            let mut last_scout_guard = LAST_SMART_RESTORE_SCOUT.lock().await;
            let now = Instant::now();
            let should_scout = match *last_scout_guard {
                None => true,
                Some(last) => now.duration_since(last).as_secs() >= SMART_RESTORE_SCOUT_INTERVAL_SECS,
            };

            if should_scout {
                *last_scout_guard = Some(now);
                let all_tier1: Vec<Server> = servers.iter()
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
                    .collect();

                let unreachable_ids: std::collections::HashSet<String> = {
                    let q_guard = SERVER_QUARANTINE.lock().await;
                    q_guard.keys().cloned().collect()
                };
                let all_tier1 = filter_scouting_candidates(&all_tier1, &unreachable_ids);
                let offset = SMART_RESTORE_SCOUT_OFFSET.load(std::sync::atomic::Ordering::Relaxed);
                let (scout_window, next_offset) = select_scouting_window(&all_tier1, offset, SMART_RESTORE_WINDOW_SIZE);
                SMART_RESTORE_SCOUT_OFFSET.store(next_offset, std::sync::atomic::Ordering::Relaxed);
                scout_window
            } else {
                Vec::new()
            }
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

                            // Выполняем реальную загрузку тестового блока 64 КБ полезной нагрузки через локальный слушатель Canary Probe
                            let canary_url = canary_payload_url(&cfg);
                            let download_res = perform_canary_download_with_url(canary_payload_port(), &canary_url, CANARY_PROBE_BYTES, ping_timeout.max(5000)).await;

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

                        let Some(canary) = canary_guard.as_mut() else {
                            return Ok(format!(
                                "Canary Soak Test: состояние '{}' сброшено во время проверки",
                                pri.name
                            ));
                        };
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
                        let canary_url = canary_payload_url(&cfg);
                        if let Ok((final_bytes, _)) = perform_canary_download_with_url(canary_payload_port(), &canary_url, CANARY_PROBE_BYTES, ping_timeout.max(5000)).await {
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

                        let target_is_bs = is_whitelist_server_full(&pri.name, &pri.id, pri.provider.as_deref(), &cfg);
                        let is_main = !chain.is_empty() && pri.id == chain[0].id;
                        let label = if is_main {
                            "Основной"
                        } else if !target_is_bs && is_active_bs && should_restore_smart {
                            "Обычный VPS (Tier 1)"
                        } else {
                            "Более приоритетный"
                        };
                        let msg = format!(
                            "{label} '{}' успешно прошёл {effective_canary_duration_secs}-секундный тест стабильности ({probes_count} проверок, {total_kb} КБ, 0 сбоев) — безопасный возврат с '{}'",
                            pri.name, active.name
                        );
                        let is_from_bs = is_whitelist_server_full(&active.name, &active.id, active.provider.as_deref(), &cfg);
                        if is_manual_vkturn {
                            crate::log_i!("Failover: пропуск переключения Canary soak test — активен ручной выбор VK-туннеля");
                            return Ok("Пропуск переключения: активен ручной выбор VK-туннеля".to_string());
                        }
                        let switch_res = if is_from_bs && !target_is_bs {
                            // Только при переходе с БС на обычный VPS переключаем со сбросом сокетов
                            mihomo::switch_server(&state.http, &cfg, &pri.id).await
                        } else {
                            // При переключении между обычными VPS, а также между серверами [БС] ↔ [БС]
                            // используем мягкий Graceful переход без сброса сокетов клиентов
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
                        if let Some(elapsed) = canary_guard
                            .as_ref()
                            .filter(|c| c.target_id == pri.id)
                            .map(|c| c.started_at.elapsed().as_secs())
                        {
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
    // DIAG-01 / F-07 / Баг 104: единая проверка через scan_allowed
    let speedtest_res = state.speedtest_lock.try_lock();
    if let Err(reason) = scan_allowed(ScanGuardFlags {
        failover_enabled: true,
        service_stopped: crate::watchdog::is_service_stopped(),
        speedtest_running: speedtest_res.is_err(),
        check_in_progress: false,
    }) {
        if reason == SkipReason::SpeedtestRunning {
            crate::log_i!("Per-device failover пропущен: выполняется Speedtest");
        }
        return Ok(reason.message("Per-device failover"));
    }
    let Ok(_speedtest_guard) = speedtest_res else {
        return Ok(SkipReason::SpeedtestRunning.message("Per-device failover"));
    };

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
                match mihomo::switch_group_scoped(&state.http, &cfg, group, &target).await {
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
                match mihomo::switch_group_scoped(&state.http, &cfg, group, best).await {
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
        let mut last_detector_scan = Instant::now();
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

            let (enabled, dev_enabled, dev_interval, detector_enabled, detector_interval_secs) = {
                let cfg = state.config.read().await;
                (
                    cfg.failover.enabled,
                    cfg.failover.device_failover_enabled,
                    cfg.failover.interval_secs.clamp(15, 3600) as u64,
                    cfg.failover.whitelist_detector.enabled,
                    cfg.failover.whitelist_detector.check_interval_mins.clamp(1, 1440) * 60,
                )
            };
            if enabled {
                let _ = run_check(&state).await;
            }
            if dev_enabled && last_device_check.elapsed() >= Duration::from_secs(dev_interval) {
                last_device_check = Instant::now();
                let _ = run_device_check(&state).await;
            }
            // Скан детектора белых списков (при незавершённом подтверждении ускоряется до 15 сек)
            let effective_detector_interval = if has_pending_detector_confirmation() {
                15
            } else {
                detector_interval_secs
            };
            if detector_enabled && last_detector_scan.elapsed() >= Duration::from_secs(effective_detector_interval) {
                last_detector_scan = Instant::now();
                let _ = run_whitelist_detector_check(&state, "Периодический скан детектора БС").await;
            }
        }
    });
}

#[cfg(test)]
mod tests;
