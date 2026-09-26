use std::path::Path;
use tokio::time::{sleep, Duration};

use crate::{config, log_i, log_w, mihomo, override_sync, rci, routing, AppState};

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
    spawn_dhcp_device_monitor(state.clone());
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

            let (config_path_str, force_domains, device_routing, ignore_servers, device_domains, adblock_enabled, flow_server, zapret_cfg, gaming_cfg) = {
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
                    cfg.gaming.clone(),
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
                    let needs_gaming = gaming_cfg.enabled;

                    let missing_device = needs_device && !content.contains("AUTO-DEVICE");
                    let missing_force = needs_force && !content.contains("AUTO-FORCE");
                    let missing_ignore = needs_ignore && !content.contains("exclude-filter:");
                    let missing_adblock = adblock_enabled && !content.contains("AUTO-ADBLOCK");
                    let missing_flow = needs_flow && !content.contains("AUTO-GOOGLE-AI");
                    let missing_zapret = needs_zapret && !content.contains("AUTO-ZAPRET-HYBRID");
                    let missing_gaming = needs_gaming && !content.contains("AUTO-GAMING");

                    if missing_device || missing_force || missing_ignore || missing_adblock || missing_flow || missing_zapret || missing_gaming {
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
                        let mut all_domains = cfg.force_domains.clone();
                        if !cfg.force_domains.is_empty() {
                            let auto_cdns = crate::cdn_discovery::expand_bundles(&cfg.force_domains);
                            all_domains.extend(auto_cdns);
                        }
                        if cfg.gaming.enabled {
                            let game_domains = routing::get_gaming_domains(&cfg.gaming);
                            all_domains.extend(game_domains);
                        }
                        if !all_domains.is_empty() {
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
        let mut active_schedules: std::collections::HashMap<String, String> = std::collections::HashMap::new();

        loop {
            for _ in 0..30 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }
            let cfg = state.config.read().await.clone();
            if cfg.schedules.is_empty() {
                // Если все расписания удалены, восстанавливаем исходные ноды для тех, что были активны
                if !active_schedules.is_empty() {
                    let rules = mihomo::m_get(&state.http, &cfg, "/rules").await.unwrap_or(serde_json::Value::Null);
                    let groups_by_ip = mihomo::ip_groups_from_rules(&rules);
                    for (key, orig_node) in active_schedules.drain() {
                        let group_name = match groups_by_ip.get(&key) {
                            Some(g) => g.clone(),
                            None => routing::group_name_for(&key, ""),
                        };
                        let _ = mihomo::switch_group(&state.http, &cfg, &group_name, &orig_node).await;
                    }
                }
                continue;
            }

            use chrono::Datelike;
            let now = chrono::Local::now();
            let now_hm = now.format("%H:%M").to_string();
            let weekday = now.weekday().number_from_monday() as u8; // 1=Пн..7=Вс

            let rules = mihomo::m_get(&state.http, &cfg, "/rules").await.unwrap_or(serde_json::Value::Null);
            let groups_by_ip = mihomo::ip_groups_from_rules(&rules);
            let proxies_opt = mihomo::get_proxies(&state.http, &cfg).await.ok();

            for s in &cfg.schedules {
                let key = if s.id.is_empty() { s.ip.clone() } else { s.id.clone() };
                let matches_day = s.days.contains(&weekday);
                let in_range = s.enabled && matches_day && is_time_in_range(&now_hm, &s.time_start, &s.time_end);

                let group_name = match groups_by_ip.get(&s.ip) {
                    Some(g) => g.clone(),
                    None => routing::group_name_for(&s.ip, ""),
                };

                if in_range {
                    if !active_schedules.contains_key(&key) {
                        let current_node = proxies_opt
                            .as_ref()
                            .and_then(|p| p.get(&group_name))
                            .and_then(|v| v.get("now"))
                            .and_then(|n| n.as_str())
                            .unwrap_or("DIRECT")
                            .to_string();

                        let target_node = match s.action.as_str() {
                            "block" => "REJECT",
                            "direct" => "DIRECT",
                            "proxy" => s.target_server.as_deref().unwrap_or("PROXY"),
                            _ => continue,
                        };

                        if current_node != target_node {
                            log_i!("[SCHEDULE] ⏰ Активация расписания '{}' ({}) для {}: {} -> {}", s.id, s.action, s.ip, current_node, target_node);
                            let _ = mihomo::switch_group(&state.http, &cfg, &group_name, target_node).await;
                        }
                        active_schedules.insert(key, current_node);
                    }
                } else if let Some(orig_node) = active_schedules.remove(&key) {
                    log_i!("[SCHEDULE] ⏰ Окончание действия расписания '{}' для {}: возврат к исходному узлу '{}'", s.id, s.ip, orig_node);
                    let _ = mihomo::switch_group(&state.http, &cfg, &group_name, &orig_node).await;
                }
            }
        }
    });
}

