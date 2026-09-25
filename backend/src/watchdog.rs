use std::path::Path;
use tokio::time::{sleep, Duration};

use crate::{log_i, log_w, mihomo, override_sync, routing, AppState};

pub static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn shutdown() {
    SHUTDOWN.store(true, std::sync::atomic::Ordering::Release);
}

pub fn is_shutdown() -> bool {
    SHUTDOWN.load(std::sync::atomic::Ordering::Acquire)
}

/// Фоновый сторожевой процесс (watchdog) для защиты конфигурации config.yaml.
/// Если служба XKeen перезапускается (S05xkeen restart или автообновление по расписанию),
/// XKeen перегенерирует config.yaml, затирая блоки AUTO-DEVICE и AUTO-FORCE.
/// Watchdog отслеживает изменения и мгновенно восстанавливает правила маршрутизации.
pub fn spawn(state: AppState) {
    spawn_schedules_monitor(state.clone());
    tokio::spawn(async move {
        // Начальная пауза перед запуском монитора
        for _ in 0..10 {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }

        let mut last_mtime: Option<std::time::SystemTime> = None;
        let mut last_len: usize = 0;

        loop {
            for _ in 0..4 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }

            let (config_path_str, force_domains, device_routing, ignore_servers, device_domains, adblock_enabled, flow_server, zapret_cfg) = {
                let cfg = state.config.read().await;
                (
                    cfg.mihomo.config_path.clone(),
                    cfg.force_domains.clone(),
                    cfg.device_routing.clone(),
                    cfg.ignore_servers.clone(),
                    cfg.device_domain_rules.clone(),
                    cfg.adblock_enabled,
                    cfg.flow_server.clone(),
                    cfg.zapret.clone(),
                )
            };

            let path = Path::new(&config_path_str);
            let meta = match tokio::fs::metadata(path).await {
                Ok(m) => m,
                Err(_) => continue,
            };

            let mtime = meta.modified().ok();
            let len = meta.len() as usize;

            // Если размер или дата модификации изменились
            let file_changed = match (mtime, last_mtime) {
                (Some(curr), Some(prev)) => curr != prev || len != last_len,
                (Some(_), None) => true,
                _ => false,
            };

            if file_changed {
                last_mtime = mtime;
                last_len = len;

                // Читаем содержимое и проверяем наличие сохраненных маркер-блоков
                if let Ok(content) = tokio::fs::read_to_string(path).await {
                    let needs_device = !device_routing.is_empty() || !device_domains.is_empty();
                    let needs_force = !force_domains.is_empty();
                    let needs_ignore = !ignore_servers.is_empty();
                    let needs_flow = flow_server.as_ref().map_or(false, |s| !s.trim().is_empty());
                    let needs_zapret = zapret_cfg.enabled && (zapret_cfg.hybrid_youtube || zapret_cfg.hybrid_discord || zapret_cfg.isolated_proxy);

                    let missing_device = needs_device && !content.contains("AUTO-DEVICE");
                    let missing_force = needs_force && !content.contains("AUTO-FORCE");
                    let missing_ignore = needs_ignore && !content.contains("exclude-filter:");
                    let missing_adblock = adblock_enabled && !content.contains("AUTO-ADBLOCK");
                    let missing_flow = needs_flow && !content.contains("AUTO-GOOGLE-AI");
                    let missing_zapret = needs_zapret && !content.contains("AUTO-ZAPRET-HYBRID");

                    if missing_device || missing_force || missing_ignore || missing_adblock || missing_flow || missing_zapret {
                        log_w!("[WATCHDOG] Обнаружена перезапись config.yaml (рестарт XKeen)! Восстановление маршрутизации...");

                        let _guard = state.routing_lock.lock().await;
                        let cfg = state.config.read().await.clone();

                        // Повторное применение маршрутизации
                        let raw_yaml = match tokio::fs::read_to_string(path).await {
                            Ok(c) => c,
                            Err(e) => {
                                log_w!("[WATCHDOG] Ошибка чтения config.yaml: {}", e);
                                continue;
                            }
                        };

                        let (new_yaml, _applied) = match routing::apply_routing(&raw_yaml, &cfg) {
                            Ok(res) => res,
                            Err(e) => {
                                log_w!("[WATCHDOG] Ошибка роутинга: {}", e);
                                continue;
                            }
                        };
                        if let Err(e) = crate::api::atomic_write_file(path, &new_yaml).await {
                            log_w!("[WATCHDOG] Ошибка записи config.yaml: {}", e);
                            continue;
                        }

                        // Перезагрузка ядра Mihomo
                        if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
                            log_w!("[WATCHDOG] Ошибка перезагрузки Mihomo: {}", e);
                            last_mtime = None; // Сброс для повторной попытки на следующем тике
                        } else {
                            log_i!("[WATCHDOG] ✓ Правила маршрутизации успешно восстановлены и применены в ядре");
                        }

                        // Синхронизация ipset geo_override (быстрые статические бандлы без сетевой нагрузки)
                        if !cfg.force_domains.is_empty() {
                            let auto_cdns = crate::cdn_discovery::expand_bundles(&cfg.force_domains);
                            let mut all_domains = cfg.force_domains.clone();
                            all_domains.extend(auto_cdns);
                            let _ = override_sync::sync_geo_override(&all_domains).await;
                        }

                        // Обновляем зафиксированные метаданные
                        if let Ok(new_meta) = tokio::fs::metadata(path).await {
                            last_mtime = new_meta.modified().ok();
                            last_len = new_meta.len() as usize;
                        }
                    }
                }
            }
        }
    });
}

/// Проверка, попадает ли текущее время now_hm ("HH:MM") в интервал start..end.
pub fn is_time_in_range(now_hm: &str, start_hm: &str, end_hm: &str) -> bool {
    if start_hm <= end_hm {
        now_hm >= start_hm && now_hm < end_hm
    } else {
        // Переход через полночь (например, с 23:00 до 07:00)
        now_hm >= start_hm || now_hm < end_hm
    }
}

/// Фоновый монитор расписаний устройств (автоматическая блокировка / переключение).
pub fn spawn_schedules_monitor(state: AppState) {
    tokio::spawn(async move {
        for _ in 0..15 {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }
        loop {
            for _ in 0..30 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }
            let cfg = state.config.read().await.clone();
            if cfg.schedules.is_empty() {
                continue;
            }

            use chrono::Datelike;
            let now = chrono::Local::now();
            let now_hm = now.format("%H:%M").to_string();
            let weekday = now.weekday().number_from_monday() as u8; // 1=Пн..7=Вс

            let rules = mihomo::m_get(&state.http, &cfg, "/rules").await.unwrap_or(serde_json::Value::Null);
            let groups_by_ip = mihomo::ip_groups_from_rules(&rules);

            for s in &cfg.schedules {
                if !s.enabled || !s.days.contains(&weekday) {
                    continue;
                }
                let in_range = is_time_in_range(&now_hm, &s.time_start, &s.time_end);
                if in_range {
                    let group_name = match groups_by_ip.get(&s.ip) {
                        Some(g) => g.clone(),
                        None => routing::group_name_for(&s.ip, ""),
                    };
                    let target_node = match s.action.as_str() {
                        "block" => "REJECT",
                        "direct" => "DIRECT",
                        "proxy" => s.target_server.as_deref().unwrap_or("PROXY"),
                        _ => continue,
                    };
                    let _ = mihomo::switch_group(&state.http, &cfg, &group_name, target_node).await;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_watchdog_shutdown_flag() {
        assert!(!is_shutdown());
        shutdown();
        assert!(is_shutdown());
    }
}

