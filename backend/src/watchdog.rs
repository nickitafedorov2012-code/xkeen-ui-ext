use std::path::Path;
use tokio::time::{sleep, Duration};

use crate::{log_i, log_w, mihomo, override_sync, routing, AppState};

/// Фоновый сторожевой процесс (watchdog) для защиты конфигурации config.yaml.
/// Если служба XKeen перезапускается (S05xkeen restart или автообновление по расписанию),
/// XKeen перегенерирует config.yaml, затирая блоки AUTO-DEVICE и AUTO-FORCE.
/// Watchdog отслеживает изменения и мгновенно восстанавливает правила маршрутизации.
pub fn spawn(state: AppState) {
    tokio::spawn(async move {
        // Начальная пауза перед запуском монитора
        sleep(Duration::from_secs(10)).await;

        let mut last_mtime: Option<std::time::SystemTime> = None;
        let mut last_len: usize = 0;

        loop {
            sleep(Duration::from_secs(4)).await;

            let (config_path_str, force_domains, device_routing, ignore_servers, device_domains, adblock_enabled) = {
                let cfg = state.config.read().await;
                (
                    cfg.mihomo.config_path.clone(),
                    cfg.force_domains.clone(),
                    cfg.device_routing.clone(),
                    cfg.ignore_servers.clone(),
                    cfg.device_domain_rules.clone(),
                    cfg.adblock_enabled,
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

                    let missing_device = needs_device && !content.contains("AUTO-DEVICE");
                    let missing_force = needs_force && !content.contains("AUTO-FORCE");
                    let missing_ignore = needs_ignore && !content.contains("AUTO-IGNORE");
                    let missing_adblock = adblock_enabled && !content.contains("AUTO-ADBLOCK");

                    if missing_device || missing_force || missing_ignore || missing_adblock {
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

                        let (new_yaml, _applied) = routing::apply_routing(&raw_yaml, &cfg);
                        let tmp = format!("{}.tmp", path.display());
                        let write_res = async {
                            tokio::fs::write(&tmp, &new_yaml).await?;
                            tokio::fs::rename(&tmp, path).await
                        }.await;
                        if let Err(e) = write_res {
                            let _ = tokio::fs::remove_file(&tmp).await;
                            log_w!("[WATCHDOG] Ошибка записи config.yaml: {}", e);
                            continue;
                        }

                        // Перезагрузка ядра Mihomo
                        if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
                            log_w!("[WATCHDOG] Ошибка перезагрузки Mihomo: {}", e);
                        } else {
                            log_i!("[WATCHDOG] ✓ Правила маршрутизации успешно восстановлены и применены в ядре");
                        }

                        // Синхронизация ipset geo_override
                        if !cfg.force_domains.is_empty() {
                            let auto_cdns = crate::cdn_discovery::discover_all_cdns(&cfg.force_domains, &cfg.mihomo_proxy_url()).await;
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

    spawn_schedules_monitor(state);
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
        sleep(Duration::from_secs(15)).await;
        loop {
            sleep(Duration::from_secs(30)).await;
            let cfg = state.config.read().await.clone();
            if cfg.schedules.is_empty() {
                continue;
            }

            use chrono::Datelike;
            let now = chrono::Local::now();
            let now_hm = now.format("%H:%M").to_string();
            let weekday = now.weekday().number_from_monday() as u8; // 1=Пн..7=Вс

            for s in &cfg.schedules {
                if !s.enabled || !s.days.contains(&weekday) {
                    continue;
                }
                let in_range = is_time_in_range(&now_hm, &s.time_start, &s.time_end);
                if in_range {
                    let group_name = routing::group_name_for(&s.ip, "");
                    let target_node = match s.action.as_str() {
                        "block" => "REJECT",
                        "direct" => "DIRECT",
                        "proxy" => s.target_server.as_deref().unwrap_or("PROXY"),
                        _ => continue,
                    };
                    let _ = mihomo::switch_server_in_group(&state.http, &cfg, &group_name, target_node).await;
                }
            }
        }
    });
}