/// Фоновый монитор изменений DHCP IP-адресов устройств игрового режима по их постоянному MAC.
pub fn spawn_dhcp_device_monitor(state: AppState) {
    tokio::spawn(async move {
        for _ in 0..12 {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }

        loop {
            for _ in 0..15 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }

            let (gaming_enabled, is_compat, has_devices) = {
                let cfg = state.config.read().await;
                (
                    cfg.gaming.enabled,
                    cfg.gaming.mode == config::GamingMode::Compatibility,
                    !cfg.gaming.devices.is_empty(),
                )
            };

            if gaming_enabled && is_compat && has_devices {
                let cfg = state.config.read().await.clone();
                let policies = rci::get_policies(&state.http, &cfg).await.unwrap_or_default();
                if let Ok(current_devs) = rci::get_devices(&state.http, &cfg, &policies, "").await {
                    let mut ip_changed = false;
                    let mut updated_devices = cfg.gaming.devices.clone();
                    let has_explicit_enabled = updated_devices.iter().any(|d| d.enabled);
                    for dev in &mut updated_devices {
                        let is_active = if has_explicit_enabled {
                            dev.enabled
                        } else {
                            updated_devices.len() == 1
                        };
                        if !is_active {
                            continue;
                        }
                        if let Some(rci_dev) = current_devs.iter().find(|d| d.mac.eq_ignore_ascii_case(&dev.mac)) {
                            let ip_diff = !rci_dev.ip.is_empty() && rci_dev.ip != dev.ip;
                            let filtered_v6: Vec<String> = rci_dev.ipv6.iter().filter(|v6| {
                                let l = v6.trim().to_lowercase();
                                !l.is_empty() && !l.starts_with("fe80:") && !l.starts_with("::1")
                            }).cloned().collect();
                            let v6_diff = !filtered_v6.is_empty() && filtered_v6 != dev.ipv6;

                            if ip_diff || v6_diff {
                                log_i!(
                                    "[DHCP-WATCHDOG] 🔄 Изменение IP адреса по DHCP для устройства '{}' (MAC {}): IPv4: {} -> {}, IPv6: {:?} -> {:?}. Автообновление маршрута...",
                                    dev.name, dev.mac, dev.ip, rci_dev.ip, dev.ipv6, filtered_v6
                                );
                                if !rci_dev.ip.is_empty() {
                                    dev.ip = rci_dev.ip.clone();
                                }
                                dev.ipv6 = filtered_v6;
                                dev.name = rci_dev.name.clone();
                                ip_changed = true;
                            }
                        }
                    }

                    if ip_changed {
                        let _cfg_guard = state.config_lock.lock().await;
                        let _routing_guard = state.routing_lock.lock().await;
                        let mut new_cfg = (**state.config.read().await).clone();
                        new_cfg.gaming.devices = updated_devices;

                        let path = Path::new(&new_cfg.mihomo.config_path);
                        if path.exists() {
                            if let Ok(raw_yaml) = tokio::fs::read_to_string(path).await {
                                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &new_cfg) {
                                    if crate::api::atomic_write_file(path, &new_yaml).await.is_ok() {
                                        if mihomo::reload_config(&state.http, &new_cfg).await.is_ok() {
                                            log_i!("[DHCP-WATCHDOG] ✓ Маршрут игрового режима успешно переприменен для нового IP");
                                        }
                                    }
                                }
                            }
                        }

                        if config::save(&state.config_path, &new_cfg).await.is_ok() {
                            *state.config.write().await = std::sync::Arc::new(new_cfg);
                        }
                    }
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

