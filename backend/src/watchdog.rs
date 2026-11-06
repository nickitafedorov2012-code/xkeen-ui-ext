use std::path::Path;
use tokio::time::{sleep, Duration};

use crate::{config, log_i, log_w, mihomo, override_sync, rci, routing, AppState};

pub static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static SERVICE_MANUALLY_STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static MIHOMO_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static ADGUARD_DOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn is_mihomo_down() -> bool {
    MIHOMO_DOWN.load(std::sync::atomic::Ordering::Acquire)
}

pub fn set_mihomo_down(down: bool) {
    MIHOMO_DOWN.store(down, std::sync::atomic::Ordering::Release);
}

pub fn is_adguard_down() -> bool {
    ADGUARD_DOWN.load(std::sync::atomic::Ordering::Acquire)
}

pub fn set_adguard_down(down: bool) {
    ADGUARD_DOWN.store(down, std::sync::atomic::Ordering::Release);
}

pub fn shutdown() {
    SHUTDOWN.store(true, std::sync::atomic::Ordering::Release);
}

pub fn is_shutdown() -> bool {
    SHUTDOWN.load(std::sync::atomic::Ordering::Acquire)
}

static SERVICE_STOPPED_FLAG_PATH: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// Флаг «службы остановлены пользователем / режим паники» на диске (баг 59): после рестарта панели
/// failover и сторожа не должны молча поднимать проксирование обратно.
pub fn init_service_stopped_flag(path: std::path::PathBuf) {
    let stopped = read_service_stopped_flag(&path);
    let _ = SERVICE_STOPPED_FLAG_PATH.set(path);
    SERVICE_MANUALLY_STOPPED.store(stopped, std::sync::atomic::Ordering::Release);
    if stopped {
        log_w!("[WATCHDOG] Службы были остановлены пользователем до перезапуска панели — режим DIRECT сохранён");
    }
}

pub fn read_service_stopped_flag(path: &std::path::Path) -> bool {
    path.exists()
}

pub fn persist_service_stopped_flag(path: &std::path::Path, stopped: bool) {
    if stopped {
        let _ = std::fs::write(path, b"stopped\n");
    } else {
        let _ = std::fs::remove_file(path);
    }
}

pub fn set_service_stopped(stopped: bool) {
    SERVICE_MANUALLY_STOPPED.store(stopped, std::sync::atomic::Ordering::Release);
    if let Some(p) = SERVICE_STOPPED_FLAG_PATH.get() {
        persist_service_stopped_flag(p, stopped);
    }
}

pub fn is_service_stopped() -> bool {
    SERVICE_MANUALLY_STOPPED.load(std::sync::atomic::Ordering::Acquire)
}

/// Фоновый сторожевой процесс (watchdog) для защиты конфигурации config.yaml.
/// Если служба XKeen перезапускается (S05xkeen restart или автообновление по расписанию),
/// XKeen перегенерирует config.yaml, затирая блоки AUTO-DEVICE и AUTO-FORCE.
/// Watchdog отслеживает изменения и мгновенно восстанавливает правила маршрутизации.
pub fn spawn(state: AppState) {
    spawn_schedules_monitor(state.clone());
    spawn_dhcp_device_monitor(state.clone());
    spawn_zapret_monitor(state.clone());
    spawn_community_hostlist_updater(state.clone());
    spawn_zapret_update_checker(state.clone());
    spawn_smart_gaming_monitor(state.clone());
    spawn_adguard_monitor(state.clone());
    spawn_mihomo_health_monitor(state.clone());
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
        let mut reload_pending = false;

        loop {
            for _ in 0..4 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }

            if is_service_stopped() {
                continue;
            }

            // RT-04: Если предыдущий reload завершился сбоем, повторяем попытку
            if reload_pending {
                log_i!("[WATCHDOG] 🔄 Повторная попытка отложенного reload Mihomo (RT-04)...");
                let cfg = state.config.read().await.clone();
                if mihomo::reload_config(&state.http, &cfg).await.is_ok() {
                    log_i!("[WATCHDOG] ✓ Отложенный reload Mihomo успешно подтвержден");
                    reload_pending = false;
                }
            }

            let (config_path_str, force_domains, direct_domains, device_routing, ignore_servers, device_domains, adblock_enabled, flow_server, zapret_cfg, gaming_cfg, schedules, whitelist_cfg) = {
                let cfg = state.config.read().await;
                (
                    cfg.mihomo.config_path.clone(),
                    cfg.force_domains.clone(),
                    cfg.direct_domains.clone(),
                    cfg.device_routing.clone(),
                    cfg.ignore_servers.clone(),
                    cfg.device_domain_rules.clone(),
                    cfg.adblock_enabled,
                    cfg.flow_server.clone(),
                    cfg.zapret.clone(),
                    cfg.gaming.clone(),
                    cfg.schedules.clone(),
                    cfg.whitelist.clone(),
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
                    // BUG-16: Проверка эффективной необходимости вставок
                    let has_active_device_assignments = device_routing.values().any(|dr| {
                        dr.servers.first().is_some_and(|s| !s.trim().is_empty() && s.trim() != "default")
                    }) || schedules.iter().any(|s| s.enabled)
                       || (gaming_cfg.enabled && gaming_cfg.devices.iter().any(|d| d.enabled && d.server.as_ref().is_some_and(|s| !s.trim().is_empty())));
                    let needs_device = has_active_device_assignments;
                    let needs_dev_domains = !device_domains.is_empty() && device_domains.values().any(|v| !v.is_empty());

                    let needs_force = force_domains.iter().any(|d| {
                        let clean = crate::config::normalize_domain(d);
                        if clean.is_empty() || !clean.contains('.') {
                            return false;
                        }
                        if direct_domains.iter().any(|dir| crate::config::normalize_domain(dir) == clean) {
                            return false;
                        }
                        if zapret_cfg.enabled && routing::is_zapret_direct_domain(&clean, &zapret_cfg) {
                            return false;
                        }
                        true
                    });

                    let needs_ignore = !ignore_servers.is_empty();
                    let needs_flow = flow_server.as_ref().is_some_and(|s| !s.trim().is_empty());
                    let needs_zapret = zapret_cfg.enabled
                        && (zapret_cfg.hybrid_youtube
                            || zapret_cfg.hybrid_discord
                            || zapret_cfg.isolated_proxy
                            || zapret_cfg.smart_tv_mode
                            || zapret_cfg.bypass_github
                            || zapret_cfg.bypass_torrents
                            || zapret_cfg.bypass_adult
                            || zapret_cfg.custom_entries.iter().any(|e| e.enabled));
                    let needs_gaming = gaming_cfg.enabled;
                    let needs_whitelist = whitelist_cfg.enabled;

                    let missing_device = needs_device && !content.contains("AUTO-DEVICE");
                    let missing_dev_domains = needs_dev_domains && !content.contains("AUTO-DEVICE-DOMAINS");
                    let missing_force = needs_force && !content.contains("AUTO-FORCE");
                    let missing_ignore = needs_ignore && !content.contains("exclude-filter:");
                    let missing_adblock = adblock_enabled && !content.contains("AUTO-ADBLOCK");
                    let missing_flow = needs_flow && !content.contains("AUTO-GOOGLE-AI");
                    let missing_zapret = needs_zapret && !content.contains("AUTO-ZAPRET-HYBRID");
                    let missing_gaming = needs_gaming && !content.contains("AUTO-GAMING");
                    let missing_whitelist = needs_whitelist && !content.contains("AUTO-WHITELIST");

                    if missing_device || missing_dev_domains || missing_force || missing_ignore || missing_adblock || missing_flow || missing_zapret || missing_gaming || missing_whitelist {
                        log_w!("[WATCHDOG] Обнаружена перезапись config.yaml (рестарт XKeen)! Восстановление маршрутизации...");

                        // BUG-15: Строгий порядок блокировок (config_lock -> routing_lock)
                        let _cfg_guard = state.config_lock.lock().await;
                        let _routing_guard = state.routing_lock.lock().await;
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

                        // Перезагрузка ядра Mihomo (RT-04)
                        if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
                            log_w!("[WATCHDOG] Ошибка перезагрузки Mihomo: {}", e);
                            reload_pending = true;
                            last_mtime = None; // Сброс для обязательного повтора
                        } else {
                            log_i!("[WATCHDOG] ✓ Правила маршрутизации успешно восстановлены и применены в ядре");
                            reload_pending = false;
                        }

                        // Синхронизация ipset geo_override (быстрые статические бандлы без сетевой нагрузки)
                        let _ = override_sync::sync_from_config(&cfg, &[]).await;

                        // RT-04: Обновляем зафиксированные метаданные только если reload успешен
                        if !reload_pending {
                            if let Ok(new_meta) = tokio::fs::metadata(path).await {
                                last_mtime = new_meta.modified().ok();
                                last_len = new_meta.len() as usize;
                            }
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
        #[derive(Clone)]
        struct ActiveScheduleEntry {
            schedule_id: String,
            device_ip: String,
            group_name: String,
            original_node: String,
        }
        let mut active_schedules: std::collections::HashMap<String, ActiveScheduleEntry> = std::collections::HashMap::new();

        loop {
            for _ in 0..30 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }
            let cfg = state.config.read().await.clone();

            // RT-06: Reconcile удалённых расписаний с корректным group_name по IP устройства
            let current_ids: std::collections::HashSet<String> = cfg
                .schedules
                .iter()
                .map(|s| if s.id.is_empty() { s.ip.clone() } else { s.id.clone() })
                .collect();

            let mut removed_keys = Vec::new();
            for key in active_schedules.keys() {
                if !current_ids.contains(key) {
                    removed_keys.push(key.clone());
                }
            }
            for key in removed_keys {
                if let Some(entry) = active_schedules.get(&key) {
                    log_i!("[SCHEDULE] ⏰ Расписание '{}' удалено/снято для {}: возврат к исходному узлу '{}'", entry.schedule_id, entry.device_ip, entry.original_node);
                    if mihomo::switch_group_scoped(&state.http, &cfg, &entry.group_name, &entry.original_node).await.is_ok() {
                        active_schedules.remove(&key);
                    }
                }
            }

            if cfg.schedules.is_empty() {
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
                    if let std::collections::hash_map::Entry::Vacant(e) = active_schedules.entry(key) {
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

                        let switch_ok = if current_node != target_node {
                            log_i!("[SCHEDULE] ⏰ Активация расписания '{}' ({}) для {}: {} -> {}", s.id, s.action, s.ip, current_node, target_node);
                            mihomo::switch_group_scoped(&state.http, &cfg, &group_name, target_node).await.is_ok()
                        } else {
                            true
                        };

                        if switch_ok {
                            e.insert(ActiveScheduleEntry {
                                schedule_id: s.id.clone(),
                                device_ip: s.ip.clone(),
                                group_name,
                                original_node: current_node,
                            });
                        }
                    }
                } else if let Some(entry) = active_schedules.get(&key) {
                    log_i!("[SCHEDULE] ⏰ Окончание действия расписания '{}' для {}: возврат к исходному узлу '{}'", entry.schedule_id, entry.device_ip, entry.original_node);
                    if mihomo::switch_group_scoped(&state.http, &cfg, &entry.group_name, &entry.original_node).await.is_ok() {
                        active_schedules.remove(&key);
                    }
                }
            }
        }
    });
}

