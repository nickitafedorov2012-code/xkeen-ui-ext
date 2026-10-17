use std::path::Path;
use tokio::time::{sleep, Duration};

use crate::{config, log_i, log_w, mihomo, override_sync, rci, routing, AppState};

pub static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub static SERVICE_MANUALLY_STOPPED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn shutdown() {
    SHUTDOWN.store(true, std::sync::atomic::Ordering::Release);
}

pub fn is_shutdown() -> bool {
    SHUTDOWN.load(std::sync::atomic::Ordering::Acquire)
}

pub fn set_service_stopped(stopped: bool) {
    SERVICE_MANUALLY_STOPPED.store(stopped, std::sync::atomic::Ordering::Release);
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

            let (config_path_str, force_domains, direct_domains, device_routing, ignore_servers, device_domains, adblock_enabled, flow_server, zapret_cfg, gaming_cfg, schedules) = {
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

                    let missing_device = needs_device && !content.contains("AUTO-DEVICE");
                    let missing_dev_domains = needs_dev_domains && !content.contains("AUTO-DEVICE-DOMAINS");
                    let missing_force = needs_force && !content.contains("AUTO-FORCE");
                    let missing_ignore = needs_ignore && !content.contains("exclude-filter:");
                    let missing_adblock = adblock_enabled && !content.contains("AUTO-ADBLOCK");
                    let missing_flow = needs_flow && !content.contains("AUTO-GOOGLE-AI");
                    let missing_zapret = needs_zapret && !content.contains("AUTO-ZAPRET-HYBRID");
                    let missing_gaming = needs_gaming && !content.contains("AUTO-GAMING");

                    if missing_device || missing_dev_domains || missing_force || missing_ignore || missing_adblock || missing_flow || missing_zapret || missing_gaming {
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
                    if mihomo::switch_group(&state.http, &cfg, &entry.group_name, &entry.original_node).await.is_ok() {
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
                            mihomo::switch_group(&state.http, &cfg, &group_name, target_node).await.is_ok()
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
                    if mihomo::switch_group(&state.http, &cfg, &entry.group_name, &entry.original_node).await.is_ok() {
                        active_schedules.remove(&key);
                    }
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
                    cfg.gaming.mode == config::GamingMode::Compatibility || cfg.gaming.mode == config::GamingMode::BypassRu,
                    !cfg.gaming.devices.is_empty(),
                )
            };

            if gaming_enabled && is_compat && has_devices {
                let cfg = state.config.read().await.clone();
                let policies = rci::get_policies(&state.http, &cfg).await.unwrap_or_default();
                if let Ok(current_devs) = rci::get_devices(&state.http, &cfg, &policies, "").await {
                    let _cfg_guard = state.config_lock.lock().await;
                    let _routing_guard = state.routing_lock.lock().await;
                    let mut new_cfg = (**state.config.read().await).clone();

                    let mut ip_changed = false;
                    let has_explicit_enabled = new_cfg.gaming.devices.iter().any(|d| d.enabled);
                    let devices_len = new_cfg.gaming.devices.len();
                    for dev in &mut new_cfg.gaming.devices {
                        let is_active = if has_explicit_enabled {
                            dev.enabled
                        } else {
                            devices_len == 1
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
                        let path = Path::new(&new_cfg.mihomo.config_path);
                        if path.exists() {
                            if let Ok(raw_yaml) = tokio::fs::read_to_string(path).await {
                                if let Ok(new_yaml) = routing::apply_gaming_rules(&raw_yaml, &new_cfg.gaming, &[]) {
                                    if crate::api::atomic_write_file(path, &new_yaml).await.is_ok()
                                        && mihomo::reload_config(&state.http, &new_cfg).await.is_ok() {
                                            log_i!("[DHCP-WATCHDOG] ✓ Маршрут игрового режима успешно переприменен для нового IP");
                                        }
                                    let redir_port = routing::parse_redir_port(&new_yaml).unwrap_or(5000);
                                    let tproxy_port = routing::parse_tproxy_port(&new_yaml).unwrap_or(5001);
                                    let dns_port = crate::api::resolve_active_dns_port(&new_cfg);
                                    let _ = crate::api::sync_gaming_full_tunnel(&new_cfg.gaming, redir_port, tproxy_port, dns_port).await;
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
            let Ok(_cfg_guard) = state.config_lock.try_lock() else {
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
                    .arg("(iptables -t mangle -S POSTROUTING 2>/dev/null | grep -q zapret || iptables -t mangle -S PREROUTING 2>/dev/null | grep -q zapret) && iptables -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE")
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

                // NET-01: Контроль здоровья DNS-редиректа на порт 1053 во избежание блэкаута LAN
                let dns_redirect_active = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("iptables -t nat -S PREROUTING 2>/dev/null | grep -q 'to-ports 1053'")
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
                    let mut cfg = (**state.config.read().await).clone();
                    cfg.zapret.enabled = false;
                    let _ = config::save(&state.config_path, &cfg).await;
                    *state.config.write().await = std::sync::Arc::new(cfg);
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
            if is_running_protected && !enabled {
                enabled = true;
                if let Ok(mut tx) = crate::transaction::ConfigTx::begin(&state).await {
                    tx.config_mut().adguard.enabled = true;
                    tx.config_mut().adblock_enabled = true;
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

            let iptables_active = crate::adguard::is_adguard_iptables_active(dns_port);

            if !enabled && !is_running_protected {
                if iptables_active {
                    let _ = crate::adguard::remove_adguard_iptables();
                }
                iptables_temporarily_suspended = false;
                continue;
            }

            if iptables_active || iptables_temporarily_suspended {
                let listener_ok = crate::adguard::test_agh_dns_listener(&host, dns_port).await;

                if !listener_ok && iptables_active {
                    // AdGuard Home упал или завис при активных правилах перенаправления!
                    log_w!(
                        "[WATCHDOG] ⚠️ AdGuard Home DNS-listener на {}:{} не отвечает! Временное отключение XKEEN_ADGUARD iptables для защиты от DNS-блэкаута...",
                        host,
                        dns_port
                    );
                    let _ = crate::adguard::remove_adguard_iptables();
                    iptables_temporarily_suspended = true;
                } else if listener_ok && iptables_temporarily_suspended {
                    // AdGuard Home восстановился
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
                let listener_ok = crate::adguard::test_agh_dns_listener(&host, dns_port).await;
                if listener_ok {
                    let _ = crate::adguard::apply_adguard_iptables(dns_port);
                    log_i!("[WATCHDOG] ✓ Восстановлен системный редирект XKEEN_ADGUARD iptables (порт 53 -> {})", dns_port);
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
    // Для UDP соединений с игрового устройства: высокие игровые порты (не DNS/QUIC/NTP/SSDP/mDNS и не STUN/WebRTC/VoIP)
    if network.eq_ignore_ascii_case("udp") && !dest_ip.is_empty() {
        if let Ok(port_num) = dest_port.parse::<u16>() {
            if !matches!(port_num, 53 | 80 | 123 | 443 | 853 | 1900 | 3478..=3481 | 5060 | 5061 | 5349 | 5353 | 5354 | 19302..=19309) && port_num >= 1024 {
                return true;
            }
        }
    }
    false
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

            let has_explicit_enabled = cfg.gaming.devices.iter().any(|d| d.enabled);
            let active_dev_ip = cfg
                .gaming
                .devices
                .iter()
                .find(|d| if has_explicit_enabled { d.enabled } else { cfg.gaming.devices.len() == 1 })
                .map(|d| d.ip.trim().to_string())
                .unwrap_or_default();

            let Ok(val) = mihomo::m_get(&state.http, &cfg, "/connections").await else {
                continue;
            };

            let mut found_non_ignored = false;
            let mut trigger_target = String::new();
            let now_str = chrono::Local::now().format("%H:%M:%S").to_string();

            if let Some(conns) = val.get("connections").and_then(|c| c.as_array()) {
                for c in conns {
                    let Some(meta) = c.get("metadata") else { continue };
                    let src_ip = meta.get("sourceIP").and_then(|s| s.as_str()).unwrap_or("");
                    if !active_dev_ip.is_empty() && src_ip != active_dev_ip {
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
                    let rule = c.get("rule").and_then(|r| r.as_str()).unwrap_or("");

                    if is_gaming_connection_match(host, dest_ip, &dest_port, net, rule, &gaming_domains) {
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
                if !cfg.gaming.enabled || (cfg.gaming.mode != config::GamingMode::Compatibility && cfg.gaming.mode != config::GamingMode::BypassRu) {
                    // Включаем игровой режим только если выбрано игровое устройство с IP
                    if !active_dev_ip.is_empty() {
                        log_i!(
                            "[SMART-GAMING] 🎮 Обнаружено игровое подключение ({}) на {} — автоматическое включение Режима совместимости!",
                            trigger_target,
                            active_dev_ip
                        );
                        let _cfg_guard = state.config_lock.lock().await;
                        let _routing_guard = state.routing_lock.lock().await;
                        let mut new_cfg = (**state.config.read().await).clone();
                        new_cfg.gaming.enabled = true;
                        if new_cfg.gaming.mode != config::GamingMode::BypassRu {
                            new_cfg.gaming.mode = config::GamingMode::Compatibility;
                        }
                        let path = Path::new(&new_cfg.mihomo.config_path);
                        if path.exists() {
                            if let Ok(raw_yaml) = tokio::fs::read_to_string(path).await {
                                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &new_cfg) {
                                    let _ = crate::api::atomic_write_file(path, &new_yaml).await;
                                    let _ = mihomo::reload_config(&state.http, &new_cfg).await;
                                    let target_srv = if new_cfg.gaming.target_server.trim().is_empty() {
                                        "Fastest"
                                    } else {
                                        new_cfg.gaming.target_server.trim()
                                    };
                                    let _ = mihomo::switch_group(&state.http, &new_cfg, routing::GAMING_GROUP_NAME, target_srv).await;
                                    let redir_port = routing::parse_redir_port(&new_yaml).unwrap_or(5000);
                                    let tproxy_port = routing::parse_tproxy_port(&new_yaml).unwrap_or(5001);
                                    let dns_port = crate::api::resolve_active_dns_port(&new_cfg);
                                    let _ = crate::api::sync_gaming_full_tunnel(&new_cfg.gaming, redir_port, tproxy_port, dns_port).await;
                                }
                            }
                        }
                        if config::save(&state.config_path, &new_cfg).await.is_ok() {
                            *state.config.write().await = std::sync::Arc::new(new_cfg);
                        }
                    }
                }
            } else if cfg.gaming.enabled {
                let idle_mins = cfg.gaming.smart_idle_timeout_mins.max(1) as u64;
                let idle_dur = Duration::from_secs(idle_mins * 60);
                let ref_instant = *last_active_instant.get_or_insert_with(std::time::Instant::now);
                if ref_instant.elapsed() >= idle_dur {
                    log_i!(
                        "[SMART-GAMING] 💤 Нет активных игровых подключений более {} мин — автоматическое отключение Режима совместимости.",
                        idle_mins
                    );
                    last_active_instant = None;
                    let _cfg_guard = state.config_lock.lock().await;
                    let _routing_guard = state.routing_lock.lock().await;
                    let mut new_cfg = (**state.config.read().await).clone();
                    new_cfg.gaming.enabled = false;
                    let path = Path::new(&new_cfg.mihomo.config_path);
                    if path.exists() {
                        if let Ok(raw_yaml) = tokio::fs::read_to_string(path).await {
                            if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &new_cfg) {
                                let _ = crate::api::atomic_write_file(path, &new_yaml).await;
                                let _ = mihomo::reload_config(&state.http, &new_cfg).await;
                                let redir_port = routing::parse_redir_port(&new_yaml).unwrap_or(5000);
                                let tproxy_port = routing::parse_tproxy_port(&new_yaml).unwrap_or(5001);
                                let dns_port = crate::api::resolve_active_dns_port(&new_cfg);
                                let _ = crate::api::sync_gaming_full_tunnel(&new_cfg.gaming, redir_port, tproxy_port, dns_port).await;
                            }
                        }
                    }
                    if config::save(&state.config_path, &new_cfg).await.is_ok() {
                        *state.config.write().await = std::sync::Arc::new(new_cfg);
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
}