/// Пересборка config.yaml после смены DHCP-адреса игрового устройства.
/// Полный apply_routing, а не только apply_gaming_rules(.., &[]): раньше пустой список
/// провайдеров выбрасывал серверы подписок из группы 🎮 Gaming, а SRC-IP правило
/// выделенного сервера устройства (AUTO-DEVICE) оставалось со старым IP.
pub fn rebuild_routing_after_dhcp_change(raw_yaml: &str, cfg: &config::AppConfig) -> Result<String, String> {
    routing::apply_routing(raw_yaml, cfg).map(|(yaml, _)| yaml)
}

/// Фоновый монитор изменений DHCP IP-адресов устройств игрового режима по их постоянному MAC.
/// Смена IP устройств по DHCP (баг 118): пары (старый IP → новый IP) для MAC, у которых IP изменился.
pub fn detect_dhcp_ip_changes(
    prev_ip_by_mac: &std::collections::HashMap<String, String>,
    current: &[(String, String)],
) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for (mac, ip) in current {
        let mac_l = mac.trim().to_lowercase();
        let ip = ip.trim();
        if ip.is_empty() {
            continue;
        }
        if let Some(old) = prev_ip_by_mac.get(&mac_l) {
            if old != ip && !old.is_empty() {
                out.push((old.clone(), ip.to_string()));
            }
        }
    }
    out
}

/// Перенос всех настроек, привязанных к IP, на новый адрес устройства (баг 118).
pub fn migrate_ip_keyed_config(cfg: &mut config::AppConfig, changes: &[(String, String)]) -> bool {
    let mut changed = false;
    for (old, new) in changes {
        if old == new {
            continue;
        }
        if let Some(v) = cfg.device_routing.remove(old) {
            cfg.device_routing.insert(new.clone(), v);
            changed = true;
        }
        if let Some(v) = cfg.device_domain_rules.remove(old) {
            cfg.device_domain_rules.insert(new.clone(), v);
            changed = true;
        }
        for list in [&mut cfg.zapret.excluded_devices, &mut cfg.whitelist.excluded_devices] {
            for e in list.iter_mut() {
                if e.trim() == old {
                    *e = new.clone();
                    changed = true;
                }
            }
        }
        for sch in &mut cfg.schedules {
            if sch.ip.trim() == old {
                sch.ip = new.clone();
                changed = true;
            }
        }
        for d in &mut cfg.whitelist.devices {
            if d.ip.trim() == old {
                d.ip = new.clone();
                changed = true;
            }
        }
        for d in &mut cfg.gaming.devices {
            if d.ip.trim() == old {
                d.ip = new.clone();
                changed = true;
            }
        }
    }
    changed
}

pub fn spawn_dhcp_device_monitor(state: AppState) {
    tokio::spawn(async move {
        for _ in 0..12 {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }

        let mut last_ip_by_mac: std::collections::HashMap<String, String> = std::collections::HashMap::new();
        loop {
            for _ in 0..15 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }

            let cfg = state.config.read().await.clone();
            let policies = rci::get_policies(&state.http, &cfg).await.unwrap_or_default();
            let Ok(current_devs) = rci::get_devices(&state.http, &cfg, &policies, "").await else {
                continue;
            };
            let current_pairs: Vec<(String, String)> =
                current_devs.iter().map(|d| (d.mac.clone(), d.ip.clone())).collect();
            // Баг 118: общий перенос IP-привязанных настроек для ВСЕХ устройств, а не только гейминга
            let ip_changes = detect_dhcp_ip_changes(&last_ip_by_mac, &current_pairs);
            for (mac, ip) in &current_pairs {
                if !ip.trim().is_empty() {
                    last_ip_by_mac.insert(mac.trim().to_lowercase(), ip.trim().to_string());
                }
            }

            let gaming_tracked = cfg.gaming.enabled
                && (cfg.gaming.mode == config::GamingMode::Compatibility || cfg.gaming.mode == config::GamingMode::BypassRu)
                && !cfg.gaming.devices.is_empty();
            if ip_changes.is_empty() && !gaming_tracked {
                continue;
            }

            // Баг 96: запись только через ConfigTx (config_lock → routing_lock, откат при сбое)
            let Ok(mut tx) = crate::transaction::ConfigTx::begin(&state).await else {
                continue;
            };
            let mut changed = migrate_ip_keyed_config(tx.config_mut(), &ip_changes);
            for (old, new) in &ip_changes {
                log_i!("[DHCP-WATCHDOG] 🔄 Устройство сменило IP {old} → {new}: настройки устройства перенесены");
            }

            if gaming_tracked {
                let new_cfg = tx.config_mut();
                let has_explicit_enabled = new_cfg.gaming.devices.iter().any(|d| d.enabled);
                let devices_len = new_cfg.gaming.devices.len();
                for dev in &mut new_cfg.gaming.devices {
                    let is_active = if has_explicit_enabled { dev.enabled } else { devices_len == 1 };
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
                            changed = true;
                        }
                    }
                }
            }

            if !changed {
                continue;
            }
            let Ok(raw_yaml) = tx.read_yaml().await else { continue };
            let Ok(new_yaml) = rebuild_routing_after_dhcp_change(&raw_yaml, tx.config()) else { continue };
            if tx.set_yaml(new_yaml.clone()).is_err() {
                continue;
            }
            match tx.commit_and_reload().await {
                Ok(()) => {
                    log_i!("[DHCP-WATCHDOG] ✓ Маршруты устройств переприменены для новых IP");
                    let cfg_now = (**state.config.read().await).clone();
                    let redir_port = routing::parse_redir_port(&new_yaml).unwrap_or(5000);
                    let tproxy_port = routing::parse_tproxy_port(&new_yaml).unwrap_or(5001);
                    let dns_port = crate::api::resolve_active_dns_port(&cfg_now);
                    crate::api::sync_full_tunnel_modes(&cfg_now, redir_port, tproxy_port, dns_port).await;
                    if !cfg_now.zapret.excluded_devices.is_empty() {
                        let _ = crate::api::sync_zapret_files(&cfg_now.zapret).await;
                    }
                }
                Err(e) => log_w!("[DHCP-WATCHDOG] Не удалось применить новые IP устройств: {e}"),
            }
        }
    });
}

/// Фоновый сторожевой процесс (watchdog) для Zapret (DPI bypass).
/// Отслеживает активность процесса nfqws и целостность правил перехвата в Netfilter/iptables.
/// При периодических сбросах правил брандмауэра KeeneticOS (ndm, DHCP renewal, ping-check)
/// мгновенно и бесшовно восстанавливает цепочки iptables.
pub fn spawn_zapret_monitor(state: AppState) {
    tokio::spawn(async move {
        // Начальная пауза для завершения загрузки роутера и сети
        for _ in 0..15 {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }

        let mut restart_failures = 0;

        loop {
            // Проверка каждые 12 секунд
            for _ in 0..12 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }

            if is_service_stopped() {
                restart_failures = 0;
                continue;
            }

            let zapret_enabled = {
                let cfg = state.config.read().await;
                cfg.zapret.enabled
            };

            if !zapret_enabled {
                restart_failures = 0;
                // Z-03/Z-05: служба выключена в конфигурации — гасим осиротевший процесс и
                // остаточные правила здесь (watchdog), а не в GET /api/zapret/status.
                // Маркер не трогаем: он означает «выключено пользователем» и ставится UI/ботом.
                let orphan_running = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
                    .output()
                    .await
                    .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
                    .unwrap_or(false);
                if orphan_running {
                    log_w!("[WATCHDOG] ⚠️ Zapret выключен в настройках, но nfqws ещё запущен — остановка осиротевшего процесса...");
                    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
                        .arg("stop")
                        .output()
                        .await;
                } else {
                    let iptables_left = tokio::process::Command::new("sh")
                        .arg("-c")
                        .arg("iptables -w 2 -t mangle -S 2>/dev/null | grep -q zapret || iptables -w 2 -t nat -S PREROUTING 2>/dev/null | grep 'xkeen-route-zapret' | grep 'to-ports 1053' | grep -q .")
                        .output()
                        .await
                        .map(|o| o.status.success())
                        .unwrap_or(false);
                    if iptables_left {
                        let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
                            .arg("stop-fw")
                            .output()
                            .await;
                    }
                }
                continue;
            }

            // Если в данный момент выполняется операция запуска/рестарта/смены пресета из UI,
            // пропускаем тик сторожа, чтобы не конфликтовать с S51zapret (Z-01)
            if crate::zapret::get_current_operation_state() != crate::zapret::ZapretOperationState::Idle {
                continue;
            }
            let Ok(_zapret_guard) = state.zapret_lock.try_lock() else {
                continue;
            };
            if !state.config.read().await.zapret.enabled {
                restart_failures = 0;
                continue;
            }

            let init_script = "/opt/etc/init.d/S51zapret";
            if !Path::new(init_script).exists() {
                continue;
            }

            // Проверяем, запущен ли бинарник nfqws / nfqws2
            let is_running = tokio::process::Command::new("sh")
                .arg("-c")
                .arg("pidof nfqws2 >/dev/null 2>&1 || pidof nfqws >/dev/null 2>&1")
                .output()
                .await
                .map(|o| o.status.success())
                .unwrap_or(false);

            if is_running {
                restart_failures = 0;
                // nfqws работает — проверяем, на месте ли правила iptables netfilter.
                // ВАЖНО: Требуется наличие КАК точки входа в POSTROUTING/PREROUTING,
                // ТАК И цепочки zapret с правилами NFQUEUE!
                // (Использование '||' было фатальной ошибкой: если Keenetic ndm сбрасывал POSTROUTING,
                // цепочка zapret с NFQUEUE оставалась в памяти, из-за чего watchdog ошибочно считал
                // перехват активным и никогда не восстанавливал вызов zapret из POSTROUTING).
                let iptables_active = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("(iptables -w 2 -t mangle -S POSTROUTING 2>/dev/null | grep -q zapret || iptables -w 2 -t mangle -S PREROUTING 2>/dev/null | grep -q zapret) && iptables -w 2 -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE")
                    .output()
                    .await
                    .map(|o| o.status.success())
                    .unwrap_or(false);

                if !iptables_active {
                    log_w!("[WATCHDOG] ⚠️ Обнаружено отключение перехвата Netfilter при работающем nfqws (сброс iptables в KeeneticOS ndm)! Восстановление правил...");
                    let res = tokio::process::Command::new(init_script)
                        .arg("start-fw")
                        .output()
                        .await;
                    match res {
                        Ok(o) if o.status.success() => {
                            log_i!("[WATCHDOG] ✓ Правила Netfilter (iptables) для Zapret успешно восстановлены");
                        }
                        Ok(o) => {
                            log_w!("[WATCHDOG] Ошибка вызова S51zapret start-fw: {}", String::from_utf8_lossy(&o.stderr));
                        }
                        Err(e) => {
                            log_w!("[WATCHDOG] Не удалось запустить S51zapret start-fw: {e}");
                        }
                    }
                }

                // Проверка целостности аппаратного учёта прямого трафика (цепочка XKEEN_DIRECT)
                if !crate::direct_traffic::is_direct_iptables_active() {
                    log_w!("[WATCHDOG] ⚠️ Цепочка XKEEN_DIRECT отсутствует в iptables FORWARD (сброс правил KeeneticOS ndm)! Восстановление...");
                    if crate::direct_traffic::init_direct_iptables() {
                        log_i!("[WATCHDOG] ✓ Цепочка XKEEN_DIRECT успешно восстановлена в iptables");
                    }
                }

                // NET-01: Контроль здоровья DNS-редиректа на порт 1053 во избежание блэкаута LAN
                let dns_redirect_active = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("iptables -w 2 -t nat -S PREROUTING 2>/dev/null | grep 'xkeen-route-zapret' | grep 'to-ports 1053' | grep -q .")
                    .output()
                    .await
                    .map(|o| o.status.success())
                    .unwrap_or(false);

                if dns_redirect_active {
                    let resolver_ready = crate::zapret::check_dns_resolver_ready(1053).await;
                    if !resolver_ready {
                        log_w!("[WATCHDOG] ⚠️ DNS-редирект на 1053 активен в iptables, но резолвер не отвечает! Временное снятие редиректа для предотвращения DNS-блэкаута LAN...");
                        let _ = tokio::process::Command::new("sh")
                            .arg("-c")
                            .arg(crate::zapret::DNS_REDIRECT_REMOVE_CMD)
                            .output()
                            .await;
                    }
                }
            } else {
                // Zapret включен в настройках, но nfqws не запущен
                if restart_failures < 5 {
                    log_w!("[WATCHDOG] ⚠️ Служба Zapret включена, но nfqws не активен! Автоматический запуск службы...");
                    let _ = tokio::process::Command::new(init_script)
                        .arg("start")
                        .output()
                        .await;
                    restart_failures += 1;
                } else if restart_failures == 5 {
                    log_w!("[WATCHDOG] ❌ Не удалось запустить службу Zapret после 5 попыток. Приостановка автозапуска.");
                    // Баг 28: выключение — транзакцией с пересборкой правил Mihomo, иначе YouTube/Discord
                    // остаются в DIRECT без DPI-обхода. Баг 95: запись идёт под config_lock (через ConfigTx).
                    if let Err(e) = disable_zapret_after_failed_starts(&state).await {
                        log_w!("[WATCHDOG] Не удалось выключить Zapret и пересобрать правила: {e}");
                    }
                    // Z-03: маркер предотвращает перезапуск S51zapret (автозапуск Entware) до явного включения пользователем
                    crate::zapret::set_zapret_disabled_marker();
                    let _ = tokio::process::Command::new(init_script)
                        .arg("stop-fw")
                        .output()
                        .await;
                    restart_failures += 1;
                }
            }
        }
    });
}

/// Выключение Zapret после неудачных автозапусков (баги 28, 95): zapret.enabled=false и пересборка правил
/// Mihomo в одной транзакции. Если ядро не принимает reload, файлы всё равно приводятся в согласованное
/// состояние (без DPI-правил), чтобы следующий старт Mihomo не вернул YouTube/Discord в DIRECT.
pub async fn disable_zapret_after_failed_starts(state: &AppState) -> Result<(), String> {
    crate::api::persist_zapret_enabled_with_routing(state, false).await
}

/// Фоновое автообновление списков доменов Community Hostlists (каждые 24 часа).
pub fn spawn_community_hostlist_updater(state: AppState) {
    tokio::spawn(async move {
        // Начальная пауза после старта роутера/панели
        for _ in 0..30 {
            if is_shutdown() {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }

        loop {
            // Проверка каждый час
            for _ in 0..3600 {
                if is_shutdown() {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }

            let (enabled, auto_update, last_updated) = {
                let cfg = state.config.read().await;
                (
                    cfg.zapret.community_hostlist_enabled,
                    cfg.zapret.community_hostlist_auto_update,
                    cfg.zapret.community_hostlist_last_updated.clone(),
                )
            };

            if !enabled || !auto_update {
                continue;
            }

            let should_update = match last_updated {
                None => true,
                Some(ref s) => {
                    if let Ok(last_dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                        let now = chrono::Local::now().naive_local();
                        (now - last_dt).num_hours() >= 24
                    } else {
                        true
                    }
                }
            };

            if should_update {
                log_i!("[COMMUNITY-HOSTS] Запуск планового автообновления community hostlist (24ч)...");
                match crate::api::do_sync_community_hostlist(&state, None).await {
                    Ok((count, dt)) => {
                        log_i!("[COMMUNITY-HOSTS] ✓ Автообновление успешно: {} доменов ({})", count, dt);
                    }
                    Err(e) => {
                        log_w!("[COMMUNITY-HOSTS] Ошибка автообновления: {}", e);
                    }
                }
            }
        }
    });
}

pub fn spawn_zapret_update_checker(state: AppState) {
    tokio::spawn(async move {
        for _ in 0..15 {
            if is_shutdown() {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }

        loop {
            let last_check = {
                let cfg = state.config.read().await;
                cfg.zapret.last_update_check.clone()
            };

            let now = chrono::Local::now();
            if crate::api::is_zapret_5am_due(last_check.as_deref(), now) {
                log_i!("[ZAPRET-UPDATER] Наступило 5:00 утра — плановая проверка обновлений Zapret...");
                if let Err(e) = crate::api::check_zapret_update_core(&state, false).await {
                    log_w!("[ZAPRET-UPDATER] Ошибка проверки обновлений Zapret: {}", e);
                }
            }

            for _ in 0..3600 {
                if is_shutdown() {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }
        }
    });
}

/// Фоновый сторож AdGuard Home: предотвращение DNS-блэкаутов при падении службы.
/// Если iptables-редирект XKEEN_ADGUARD активен, но AdGuard DNS listener не отвечает,
/// сторож временно снимает правила iptables для спасения доступа в интернет на роутере и в LAN.
/// Как только AdGuard оживает, правила автоматически восстанавливаются.
pub fn spawn_adguard_monitor(state: AppState) {
    tokio::spawn(async move {
        // Начальная пауза для старта служб
        for _ in 0..12 {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }

        let mut iptables_temporarily_suspended = false;

        loop {
            for _ in 0..6 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }

            if is_service_stopped() {
                continue;
            }

            // Защита от каскадного сбоя: пока Mihomo упал, AdGuard iptables НЕ восстанавливаются
            // (порядок восстановления: сначала Mihomo -> затем AdGuard). Но защитное СНЯТИЕ
            // редиректа при мёртвом AdGuard выполняется всегда — раньше сторож целиком
            // пропускал тик при MIHOMO_DOWN и оставлял LAN без DNS, если падали оба сервиса.
            let mihomo_down = is_mihomo_down();

            let (mut enabled, host, configured_port, adguard_cfg, router_ip) = {
                let cfg = state.config.read().await;
                (
                    cfg.adguard.enabled || cfg.adblock_enabled,
                    cfg.adguard.host.clone(),
                    cfg.adguard.dns_port,
                    cfg.adguard.clone(),
                    cfg.rci.host.clone(),
                )
            };
            let dns_port = crate::adguard::detect_adguard_dns_port().unwrap_or(configured_port);

            // Автономный мониторинг фактического состояния службы AdGuard Home
            let agh_status = crate::adguard::get_status(&state.http, &adguard_cfg).await;
            let is_running_protected = agh_status.running && agh_status.protection_enabled;

            // Если AdGuard запущен и активен, но в конфигурации был выключен — автоматически синхронизируем
            let needs_cfg_sync =
                should_auto_integrate_adguard(is_running_protected, enabled, adguard_cfg.integration_choice_locked);
            if needs_cfg_sync {
                enabled = true;
            }
            if needs_cfg_sync && !mihomo_down {
                // Транзакция завершается reload Mihomo — при упавшем ядре она бессмысленна
                if let Ok(mut tx) = crate::transaction::ConfigTx::begin(&state).await {
                    tx.config_mut().adguard.enabled = true;
                    tx.config_mut().adblock_enabled = true;
                    // Автоподхват — один раз: дальше решает только пользователь (баг 116)
                    tx.config_mut().adguard.integration_choice_locked = true;
                    if dns_port > 0 {
                        tx.config_mut().adguard.dns_port = dns_port;
                    }
                    let _ = tx.commit_and_reload().await;
                    log_i!("[WATCHDOG] ✓ Автономная синхронизация: AdGuard Home активен, adguard.enabled=true зафиксирован в конфигурации");
                }
            }

            // Автономное устранение петель (1053) через live API
            if agh_status.running {
                let _ = crate::adguard::heal_adguard_live_config(&state.http, &adguard_cfg, Some(&router_ip)).await;
            }

            // G-02: десятки синхронных iptables-вызовов внутри is_adguard_iptables_active
            // фризили async-цикл — выносим в блокирующий пул tokio
            let iptables_active = tokio::task::spawn_blocking(move || {
                crate::adguard::is_adguard_iptables_active(dns_port)
            })
            .await
            .unwrap_or(false);

            if !enabled && !is_running_protected {
                if iptables_active {
                    let _ = crate::adguard::remove_adguard_iptables();
                }
                iptables_temporarily_suspended = false;
                continue;
            }

            if iptables_active || iptables_temporarily_suspended {
                let listener_ok = crate::adguard::test_agh_dns_listener(&host, dns_port).await;
                set_adguard_down(!listener_ok);

                if !listener_ok && iptables_active {
                    // AdGuard Home упал или завис при активных правилах перенаправления!
                    log_w!(
                        "[WATCHDOG] ⚠️ AdGuard Home DNS-listener на {}:{} не отвечает! Временное отключение XKEEN_ADGUARD iptables для защиты от DNS-блэкаута...",
                        host,
                        dns_port
                    );
                    let _ = crate::adguard::remove_adguard_iptables();
                    iptables_temporarily_suspended = true;
                } else if iptables_temporarily_suspended && can_restore_adguard_iptables(mihomo_down, listener_ok) {
                    // AdGuard Home восстановился (и Mihomo жив — каскадная защита)
                    log_i!(
                        "[WATCHDOG] ✓ AdGuard Home DNS-listener на {}:{} восстановил работу! Повторное включение XKEEN_ADGUARD iptables.",
                        host,
                        dns_port
                    );
                    let _ = crate::adguard::apply_adguard_iptables(dns_port);
                    iptables_temporarily_suspended = false;
                }
            } else if (enabled || is_running_protected) && !iptables_active && !iptables_temporarily_suspended {
                // Если служба запущена и отвечает, но правила слетели (например, сброс KeeneticOS iptables)
                // Защита от каскадного сбоя: не восстанавливаем AdGuard iptables, если Mihomo упал!
                if !mihomo_down {
                    let listener_ok = crate::adguard::test_agh_dns_listener(&host, dns_port).await;
                    if can_restore_adguard_iptables(mihomo_down, listener_ok) {
                        let _ = crate::adguard::apply_adguard_iptables(dns_port);
                        log_i!("[WATCHDOG] ✓ Восстановлен системный редирект XKEEN_ADGUARD iptables (порт 53 -> {})", dns_port);
                    }
                }
            }
        }
    });
}

/// Фоновый сторож Mihomo (spawn_mihomo_health_monitor):
/// Предотвращение потери интернета при аварийном падении ядра Mihomo.
/// Если Mihomo падает при активных цепочках XKEEN_BS или XKEEN_GAMING,
/// весь TCP/UDP перенаправляется в мёртвые порты (5000/5001) -> блэкаут сети.
/// Сторож проверяет 127.0.0.1:9090/version каждые 12 секунд.
/// При 3 подряд сбоях (36 сек):
/// - Экстренно снимает iptables-цепочки XKEEN_BS и XKEEN_GAMING.
/// - Удаляет NDM-хуки (060-xkeen-gaming.sh, 061-xkeen-bs.sh).
/// - Пытается перезапустить службу (/opt/etc/init.d/S05xkeen restart или /opt/sbin/xkeen -restart).
/// - Устанавливает флаг MIHOMO_DOWN = true (блокируя также нежелательный подъем AdGuard).
/// При восстановлении Mihomo:
/// - Переприменяет правила перехвата через sync_full_tunnel_modes.
/// - Сбрасывает флаг MIHOMO_DOWN = false.
pub fn spawn_mihomo_health_monitor(state: AppState) {
    tokio::spawn(async move {
        // Начальная пауза для старта служб
        for _ in 0..15 {
            if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }

        let mut consecutive_failures: u32 = 0;
        let mut iptables_cleaned: bool = false;

        loop {
            for _ in 0..12 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }

            if is_service_stopped() {
                consecutive_failures = 0;
                continue;
            }

            let cfg = state.config.read().await.clone();
            let is_healthy = crate::mihomo::get_version(&state.http, &cfg).await.is_some();
            let (new_failures, should_cleanup, should_restore) =
                evaluate_mihomo_health_step(consecutive_failures, is_healthy, iptables_cleaned);
            consecutive_failures = new_failures;

            if should_cleanup {
                log_w!(
                    "[WATCHDOG] ⚠️ Mihomo ({}) не отвечает! Экстренное снятие iptables-правил перехвата трафика...",
                    cfg.mihomo_url()
                );

                // a & b) Экстренное снятие цепочек XKEEN_BS / XKEEN_GAMING
                // c) Удаление NDM-хуков
                crate::api::emergency_cleanup_full_tunnel_rules().await;

                // d) Попытаться автоматически перезапустить Mihomo
                #[cfg(target_os = "linux")]
                {
                    tokio::spawn(async {
                        let _ = tokio::process::Command::new("sh")
                            .arg("-c")
                            .arg(
                                "if [ -x /opt/etc/init.d/S99mihomo ]; then \
                                    /opt/etc/init.d/S99mihomo restart >/dev/null 2>&1; \
                                 elif [ -x /opt/etc/init.d/S05xkeen ]; then \
                                    /opt/etc/init.d/S05xkeen restart >/dev/null 2>&1; \
                                 elif [ -x /opt/sbin/xkeen ]; then \
                                    /opt/sbin/xkeen -restart >/dev/null 2>&1; \
                                 fi"
                            )
                            .output()
                            .await;
                    });
                }

                // e) Установить флаг MIHOMO_DOWN = true
                set_mihomo_down(true);
                iptables_cleaned = true;
            } else if should_restore || (is_healthy && is_mihomo_down()) {
                log_i!("[WATCHDOG] ✓ Mihomo восстановил работу! Повторное применение iptables-правил.");

                // Флаг снимаем ДО синка: sync_full_tunnel_modes не ставит перехват,
                // пока MIHOMO_DOWN=true, иначе восстановление было бы пустой операцией.
                set_mihomo_down(false);
                iptables_cleaned = false;

                // Восстановление iptables и NDM-хуков через sync_full_tunnel_modes
                let bs_active = cfg.whitelist.enabled && cfg.whitelist.strategy == config::WhitelistStrategy::StrictXKeen;
                let gaming_active = cfg.gaming.enabled && cfg.gaming.mode == config::GamingMode::Compatibility;

                if bs_active || gaming_active {
                    let (redir_port, tproxy_port) = if let Ok(yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                        (
                            routing::parse_redir_port(&yaml).unwrap_or(5000),
                            routing::parse_tproxy_port(&yaml).unwrap_or(5001),
                        )
                    } else {
                        (5000, 5001)
                    };
                    let dns_port = crate::api::resolve_active_dns_port(&cfg);
                    crate::api::sync_full_tunnel_modes(&cfg, redir_port, tproxy_port, dns_port).await;
                }
            }
        }
    });
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RecentGamingConn {
    pub target: String,
    #[serde(default)]
    pub host: String,
    pub destination: String,
    pub network: String,
    pub device_ip: String,
    pub last_seen: String,
    pub ignored: bool,
}

static RECENT_GAMING_CONNS: std::sync::Mutex<Vec<RecentGamingConn>> = std::sync::Mutex::new(Vec::new());

pub fn get_recent_gaming_conns(ignored_list: &[String]) -> Vec<RecentGamingConn> {
    let mut list = RECENT_GAMING_CONNS.lock().map(|g| g.clone()).unwrap_or_default();
    for item in &mut list {
        if item.host.is_empty() && !item.target.is_empty() {
            item.host = item.target.clone();
        }
        item.ignored = is_conn_target_ignored(&item.target, &item.destination, ignored_list);
    }
    list
}

/// Вспомогательная логика шага проверки состояния Mihomo для unit-тестов и основного цикла.
/// Принимает текущее число сбоев, состояние Mihomo (healthy), и флаги.
/// Возвращает (новое число сбоев, нужно ли вызвать аварийную очистку, нужно ли вызвать восстановление).
pub fn evaluate_mihomo_health_step(
    consecutive_failures: u32,
    is_healthy: bool,
    iptables_cleaned: bool,
) -> (u32, bool, bool) {
    if !is_healthy {
        let new_failures = consecutive_failures + 1;
        let should_cleanup = new_failures >= 3 && !iptables_cleaned;
        (new_failures, should_cleanup, false)
    } else {
        let should_restore = iptables_cleaned;
        (0, false, should_restore)
    }
}

/// Проверка, разрешено ли восстанавливать AdGuard Home iptables с учетом статуса Mihomo.
/// Защита от каскадного сбоя: если Mihomo упал, AdGuard iptables НЕ восстанавливаются.
pub fn can_restore_adguard_iptables(mihomo_down: bool, adguard_listener_ok: bool) -> bool {
    !mihomo_down && adguard_listener_ok
}

pub fn is_conn_target_ignored(target: &str, destination: &str, ignored_list: &[String]) -> bool {
    let t_low = target.trim().to_lowercase();
    let d_low = destination.trim().to_lowercase();
    let dest_ip = d_low.split(':').next().unwrap_or("");
    ignored_list.iter().any(|ig| {
        let ig_low = ig.trim().to_lowercase();
        if ig_low.is_empty() {
            return false;
        }
        t_low == ig_low
            || d_low == ig_low
            || dest_ip == ig_low
            || t_low.ends_with(&format!(".{ig_low}"))
    })
}

pub fn is_gaming_connection_match(
    host: &str,
    dest_ip: &str,
    dest_port: &str,
    network: &str,
    rule: &str,
    gaming_domains: &[String],
) -> bool {
    let rule_low = rule.to_lowercase();
    if rule_low.contains("category-games") || rule_low.contains("gaming") {
        return true;
    }
    let host_low = host.trim().to_lowercase();
    if !host_low.is_empty() {
        for gd in gaming_domains {
            let gd_low = gd.trim().to_lowercase();
            if !gd_low.is_empty() && (host_low == gd_low || host_low.ends_with(&format!(".{gd_low}"))) {
                return true;
            }
        }
    }
    is_gaming_udp_heuristic(dest_ip, dest_port, network)
}

/// UDP-эвристика игрового трафика: высокие порты, кроме DNS/QUIC/NTP/SSDP/mDNS, STUN/WebRTC/VoIP
/// и (баг 11a) VPN (WireGuard 51820, OpenVPN 1194, IPsec 500/4500), Zoom 8801–8810, BitTorrent 6881–6889.
pub fn is_gaming_udp_heuristic(dest_ip: &str, dest_port: &str, network: &str) -> bool {
    if !network.eq_ignore_ascii_case("udp") || dest_ip.is_empty() {
        return false;
    }
    let Ok(port_num) = dest_port.parse::<u16>() else {
        return false;
    };
    port_num >= 1024
        && !matches!(
            port_num,
            1194 | 1900 | 3478..=3481 | 4500 | 5060 | 5061 | 5349 | 5353 | 5354 | 6881..=6889 | 8801..=8810 | 19302..=19309 | 51820
        )
}

/// Порог P2P (баг 11a): игра держит связь с 1–3 серверами, торрент/DHT — с десятками пиров.
pub const P2P_UDP_PEER_THRESHOLD: usize = 12;

/// Источники, у которых UDP-эвристике соответствует больше P2P_UDP_PEER_THRESHOLD разных адресов —
/// это P2P (торрент), и UDP-эвристика для них не продлевает игровую активность.
pub fn p2p_udp_sources(conns: &[serde_json::Value]) -> std::collections::HashSet<String> {
    let mut peers: std::collections::HashMap<String, std::collections::HashSet<String>> = std::collections::HashMap::new();
    for c in conns {
        let Some(meta) = c.get("metadata") else { continue };
        let s = |k: &str| meta.get(k).and_then(|v| v.as_str()).unwrap_or("").to_string();
        let port = meta
            .get("destinationPort")
            .map(|p| p.as_str().map(String::from).or_else(|| p.as_u64().map(|n| n.to_string())).unwrap_or_default())
            .unwrap_or_default();
        let (src, dst, net) = (s("sourceIP"), s("destinationIP"), s("network"));
        if !src.is_empty() && is_gaming_udp_heuristic(&dst, &port, &net) {
            peers.entry(src).or_default().insert(dst);
        }
    }
    peers.into_iter().filter(|(_, d)| d.len() > P2P_UDP_PEER_THRESHOLD).map(|(s, _)| s).collect()
}

pub fn record_recent_gaming_conn(conn: RecentGamingConn) {
    if let Ok(mut guard) = RECENT_GAMING_CONNS.lock() {
        if let Some(existing) = guard.iter_mut().find(|c| c.target.eq_ignore_ascii_case(&conn.target)) {
            existing.destination = conn.destination;
            existing.network = conn.network;
            existing.device_ip = conn.device_ip;
            existing.last_seen = conn.last_seen;
            existing.ignored = conn.ignored;
        } else {
            guard.insert(0, conn);
            if guard.len() > 25 {
                guard.truncate(25);
            }
        }
    }
}

/// Баг 10 & 17: IP устройств, за которыми следит умный режим: только включённые устройства с IP.
pub fn smart_gaming_watch_ips(gaming: &config::GamingConfig) -> Vec<String> {
    gaming
        .devices
        .iter()
        .filter(|d| d.enabled)
        .map(|d| d.ip.trim().to_string())
        .filter(|ip| !ip.is_empty())
        .collect()
}

pub fn spawn_smart_gaming_monitor(state: AppState) {
    tokio::spawn(async move {
        for _ in 0..12 {
            if is_shutdown() {
                return;
            }
            sleep(Duration::from_secs(1)).await;
        }

        let mut last_active_instant: Option<std::time::Instant> = None;

        loop {
            for _ in 0..8 {
                if is_shutdown() {
                    return;
                }
                sleep(Duration::from_secs(1)).await;
            }

            if is_service_stopped() {
                continue;
            }

            let cfg = state.config.read().await.clone();
            if !cfg.gaming.smart_mode && !cfg.gaming.enabled {
                continue;
            }

            // Собираем список всех известных игровых доменов (даже если сейчас gaming.enabled == false)
            let mut probe_gaming = cfg.gaming.clone();
            probe_gaming.enabled = true;
            let gaming_domains = routing::get_gaming_domains(&probe_gaming);

            // Баг 10: следим за всеми выбранными игровыми устройствами, а не только за первым
            let watch_ips = smart_gaming_watch_ips(&cfg.gaming);

            let Ok(val) = mihomo::m_get(&state.http, &cfg, "/connections").await else {
                continue;
            };

            let mut found_non_ignored = false;
            let mut trigger_target = String::new();
            let now_str = chrono::Local::now().format("%H:%M:%S").to_string();

            if let Some(conns) = val.get("connections").and_then(|c| c.as_array()) {
                // Баг 11a: у источника P2P-трафика (торрент) UDP-эвристика не считается игрой
                let p2p_sources = p2p_udp_sources(conns);
                for c in conns {
                    let Some(meta) = c.get("metadata") else { continue };
                    let src_ip = meta.get("sourceIP").and_then(|s| s.as_str()).unwrap_or("");
                    if !watch_ips.is_empty() && !watch_ips.iter().any(|ip| ip == src_ip) {
                        continue;
                    }
                    let host = meta.get("host").and_then(|h| h.as_str()).unwrap_or("");
                    let dest_ip = meta.get("destinationIP").and_then(|d| d.as_str()).unwrap_or("");
                    let dest_port = meta
                        .get("destinationPort")
                        .map(|p| {
                            if let Some(s) = p.as_str() {
                                s.to_string()
                            } else if let Some(n) = p.as_u64() {
                                n.to_string()
                            } else {
                                String::new()
                            }
                        })
                        .unwrap_or_default();
                    let net = meta.get("network").and_then(|n| n.as_str()).unwrap_or("tcp");
                    // Баг 11c: `rule` — тип правила (DomainSuffix, GeoSite…), категория/имя набора — в `rulePayload`
                    let rule = c.get("rulePayload").and_then(|r| r.as_str()).unwrap_or("");

                    let is_game = if p2p_sources.contains(src_ip) {
                        is_gaming_connection_match(host, "", "", "", rule, &gaming_domains)
                    } else {
                        is_gaming_connection_match(host, dest_ip, &dest_port, net, rule, &gaming_domains)
                    };
                    if is_game {
                        let destination = format!("{dest_ip}:{dest_port}");
                        let target = if !host.trim().is_empty() {
                            host.trim().to_lowercase()
                        } else {
                            destination.clone()
                        };
                        let ignored = is_conn_target_ignored(&target, &destination, &cfg.gaming.ignored_game_conns);
                        record_recent_gaming_conn(RecentGamingConn {
                            host: target.clone(),
                            target: target.clone(),
                            destination,
                            network: net.to_uppercase(),
                            device_ip: src_ip.to_string(),
                            last_seen: now_str.clone(),
                            ignored,
                        });
                        if !ignored {
                            found_non_ignored = true;
                            if trigger_target.is_empty() {
                                trigger_target = target;
                            }
                        }
                    }
                }
            }

            if !cfg.gaming.smart_mode {
                continue;
            }

            if found_non_ignored {
                last_active_instant = Some(std::time::Instant::now());
                // Баг 7: умный режим только включает игровой туннель, выбранный режим не трогает
                if !cfg.gaming.enabled {
                    // X-02: пока failover приостановил игровой режим (активен узел [БС],
                    // gaming_suspended=true) — авто-включение запрещено во избежание
                    // воспроизведения конфликта сокетов БС↔gaming.
                    if cfg.whitelist.gaming_suspended {
                        log_i!(
                            "[SMART-GAMING] ⏸️ Обнаружено игровое подключение ({}), но игровой режим приостановлен failover (активен узел [БС]) — авто-включение пропущено",
                            trigger_target
                        );
                    }
                    // Включаем игровой режим только если выбрано игровое устройство с IP
                    else if !watch_ips.is_empty() {
                        log_i!(
                            "[SMART-GAMING] 🎮 Обнаружено игровое подключение ({}) на {} — автоматическое включение игрового режима",
                            trigger_target,
                            watch_ips.join(", ")
                        );
                        if let Err(e) = apply_smart_gaming_toggle(&state, true).await {
                            log_w!("[SMART-GAMING] Не удалось включить игровой режим, настройки не изменены: {e}");
                        }
                    }
                }
            } else if cfg.gaming.enabled {
                let idle_mins = cfg.gaming.smart_idle_timeout_mins.max(1) as u64;
                let idle_dur = Duration::from_secs(idle_mins * 60);
                let ref_instant = *last_active_instant.get_or_insert_with(std::time::Instant::now);
                if ref_instant.elapsed() >= idle_dur {
                    log_i!(
                        "[SMART-GAMING] 💤 Нет активных игровых подключений более {} мин — автоматическое отключение игрового режима",
                        idle_mins
                    );
                    last_active_instant = None;
                    if let Err(e) = apply_smart_gaming_toggle(&state, false).await {
                        log_w!("[SMART-GAMING] Не удалось выключить игровой режим, настройки не изменены: {e}");
                    }
                }
            }
        }
    });
}

/// Изменения конфига при авто-переключении умным режимом: только флаг включения (баг 7 — режим
/// пользователя KnownServices/SmartSplit/BypassRu/Compatibility сохраняется).
pub fn apply_smart_gaming_flags(c: &mut config::AppConfig, enable: bool) {
    c.gaming.enabled = enable;
}

/// Включение/выключение игрового режима умным режимом (баг 9): config.json, config.yaml и reload Mihomo —
/// одна транзакция. Если ядро не приняло конфиг, ничего не сохраняется и iptables не трогаются.
pub async fn apply_smart_gaming_toggle(state: &AppState, enable: bool) -> Result<config::AppConfig, String> {
    if enable {
        // Баг 5: проверяем фактический активный узел, а не только флаг приостановки
        let cfg = state.config.read().await.clone();
        let active = mihomo::get_active_server(&state.http, &cfg).await?;
        if crate::failover::gaming_enable_blocked_by_bs(&cfg, active.as_ref()) {
            return Err(crate::failover::GAMING_BLOCKED_BY_BS.to_string());
        }
    }
    let (new_cfg, new_yaml) = crate::api::commit_config_with_routing(state, |c| apply_smart_gaming_flags(c, enable)).await?;
    if enable {
        let target_srv = if new_cfg.gaming.target_server.trim().is_empty() {
            "Fastest"
        } else {
            new_cfg.gaming.target_server.trim()
        };
        // Баг 8: сбрасываем только сокеты игровых устройств, а не всей LAN
        let _ = mihomo::switch_gaming_group(&state.http, &new_cfg, target_srv).await;
    }
    let redir_port = routing::parse_redir_port(&new_yaml).unwrap_or(5000);
    let tproxy_port = routing::parse_tproxy_port(&new_yaml).unwrap_or(5001);
    let dns_port = crate::api::resolve_active_dns_port(&new_cfg);
    let _ = crate::api::sync_full_tunnel_modes(&new_cfg, redir_port, tproxy_port, dns_port).await;
    Ok(new_cfg)
}

/// Можно ли сторожу самому включить интеграцию AdGuard (баг 116): только для работающего с защитой AGH,
/// пока интеграция выключена и пользователь ещё ни разу не делал выбор (Zero-Touch для внешнего AGH).
pub fn should_auto_integrate_adguard(running_protected: bool, integration_enabled: bool, choice_locked: bool) -> bool {
    running_protected && !integration_enabled && !choice_locked
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

    #[test]
    fn test_smart_gaming_connection_match_and_ignore() {
        let domains = vec!["steamcommunity.com".to_string(), "discord.gg".to_string()];
        assert!(is_gaming_connection_match("api.steamcommunity.com", "1.2.3.4", "443", "tcp", "MATCH", &domains));
        assert!(is_gaming_connection_match("", "155.133.248.34", "27015", "udp", "MATCH", &domains));
        assert!(!is_gaming_connection_match("google.com", "8.8.8.8", "443", "udp", "MATCH", &domains));

        let ignored = vec!["api.steamcommunity.com".to_string(), "155.133.248.34:27015".to_string()];
        assert!(is_conn_target_ignored("api.steamcommunity.com", "1.2.3.4:443", &ignored));
        assert!(is_conn_target_ignored("155.133.248.34:27015", "155.133.248.34:27015", &ignored));
        assert!(!is_conn_target_ignored("discord.gg", "162.159.130.234:443", &ignored));
    }

    #[test]
    fn test_watchdog_effective_needs_checks_prevents_infinite_loop() {
        // BUG-16: When all force_domains are intercepted by Zapret Direct (e.g. youtube.com),
        // needs_force must be FALSE, preventing missing_force = true and 4-second reload loop.
        let mut zapret_cfg = crate::config::ZapretConfig::default();
        zapret_cfg.enabled = true;
        zapret_cfg.hybrid_youtube = true;

        let force_domains = vec!["youtube.com".to_string(), "googlevideo.com".to_string()];
        let direct_domains: Vec<String> = vec![];

        let needs_force = force_domains.iter().any(|d| {
            let clean = crate::config::normalize_domain(d);
            if clean.is_empty() || !clean.contains('.') {
                return false;
            }
            if direct_domains.iter().any(|dir| crate::config::normalize_domain(dir) == clean) {
                return false;
            }
            if zapret_cfg.enabled && routing::is_zapret_direct_domain(&clean, &zapret_cfg) {
                return false;
            }
            true
        });
        assert!(!needs_force, "needs_force must be false when all domains are intercepted by zapret hybrid");

        // When device_routing has entries but none have active servers
        let mut dev_routing = std::collections::BTreeMap::new();
        dev_routing.insert("192.168.1.50".to_string(), crate::config::DeviceRouting { servers: vec![], ..Default::default() });
        dev_routing.insert("192.168.1.51".to_string(), crate::config::DeviceRouting { servers: vec!["default".to_string()], ..Default::default() });
        let schedules: Vec<crate::config::DeviceSchedule> = vec![];
        let gaming_cfg = crate::config::GamingConfig::default();

        let has_active_device_assignments = dev_routing.values().any(|dr| {
            dr.servers.first().is_some_and(|s| !s.trim().is_empty() && s.trim() != "default")
        }) || schedules.iter().any(|s| s.enabled)
           || (gaming_cfg.enabled && gaming_cfg.devices.iter().any(|d| d.enabled && d.server.as_ref().is_some_and(|s| !s.trim().is_empty())));

        assert!(!has_active_device_assignments, "needs_device must be false when no device has an active non-default server");
    }

    #[test]
    fn test_gaming_watchdog_port_exclusions() {
        // BUG-23: Exclude STUN, VoIP, and WebRTC ports from gaming match
        let domains = vec!["steamcommunity.com".to_string()];
        // STUN / VoIP / WebRTC ports should return false
        assert!(!is_gaming_connection_match("", "1.2.3.4", "3478", "udp", "MATCH", &domains));
        assert!(!is_gaming_connection_match("", "1.2.3.4", "5060", "udp", "MATCH", &domains));
        assert!(!is_gaming_connection_match("", "1.2.3.4", "5061", "tcp", "MATCH", &domains));
        assert!(!is_gaming_connection_match("", "1.2.3.4", "5349", "udp", "MATCH", &domains));
        assert!(!is_gaming_connection_match("", "1.2.3.4", "19302", "udp", "MATCH", &domains));
        assert!(!is_gaming_connection_match("", "1.2.3.4", "19309", "udp", "MATCH", &domains));

        // Actual game server port should return true
        assert!(is_gaming_connection_match("", "155.133.248.34", "27015", "udp", "MATCH", &domains));
    }

    #[test]
    fn test_gaming_conn_struct_sync() {
        // BUG-24: Host and target fields in RecentGamingConn
        let conn = RecentGamingConn {
            target: "155.133.248.34:27015".into(),
            host: "155.133.248.34:27015".into(),
            destination: "155.133.248.34:27015".into(),
            network: "udp".into(),
            device_ip: "192.168.1.100".into(),
            last_seen: "12:00:00".into(),
            ignored: false,
        };
        assert_eq!(conn.host, conn.target);
    }

    #[test]
    fn test_mihomo_health_check_failure_and_recovery_lifecycle() {
        // Проверка жизненного цикла сторожа Mihomo:
        // 1-2 неудачные проверки -> счетчик растет, очистка НЕ вызывается
        let (f1, clean1, rest1) = evaluate_mihomo_health_step(0, false, false);
        assert_eq!(f1, 1);
        assert!(!clean1, "1 сбой не должен инициировать снятие правил");
        assert!(!rest1);

        let (f2, clean2, rest2) = evaluate_mihomo_health_step(1, false, false);
        assert_eq!(f2, 2);
        assert!(!clean2, "2 сбоя не должны инициировать снятие правил");
        assert!(!rest2);

        // 3-й сбой подряд (36 сек) -> экстренная очистка iptables!
        let (f3, clean3, rest3) = evaluate_mihomo_health_step(2, false, false);
        assert_eq!(f3, 3);
        assert!(clean3, "3 подряд сбоя ДОЛЖНЫ инициировать экстренное снятие iptables!");
        assert!(!rest3);

        // Последующие сбои, если очистка уже проведена -> не спамим очистку повторно
        let (f4, clean4, rest4) = evaluate_mihomo_health_step(3, false, true);
        assert_eq!(f4, 4);
        assert!(!clean4, "Не должно повторно инициировать очистку при уже снятых правилах");
        assert!(!rest4);

        // Восстановление Mihomo -> триггер восстановления sync_full_tunnel_modes
        let (f_rec, clean_rec, rest_rec) = evaluate_mihomo_health_step(4, true, true);
        assert_eq!(f_rec, 0, "Счетчик сбоев должен сброситься в 0");
        assert!(!clean_rec);
        assert!(rest_rec, "При оживлении Mihomo должно быть вызвано восстановление правил");
    }

    /// Продакшн-часть исходника (без модуля тестов) для проверок порядка вызовов.
    fn production_source() -> &'static str {
        let src = include_str!("watchdog.rs");
        let cut = src.find("#[cfg(test)]\nmod tests").or_else(|| src.find("#[cfg(test)]\r\nmod tests")).unwrap_or(src.len());
        &src[..cut]
    }

    #[test]
    fn test_mihomo_restore_clears_down_flag_before_tunnel_sync() {
        // REG: sync_full_tunnel_modes не ставит перехват при MIHOMO_DOWN=true,
        // поэтому флаг обязан сниматься ДО синка — иначе восстановление пустое.
        let src = production_source();
        let restore = src.find("Mihomo восстановил работу").expect("restore branch");
        let tail = &src[restore..];
        let clear = tail.find("set_mihomo_down(false)").expect("flag reset in restore branch");
        let sync = tail.find("sync_full_tunnel_modes(").expect("tunnel sync in restore branch");
        assert!(clear < sync, "MIHOMO_DOWN должен сниматься до sync_full_tunnel_modes");
    }

    #[test]
    fn test_schedules_do_not_drop_whole_lan_connections() {
        // REG: расписание одного устройства вызывало switch_group → DELETE /connections
        // для ВСЕХ устройств LAN. Допустим только точечный switch_group_scoped.
        let src = production_source();
        let start = src.find("pub fn spawn_schedules_monitor").expect("schedules monitor");
        let end = src[start..].find("pub fn spawn_dhcp_device_monitor").map(|e| start + e).unwrap_or(src.len());
        let body = &src[start..end];
        assert!(!body.contains("mihomo::switch_group("), "расписания не должны рвать соединения всей LAN");
        assert!(body.contains("mihomo::switch_group_scoped("));
    }

    #[test]
    fn test_dhcp_rebuild_keeps_provider_nodes_and_updates_device_ip() {
        // REG: DHCP-монитор пересобирал только игровой блок с providers = &[] —
        // группа 🎮 Gaming теряла `use:` подписки, а выделенный сервер устройства
        // оставался привязан к старому IP. Пересборка обязана быть полной.
        let yaml = "port: 7890\nproxy-providers:\n  myprov:\n    type: http\n    url: \"https://example.invalid/sub\"\n    path: ./providers/myprov.yaml\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - Fastest\n  - name: Fastest\n    type: url-test\n    use:\n      - myprov\nrules:\n  - GEOIP,RU,DIRECT\n  - MATCH,PROXY\n";
        let mut cfg = crate::config::AppConfig::default();
        cfg.gaming.enabled = true;
        cfg.gaming.mode = crate::config::GamingMode::Compatibility;
        cfg.gaming.devices.push(crate::config::GamingDevice {
            mac: "00:11:22:33:44:55".into(),
            ip: "192.168.2.200".into(),
            ipv6: vec![],
            name: "Console".into(),
            enabled: true,
            server: Some("Fastest".into()),
        });

        let out = rebuild_routing_after_dhcp_change(yaml, &cfg).expect("rebuild must succeed");

        let begin = out.find(routing::GAMING_GROUP_BEGIN).expect("gaming group block");
        let end = out.find(routing::GAMING_GROUP_END).expect("gaming group end");
        let group_block = &out[begin..end];
        assert!(group_block.contains("use:"), "🎮 Gaming обязана сохранить серверы подписок: {group_block}");
        assert!(group_block.contains("- myprov"), "🎮 Gaming обязана ссылаться на провайдера myprov: {group_block}");
        assert!(out.contains("192.168.2.200"), "новый DHCP-адрес устройства должен попасть в правила");
    }

    #[test]
    fn test_cascade_failure_protection_adguard_and_mihomo() {
        // Проверка защиты от каскадного сбоя:
        // Если Mihomo упал (mihomo_down = true), AdGuard Home iptables НЕ должны восстанавливаться,
        // даже если сам AdGuard listener отвечает OK.
        assert!(
            !can_restore_adguard_iptables(true, true),
            "При падении Mihomo правила AdGuard НЕ должны восстанавливаться!"
        );
        assert!(
            !can_restore_adguard_iptables(true, false),
            "При падении обоих сервисов восстановление AdGuard запрещено"
        );

        // Только когда Mihomo поднялся (mihomo_down = false) и AdGuard здоров -> разрешено восстановление
        assert!(
            can_restore_adguard_iptables(false, true),
            "При здоровом Mihomo и работающем AdGuard восстановление разрешено"
        );
        assert!(
            !can_restore_adguard_iptables(false, false),
            "Если AdGuard мертв, восстанавливать нельзя"
        );
    }

    #[test]
    fn regression_bug_116_watchdog_respects_user_adguard_choice() {
        // Баг 116: сторож включал adguard.enabled и adblock_enabled на каждом тике, если AGH работает
        // с защитой, — выключить интеграцию в панели, оставив AGH, было невозможно.
        assert!(
            should_auto_integrate_adguard(true, false, false),
            "Zero-Touch: впервые найденный работающий AGH подхватывается автоматически"
        );
        assert!(
            !should_auto_integrate_adguard(true, false, true),
            "пользователь выключил интеграцию (или автоподхват уже был) — сторож не включает её снова"
        );
        assert!(!should_auto_integrate_adguard(true, true, false), "уже включено — делать нечего");
        assert!(!should_auto_integrate_adguard(false, false, false), "AGH не работает с защитой — не трогаем");
        let cfg = crate::config::AdGuardConfig::default();
        assert!(!cfg.integration_choice_locked, "по умолчанию выбор пользователя не зафиксирован");
    }

    #[test]
    fn regression_bug_10_smart_watch_ips_cover_every_enabled_device() {
        let dev = |ip: &str, enabled: bool| config::GamingDevice { mac: format!("m{ip}"), ip: ip.into(), enabled, ..Default::default() };
        let mut g = config::GamingConfig::default();
        g.devices = vec![dev("192.168.1.50", true), dev("192.168.1.60", true), dev("192.168.1.70", false), dev("", true)];
        assert_eq!(smart_gaming_watch_ips(&g), vec!["192.168.1.50", "192.168.1.60"], "все включённые с IP");
        // Баг 17: Единственное выключенное игровое устройство НЕ считается активным
        g.devices = vec![dev("192.168.1.70", false)];
        assert!(smart_gaming_watch_ips(&g).is_empty(), "единственное выключенное устройство не активно (баг 17)");
        g.devices = vec![dev("192.168.1.70", false), dev("192.168.1.80", false)];
        assert!(smart_gaming_watch_ips(&g).is_empty(), "не выбрано ни одно из нескольких — не включаем");
    }

    #[test]
    fn regression_bug_11a_non_game_udp_ports_are_not_games() {
        // Баг 11a: WireGuard, OpenVPN, IPsec NAT-T, Zoom, BitTorrent считались игрой.
        let domains: Vec<String> = Vec::new();
        for port in ["51820", "1194", "4500", "500", "8801", "8810", "6881", "6889"] {
            assert!(!is_gaming_connection_match("", "1.2.3.4", port, "udp", "", &domains), "UDP {port} — не игра");
        }
        assert!(is_gaming_connection_match("", "155.133.248.34", "27015", "udp", "", &domains), "игровой сервер Valve");
    }

    #[test]
    fn regression_bug_11a_p2p_sources_detected() {
        let conn = |src: &str, dst: &str, port: u64| serde_json::json!({ "metadata": {
            "sourceIP": src, "destinationIP": dst, "destinationPort": port, "network": "udp" } });
        let mut conns: Vec<serde_json::Value> = (0..20).map(|i| conn("192.168.1.50", &format!("5.6.7.{i}"), 40000 + i)).collect();
        conns.push(conn("192.168.1.60", "155.133.248.34", 27015));
        conns.push(conn("192.168.1.60", "155.133.248.35", 27016));
        let p2p = p2p_udp_sources(&conns);
        assert!(p2p.contains("192.168.1.50"), "20 разных пиров — P2P");
        assert!(!p2p.contains("192.168.1.60"), "два игровых сервера — игра");
    }

    #[test]
    fn regression_bug_7_smart_flags_preserve_mode() {
        for mode in [
            config::GamingMode::KnownServices,
            config::GamingMode::SmartSplit,
            config::GamingMode::BypassRu,
            config::GamingMode::Compatibility,
        ] {
            let mut c = config::AppConfig::default();
            c.gaming.mode = mode.clone();
            apply_smart_gaming_flags(&mut c, true);
            assert!(c.gaming.enabled);
            assert_eq!(c.gaming.mode, mode, "режим пользователя сохранён");
            apply_smart_gaming_flags(&mut c, false);
            assert!(!c.gaming.enabled);
            assert_eq!(c.gaming.mode, mode);
        }
    }

    #[tokio::test]
    async fn regression_bug_9_smart_gaming_failed_reload_changes_nothing() {
        // Баг 9: при отказе ядра умный режим всё равно сохранял config.json — UI показывал игровой режим,
        // а правил в ядре не было.
        let root = std::env::temp_dir().join(format!("xkeen-bug9-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&root).unwrap();
        let yaml_path = root.join("config.yaml");
        let json_path = root.join("config.json");
        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        cfg.mihomo.port = 1; // ядра нет: reload гарантированно не удастся
        cfg.gaming.smart_mode = true;
        cfg.gaming.enabled = false;
        let base = "mode: rule\nproxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";
        std::fs::write(&yaml_path, base).unwrap();
        crate::config::save(&json_path, &cfg).await.unwrap();
        let json_before = std::fs::read_to_string(&json_path).unwrap();
        let config_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let ag = std::sync::Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: std::sync::Arc::new(json_path.clone()),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        assert!(apply_smart_gaming_toggle(&state, true).await.is_err(), "отказ ядра — ошибка, а не тихий успех");
        assert_eq!(std::fs::read_to_string(&json_path).unwrap(), json_before, "config.json не изменён");
        assert_eq!(std::fs::read_to_string(&yaml_path).unwrap(), base, "config.yaml откатан");
        assert!(!state.config.read().await.gaming.enabled, "в памяти игровой режим не включён");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn regression_bug_28_zapret_give_up_rebuilds_mihomo_rules() {
        // Баг 28: после 5 неудачных стартов сторож ставил zapret.enabled=false, но не пересобирал
        // правила Mihomo — YouTube/Discord оставались в DIRECT уже без DPI-обхода.
        let root = std::env::temp_dir().join(format!("xkeen-bug28-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&root).unwrap();
        let yaml_path = root.join("config.yaml");
        let json_path = root.join("config.json");
        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        cfg.mihomo.port = 1; // ядра нет: reload гарантированно не удастся
        cfg.zapret.enabled = true;
        cfg.zapret.hybrid_youtube = true;
        let base = "mode: rule\nproxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";
        let (with_zapret, _) = crate::routing::apply_routing(base, &cfg).unwrap();
        assert!(with_zapret.contains(crate::routing::ZAPRET_HYBRID_BEGIN), "предпосылка: блок Zapret есть");
        std::fs::write(&yaml_path, &with_zapret).unwrap();
        crate::config::save(&json_path, &cfg).await.unwrap();
        let config_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let ag = std::sync::Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: std::sync::Arc::new(json_path.clone()),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        disable_zapret_after_failed_starts(&state).await.unwrap();

        let yaml_after = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(
            !yaml_after.contains(crate::routing::ZAPRET_HYBRID_BEGIN),
            "правила Zapret-hybrid должны быть убраны из config.yaml:\n{yaml_after}"
        );
        let json_after: crate::config::AppConfig =
            serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
        assert!(!json_after.zapret.enabled);
        assert!(!state.config.read().await.zapret.enabled);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn regression_bug_57_zapret_toggle_rebuilds_rules_both_ways() {
        // Баг 57: вкл/выкл Zapret из бота меняло только config.json — правила Mihomo не пересобирались.
        let root = std::env::temp_dir().join(format!("xkeen-bug57-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&root).unwrap();
        let yaml_path = root.join("config.yaml");
        let json_path = root.join("config.json");
        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        cfg.mihomo.port = 1;
        cfg.zapret.enabled = false;
        cfg.zapret.hybrid_youtube = true;
        let base = "mode: rule\nproxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";
        std::fs::write(&yaml_path, base).unwrap();
        crate::config::save(&json_path, &cfg).await.unwrap();
        let config_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let ag = std::sync::Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: std::sync::Arc::new(json_path.clone()),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        crate::api::persist_zapret_enabled_with_routing(&state, true).await.unwrap();
        let on = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(on.contains(crate::routing::ZAPRET_HYBRID_BEGIN), "включение добавляет правила Zapret:\n{on}");
        assert!(state.config.read().await.zapret.enabled);

        crate::api::persist_zapret_enabled_with_routing(&state, false).await.unwrap();
        let off = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(!off.contains(crate::routing::ZAPRET_HYBRID_BEGIN), "выключение убирает правила Zapret");
        let j: crate::config::AppConfig = serde_json::from_str(&std::fs::read_to_string(&json_path).unwrap()).unwrap();
        assert!(!j.zapret.enabled);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn regression_bug_59c_service_stopped_flag_survives_panel_restart() {
        // Баг 59: состояние «службы остановлены/паника» жило только в памяти и терялось при рестарте панели.
        let path = std::env::temp_dir().join(format!("xkeen-stopped-{}.flag", crate::auth::random_hex(8)));
        assert!(!read_service_stopped_flag(&path));
        persist_service_stopped_flag(&path, true);
        assert!(read_service_stopped_flag(&path), "флаг читается после «перезапуска»");
        persist_service_stopped_flag(&path, false);
        assert!(!read_service_stopped_flag(&path));
        assert!(!path.exists());
    }

    #[test]
    fn regression_bug_36_zapret_watchdog_fixes() {
        // Баг 36:
        // 1. При выключенном Zapret любое чужое правило на 1053 запускало stop-fw каждые 12 секунд.
        // 2. Сторож держал config_lock на всё время выполнения команд S51zapret.
        let wd_src = include_str!("watchdog.rs");
        let start = wd_src.find("pub fn spawn_zapret_monitor(").expect("spawn_zapret_monitor exists");
        let b = &wd_src[start..start + 3000];

        // Проверка 1: при проверке iptables_left не должно быть слепого grep 'to-ports 1053' без привязки к xkeen-route-zapret
        assert!(
            !b.contains("grep -q 'to-ports 1053'"),
            "Watchdog must not blindly match any foreign 1053 rule without xkeen-route-zapret comment"
        );

        // Проверка 2: config_lock не должен удерживаться во время вызова start-fw / S51zapret
        assert!(
            !b.contains("let Ok(_cfg_guard) = state.config_lock.try_lock()"),
            "Watchdog must not hold config_lock across long shell command execution"
        );
    }
    #[test]
    fn regression_bug_118_ip_keyed_settings_follow_device_after_dhcp_change() {
        // по IP; DHCP-слежение было только для гейминга в Compatibility — после смены IP правила доставались
        // тому, кто получил старый адрес.
        let mut prev = std::collections::HashMap::new();
        prev.insert("aa:aa:aa:aa:aa:01".to_string(), "192.168.1.10".to_string());
        prev.insert("aa:aa:aa:aa:aa:02".to_string(), "192.168.1.20".to_string());
        let now = vec![
            ("AA:AA:AA:AA:AA:01".to_string(), "192.168.1.30".to_string()),
            ("aa:aa:aa:aa:aa:02".to_string(), "192.168.1.20".to_string()),
            ("aa:aa:aa:aa:aa:03".to_string(), "192.168.1.10".to_string()),
        ];
        let changes = detect_dhcp_ip_changes(&prev, &now);
        assert_eq!(changes, vec![("192.168.1.10".to_string(), "192.168.1.30".to_string())]);

        let mut cfg = crate::config::AppConfig::default();
        cfg.device_routing.insert("192.168.1.10".to_string(), crate::config::DeviceRouting::default());
        cfg.device_domain_rules.insert("192.168.1.10".to_string(), vec![]);
        cfg.zapret.excluded_devices = vec!["192.168.1.10".to_string(), "aa:aa:aa:aa:aa:09".to_string()];
        cfg.whitelist.excluded_devices = vec!["192.168.1.10".to_string()];
        cfg.schedules.push(crate::config::DeviceSchedule { id: "s1".into(), ip: "192.168.1.10".into(), ..Default::default() });
        cfg.whitelist.devices.push(crate::config::WhitelistDevice {
            mac: "aa:aa:aa:aa:aa:01".into(), ip: "192.168.1.10".into(), enabled: true, name: None, server: None,
        });

        assert!(migrate_ip_keyed_config(&mut cfg, &changes));
        assert!(cfg.device_routing.contains_key("192.168.1.30") && !cfg.device_routing.contains_key("192.168.1.10"));
        assert!(cfg.device_domain_rules.contains_key("192.168.1.30"));
        assert_eq!(cfg.zapret.excluded_devices, vec!["192.168.1.30".to_string(), "aa:aa:aa:aa:aa:09".to_string()]);
        assert_eq!(cfg.whitelist.excluded_devices, vec!["192.168.1.30".to_string()]);
        assert_eq!(cfg.schedules[0].ip, "192.168.1.30");
        assert_eq!(cfg.whitelist.devices[0].ip, "192.168.1.30");
        assert!(!migrate_ip_keyed_config(&mut cfg, &[]), "нет изменений — нет записи");
    }
}

#[cfg(test)]
mod structural_regression_tests {
    /// Тело функции в исходнике (до следующей функции верхнего уровня), без тестовых модулей.
    fn body(src: &'static str, sig: &str) -> &'static str {
        let cut = src.find("#[cfg(test)]").unwrap_or(src.len());
        let prod = &src[..cut];
        let start = prod.find(sig).unwrap_or_else(|| panic!("нет {sig}"));
        let rest = &prod[start..];
        let end = rest[1..]
            .find("\npub fn ")
            .into_iter()
            .chain(rest[1..].find("\npub async fn "))
            .chain(rest[1..].find("\nfn "))
            .chain(rest[1..].find("\nasync fn "))
            .min()
            .map(|e| e + 1)
            .unwrap_or(rest.len());
        &rest[..end]
    }

    #[test]
    fn regression_bug_5_smart_gaming_checks_active_bs_not_only_flag() {
        // Баг 5: умный режим проверял только gaming_suspended — при вручную выбранном БС включал игровой.
        let b = body(include_str!("watchdog.rs"), "pub async fn apply_smart_gaming_toggle(");
        assert!(b.contains("gaming_enable_blocked_by_bs("), "проверка фактического активного узла");
    }

    #[test]
    fn regression_bug_7_smart_gaming_keeps_user_mode() {
        // Баг 7: умный режим при первом игровом соединении менял выбранный пользователем режим
        // (KnownServices, SmartSplit) на Compatibility.
        let src = include_str!("watchdog.rs");
        let toggle = body(src, "pub async fn apply_smart_gaming_toggle(");
        assert!(!toggle.contains("c.gaming.mode = config::GamingMode::Compatibility"), "режим пользователя не подменяется");
        let mon = body(src, "pub fn spawn_smart_gaming_monitor(");
        assert!(!mon.contains("cfg.gaming.mode != config::GamingMode::Compatibility"), "включённый режим любого типа не «перевключается»");
    }

    #[test]
    fn regression_bug_8_smart_gaming_does_not_drop_whole_lan() {
        // Баг 8: умный режим вызывал switch_group (close_all) — рвались звонки и загрузки всей сети.
        let b = body(include_str!("watchdog.rs"), "pub async fn apply_smart_gaming_toggle(");
        assert!(!b.contains("mihomo::switch_group("), "без полного сброса соединений LAN");
    }

    #[test]
    fn regression_bug_10_smart_gaming_watches_all_enabled_devices() {
        // Баг 10: умный режим брал только первое включённое устройство (`find`) — игра на втором
        // не продлевала активность, и по таймауту игровой режим выключался у всех.
        let b = body(include_str!("watchdog.rs"), "pub fn spawn_smart_gaming_monitor(");
        assert!(!b.contains(".find(|d| if has_explicit_enabled"), "не только первое устройство");
        assert!(b.contains("smart_gaming_watch_ips("), "список всех отслеживаемых устройств");
    }

    #[test]
    fn regression_bug_11a_torrent_like_udp_does_not_keep_gaming_alive() {
        // Баг 11a: любой UDP на порт ≥1024 считался игрой — торрент (десятки пиров) не давал наступить таймауту.
        let b = body(include_str!("watchdog.rs"), "pub fn spawn_smart_gaming_monitor(");
        assert!(b.contains("p2p_udp_sources("), "источники P2P-трафика исключаются из UDP-эвристики");
    }

    #[test]
    fn regression_bug_11c_detection_reads_rule_payload_not_rule_type() {
        // Баг 11c: в поле `rule` Mihomo — тип правила (DomainSuffix, GeoSite, Match); проверка
        // `contains("category-games")` по нему никогда не срабатывала. Категория — в `rulePayload`.
        let b = body(include_str!("watchdog.rs"), "pub fn spawn_smart_gaming_monitor(");
        assert!(b.contains("c.get(\"rulePayload\")"), "категория правила берётся из rulePayload");
        assert!(!b.contains("c.get(\"rule\")"), "тип правила не используется как категория");
    }

    #[test]
    fn regression_bug_9_smart_gaming_writes_via_configtx() {
        // Баг 9: умный режим писал config.yaml напрямую, игнорировал ошибку reload и сохранял config.json
        // в любом случае — конфиг расходился с ядром.
        let b = body(include_str!("watchdog.rs"), "pub fn spawn_smart_gaming_monitor(");
        assert!(!b.contains("atomic_write_file"), "без прямой записи config.yaml");
        assert!(!b.contains("config::save("), "без прямой записи config.json");
        assert!(b.contains("apply_smart_gaming_toggle("), "переключение через общую транзакцию");
    }

    #[test]
    fn regression_bug_95_zapret_watchdog_writes_via_configtx() {
        // Баг 95: авто-отключение Zapret писало config.json без config_lock (мимо ConfigTx).
        let b = body(include_str!("watchdog.rs"), "pub fn spawn_zapret_monitor(");
        assert!(b.contains("disable_zapret_after_failed_starts"), "выключение — через общую транзакцию");
        assert!(!b.contains("config::save("), "без прямой записи config.json");
    }

    #[test]
    fn regression_bug_96_dhcp_watchdog_writes_via_configtx() {
        // Баг 96: DHCP-сторож писал config.yaml мимо ConfigTx (без валидации и отката).
        let b = body(include_str!("watchdog.rs"), "pub fn spawn_dhcp_device_monitor(");
        assert!(b.contains("ConfigTx::begin"), "запись через ConfigTx");
        assert!(!b.contains("atomic_write_file"), "без прямой записи config.yaml");
        assert!(!b.contains("config::save("), "без прямой записи config.json");
    }

    #[test]
    fn regression_bug_83_no_commit_per_provider_in_background_sync() {
        // Баг 83: миграция провайдеров делала N коммитов и N reload Mihomo подряд.
        let b = body(include_str!("api.rs"), "pub async fn sync_all_mobile_providers(");
        assert!(!b.contains("ConfigTx::begin"), "фоновая синхронизация не открывает транзакцию на каждый провайдер");
        assert!(!b.contains("commit_and_reload"), "и не перезагружает Mihomo в цикле");
    }

    #[test]
    fn regression_bug_85_speedtest_restores_only_its_group() {
        // Баг 85: speedtest восстанавливал ВСЕ selector-группы из снимка, откатывая чужие изменения за время теста.
        let b = body(include_str!("speedtest.rs"), "pub async fn run_speedtest(");
        assert!(!b.contains("take_groups_snapshot("), "без снимка всех групп");
        assert!(b.contains("isolated_speedtest_snapshot("), "снимок только изолированной группы");
    }

    #[test]
    fn regression_bug_86_speedtest_measures_selected_node() {
        // Баг 86: замер шёл через mixed-port по правилам (мог уйти в DIRECT/Flow), группы PROXY/GLOBAL/Proxy/auto захардкожены.
        let b = body(include_str!("speedtest.rs"), "pub async fn run_speedtest(");
        assert!(!b.contains("mihomo_proxy_url()"), "не через общий mixed-port");
        assert!(b.contains("CANARY_PROBE_PORT"), "через слушатель изолированной группы выбранного узла");
        assert!(!b.contains("\"auto\""), "без захардкоженного списка групп");
    }
}
