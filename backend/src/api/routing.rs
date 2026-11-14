use axum::extract::{ConnectInfo, State};
use axum::response::{Json, Response};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{api_err, api_err_bad_request, api_ok, atomic_write_file, resolve_active_dns_port, sync_whitelist_mode_tunnel};
use crate::{config, failover, log_i, log_w, mihomo, rci, routing, transaction::ConfigTx, AppState};
/// GET /api/devices — устройства с политиками.
pub async fn get_devices(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let policies = match rci::get_policies(&state.http, &cfg).await {
        Ok(p) => p,
        Err(e) => return api_err(e),
    };
    let client_ip = peer.ip().to_string();
    match rci::get_devices(&state.http, &cfg, &policies, &client_ip).await {
        Ok(devices) => {
            let live = mihomo::live_device_servers(&state.http, &cfg).await;
            let arr: Vec<serde_json::Value> = devices
                .iter()
                .map(|d| {
                    let is_whitelist = cfg.whitelist.devices.iter().any(|wd| {
                        wd.enabled && (wd.mac.eq_ignore_ascii_case(&d.mac) || (!wd.ip.is_empty() && wd.ip == d.ip))
                    });
                    json!({
                        "mac": d.mac, "name": d.name, "ip": d.ip,
                        "ipv6": d.ipv6,
                        "policy": d.policy, "policy_name": d.policy_name,
                        "online": d.online, "interface": d.interface,
                        "is_current_device": d.is_current_device,
                        "rxbytes": d.rxbytes, "txbytes": d.txbytes,
                        "speed_limit_kbps": d.speed_limit_kbps,
                        "current_server": live.get(&d.ip).cloned().unwrap_or_default(),
                        "is_whitelist": is_whitelist,
                    })
                })
                .collect();
            api_ok(json!({ "devices": arr }))
        }
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct PolicyReq {
    pub macs: Vec<String>,
    pub policy_id: String,
}

pub fn finish_rci_apply(
    ok: usize,
    errors: Vec<String>,
    save_result: Option<Result<(), String>>,
) -> Result<serde_json::Value, String> {
    if ok == 0 {
        return Err(errors.join("; "));
    }
    if let Some(Err(e)) = save_result {
        return Err(format!(
            "Настройки применены ({ok}), но не удалось сохранить конфигурацию роутера: {e}"
        ));
    }
    Ok(json!({ "applied": ok, "saved": true, "errors": errors }))
}

/// POST /api/devices/policy — одиночно и батчем.
pub async fn set_device_policy(State(state): State<AppState>, Json(req): Json<PolicyReq>) -> Response {
    let cfg = state.config.read().await.clone();
    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for mac in &req.macs {
        match rci::set_device_policy(&state.http, &cfg, mac, &req.policy_id, false).await {
            Ok(_) => ok += 1,
            Err(e) => errors.push(format!("{mac}: {e}")),
        }
    }
    let save_res = if ok > 0 {
        Some(rci::save_config(&state.http, &cfg).await)
    } else {
        None
    };
    match finish_rci_apply(ok, errors, save_res) {
        Ok(val) => api_ok(val),
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct SpeedReq {
    pub macs: Vec<String>,
    pub kbps: u64,
}

/// POST /api/devices/speed — одиночно и батчем (0 = снять ограничение).
pub async fn set_device_speed(State(state): State<AppState>, Json(req): Json<SpeedReq>) -> Response {
    let cfg = state.config.read().await.clone();
    let mut ok = 0usize;
    let mut errors: Vec<String> = Vec::new();
    for mac in &req.macs {
        match rci::set_device_speed(&state.http, &cfg, mac, req.kbps, false).await {
            Ok(_) => ok += 1,
            Err(e) => errors.push(format!("{mac}: {e}")),
        }
    }
    let save_res = if ok > 0 {
        Some(rci::save_config(&state.http, &cfg).await)
    } else {
        None
    };
    match finish_rci_apply(ok, errors, save_res) {
        Ok(val) => api_ok(val),
        Err(e) => api_err(e),
    }
}

/// GET /api/ignore — текущий игнор-лист (exclude-filter для Fastest/Fallback).
pub async fn get_ignore(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    api_ok(json!({ "servers": cfg.ignore_servers }))
}

#[derive(Deserialize)]
pub struct IgnoreReq {
    pub servers: Vec<String>,
}

/// POST /api/ignore — сохранить игнор-лист, применить exclude-filter к config.yaml, reload Mihomo.
pub async fn set_ignore(State(state): State<AppState>, Json(req): Json<IgnoreReq>) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка начала транзакции: {e}")),
    };

    let mut servers: Vec<String> = req
        .servers
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    servers.sort();
    servers.dedup();
    // Если в игнор-лист попало mojibake-имя — добавляем починенный вариант,
    // чтобы подстрока совпала с реальным именем сервера у провайдера.
    let repaired: Vec<String> = servers.iter().map(|s| mihomo::fix_mojibake_smart(s)).collect();
    servers.extend(repaired);
    servers.sort();
    servers.dedup();

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    let new_yaml = match routing::apply_ignore_to_groups(&yaml, &servers) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let mut current_cfg = tx.config().clone();
    current_cfg.ignore_servers = servers.clone();
    let mut saved = current_cfg.provider_filters.clone();
    let new_yaml = routing::apply_ignore_to_providers(&new_yaml, &servers, &mut saved);
    current_cfg.provider_filters = saved;

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }
    tx.stage_json(current_cfg);

    let reload_res = tx.commit_and_reload().await;
    let cfg = state.config.read().await.clone();
    let updated = mihomo::force_update_all_providers(&state.http, &cfg).await;

    if let Err(err) = reload_res {
        return api_ok(json!({
            "applied": servers.len(),
            "providers_updated": updated,
            "warning": format!("Игнор-лист сохранён, но reload Mihomo вернул ошибку: {err}")
        }));
    }
    api_ok(json!({ "applied": servers.len(), "providers_updated": updated }))
}

/// POST /api/servers/fix-names — ремонт mojibake-имён статических прокси в config.yaml
/// (глобальная замена битого имени на починенное затрагивает и группы, и правила), reload.
pub async fn fix_names(State(state): State<AppState>) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка начала транзакции: {e}")),
    };
    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    let mut new_yaml = yaml.clone();
    let mut fixed: Vec<String> = Vec::new();
    for name in routing::parse_static_proxy_names(&yaml) {
        let repaired = mihomo::fix_mojibake_smart(&name);
        if repaired != name {
            let patterns = [
                format!("name: \"{}\"", name),
                format!("name: '{}'", name),
                format!("name: {}", name),
                format!("- \"{}\"", name),
                format!("- '{}'", name),
                format!("- {}", name),
            ];
            let mut did_fix = false;
            for p in patterns {
                if new_yaml.contains(&p) {
                    let rep = p.replace(&name, &repaired);
                    new_yaml = new_yaml.replace(&p, &rep);
                    did_fix = true;
                }
            }
            if did_fix {
                fixed.push(format!("{name} → {repaired}"));
            }
        }
    }
    if fixed.is_empty() {
        return api_ok(json!({ "fixed": 0, "names": fixed }));
    }
    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }
    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Имена исправлены, но reload Mihomo не удался: {e}"));
    }
    api_ok(json!({ "fixed": fixed.len(), "names": fixed }))
}

/// GET /api/device-routing — per-device цепочки серверов (основной + резервы).
pub async fn get_device_routing(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let map: serde_json::Map<String, serde_json::Value> = cfg
        .device_routing
        .iter()
        .map(|(ip, dr)| {
            (
                ip.clone(),
                json!({
                    "servers": dr.servers,
                    "ping_threshold_ms": dr.ping_threshold_ms,
                    "auto_restore": dr.auto_restore,
                }),
            )
        })
        .collect();
    api_ok(json!({
        "routing": map,
        "device_failover_enabled": cfg.failover.device_failover_enabled,
        "global_threshold_ms": cfg.failover.ping_threshold_ms,
    }))
}

#[derive(Deserialize)]
pub struct DeviceRoutingReq {
    pub ip: String,
    #[serde(default)]
    pub name: String,
    /// Пусто = снять маршрутизацию устройства.
    #[serde(default)]
    pub servers: Vec<String>,
    #[serde(default)]
    pub ping_threshold_ms: u32,
    #[serde(default = "default_true")]
    pub auto_restore: bool,
    #[serde(default = "default_true")]
    pub follow_whitelist: bool,
}

fn default_true() -> bool {
    true
}

/// POST /api/device-routing — сохранить цепочку устройства, применить AUTO-DEVICE
/// назначение (основной сервер), reload Mihomo. Пустой servers = снять.
pub async fn set_device_routing(State(state): State<AppState>, Json(req): Json<DeviceRoutingReq>) -> Response {
    let ip = req.ip.trim().to_string();
    if ip.is_empty() {
        return api_err("Пустой IP устройства");
    }
    if ip.parse::<std::net::IpAddr>().is_err() {
        return api_err(format!("Некорректный IP-адрес устройства: '{ip}'"));
    }
    let mut servers: Vec<String> = req
        .servers
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    servers.dedup();

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    // 1. Сохранить/удалить настройки в конфиге
    if servers.is_empty() {
        tx.config_mut().device_routing.remove(&ip);
    } else {
        tx.config_mut().device_routing.insert(
            ip.clone(),
            config::DeviceRouting {
                servers: servers.clone(),
                ping_threshold_ms: req.ping_threshold_ms,
                auto_restore: req.auto_restore,
                follow_whitelist: req.follow_whitelist,
            },
        );
    }

    // 2. Применить AUTO-DEVICE назначение (primary или снятие)
    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    let assignment = routing::Assignment {
        ip: ip.clone(),
        name: req.name.clone(),
        server: servers.first().cloned(),
    };
    let providers = device_providers_for(tx.config(), &yaml);
    let pinned = routing::pinned_whitelist_ips(tx.config());
    let new_yaml = match routing::apply_assignments(&yaml, &[assignment], &providers, &pinned) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }

    // Атомарное сохранение и перезагрузка Mihomo с двухфазным откатом при сбое
    if let Err(e) = tx.commit_and_reload().await {
        return api_err(e);
    }

    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    // 3. Перевыбор основного сервера в новой группе
    let mut reselected = false;
    if let Some(primary) = servers.first() {
        let gname = routing::group_name_for(&ip, &req.name);
        if mihomo::switch_group_scoped(&state.http, tx.config(), &gname, primary).await.is_ok() {
            reselected = true;
        }
    }

    let active_id = mihomo::get_active_server(&state.http, tx.config()).await.ok().flatten().map(|s| s.id).unwrap_or_default();
    if !active_id.is_empty() {
        let _ = sync_whitelist_mode_tunnel(&state, &active_id).await;
    }

    api_ok(json!({
        "applied": !servers.is_empty(),
        "servers": servers,
        "reselected": reselected,
    }))
}

/// Провайдеры для use: групп устройств: из настроек, иначе все proxy-providers
/// из config.yaml (универсальность — на другом железе имена свои).
fn device_providers_for(cfg: &config::AppConfig, yaml: &str) -> Vec<String> {
    if cfg.mihomo.device_providers.is_empty() {
        routing::parse_provider_names(yaml)
    } else {
        cfg.mihomo.device_providers.clone()
    }
}


/// GET /api/routing — текущие AUTO-DEVICE назначения + live-серверы.
pub async fn get_routing(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Не удалось прочитать {}: {e}", cfg.mihomo.config_path)),
    };
    let groups = routing::parse_groups(&yaml);
    let live = mihomo::live_device_servers(&state.http, &cfg).await;
    let mut assignments: Vec<serde_json::Value> = groups
        .iter()
        .map(|(ip, gname)| {
            json!({
                "ip": ip,
                "group": gname,
                "current_server": live.get(ip).cloned().unwrap_or_default(),
            })
        })
        .collect();

    // Синхронизация серверов для устройств в режиме «Игры» (100% proxy / Compatibility Mode):
    // Если для игрового устройства задан конкретный сервер (dev.server) или активен целевой сервер (target_server),
    // в выпадающем списке на вкладке «Устройства» отображается именно он.
    if cfg.gaming.enabled && cfg.gaming.mode == config::GamingMode::Compatibility {
        for dev in &cfg.gaming.devices {
            if !config::is_gaming_device_active(dev, true) || dev.ip.trim().is_empty() {
                continue;
            }
            let dev_ip = dev.ip.trim();
            let effective_server = dev.server.as_ref()
                .filter(|s| !s.trim().is_empty() && s.trim() != "default")
                .cloned()
                .unwrap_or_else(|| {
                    if !cfg.gaming.target_server.trim().is_empty() {
                        cfg.gaming.target_server.clone()
                    } else {
                        "Fastest".to_string()
                    }
                });

            if let Some(existing) = assignments.iter_mut().find(|a| a.get("ip").and_then(|v| v.as_str()) == Some(dev_ip)) {
                if let Some(obj) = existing.as_object_mut() {
                    obj.insert("current_server".to_string(), json!(effective_server));
                    obj.insert("is_gaming".to_string(), json!(true));
                }
            } else {
                assignments.push(json!({
                    "ip": dev_ip,
                    "group": routing::GAMING_GROUP_NAME,
                    "current_server": effective_server,
                    "is_gaming": true,
                }));
            }
        }
    }

    api_ok(json!({ "assignments": assignments, "config_path": cfg.mihomo.config_path }))
}

#[derive(Deserialize)]
pub struct RoutingAssignment {
    pub ip: String,
    #[serde(default)]
    pub name: String,
    /// null / "" / "default" — снять назначение.
    pub server: Option<String>,
}

#[derive(Deserialize)]
pub struct RoutingReq {
    pub assignments: Vec<RoutingAssignment>,
}

/// Перенос пакетных назначений в device_routing (баг 64): сервер становится первым в цепочке устройства,
/// остальные сохраняются; пустой/«default» снимает назначение.
pub fn merge_assignments_into_device_routing(
    map: &mut std::collections::BTreeMap<String, config::DeviceRouting>,
    assignments: &[routing::Assignment],
) {
    for a in assignments {
        let ip = a.ip.trim().to_string();
        let srv = a.server.as_deref().map(str::trim).unwrap_or("");
        if srv.is_empty() || srv == "default" {
            map.remove(&ip);
            continue;
        }
        let entry = map.entry(ip).or_default();
        entry.servers.retain(|s| s != srv);
        entry.servers.insert(0, srv.to_string());
    }
}

/// POST /api/routing — применить назначения (merge), reload Mihomo, перевыбор серверов.
pub async fn apply_routing(State(state): State<AppState>, Json(req): Json<RoutingReq>) -> Response {
    // Валидация всех IP устройств в назначениях (RT-02)
    for a in &req.assignments {
        let clean_ip = a.ip.trim();
        if !crate::config::is_valid_ip_or_cidr(clean_ip) {
            return api_err(format!("Некорректный IP-адрес или CIDR устройства: '{clean_ip}'"));
        }
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(e),
    };

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Не удалось прочитать config.yaml: {e}")),
    };
    let assignments: Vec<routing::Assignment> = req
        .assignments
        .into_iter()
        .map(|a| routing::Assignment { ip: a.ip, name: a.name, server: a.server })
        .collect();

    // Баг 64: назначения сохраняются в device_routing — переживают рестарт Mihomo и видны в UI
    merge_assignments_into_device_routing(&mut tx.config_mut().device_routing, &assignments);

    // Синхронизация выбора сервера для игровых устройств в режиме 100% Compatibility:
    let mut updated_gaming = false;
    if tx.config().gaming.enabled && tx.config().gaming.mode == config::GamingMode::Compatibility {
        for a in &assignments {
            let a_ip = a.ip.trim();
            for dev in &mut tx.config_mut().gaming.devices {
                if dev.ip == a_ip {
                    let s = a.server.as_deref().unwrap_or("").trim();
                    let new_val = if s.is_empty() || s == "default" {
                        None
                    } else {
                        Some(s.to_string())
                    };
                    if dev.server != new_val {
                        dev.server = new_val;
                        updated_gaming = true;
                    }
                }
            }
        }
    }

    let mut new_yaml = match routing::apply_assignments(&yaml, &assignments, &device_providers_for(tx.config(), &yaml), &routing::pinned_whitelist_ips(tx.config())) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    if updated_gaming {
        let providers = device_providers_for(tx.config(), &new_yaml);
        if let Ok(g_yaml) = routing::apply_gaming_rules(&new_yaml, &tx.config().gaming, &providers) {
            new_yaml = g_yaml;
        }
    }

    if let Err(e) = tx.set_yaml(new_yaml.clone()) {
        return api_err(e);
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка применения конфигурации маршрутов: {e}"));
    }

    if updated_gaming {
        let ports = routing::redir_ports(&new_yaml);
        let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
        let cfg_guard = state.config.read().await;
        let dns_port = resolve_active_dns_port(&cfg_guard);
        let _ = sync_full_tunnel_modes(&cfg_guard, redir_port, tproxy_port, dns_port).await;
    }

    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let cfg = state.config.read().await.clone();
    let mut reselected = 0usize;
    for a in &assignments {
        if let Some(server) = &a.server {
            if server.trim().is_empty() || server.trim() == "default" {
                continue;
            }
            let gname = routing::group_name_for(&a.ip, &a.name);
            // Без полного сброса соединений всей LAN на каждое устройство (INV-7)
            if mihomo::switch_selector(&state.http, &cfg, &gname, server).await.is_ok() {
                reselected += 1;
            }
        }
    }
    api_ok(json!({ "applied": assignments.len(), "reselected": reselected }))
}


// ==================== МОНИТОРИНГ ТРАФИКА УСТРОЙСТВ =============

/// GET /api/devices/traffic — активные соединения и трафик per-device из Mihomo /connections
pub async fn get_devices_traffic(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let url = format!("{}/connections", cfg.mihomo_url());
    let mut req = state.http.get(&url);
    if !cfg.mihomo.secret.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
    }

    let resp_val = match req.timeout(std::time::Duration::from_secs(3)).send().await {
        Ok(res) => res.json::<serde_json::Value>().await.unwrap_or_default(),
        Err(e) => return api_err(format!("Ошибка опроса /connections: {}", e)),
    };

    // В Mihomo 'upload' от прокси к клиенту соответствует клиентской Загрузке (download),
    // а 'download' от клиента в прокси — клиентской Отдаче (upload).
    let download_total = resp_val.get("uploadTotal").and_then(|v| v.as_u64()).unwrap_or(0);
    let upload_total = resp_val.get("downloadTotal").and_then(|v| v.as_u64()).unwrap_or(0);

    let mut per_device: std::collections::BTreeMap<String, serde_json::Value> = std::collections::BTreeMap::new();

    if let Some(connections) = resp_val.get("connections").and_then(|c| c.as_array()) {
        for conn in connections {
            let src_ip = conn.get("metadata")
                .and_then(|m| m.get("sourceIP"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();

            if src_ip.is_empty() {
                continue;
            }

            let dl = conn.get("upload").and_then(|v| v.as_u64()).unwrap_or(0);
            let ul = conn.get("download").and_then(|v| v.as_u64()).unwrap_or(0);
            let host = conn.get("metadata")
                .and_then(|m| m.get("host"))
                .and_then(|h| h.as_str())
                .unwrap_or("")
                .to_string();
            let chain = conn.get("chains")
                .and_then(|c| c.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let entry = per_device.entry(src_ip.clone()).or_insert_with(|| {
                json!({
                    "ip": src_ip,
                    "download_bytes": 0,
                    "upload_bytes": 0,
                    "active_connections": 0,
                    "active_server": "",
                    "recent_hosts": Vec::<String>::new()
                })
            });

            if let Some(obj) = entry.as_object_mut() {
                if let Some(d) = obj.get_mut("download_bytes").and_then(|v| v.as_u64()) {
                    obj.insert("download_bytes".into(), json!(d + dl));
                }
                if let Some(u) = obj.get_mut("upload_bytes").and_then(|v| v.as_u64()) {
                    obj.insert("upload_bytes".into(), json!(u + ul));
                }
                if let Some(c) = obj.get_mut("active_connections").and_then(|v| v.as_u64()) {
                    obj.insert("active_connections".into(), json!(c + 1));
                }
                if !chain.is_empty() {
                    obj.insert("active_server".into(), json!(chain));
                }
                if !host.is_empty() {
                    if let Some(hosts_arr) = obj.get_mut("recent_hosts").and_then(|v| v.as_array_mut()) {
                        if !hosts_arr.iter().any(|h| h.as_str() == Some(&host)) && hosts_arr.len() < 10 {
                            hosts_arr.push(json!(host));
                        }
                    }
                }
            }
        }
    }

    api_ok(json!({
        "download_total": download_total,
        "upload_total": upload_total,
        "devices": per_device
    }))
}


// ==================== KEENETIC POLICIES MAP =============

/// GET /api/policies/map — граф связей: Устройства -> Политики Keenetic / XKeen -> Выходные серверы
pub async fn get_policies_map(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();

    let policies_res = rci::get_policies(&state.http, &cfg).await.unwrap_or_default();
    let devices_res = rci::get_devices(&state.http, &cfg, &policies_res, "").await.unwrap_or_default();
    let servers_res = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await.unwrap_or_default();

    let mut nodes_devices = Vec::new();
    for d in devices_res {
        let assigned_srv = cfg.device_routing.get(&d.ip)
            .and_then(|dr| dr.servers.first())
            .cloned()
            .unwrap_or_default();

        nodes_devices.push(json!({
            "ip": d.ip,
            "mac": d.mac,
            "name": d.name,
            "policy_id": d.policy,
            "xkeen_server": assigned_srv,
            "active": d.online,
        }));
    }

    let mut nodes_policies = Vec::new();
    for p in policies_res {
        nodes_policies.push(json!({
            "id": p.id,
            "name": p.name,
            "is_default": p.is_default,
        }));
    }

    let nodes_servers: Vec<serde_json::Value> = servers_res.iter().map(|s| {
        json!({
            "id": s.id,
            "name": s.name,
            "is_active": s.is_active,
            "ping_ms": s.ping_ms,
            "protocol": s.protocol,
        })
    }).collect();

    api_ok(json!({
        "devices": nodes_devices,
        "policies": nodes_policies,
        "servers": nodes_servers,
    }))
}


pub fn calc_schedules_revision(schedules: &[config::DeviceSchedule]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for s in schedules {
        s.id.hash(&mut hasher);
        s.time_start.hash(&mut hasher);
        s.time_end.hash(&mut hasher);
        s.days.hash(&mut hasher);
        s.action.hash(&mut hasher);
        s.target_server.hash(&mut hasher);
        s.ip.hash(&mut hasher);
        s.enabled.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// GET /api/schedules — список расписаний
pub async fn get_schedules(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    let rev = calc_schedules_revision(&cfg.schedules);
    api_ok(json!({ "schedules": cfg.schedules, "revision": rev }))
}

#[derive(Deserialize)]
pub struct SaveSchedulesReq {
    pub schedules: Vec<config::DeviceSchedule>,
    pub expected_revision: Option<String>,
}

/// POST /api/schedules — сохранение расписаний
pub async fn save_schedules(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<SaveSchedulesReq>,
) -> Response {
    let _guard = state.config_lock.lock().await;
    let _rguard = state.routing_lock.lock().await;
    let mut cfg = state.config.read().await.as_ref().clone();

    // UI-03: Optimistic concurrency check
    if let Some(exp) = &body.expected_revision {
        let cur_rev = calc_schedules_revision(&cfg.schedules);
        if exp != &cur_rev {
            return api_err("Конфликт параллельного сохранения расписаний: данные были изменены другим клиентом");
        }
    }

    cfg.schedules = body.schedules;
    let new_rev = calc_schedules_revision(&cfg.schedules);

    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения расписаний: {}", e));
    }
    if std::path::Path::new(&cfg.mihomo.config_path).exists() {
        if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
            if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                let _ = mihomo::reload_config(&state.http, &cfg).await;
            }
        }
    }
    *state.config.write().await = std::sync::Arc::new(cfg);
    api_ok(json!({ "saved": true, "revision": new_rev }))
}

// ==================== ИГРОВОЙ РЕЖИМ (GAMING MODE) =============

fn resolve_group_leaf(proxies: &std::collections::BTreeMap<String, serde_json::Value>, start: &str) -> String {
    let mut cur = start.to_string();
    let mut visited = std::collections::HashSet::new();
    while let Some(sub) = proxies.get(&cur) {
        if visited.len() >= 32 || visited.contains(&cur) {
            break;
        }
        visited.insert(cur.clone());
        let typ = sub.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        if typ != "fallback" && typ != "urltest" && typ != "selector" && typ != "select" {
            break;
        }
        match sub.get("now").and_then(|n| n.as_str()) {
            Some(next) if !next.is_empty() && next != cur => cur = next.to_string(),
            _ => break,
        }
    }
    cur
}

/// Проверка существования целевого узла в объединении статических серверов/групп и proxy-providers.
pub fn target_exists(
    proxies: &std::collections::BTreeMap<String, serde_json::Value>,
    providers: &std::collections::BTreeMap<String, serde_json::Value>,
    name: &str,
) -> bool {
    if name.is_empty() {
        return false;
    }
    proxies.contains_key(name) || providers.contains_key(name)
}

/// Определение доступности и задержки туннеля для игрового режима.
/// Возвращает (reachable, latency_ms).
pub fn tunnel_reachability(
    proxies: &std::collections::BTreeMap<String, serde_json::Value>,
    providers: &std::collections::BTreeMap<String, serde_json::Value>,
    node: &str,
    delay: i64,
) -> (bool, Option<i64>) {
    if node.is_empty() {
        return (false, None);
    }
    if node == "DIRECT" {
        return (true, Some(0));
    }

    let in_proxies = proxies.contains_key(node);
    let provider_entry = providers.get(node);
    let in_providers = provider_entry.is_some();

    // Недоступным считать только если узла нет ни в /proxies, ни в провайдерах
    if !in_proxies && !in_providers {
        return (false, None);
    }

    // Либо провайдер явно сообщил alive == false
    if let Some(p) = provider_entry {
        if p.get("alive").and_then(|a| a.as_bool()) == Some(false) {
            return (false, None);
        }
    }
    if let Some(p) = proxies.get(node) {
        if p.get("alive").and_then(|a| a.as_bool()) == Some(false) {
            return (false, None);
        }
    }

    // Если ping_server_url вернул > 0, считать доступным с этой задержкой
    if delay > 0 {
        (true, Some(delay))
    } else {
        // Неизвестная задержка не означает недоступность (узел есть в провайдерах/прокси и alive != false)
        (true, None)
    }
}

async fn check_gaming_interception(cfg: &config::AppConfig) -> (bool, bool, bool) {
    #[cfg(target_os = "linux")]
    {
        let yaml_opt = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.ok();
        let tun_mode = yaml_opt.as_ref().map_or(false, |y| {
            (y.contains("tun:") && y.contains("enable: true")) || y.contains("auto-route: true")
        });

        // 1. Проверка перехвата TCP (PREROUTING nat -> xkeen или REDIRECT / TPROXY)
        let tcp_check = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("iptables -w 2 -t nat -C PREROUTING -j xkeen 2>/dev/null || iptables -w 2 -t nat -L xkeen -n 2>/dev/null || iptables -w 2 -t nat -L PREROUTING -n 2>/dev/null | grep -qE 'REDIRECT|TPROXY'")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        // 2. Проверка перехвата UDP (PREROUTING mangle -> xkeen или TPROXY / NFQUEUE)
        let udp_check = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("iptables -w 2 -t mangle -C PREROUTING -j xkeen 2>/dev/null || iptables -w 2 -t mangle -L xkeen -n 2>/dev/null || iptables -w 2 -t mangle -L PREROUTING -n 2>/dev/null | grep -qE 'TPROXY|NFQUEUE'")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        // 3. Честная проверка перехвата IPv6 (TPROXY/REDIRECT/xkeen в ip6tables mangle/nat)
        let ip6_iptables_check = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("ip6tables -w 2 -t mangle -C PREROUTING -j xkeen 2>/dev/null || ip6tables -w 2 -t mangle -L xkeen -n 2>/dev/null || ip6tables -w 2 -t nat -L xkeen -n 2>/dev/null || ip6tables -w 2 -t mangle -L PREROUTING -n 2>/dev/null | grep -qE 'TPROXY|REDIRECT|xkeen'")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        let yaml_ipv6_enabled = yaml_opt.as_ref().map_or(false, |y| y.contains("ipv6: true"));

        let tcp_ok = tcp_check || tun_mode;
        let udp_ok = udp_check || tun_mode;
        let ipv6_ok = (ip6_iptables_check || (tun_mode && yaml_ipv6_enabled)) && yaml_ipv6_enabled;

        (tcp_ok, udp_ok, ipv6_ok)
    }
    #[cfg(not(target_os = "linux"))]
    {
        if let Ok(yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
            (
                yaml.contains("redir-port:") || yaml.contains("tproxy-port:") || yaml.contains("port:"),
                yaml.contains("tproxy-port:") || yaml.contains("tun:"),
                yaml.contains("ipv6: true"),
            )
        } else {
            (true, true, false)
        }
    }
}

/// Баг 5: ручное включение игрового режима при активном БС блокируется с понятным текстом.
/// Выключение всегда разрешено.
pub fn manual_gaming_enable_allowed(
    cfg: &crate::config::AppConfig,
    active_server: Option<&crate::mihomo::Server>,
    enable: bool,
) -> Result<(), String> {
    if enable && crate::failover::gaming_enable_blocked_by_bs(cfg, active_server) {
        Err(crate::failover::GAMING_BLOCKED_BY_BS.to_string())
    } else {
        Ok(())
    }
}

async fn apply_and_verify_gaming(
    tx: &mut ConfigTx,
) -> Result<(), String> {
    // Баг 5: проверяем блокировку ручного включения при активном БС
    let active = mihomo::get_active_server(&tx.state.http, tx.config()).await.ok().flatten();
    manual_gaming_enable_allowed(tx.config(), active.as_ref(), tx.config().gaming.enabled)?;

    if tx.config().gaming.enabled {
        // 1. По нажатию кнопки получаем актуальный IP устройства по MAC через Keenetic RCI
        if tx.config().gaming.mode == config::GamingMode::Compatibility || tx.config().gaming.mode == config::GamingMode::BypassRu {
            let policies = rci::get_policies(&tx.state.http, tx.config()).await.unwrap_or_default();
            if let Ok(devices) = rci::get_devices(&tx.state.http, tx.config(), &policies, "").await {
                for dev in &mut tx.config_mut().gaming.devices {
                    if !config::is_gaming_device_active(dev, true) {
                        continue;
                    }
                    if let Some(rci_dev) = devices.iter().find(|d| d.mac.eq_ignore_ascii_case(&dev.mac)) {
                        if !rci_dev.ip.is_empty() {
                            dev.ip = rci_dev.ip.clone();
                            dev.ipv6 = rci_dev.ipv6.iter().filter(|v6| {
                                let l = v6.trim().to_lowercase();
                                !l.is_empty() && !l.starts_with("fe80:") && !l.starts_with("::1")
                            }).cloned().collect();
                            dev.name = rci_dev.name.clone();
                        }
                    }
                }
            }

            let active_devices: Vec<_> = tx.config().gaming.devices.iter()
                .filter(|d| config::is_gaming_device_active(d, true))
                .collect();
            if active_devices.is_empty() {
                return Err("Не выбрано устройство для игрового режима. Выберите устройство в списке.".to_string());
            }
            for dev in active_devices {
                if dev.ip.trim().is_empty() {
                    return Err(format!("Устройство '{}' (MAC {}) не имеет назначенного IP адреса в сети роутера", dev.name, dev.mac));
                }
            }
        }

        // 2. Проверяем туннель
        let target_srv = tx.config().gaming.target_server.trim().to_string();
        if !target_srv.is_empty() && target_srv != "DIRECT" && target_srv != "Fastest" && target_srv != "PROXY" {
            let (proxies_res, providers_res) = tokio::join!(
                mihomo::get_proxies(&tx.state.http, tx.config()),
                mihomo::get_provider_proxies(&tx.state.http, tx.config())
            );
            let proxies = proxies_res.unwrap_or_default();
            let providers = providers_res.unwrap_or_default();
            if (!proxies.is_empty() || !providers.is_empty()) && !target_exists(&proxies, &providers, &target_srv) {
                return Err(format!("Игровой туннель '{}' не найден в списке серверов ядра Mihomo", target_srv));
            }
        }

        // 3. Проверяем перехват TCP/UDP
        let (tcp_ok, udp_ok, _) = check_gaming_interception(tx.config()).await;
        if !tcp_ok && !udp_ok {
            return Err("Сбой перехвата трафика: ядро Mihomo не настроено на прозрачный прокси (проверьте redir-port/tproxy-port в config.yaml)".into());
        }
    }

    // 4. Применяем конфигурацию к config.yaml через ConfigTx с атомарным двухфазным откатом
    let raw_yaml = tx.read_yaml().await?;
    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return Err(format!("Ошибка генерации правил роутинга: {e}")),
    };
    tx.set_yaml(&new_yaml)?;

    // Атомарная запись на диск и reload Mihomo (ConfigTx автоматически выполнит откат при сбое)
    tx.apply_and_reload().await?;

    // Синхронизация селектор-группы 🎮 Gaming на выбранный узел
    if tx.config().gaming.enabled {
        let target_srv = if tx.config().gaming.target_server.trim().is_empty() {
            "Fastest"
        } else {
            tx.config().gaming.target_server.trim()
        };
        let _ = mihomo::switch_gaming_group(&tx.state.http, tx.config(), target_srv).await;

        // Верификация результата в ядре Mihomo
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        match mihomo::get_proxies(&tx.state.http, tx.config()).await {
            Ok(proxies) => {
                let has_group = proxies.contains_key(routing::GAMING_GROUP_NAME) || proxies.contains_key("Gaming");
                if !has_group {
                    let _ = tx.rollback().await;
                    return Err("Селекторная группа 🎮 Gaming не была создана в ядре Mihomo. Прежние настройки возвращены.".into());
                }
                for dev in &tx.config().gaming.devices {
                    if !config::is_gaming_device_active(dev, true) || dev.ip.trim().is_empty() {
                        continue;
                    }
                    let Some(srv) = dev.server.as_deref().map(str::trim).filter(|s| {
                        !s.is_empty()
                            && *s != "default"
                            && *s != routing::GAMING_GROUP_NAME
                            && !matches!(*s, "DIRECT" | "REJECT" | "REJECT-DROP" | "PASS")
                    }) else {
                        continue;
                    };
                    let gname = routing::gaming_device_group_name(&dev.ip, &dev.name);
                    if proxies.contains_key(&gname) {
                        if let Err(e) = mihomo::switch_selector(&tx.state.http, tx.config(), &gname, srv).await {
                            log_w!(
                                "Игровое устройство '{}': выбранный узел '{}' недоступен ({e}), откат на '{}'",
                                dev.name,
                                srv,
                                routing::GAMING_GROUP_NAME
                            );
                            let _ = mihomo::switch_selector(&tx.state.http, tx.config(), &gname, routing::GAMING_GROUP_NAME).await;
                        }
                    }
                }
            }
            Err(e) => {
                let _ = tx.rollback().await;
                return Err(format!("Сбой верификации ядра Mihomo: {e}. Прежние настройки возвращены."));
            }
        }

        // Дополнительная верификация правил для режима совместимости
        if tx.config().gaming.mode == config::GamingMode::Compatibility || tx.config().gaming.mode == config::GamingMode::BypassRu {
            let active_ips: Vec<String> = tx.config().gaming.devices.iter()
                .filter(|d| config::is_gaming_device_active(d, true))
                .map(|d| d.ip.clone())
                .collect();
            for ip in active_ips {
                let expected_cidr = format!("SRC-IP-CIDR,{}/32", ip);
                if !new_yaml.contains(&expected_cidr) {
                    let _ = tx.rollback().await;
                    return Err(format!("Маршрутное правило для {} не сформировано. Прежние настройки возвращены.", ip));
                }
            }
        }
    }

    // Синхронизация сетевого фильтра ядра (iptables) для режима полной совместимости:
    // пакеты устройств в Compat-режиме направляются в REDIRECT (TCP) и TPROXY (UDP) минуя geo_exclude RETURN
    let ports = routing::redir_ports(&new_yaml);
    let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
    let dns_port = resolve_active_dns_port(tx.config());
    let _ = sync_full_tunnel_modes(tx.config(), redir_port, tproxy_port, dns_port).await;

    Ok(())
}

/// Синхронизация перехвата полного туннеля в iptables для Compatibility-режима (100% Всё в прокси)
/// Генерация shell-команд для полной изоляции/перехвата игрового режима (iptables REDIRECT, TPROXY, policy routing).
#[cfg(any(target_os = "linux", test))]
/// Баг 112: нужно ли восстанавливать исходные значения rp_filter (только когда оба режима выключены).
pub fn rp_filter_restore_needed(gaming_on: bool, bs_on: bool) -> bool {
    !gaming_on && !bs_on
}

/// Баг 112: сохраняет текущие sysctl rp_filter в /tmp/xkeen_rp_filter.saved (если ещё не сохранены)
/// и ослабляет до 2 только если текущее значение не равно 0 или 2.
pub fn rp_filter_set_script() -> String {
    "cur_all=$(sysctl -n net.ipv4.conf.all.rp_filter 2>/dev/null || echo 1); \
cur_def=$(sysctl -n net.ipv4.conf.default.rp_filter 2>/dev/null || echo 1); \
if [ ! -f /tmp/xkeen_rp_filter.saved ]; then printf '%s %s\\n' \"$cur_all\" \"$cur_def\" > /tmp/xkeen_rp_filter.saved 2>/dev/null || true; fi; \
[ \"$cur_all\" != \"0\" ] && [ \"$cur_all\" != \"2\" ] && sysctl -w net.ipv4.conf.all.rp_filter=2 2>/dev/null || true; \
[ \"$cur_def\" != \"0\" ] && [ \"$cur_def\" != \"2\" ] && sysctl -w net.ipv4.conf.default.rp_filter=2 2>/dev/null || true".to_string()
}

/// Баг 112: восстанавливает сохранённые значения rp_filter из /tmp/xkeen_rp_filter.saved (или 1 по умолчанию)
/// и удаляет файл сохранения.
pub fn rp_filter_restore_script() -> String {
    "if [ -f /tmp/xkeen_rp_filter.saved ]; then \
read -r saved_all saved_def < /tmp/xkeen_rp_filter.saved 2>/dev/null || true; \
sysctl -w net.ipv4.conf.all.rp_filter=\"${saved_all:-1}\" 2>/dev/null || true; \
sysctl -w net.ipv4.conf.default.rp_filter=\"${saved_def:-1}\" 2>/dev/null || true; \
rm -f /tmp/xkeen_rp_filter.saved 2>/dev/null || true; \
else \
sysctl -w net.ipv4.conf.all.rp_filter=1 2>/dev/null || true; \
sysctl -w net.ipv4.conf.default.rp_filter=1 2>/dev/null || true; \
fi".to_string()
}

pub fn build_gaming_full_tunnel_commands(
    cfg: &config::GamingConfig,
    redir_port: u16,
    tproxy_port: u16,
    dns_port: u16,
) -> (String, Vec<String>, Vec<String>) {
    let r_port = if redir_port > 0 { redir_port } else { 5000 };
    let tp_port = if tproxy_port > 0 { tproxy_port } else { 5001 };
    let d_port = if dns_port > 0 { dns_port } else { crate::routing::DEFAULT_MIHOMO_DNS_PORT };

    let cleanup_cmd = "\
while iptables -t nat -D PREROUTING -j XKEEN_GAMING -w 2 2>/dev/null; do :; done; \
iptables -t nat -F XKEEN_GAMING -w 2 2>/dev/null || true; \
iptables -t nat -X XKEEN_GAMING -w 2 2>/dev/null || true; \
while iptables -t mangle -D PREROUTING -j XKEEN_GAMING -w 2 2>/dev/null; do :; done; \
iptables -t mangle -F XKEEN_GAMING -w 2 2>/dev/null || true; \
iptables -t mangle -X XKEEN_GAMING -w 2 2>/dev/null || true; \
while ip6tables -D FORWARD -j XKEEN_GAMING -w 2 2>/dev/null; do :; done; \
ip6tables -F XKEEN_GAMING -w 2 2>/dev/null || true; \
ip6tables -X XKEEN_GAMING -w 2 2>/dev/null || true; \
while ip rule del fwmark 0x115 lookup 115 2>/dev/null; do :; done; \
while ip rule del fwmark 0x115/0x115 lookup 115 2>/dev/null; do :; done; \
while ip -6 rule del fwmark 0x115 lookup 115 2>/dev/null; do :; done; \
while ip -6 rule del fwmark 0x115/0x115 lookup 115 2>/dev/null; do :; done; \
ip route del local default dev lo table 115 2>/dev/null || ip route del local 0.0.0.0/0 dev lo table 115 2>/dev/null || true; \
ip route flush table 115 2>/dev/null || true; \
ip -6 route del local default dev lo table 115 2>/dev/null || true; \
ip -6 route flush table 115 2>/dev/null || true".to_string();

    let mut setup_cmds = Vec::new();
    let mut conntrack_cmds = Vec::new();

    if cfg.enabled && cfg.mode == config::GamingMode::Compatibility {
        let active_devices: Vec<_> = cfg.devices.iter()
            .filter(|d| config::is_gaming_device_active(d, true))
            .collect();

        if !active_devices.is_empty() {
            // Ослабление rp_filter для корректного прохождения TPROXY-пакетов через сетевой мост LAN (баг 112: с сохранением исходного)
            setup_cmds.push(rp_filter_set_script());

            // TPROXY policy routing: связывание fwmark 0x115 с локальной таблицей сокетов dev lo table 115
            setup_cmds.push("ip route replace local default dev lo table 115 2>/dev/null || ip route add local 0.0.0.0/0 dev lo table 115 2>/dev/null || true".to_string());
            setup_cmds.push("while ip rule del fwmark 0x115 lookup 115 2>/dev/null; do :; done; while ip rule del fwmark 0x115/0x115 lookup 115 2>/dev/null; do :; done; ip rule add fwmark 0x115 lookup 115 priority 100 2>/dev/null || ip rule add fwmark 0x115/0x115 lookup 115 priority 100 2>/dev/null || ip rule add fwmark 0x115 lookup 115 2>/dev/null || true".to_string());
            setup_cmds.push("ip -6 route replace local default dev lo table 115 2>/dev/null || ip -6 route add local default dev lo table 115 2>/dev/null || true".to_string());
            setup_cmds.push("while ip -6 rule del fwmark 0x115 lookup 115 2>/dev/null; do :; done; while ip -6 rule del fwmark 0x115/0x115 lookup 115 2>/dev/null; do :; done; ip -6 rule add fwmark 0x115 lookup 115 priority 100 2>/dev/null || ip -6 rule add fwmark 0x115/0x115 lookup 115 priority 100 2>/dev/null || ip -6 rule add fwmark 0x115 lookup 115 2>/dev/null || true".to_string());

            // Изолированные цепочки XKEEN_GAMING
            setup_cmds.push("iptables -t nat -N XKEEN_GAMING -w 2 2>/dev/null || true".to_string());
            setup_cmds.push("iptables -t nat -I PREROUTING 1 -j XKEEN_GAMING -w 2".to_string());
            setup_cmds.push("iptables -t mangle -N XKEEN_GAMING -w 2 2>/dev/null || true".to_string());
            setup_cmds.push("iptables -t mangle -I PREROUTING 1 -j XKEEN_GAMING -w 2".to_string());
            setup_cmds.push("ip6tables -N XKEEN_GAMING -w 2 2>/dev/null || true".to_string());
            setup_cmds.push("ip6tables -I FORWARD 1 -j XKEEN_GAMING -w 2 2>/dev/null || true".to_string());
            // Баг 20: подменные IP Antigravity в user_exclude не должны перехватываться XKEEN_GAMING
            setup_cmds.push("ipset create user_exclude hash:ip -exist 2>/dev/null || true".to_string());
            setup_cmds.push("iptables -t nat -A XKEEN_GAMING -m set --match-set user_exclude dst -j RETURN -w 2".to_string());
            setup_cmds.push("iptables -t mangle -A XKEEN_GAMING -m set --match-set user_exclude dst -j RETURN -w 2".to_string());

            let mut seen_ips = std::collections::HashSet::new();

            for dev in active_devices {
                let ip_raw = dev.ip.trim();
                if ip_raw.is_empty() {
                    log_w!("Игровое устройство '{}' (MAC {}) пропущено при сборке iptables-правил: пустой IP", dev.name, dev.mac);
                    continue;
                }
                let ip_clean = ip_raw.split('/').next().unwrap_or(ip_raw).trim();
                let ipv4 = match ip_clean.parse::<std::net::Ipv4Addr>() {
                    Ok(addr) if !addr.is_unspecified() && !addr.is_loopback() && !addr.is_multicast() && !addr.is_broadcast() => addr,
                    _ => continue,
                };
                if !seen_ips.insert(ipv4) {
                    continue;
                }
                let ip = ipv4.to_string();

                // Сброс conntrack-сессий устройства (только для прямого вызова из API, исключается из NDM-хука)
                conntrack_cmds.push(format!("conntrack -D -s {ip} 2>/dev/null || true"));

                // IPv6: блокировка транзитного IPv6 для игрового устройства (предотвращение утечки трафика мимо туннеля)
                let mac = dev.mac.trim().to_lowercase().replace('-', ":");
                let is_valid_mac = mac != "00:00:00:00:00:00"
                    && mac != "ff:ff:ff:ff:ff:ff"
                    && mac.split(':').count() == 6
                    && mac.split(':').all(|p| p.len() == 2 && u8::from_str_radix(p, 16).is_ok());
                if is_valid_mac {
                    setup_cmds.push(format!(
                        "ip6tables -A XKEEN_GAMING -m mac --mac-source {mac} -j REJECT -w 2 2>/dev/null || true"
                    ));
                }
                for v6_raw in &dev.ipv6 {
                    let v6_clean = v6_raw.trim().split('/').next().unwrap_or(v6_raw).trim();
                    if let Ok(v6) = v6_clean.parse::<std::net::Ipv6Addr>() {
                        if !v6.is_unspecified() && !v6.is_loopback() && !v6.is_multicast() && (v6.segments()[0] & 0xffc0 != 0xfe80) {
                            setup_cmds.push(format!(
                                "ip6tables -A XKEEN_GAMING -s {v6} -j REJECT -w 2 2>/dev/null || true"
                            ));
                        }
                    }
                }

                // NAT: прямой порядок правил (-A) в цепочке XKEEN_GAMING
                // 1. Перехват DNS (UDP/TCP 53) -> REDIRECT --to-ports {d_port} (до локальных подсетей)
                setup_cmds.push(format!(
                    "iptables -t nat -A XKEEN_GAMING -s {ip} -p udp --dport 53 -j REDIRECT --to-ports {d_port} -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t nat -A XKEEN_GAMING -s {ip} -p tcp --dport 53 -j REDIRECT --to-ports {d_port} -w 2"
                ));
                // 2. Байпас локальных сетей через -j RETURN: 127.0.0.0/8, 169.254.0.0/16, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                setup_cmds.push(format!(
                    "iptables -t nat -A XKEEN_GAMING -s {ip} -d 127.0.0.0/8 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t nat -A XKEEN_GAMING -s {ip} -d 169.254.0.0/16 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t nat -A XKEEN_GAMING -s {ip} -d 10.0.0.0/8 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t nat -A XKEEN_GAMING -s {ip} -d 172.16.0.0/12 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t nat -A XKEEN_GAMING -s {ip} -d 192.168.0.0/16 -j RETURN -w 2"
                ));
                // 3. Весь остальной TCP -> REDIRECT --to-ports {r_port}
                setup_cmds.push(format!(
                    "iptables -t nat -A XKEEN_GAMING -s {ip} -p tcp -j REDIRECT --to-ports {r_port} -w 2"
                ));

                // MANGLE: прямой порядок правил (-A) в цепочке XKEEN_GAMING
                // 1. Пропуск DNS: -p udp --dport 53 -j RETURN (чтобы уходил в NAT REDIRECT)
                setup_cmds.push(format!(
                    "iptables -t mangle -A XKEEN_GAMING -s {ip} -p udp --dport 53 -j RETURN -w 2"
                ));
                // 2. Байпас локальных сетей через -j RETURN: 127.0.0.0/8, 169.254.0.0/16, 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16, 224.0.0.0/4, 255.255.255.255/32
                setup_cmds.push(format!(
                    "iptables -t mangle -A XKEEN_GAMING -s {ip} -d 127.0.0.0/8 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t mangle -A XKEEN_GAMING -s {ip} -d 169.254.0.0/16 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t mangle -A XKEEN_GAMING -s {ip} -d 10.0.0.0/8 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t mangle -A XKEEN_GAMING -s {ip} -d 172.16.0.0/12 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t mangle -A XKEEN_GAMING -s {ip} -d 192.168.0.0/16 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t mangle -A XKEEN_GAMING -s {ip} -d 224.0.0.0/4 -j RETURN -w 2"
                ));
                setup_cmds.push(format!(
                    "iptables -t mangle -A XKEEN_GAMING -s {ip} -d 255.255.255.255/32 -j RETURN -w 2"
                ));
                // 3. Весь остальной UDP -> TPROXY --tproxy-mark 0x115/0xffffffff --on-port {tp_port} --on-ip 127.0.0.1
                setup_cmds.push(format!(
                    "iptables -t mangle -A XKEEN_GAMING -s {ip} -p udp -j TPROXY --tproxy-mark 0x115/0xffffffff --on-port {tp_port} --on-ip 127.0.0.1 -w 2"
                ));
            }
        }
    }

    (cleanup_cmd, setup_cmds, conntrack_cmds)
}

pub async fn sync_gaming_full_tunnel(cfg: &config::GamingConfig, redir_port: u16, tproxy_port: u16, dns_port: u16) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let (cleanup_cmd, setup_cmds, conntrack_cmds) = build_gaming_full_tunnel_commands(cfg, redir_port, tproxy_port, dns_port);
        let script = format!("{}\n{}", cleanup_cmd, setup_cmds.join("\n"));
        let out = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .output()
            .await;
        match out {
            Ok(o) => {
                if !o.status.success() {
                    let err = String::from_utf8_lossy(&o.stderr);
                    if !err.trim().is_empty() {
                        log_w!("[GAMING_TUNNEL] Предупреждение при настройке правил: {}", err.trim());
                    }
                }
            }
            Err(e) => {
                log_w!("[GAMING_TUNNEL] Не удалось запустить sh для настройки правил: {}", e);
            }
        }

        if !conntrack_cmds.is_empty() {
            let conntrack_script = conntrack_cmds.join("\n");
            let ct_out = tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&conntrack_script)
                .output()
                .await;
            match ct_out {
                Ok(o) => {
                    if !o.status.success() {
                        let err = String::from_utf8_lossy(&o.stderr);
                        if !err.trim().is_empty() {
                            log_w!("[GAMING_TUNNEL] Предупреждение при сбросе conntrack: {}", err.trim());
                        }
                    }
                }
                Err(e) => {
                    log_w!("[GAMING_TUNNEL] Не удалось запустить sh для conntrack: {}", e);
                }
            }
        }

        // BUG-10: NDM netfilter hook для автоматического восстановления правил при перезагрузке фаервола роутером
        let hook_path = std::path::Path::new("/opt/etc/ndm/netfilter.d/060-xkeen-gaming.sh");
        if cfg.enabled && cfg.mode == config::GamingMode::Compatibility && !setup_cmds.is_empty() {
            if let Some(parent) = hook_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let mut hook_content = String::from("#!/bin/sh\n[ \"$table\" != \"nat\" ] && [ \"$table\" != \"mangle\" ] && [ \"$table\" != \"filter\" ] && [ -n \"$table\" ] && exit 0\n");
            hook_content.push_str(&cleanup_cmd);
            hook_content.push('\n');
            for cmd in &setup_cmds {
                hook_content.push_str(cmd);
                hook_content.push('\n');
            }
            if let Ok(()) = tokio::fs::write(hook_path, hook_content).await {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = tokio::fs::set_permissions(hook_path, std::fs::Permissions::from_mode(0o755)).await;
                }
            }
        } else {
            let _ = tokio::fs::remove_file(hook_path).await;
        }

        // Диагностика состояния маршрутизации и таблицы 115 для системного журнала
        if cfg.enabled && cfg.mode == config::GamingMode::Compatibility {
            let diag = tokio::process::Command::new("sh")
                .arg("-c")
                .arg("ip rule show; echo '--- table 115:'; ip route show table 115; echo '--- PREROUTING mangle:'; iptables -w 2 -t mangle -S PREROUTING | grep -E 'compat|XKEEN_GAMING' || true; echo '--- PREROUTING nat:'; iptables -w 2 -t nat -S PREROUTING | grep -E 'compat|XKEEN_GAMING' || true; echo '--- XKEEN_GAMING mangle:'; iptables -w 2 -t mangle -S XKEEN_GAMING 2>/dev/null || true; echo '--- XKEEN_GAMING nat:'; iptables -w 2 -t nat -S XKEEN_GAMING 2>/dev/null || true")
                .output()
                .await;
            if let Ok(d) = diag {
                let txt = String::from_utf8_lossy(&d.stdout);
                log_i!("[GAMING_TUNNEL] Диагностика policy routing и PREROUTING:\n{}", txt.trim());
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (cfg, redir_port, tproxy_port, dns_port);
    }
    Ok(())
}

/// Генерация полного перехвата для обеих стратегий режима белых списков.
/// Захватывает ВЕСЬ трафик локальных подсетей (все TCP/UDP порты, а не только 80/443,
/// которые берёт внешняя цепочка xkeen ядра) и направляет его в Mihomo, где правила БС
/// решают судьбу каждого соединения: Standard может отправлять РФ-каталог в DIRECT,
/// а StrictXKeen по умолчанию направляет клиентский трафик через выбранный туннель.
/// Исключённые устройства БС обходят перехват целиком (RETURN в начале цепочки).
#[cfg(any(target_os = "linux", test))]
pub fn build_whitelist_full_tunnel_commands(
    cfg: &config::AppConfig,
    redir_port: u16,
    tproxy_port: u16,
    dns_port: u16,
) -> (String, Vec<String>) {
    let r_port = if redir_port > 0 { redir_port } else { 5000 };
    let tp_port = if tproxy_port > 0 { tproxy_port } else { 5001 };
    let d_port = if dns_port > 0 { dns_port } else { crate::routing::DEFAULT_MIHOMO_DNS_PORT };

    // Очистка не трогает ip rule/таблицу 115: ими управляет sync_gaming_full_tunnel
    // (при взаимно исключающихся режимах порядок синков гарантирует согласованность).
    let cleanup_cmd = "\
while iptables -w -t filter -D INPUT -m mark --mark 0x115/0x115 -p udp --dport 443 -j REJECT --reject-with icmp-port-unreachable 2>/dev/null; do :; done; \
while iptables -t nat -D PREROUTING -j XKEEN_BS -w 2>/dev/null; do :; done; \
iptables -t nat -F XKEEN_BS -w 2>/dev/null || true; \
iptables -t nat -X XKEEN_BS -w 2>/dev/null || true; \
while iptables -t mangle -D PREROUTING -j XKEEN_BS -w 2>/dev/null; do :; done; \
iptables -t mangle -F XKEEN_BS -w 2>/dev/null || true; \
iptables -t mangle -X XKEEN_BS -w 2>/dev/null || true; \
while ip6tables -w -D FORWARD -i br+ -j XKEEN_BS6 2>/dev/null; do :; done; \
while ip6tables -w -D FORWARD -i Bridge+ -j XKEEN_BS6 2>/dev/null; do :; done; \
ip6tables -w -F XKEEN_BS6 2>/dev/null || true; \
ip6tables -w -X XKEEN_BS6 2>/dev/null || true".to_string();

    let mut setup_cmds = Vec::new();
    if !cfg.whitelist.enabled {
        return (cleanup_cmd, setup_cmds);
    }

    // QUIC-INGR-01: Мгновенное отклонение QUIC (UDP/443) на уровне ядра Linux (ICMP Port Unreachable)
    // предотвращает многосекундные зависания Chromium/Firefox/YouTube при попытке соединиться по HTTP/3.
    setup_cmds.push("iptables -w -t filter -I INPUT 1 -m mark --mark 0x115/0x115 -p udp --dport 443 -j REJECT --reject-with icmp-port-unreachable".to_string());

    // Ослабление rp_filter и policy routing для TPROXY (общие с игровым режимом,
    // режимы взаимно исключаются в sync_full_tunnel_modes; баг 112: с сохранением исходного)
    setup_cmds.push(rp_filter_set_script());
    setup_cmds.push("ip route replace local default dev lo table 115 2>/dev/null || ip route add local 0.0.0.0/0 dev lo table 115 2>/dev/null || true".to_string());
    setup_cmds.push("while ip rule del fwmark 0x115 lookup 115 2>/dev/null; do :; done; while ip rule del fwmark 0x115/0x115 lookup 115 2>/dev/null; do :; done; ip rule add fwmark 0x115 lookup 115 priority 100 2>/dev/null || ip rule add fwmark 0x115/0x115 lookup 115 priority 100 2>/dev/null || ip rule add fwmark 0x115 lookup 115 2>/dev/null || true".to_string());

    // Изолированные цепочки XKEEN_BS
    setup_cmds.push("iptables -t nat -N XKEEN_BS -w 2>/dev/null || true".to_string());
    setup_cmds.push("iptables -t nat -I PREROUTING 1 -j XKEEN_BS -w".to_string());
    setup_cmds.push("iptables -t mangle -N XKEEN_BS -w 2>/dev/null || true".to_string());
    setup_cmds.push("iptables -t mangle -I PREROUTING 1 -j XKEEN_BS -w".to_string());
    setup_cmds.push(format!("iptables -t nat -A XKEEN_BS -i {} -j RETURN -w", crate::vkturn::TUN_IFACE));
    setup_cmds.push(format!("iptables -t mangle -A XKEEN_BS -i {} -j RETURN -w", crate::vkturn::TUN_IFACE));
    setup_cmds.push(format!("iptables -t nat -A XKEEN_BS -m mark --mark {} -j RETURN -w", crate::vkturn::ROUTE_TABLE));
    setup_cmds.push(format!("iptables -t mangle -A XKEEN_BS -m mark --mark {} -j RETURN -w", crate::vkturn::ROUTE_TABLE));
    // Баг 20: подменные IP Antigravity в user_exclude не должны перехватываться XKEEN_BS
    setup_cmds.push("ipset create user_exclude hash:ip -exist 2>/dev/null || true".to_string());
    setup_cmds.push("iptables -t nat -A XKEEN_BS -m set --match-set user_exclude dst -j RETURN -w".to_string());
    setup_cmds.push("iptables -t mangle -A XKEEN_BS -m set --match-set user_exclude dst -j RETURN -w".to_string());

    // Исключённые устройства БС: полный обход перехвата (и TCP, и UDP), до любых правил захвата
    for ex_ip in resolve_excluded_whitelist_ips_pub(cfg) {
        let ip_clean = ex_ip.split('/').next().unwrap_or(&ex_ip).trim();
        if let Ok(v4) = ip_clean.parse::<std::net::Ipv4Addr>() {
            let ip = v4.to_string();
            setup_cmds.push(format!("iptables -t nat -A XKEEN_BS -s {ip} -j RETURN -w"));
            setup_cmds.push(format!("iptables -t mangle -A XKEEN_BS -s {ip} -j RETURN -w"));
        }
    }

    // Перехват по приватным подсетям или по списку включенных устройств (баг 38).
    // Если режим БС включён выборочно для устройств (Devices), перехват настраивается только для них.
    let is_devices_only = cfg.whitelist.enabled_by == config::WhitelistEnabledBy::Devices;
    let target_subnets_or_ips: Vec<String> = if is_devices_only {
        let mut ips = Vec::new();
        let excluded = resolve_excluded_whitelist_ips_pub(cfg);
        for dev in &cfg.whitelist.devices {
            if dev.enabled && !dev.ip.trim().is_empty() {
                let clean_ip = dev.ip.split('/').next().unwrap_or(&dev.ip).trim();
                if let Ok(v4) = clean_ip.parse::<std::net::Ipv4Addr>() {
                    let s = v4.to_string();
                    if !excluded.iter().any(|ex| ex.split('/').next().unwrap_or(ex).trim() == s) && !ips.contains(&s) {
                        ips.push(s);
                    }
                }
            }
        }
        ips
    } else {
        vec!["192.168.0.0/16".to_string(), "10.0.0.0/8".to_string(), "172.16.0.0/12".to_string()]
    };

    for target in &target_subnets_or_ips {
        // NAT: DNS -> REDIRECT на активный DNS-порт (до байпаса локальных сетей)
        setup_cmds.push(format!(
            "iptables -t nat -A XKEEN_BS -s {target} -p udp --dport 53 -j REDIRECT --to-ports {d_port} -w"
        ));
        setup_cmds.push(format!(
            "iptables -t nat -A XKEEN_BS -s {target} -p tcp --dport 53 -j REDIRECT --to-ports {d_port} -w"
        ));
        // NAT: байпас локальных подсетей (та же логика, что в игровом режиме)
        for local_dst in ["127.0.0.0/8", "169.254.0.0/16", "10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16"] {
            setup_cmds.push(format!("iptables -t nat -A XKEEN_BS -s {target} -d {local_dst} -j RETURN -w"));
        }
        // NAT: весь остальной TCP (ЛЮБЫЕ порты — включая игровые 9339) -> REDIRECT в Mihomo
        setup_cmds.push(format!(
            "iptables -t nat -A XKEEN_BS -s {target} -p tcp -j REDIRECT --to-ports {r_port} -w"
        ));
        // MANGLE: DNS пропускаем (его забирает NAT REDIRECT)
        setup_cmds.push(format!(
            "iptables -t mangle -A XKEEN_BS -s {target} -p udp --dport 53 -j RETURN -w"
        ));
        // MANGLE: байпас локальных и служебных подсетей
        for local_dst in [
            "127.0.0.0/8",
            "169.254.0.0/16",
            "10.0.0.0/8",
            "172.16.0.0/12",
            "192.168.0.0/16",
            "224.0.0.0/4",
            "255.255.255.255/32",
        ] {
            setup_cmds.push(format!("iptables -t mangle -A XKEEN_BS -s {target} -d {local_dst} -j RETURN -w"));
        }
        // MANGLE: весь остальной UDP (ЛЮБЫЕ порты) -> TPROXY в Mihomo
        setup_cmds.push(format!(
            "iptables -t mangle -A XKEEN_BS -s {target} -p udp -j TPROXY --tproxy-mark 0x115/0xffffffff --on-port {tp_port} --on-ip 127.0.0.1 -w"
        ));
    }

    // Баг 41: IPv6 не проходит через Mihomo и обходил БС. Пересылаемый из LAN внешний IPv6 отклоняется
    // (локальный, ULA и multicast — нет), клиенты по Happy Eyeballs уходят на IPv4, который идёт через БС.
    setup_cmds.push("ip6tables -w -N XKEEN_BS6 2>/dev/null || true".to_string());
    setup_cmds.push("ip6tables -w -F XKEEN_BS6 2>/dev/null || true".to_string());
    for v6_local in ["fe80::/10", "fc00::/7", "ff00::/8", "::1/128"] {
        setup_cmds.push(format!("ip6tables -w -A XKEEN_BS6 -d {v6_local} -j RETURN"));
    }
    for ex in &cfg.whitelist.excluded_devices {
        let mac = ex.trim();
        if config::is_valid_mac(mac) {
            setup_cmds.push(format!("ip6tables -w -A XKEEN_BS6 -m mac --mac-source {mac} -j RETURN"));
        }
    }
    for dev in &cfg.whitelist.devices {
        if !dev.enabled {
            let mac = dev.mac.trim();
            if config::is_valid_mac(mac) {
                setup_cmds.push(format!("ip6tables -w -A XKEEN_BS6 -m mac --mac-source {mac} -j RETURN"));
            }
        }
    }
    setup_cmds.push("ip6tables -w -A XKEEN_BS6 -p tcp -j REJECT --reject-with tcp-reset".to_string());
    setup_cmds.push("ip6tables -w -A XKEEN_BS6 -j REJECT".to_string());
    setup_cmds.push("ip6tables -w -I FORWARD 1 -i br+ -j XKEEN_BS6".to_string());
    setup_cmds.push("ip6tables -w -I FORWARD 1 -i Bridge+ -j XKEEN_BS6 2>/dev/null || true".to_string());

    (cleanup_cmd, setup_cmds)
}

/// Публичная обёртка резолва исключённых IP БС для генератора команд (unit-тестируемость).
#[cfg(any(target_os = "linux", test))]
pub fn resolve_excluded_whitelist_ips_pub(cfg: &config::AppConfig) -> Vec<String> {
    crate::routing::resolve_excluded_whitelist_ips(cfg)
}

/// BS-FIX: применение/снятие полного перехвата режима БС + NDM-хук автовосстановления.
pub async fn sync_whitelist_full_tunnel(cfg: &config::AppConfig, redir_port: u16, tproxy_port: u16, dns_port: u16) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let (cleanup_cmd, setup_cmds) = build_whitelist_full_tunnel_commands(cfg, redir_port, tproxy_port, dns_port);
        let script = format!("{}\n{}", cleanup_cmd, setup_cmds.join("\n"));
        let out = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&script)
            .output()
            .await;
        match out {
            Ok(o) => {
                if !o.status.success() {
                    let err = String::from_utf8_lossy(&o.stderr);
                    if !err.trim().is_empty() {
                        log_w!("[BS_TUNNEL] Предупреждение при настройке правил: {}", err.trim());
                    }
                }
            }
            Err(e) => {
                log_w!("[BS_TUNNEL] Не удалось запустить sh для настройки правил: {}", e);
            }
        }

        // NDM-хук: восстановление правил после перезагрузки фаервола KeeneticOS
        let hook_path = std::path::Path::new("/opt/etc/ndm/netfilter.d/061-xkeen-bs.sh");
        let active = cfg.whitelist.enabled;
        if active && !setup_cmds.is_empty() {
            if let Some(parent) = hook_path.parent() {
                let _ = tokio::fs::create_dir_all(parent).await;
            }
            let mut hook_content = String::from("#!/bin/sh\n[ \"$table\" != \"nat\" ] && [ \"$table\" != \"mangle\" ] && [ \"$table\" != \"filter\" ] && [ -n \"$table\" ] && exit 0\n");
            hook_content.push_str(&cleanup_cmd);
            hook_content.push('\n');
            for cmd in &setup_cmds {
                hook_content.push_str(cmd);
                hook_content.push('\n');
            }
            if let Ok(()) = tokio::fs::write(hook_path, hook_content).await {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ = tokio::fs::set_permissions(hook_path, std::fs::Permissions::from_mode(0o755)).await;
                }
            }
        } else {
            let _ = tokio::fs::remove_file(hook_path).await;
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (cfg, redir_port, tproxy_port, dns_port);
    }
    Ok(())
}

/// Экстренная очистка цепочек полного перехвата XKEEN_BS и XKEEN_GAMING, а также удаление NDM-хуков
pub async fn emergency_cleanup_full_tunnel_rules() {
    #[cfg(target_os = "linux")]
    {
        // 1. Команды очистки XKEEN_BS и XKEEN_GAMING
        let dummy_cfg = config::AppConfig::default();
        let mihomo_dns_port = crate::routing::mihomo_dns_port(&dummy_cfg);
        let (wl_cleanup, _) = build_whitelist_full_tunnel_commands(&dummy_cfg, 5000, 5001, mihomo_dns_port);
        let dummy_gaming = config::GamingConfig::default();
        let (gaming_cleanup, _, _) = build_gaming_full_tunnel_commands(&dummy_gaming, 5000, 5001, mihomo_dns_port);

        let cleanup_script = format!("{}\n{}", wl_cleanup, gaming_cleanup);
        let _ = tokio::process::Command::new("sh")
            .arg("-c")
            .arg(&cleanup_script)
            .output()
            .await;

        // 2. Удаление NDM-хуков, чтобы netfilter.d не восстановил правила при падении
        let _ = tokio::fs::remove_file("/opt/etc/ndm/netfilter.d/060-xkeen-gaming.sh").await;
        let _ = tokio::fs::remove_file("/opt/etc/ndm/netfilter.d/061-xkeen-bs.sh").await;
    }
}

/// BS-FIX: единая точка синхронизации полного перехвата. Режимы взаимоисключающие:
/// активен режим БС (StrictXKeen) ИЛИ игровой Compatibility — второй всегда приводится
/// к чистому состоянию. Порядок важен: gaming управляет ip rule/таблицей 115,
/// поэтому его синк идёт первым, а БС при необходимости поднимает таблицу заново.
/// Эффективные флаги полного перехвата (bs, gaming) с учётом состояния ядра Mihomo.
/// Пока сторож считает Mihomo упавшим, перехват НЕ ставится ни для одного режима:
/// REDIRECT/TPROXY в мёртвые порты 5000/5001 = блэкаут всей LAN. Раньше любой вызов
/// синка (failover, DHCP-монитор, smart-gaming, API) возвращал правила, снятые сторожем.
pub fn effective_full_tunnel_flags(bs_active: bool, gaming_active: bool, mihomo_down: bool) -> (bool, bool) {
    if mihomo_down {
        (false, false)
    } else {
        (bs_active, gaming_active)
    }
}

/// Нужна ли пересинхронизация полного перехвата (игровой Compatibility или активный режим БС).
/// Раньше после переключения AdGuard синк шёл только для игрового режима — в режиме БС
/// DNS-редирект XKEEN_BS оставался на старом порту (1053 ↔ порт AdGuard).
pub fn full_tunnel_resync_needed(cfg: &config::AppConfig) -> bool {
    (cfg.gaming.enabled && cfg.gaming.mode == config::GamingMode::Compatibility)
        || cfg.whitelist.enabled
}

/// Пересинхронизация полного перехвата с портами из текущего config.yaml (Z-09: без хардкода 5000/5001).
pub async fn resync_full_tunnel_from_disk(cfg: &config::AppConfig) {
    if !full_tunnel_resync_needed(cfg) {
        return;
    }
    let yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let ports = routing::redir_ports(&yaml);
    let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
    let dns_port = resolve_active_dns_port(cfg);
    sync_full_tunnel_modes(cfg, redir_port, tproxy_port, dns_port).await;
}

pub async fn sync_full_tunnel_modes(cfg: &config::AppConfig, redir_port: u16, tproxy_port: u16, dns_port: u16) {
    let want_bs = cfg.whitelist.enabled;
    let want_gaming = cfg.gaming.enabled && cfg.gaming.mode == config::GamingMode::Compatibility;
    // Ядро недоступно и при аварии (сторож), и при штатной остановке служб пользователем:
    // DHCP-монитор и другие фоновые задачи не должны ставить перехват в остановленный Mihomo.
    let mihomo_down = crate::watchdog::is_mihomo_down() || crate::watchdog::is_service_stopped();
    let (bs_active, gaming_active) = effective_full_tunnel_flags(want_bs, want_gaming, mihomo_down);
    if mihomo_down && (want_bs || want_gaming) {
        log_w!("[TUNNEL] Mihomo недоступен — полный перехват не устанавливается до восстановления ядра (защита от блэкаута)");
        let mut off = cfg.clone();
        off.whitelist.enabled = false;
        off.gaming.enabled = false;
        let _ = sync_gaming_full_tunnel(&off.gaming, redir_port, tproxy_port, dns_port).await;
        let _ = sync_whitelist_full_tunnel(&off, redir_port, tproxy_port, dns_port).await;
        let _ = tokio::process::Command::new("sh").arg("-c").arg(rp_filter_restore_script()).output().await;
        return;
    }

    // Игровой синк: при активном БС принудительно в режим очистки (мьютекс правил)
    let gaming_cfg = if bs_active {
        let mut g = cfg.gaming.clone();
        g.enabled = false;
        g
    } else {
        cfg.gaming.clone()
    };
    let _ = sync_gaming_full_tunnel(&gaming_cfg, redir_port, tproxy_port, dns_port).await;

    // Синк БС: при активном игровом режиме принудительно в режим очистки
    let wl_cfg = if gaming_active && !bs_active {
        let mut w = cfg.clone();
        w.whitelist.enabled = false;
        w
    } else {
        cfg.clone()
    };
    let _ = sync_whitelist_full_tunnel(&wl_cfg, redir_port, tproxy_port, dns_port).await;
    if rp_filter_restore_needed(gaming_active, bs_active) {
        let _ = tokio::process::Command::new("sh").arg("-c").arg(rp_filter_restore_script()).output().await;
    }
}



/// GET /api/gaming/status — статус, конфиг, активный узел, туннель, перехват и реальные соединения
pub async fn get_gaming_status(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let domains_count = routing::get_gaming_domains(&cfg.gaming).len();

    // 1. Активный узел группы 🎮 Gaming
    let (proxies_res, providers_res) = tokio::join!(
        mihomo::get_proxies(&state.http, &cfg),
        mihomo::get_provider_proxies(&state.http, &cfg)
    );
    let proxies_map = proxies_res.ok();
    let providers_map = providers_res.ok();

    let group_now = proxies_map
        .as_ref()
        .and_then(|map| {
            map.get(routing::GAMING_GROUP_NAME)
                .or_else(|| map.get("Gaming"))
                .and_then(|g| g.get("now"))
                .and_then(|n| n.as_str())
        })
        .unwrap_or(cfg.gaming.target_server.as_str());

    let active_server = proxies_map
        .as_ref()
        .map(|map| resolve_group_leaf(map, group_now))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| group_now.to_string());

    // 2. Статус туннеля (пинг / задержка)
    let target_srv = cfg.gaming.target_server.trim();
    let (tunnel_reachable, tunnel_latency) = if target_srv == "DIRECT" || active_server == "DIRECT" {
        (true, Some(0i64))
    } else {
        let empty_map = std::collections::BTreeMap::new();
        let proxies = proxies_map.as_ref().unwrap_or(&empty_map);
        let providers = providers_map.as_ref().unwrap_or(&empty_map);

        let leaf_to_ping = resolve_group_leaf(proxies, if target_srv.is_empty() { "Fastest" } else { target_srv });
        let node = if !leaf_to_ping.is_empty() { leaf_to_ping } else { active_server.clone() };

        if node == "DIRECT" {
            (true, Some(0i64))
        } else {
            let mut delay = mihomo::ping_server_url(&state.http, &cfg, &node, 2500, None).await;
            if delay <= 0 && node != routing::GAMING_GROUP_NAME && node != "Gaming" {
                // Дополнительно меряем задержку выбранного узла через /proxies/{GAMING_GROUP_NAME}/delay
                // Группа 🎮 Gaming имеет тип selector в /proxies, и её delay-эндпоинт тестирует текущий now-узел
                let group_delay = mihomo::ping_server_url(&state.http, &cfg, routing::GAMING_GROUP_NAME, 2500, None).await;
                if group_delay > 0 {
                    delay = group_delay;
                }
            }

            tunnel_reachability(proxies, providers, &node, delay)
        }
    };

    // 3. Статус перехвата TCP / UDP / IPv6
    let (tcp_ok, udp_ok, ipv6_ok) = check_gaming_interception(&cfg).await;

    // 4. Активные игровые устройства и их реальные IP
    let active_devices: Vec<config::GamingDevice> = cfg.gaming.devices.iter()
        .filter(|d| config::is_gaming_device_active(d, true))
        .cloned()
        .collect();
    let active_device = active_devices.first().cloned();

    // 5. Реальные соединения устройств через Mihomo (/connections)
    let mut real_connections = Vec::new();
    if !active_devices.is_empty() {
        if let Ok(val) = mihomo::m_get(&state.http, &cfg, "/connections").await {
            if let Some(conns) = val.get("connections").and_then(|c| c.as_array()) {
                for c in conns {
                    let src = c.get("metadata").and_then(|m| m.get("sourceIP")).and_then(|s| s.as_str()).unwrap_or("");
                    if let Some(matched_dev) = active_devices.iter().find(|d| src == d.ip || d.ipv6.iter().any(|v6| v6 == src)) {
                        let host = c.get("metadata").and_then(|m| m.get("host")).and_then(|h| h.as_str()).unwrap_or("");
                        let dest_ip = c.get("metadata").and_then(|m| m.get("destinationIP")).and_then(|d| d.as_str()).unwrap_or("");
                        let dest_port = c.get("metadata")
                            .and_then(|m| m.get("destinationPort"))
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
                        let net = c.get("metadata").and_then(|m| m.get("network")).and_then(|n| n.as_str()).unwrap_or("TCP");
                        let rule = c.get("rule").and_then(|r| r.as_str()).unwrap_or("");
                        let chains = c.get("chains").and_then(|ch| ch.as_array())
                            .map(|arr| arr.iter().filter_map(|x| x.as_str().map(|s| s.to_string())).collect::<Vec<_>>())
                            .unwrap_or_default();
                        let dl = c.get("download").and_then(|v| v.as_u64()).unwrap_or(0);
                        let ul = c.get("upload").and_then(|v| v.as_u64()).unwrap_or(0);

                        real_connections.push(json!({
                            "id": c.get("id").and_then(|i| i.as_str()).unwrap_or(""),
                            "host": if !host.is_empty() { host } else { dest_ip },
                            "destination": format!("{dest_ip}:{dest_port}"),
                            "network": net.to_uppercase(),
                            "chains": chains,
                            "rule": rule,
                            "download": dl,
                            "upload": ul,
                            "device_name": matched_dev.name,
                            "device_ip": matched_dev.ip,
                        }));

                        if real_connections.len() >= 30 {
                            break;
                        }
                    }
                }
            }
        }
    }

    // 6. Реальная верификация: не считать включенным только потому, что кнопка нажата
    let mut is_active = false;
    let mut verification_error: Option<String> = None;

    if cfg.gaming.enabled {
        let yaml_content = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
        let has_gaming_rules = yaml_content.contains(routing::GAMING_BEGIN) && yaml_content.contains(routing::GAMING_GROUP_BEGIN);

        if !has_gaming_rules {
            verification_error = Some("Маршрутные правила не найдены в config.yaml ядра Mihomo".to_string());
        } else {
            match cfg.gaming.mode {
                config::GamingMode::Compatibility | config::GamingMode::BypassRu => {
                    if active_devices.is_empty() {
                        verification_error = Some("Не выбрано устройство для игрового режима".to_string());
                    } else {
                        let mut all_ok = true;
                        for dev in &active_devices {
                            if dev.ip.is_empty() {
                                verification_error = Some(format!("Устройство '{}' не имеет активного IP адреса", dev.name));
                                all_ok = false;
                                break;
                            } else {
                                let target = dev.server.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or(routing::GAMING_GROUP_NAME);
                                let expected_cidr = format!("SRC-IP-CIDR,{}/32,{}", dev.ip, target);
                                if !yaml_content.contains(&expected_cidr) {
                                    verification_error = Some(format!("Приоритетный маршрут для {} отсутствует в config.yaml", dev.ip));
                                    all_ok = false;
                                    break;
                                }
                            }
                        }
                        if all_ok {
                            if !tunnel_reachable && target_srv != "DIRECT" {
                                verification_error = Some(format!("Игровой туннель '{}' недоступен", target_srv));
                            } else {
                                is_active = true;
                            }
                        }
                    }
                }
                config::GamingMode::KnownServices | config::GamingMode::SmartSplit => {
                    if !tunnel_reachable && target_srv != "DIRECT" {
                        verification_error = Some(format!("Игровой туннель '{}' недоступен", target_srv));
                    } else if domains_count == 0 && !cfg.gaming.platforms.category_games {
                        verification_error = Some("В режиме известных сервисов не выбрана ни одна игровая платформа или категория правил".to_string());
                    } else {
                        is_active = true;
                    }
                }
            }
        }
    }

    let recent_gaming_conns = crate::watchdog::get_recent_gaming_conns(&cfg.gaming.ignored_game_conns);

    api_ok(json!({
        "enabled": cfg.gaming.enabled,
        "config": cfg.gaming,
        "active_server": active_server,
        "domains_count": domains_count,
        "is_active": is_active,
        "tunnel_status": {
            "target": cfg.gaming.target_server,
            "active_node": active_server,
            "reachable": tunnel_reachable,
            "latency_ms": tunnel_latency,
        },
        "tcp_interception": tcp_ok,
        "udp_interception": udp_ok,
        "ipv6_status": {
            "supported": ipv6_ok,
            "active": ipv6_ok && active_devices.iter().any(|d| d.ipv6.iter().any(|v| !v.to_lowercase().starts_with("fe80:") && !v.starts_with("::1"))),
            "addresses": active_devices.iter().flat_map(|d| {
                d.ipv6.iter().filter(|v| !v.to_lowercase().starts_with("fe80:") && !v.starts_with("::1")).cloned()
            }).collect::<Vec<_>>(),
        },
        "active_device": active_device,
        "active_devices": active_devices,
        "real_connections": real_connections,
        "recent_gaming_conns": recent_gaming_conns,
        "verification_error": verification_error,
    }))
}

#[derive(Deserialize)]
pub struct IgnoreGamingConnReq {
    pub target: String,
    #[serde(default)]
    pub remove: bool,
}

/// POST /api/gaming/ignore-conn — добавление или удаление соединения из игнор-листа Умного игрового режима
pub async fn ignore_gaming_conn(
    State(state): State<AppState>,
    Json(body): Json<IgnoreGamingConnReq>,
) -> Response {
    let target = body.target.trim().to_string();
    if target.is_empty() {
        return api_err("Не указана цель соединения (хост или IP:порт)");
    }
    let _guard = state.config_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();
    if body.remove {
        cfg.gaming.ignored_game_conns.retain(|x| !x.eq_ignore_ascii_case(&target));
    } else if !cfg.gaming.ignored_game_conns.iter().any(|x| x.eq_ignore_ascii_case(&target)) {
        cfg.gaming.ignored_game_conns.push(target.clone());
    }
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }
    let ignored_list = cfg.gaming.ignored_game_conns.clone();
    let recent = crate::watchdog::get_recent_gaming_conns(&ignored_list);
    *state.config.write().await = std::sync::Arc::new(cfg);
    api_ok(json!({
        "saved": true,
        "ignored_game_conns": ignored_list,
        "recent_gaming_conns": recent,
    }))
}

/// Баг 6: частичное обновление настроек игрового режима накладывается на текущий конфиг.
/// Поля `ignored_game_conns`, `gaming_suspended` через патч не меняются.
pub fn merge_gaming_patch(
    current: &crate::config::AppConfig,
    patch: &serde_json::Value,
) -> Result<crate::config::AppConfig, String> {
    let mut updated = current.clone();
    let raw_patch = if let Some(sub) = patch.get("gaming") {
        sub
    } else {
        patch
    };

    let patch_obj = raw_patch.as_object().ok_or("Ожидается JSON-объект настроек игрового режима")?;
    let mut base_val = serde_json::to_value(&current.gaming).map_err(|e| e.to_string())?;
    let base_obj = base_val.as_object_mut().ok_or("internal")?;

    for (k, v) in patch_obj {
        // ignored_game_conns не меняются через патч формы
        if k == "ignored_game_conns" {
            continue;
        }
        base_obj.insert(k.clone(), v.clone());
    }

    let merged_gaming: config::GamingConfig = serde_json::from_value(base_val)
        .map_err(|e| format!("Некорректные настройки игрового режима: {e}"))?;

    // Сохраняем исходные ignored_game_conns и gaming_suspended
    updated.gaming = merged_gaming;
    updated.gaming.ignored_game_conns = current.gaming.ignored_game_conns.clone();
    // gaming_suspended хранится в whitelist.gaming_suspended
    updated.whitelist.gaming_suspended = current.whitelist.gaming_suspended;

    Ok(updated)
}

/// POST /api/gaming/save — сохранение настроек игрового режима и применение маршрутов
pub async fn save_gaming_config(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    let updated = match merge_gaming_patch(tx.config(), &body) {
        Ok(u) => u,
        Err(e) => return api_err(e),
    };

    *tx.config_mut() = updated;

    let conflicts = config::device_conflicts(tx.config());
    if !conflicts.is_empty() {
        return api_err_bad_request(config::format_device_conflicts(&conflicts));
    }

    if let Err(e) = apply_and_verify_gaming(&mut tx).await {
        return api_err(e);
    }

    // Синхронизируем ipset geo_override ядра Keenetic: очищаем от старых доменов при выключении/переключении
    let _ = crate::override_sync::sync_from_config(tx.config(), &[]).await;

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    log_i!("Настройки игрового режима успешно сохранены и применены");
    api_ok(json!({ "saved": true, "config": tx.config().gaming }))
}

#[derive(Deserialize)]
pub struct ToggleGamingReq {
    pub enabled: bool,
    #[serde(default)]
    pub mode: Option<config::GamingMode>,
    #[serde(default)]
    pub device_mac: Option<String>,
    #[serde(default)]
    pub device_ip: Option<String>,
    #[serde(default)]
    pub device_name: Option<String>,
    #[serde(default)]
    pub target_server: Option<String>,
}

/// Разрешение IP игрового устройства при включении (баг 135):
/// 1) непустой существующий IP -> он же;
/// 2) иначе ARP-таблица;
/// 3) иначе RCI-поиск;
/// 4) иначе Err с понятным текстом.
pub fn resolve_device_ip_for_enable(
    existing_ip: Option<&str>,
    arp_lookup: Option<&str>,
    rci_lookup: Option<&str>,
) -> Result<String, String> {
    for candidate in [existing_ip, arp_lookup, rci_lookup].into_iter().flatten() {
        let trimmed = candidate.trim();
        if !trimmed.is_empty() && config::is_valid_ip_or_cidr(trimmed) {
            return Ok(trimmed.to_string());
        }
    }
    Err("Не удалось определить IP устройства — игровой режим по MAC без IP не работает. Подключите устройство к сети и повторите".to_string())
}

/// Добавление/обновление игрового устройства по MAC (баг 135): включённое устройство обязано иметь
/// валидный IP — без него правила Mihomo и iptables игрового режима ничего не перехватывают.
pub fn upsert_gaming_device(
    devices: &mut Vec<config::GamingDevice>,
    mac: &str,
    ip: Option<&str>,
    name: Option<&str>,
    enabled: bool,
) -> Result<(), String> {
    let mac = mac.trim().to_lowercase();
    let ip = ip.map(str::trim).filter(|s| !s.is_empty());
    if let Some(ip) = ip {
        if !config::is_valid_ip_or_cidr(ip) {
            return Err(format!("Некорректный IP-адрес устройства: {ip}"));
        }
    }
    let mut found = false;
    for d in devices.iter_mut() {
        if d.mac.eq_ignore_ascii_case(&mac) {
            if enabled {
                if let Some(new_ip) = ip {
                    d.ip = new_ip.to_string();
                } else {
                    let arp_data = std::fs::read_to_string("/proc/net/arp").ok();
                    let arp_ip = arp_data
                        .as_deref()
                        .and_then(|data| crate::routing::find_ip_for_mac_in_arp(&mac, data));
                    let resolved = resolve_device_ip_for_enable(
                        Some(&d.ip),
                        arp_ip.as_deref(),
                        None,
                    )?;
                    d.ip = resolved;
                }
            } else if let Some(new_ip) = ip {
                d.ip = new_ip.to_string();
            }
            d.enabled = enabled;
            found = true;
        }
    }
    if !found && enabled {
        let Some(ip) = ip else {
            return Err("Не удалось определить IP устройства — игровой режим по MAC без IP не работает. Подключите устройство к сети и повторите".to_string());
        };
        devices.push(config::GamingDevice {
            mac,
            ip: ip.to_string(),
            ipv6: Vec::new(),
            name: name.map(str::trim).filter(|n| !n.is_empty()).unwrap_or("Игровое устройство").to_string(),
            enabled: true,
            server: None,
        });
    }
    Ok(())
}

/// POST /api/gaming/toggle — быстрое включение/отключение игрового режима с выбором устройства
pub async fn toggle_gaming(
    State(state): State<AppState>,
    Json(body): Json<ToggleGamingReq>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };
    if let Some(mode) = body.mode {
        tx.config_mut().gaming.mode = mode;
    }
    if let Some(target) = body.target_server {
        if !target.trim().is_empty() {
            tx.config_mut().gaming.target_server = target;
        }
    }
    if let Some(mac) = body.device_mac {
        let mac_trimmed = mac.trim().to_lowercase();
        if !mac_trimmed.is_empty() {
            // Если UI не прислал IP — ищем его по MAC в списке устройств роутера
            let mut ip = body.device_ip.clone().filter(|s| !s.trim().is_empty());
            let mut name = body.device_name.clone();
            if ip.is_none() && body.enabled {
                let cfg_snap = tx.config().clone();
                let policies = rci::get_policies(&state.http, &cfg_snap).await.unwrap_or_default();
                if let Ok(devs) = rci::get_devices(&state.http, &cfg_snap, &policies, "").await {
                    if let Some(found) = devs.iter().find(|d| d.mac.eq_ignore_ascii_case(&mac_trimmed)) {
                        if !found.ip.trim().is_empty() {
                            ip = Some(found.ip.clone());
                        }
                        if name.is_none() {
                            name = Some(found.name.clone());
                        }
                    }
                }
            }
            if let Err(e) = upsert_gaming_device(
                &mut tx.config_mut().gaming.devices,
                &mac_trimmed,
                ip.as_deref(),
                name.as_deref(),
                body.enabled,
            ) {
                return api_err(e);
            }
            let has_active = tx.config().gaming.devices.iter().any(|d| d.enabled);
            tx.config_mut().gaming.enabled = has_active;
        }
    } else {
        tx.config_mut().gaming.enabled = body.enabled;
    }

    let conflicts = config::device_conflicts(tx.config());
    if !conflicts.is_empty() {
        return api_err_bad_request(config::format_device_conflicts(&conflicts));
    }

    if let Err(e) = apply_and_verify_gaming(&mut tx).await {
        return api_err(e);
    }

    // Синхронизируем ipset geo_override ядра Keenetic: очищаем от старых доменов при выключении/переключении
    let _ = crate::override_sync::sync_from_config(tx.config(), &[]).await;

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }
    log_i!("Игровой режим {}", if body.enabled { "включен" } else { "отключен" });
    api_ok(json!({ "enabled": body.enabled, "saved": true }))
}

/// POST /api/gaming/ping — замер реальной сетевой задержки (TCP 443) до популярных игровых серверов
pub async fn ping_gaming_targets(State(_state): State<AppState>) -> Response {
    let targets = [
        ("Steam", "steamcommunity.com"),
        ("Discord", "discord.com"),
        ("Xbox Live", "user.auth.xboxlive.com"),
        ("PlayStation", "playstation.com"),
        ("Brawl Stars", "brawlstars.com"),
        ("Battle.net", "battle.net"),
        ("EA App", "ea.com"),
    ];

    let mut set = tokio::task::JoinSet::new();
    for (name, host) in targets {
        let h = host.to_string();
        let n = name.to_string();
        set.spawn(async move {
            let start = tokio::time::Instant::now();
            let addr = format!("{h}:443");
            let connect_fut = tokio::net::TcpStream::connect(&addr);
            let res = tokio::time::timeout(std::time::Duration::from_millis(2500), connect_fut).await;
            let elapsed_ms = start.elapsed().as_millis() as u64;
            let success = matches!(res, Ok(Ok(_)));
            (n, h, elapsed_ms, success)
        });
    }

    let mut results = Vec::new();
    while let Some(res) = set.join_next().await {
        if let Ok((name, host, ping_ms, ok)) = res {
            results.push(json!({
                "name": name,
                "host": host,
                "ping_ms": ping_ms,
                "available": ok,
            }));
        }
    }

    results.sort_by(|a, b| {
        a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
    });

    api_ok(json!({ "results": results }))
}


// =====================================================================
// Whitelist Mode (Белые списки) API
// =====================================================================

#[derive(Serialize)]
pub struct WhitelistStatusResponse {
    pub enabled: bool,
    pub auto_by_failover: bool,
    pub strategy: config::WhitelistStrategy,
    pub target_server: Option<String>,
    pub follow_failover: bool,
    pub excluded_devices: Vec<String>,
    pub devices: Vec<config::WhitelistDevice>,
    pub gaming_suspended: bool,
    pub custom_domains: Vec<String>,
    pub domestic_catalog_count: usize,
    pub active_server: String,
    pub is_bs_server: bool,
    pub whitelist_servers: Vec<String>,
    pub normal_servers: Vec<String>,
    pub whitelist_providers: Vec<String>,
}

/// GET /api/whitelist — статус и конфигурация режима белых списков
pub async fn get_whitelist(State(state): State<AppState>) -> Response {
    let cfg = (**state.config.read().await).clone();
    let domestic_catalog_count = routing::DOMESTIC_WHITELIST_DOMAINS.len();

    let proxies_map = mihomo::get_proxies(&state.http, &cfg).await.ok();
    let group_now = proxies_map.as_ref().and_then(|map| {
        map.get(routing::WHITELIST_GROUP_NAME)
            .or_else(|| map.get("PROXY"))
            .and_then(|g| g.get("now"))
            .and_then(|n| n.as_str())
    }).unwrap_or("Fastest");

    let fallback_active = proxies_map.as_ref()
        .map(|map| resolve_group_leaf(map, group_now))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| group_now.to_string());

    let (active_server, is_bs_server) = if let Ok(servers) = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await {
        if let Some(s) = servers.iter().find(|srv| srv.is_active && !srv.is_pool) {
            let is_bs = failover::is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);
            (s.name.clone(), is_bs)
        } else {
            let is_bs = failover::is_whitelist_server_full(&fallback_active, &fallback_active, None, &cfg);
            (fallback_active, is_bs)
        }
    } else {
        let is_bs = failover::is_whitelist_server_full(&fallback_active, &fallback_active, None, &cfg);
        (fallback_active, is_bs)
    };

    api_ok(json!(WhitelistStatusResponse {
        enabled: cfg.whitelist.enabled,
        auto_by_failover: cfg.whitelist.auto_by_failover,
        strategy: cfg.whitelist.strategy,
        target_server: cfg.whitelist.target_server,
        follow_failover: cfg.whitelist.follow_failover,
        excluded_devices: cfg.whitelist.excluded_devices,
        devices: cfg.whitelist.devices,
        gaming_suspended: cfg.whitelist.gaming_suspended,
        custom_domains: cfg.whitelist.custom_domains,
        domestic_catalog_count,
        active_server,
        is_bs_server,
        whitelist_servers: cfg.whitelist_servers,
        normal_servers: cfg.normal_servers,
        whitelist_providers: cfg.whitelist_providers,
    }))
}

#[derive(Deserialize)]
pub struct WhitelistStrategyReq {
    pub strategy: config::WhitelistStrategy,
}

/// POST /api/whitelist/strategy — переключить стратегию работы сервера белых списков
pub async fn set_whitelist_strategy(
    State(state): State<AppState>,
    Json(body): Json<WhitelistStrategyReq>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    tx.config_mut().whitelist.strategy = body.strategy;

    let conflicts = config::device_conflicts(tx.config());
    if !conflicts.is_empty() {
        return api_err_bad_request(config::format_device_conflicts(&conflicts));
    }

    let raw_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка генерации правил маршрутизации: {e}")),
    };

    if let Err(e) = tx.set_yaml(&new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.apply_and_reload().await {
        return api_err(e);
    }

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    // BS-FIX: смена стратегии включает/выключает полный перехват XKEEN_BS немедленно
    {
        let yaml = tokio::fs::read_to_string(&tx.config().mihomo.config_path).await.unwrap_or_default();
        let ports = routing::redir_ports(&yaml);
        let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
        let dns_port = resolve_active_dns_port(tx.config());
        let _ = sync_full_tunnel_modes(tx.config(), redir_port, tproxy_port, dns_port).await;
    }

    log_i!("Стратегия белых списков переключена на: {:?}", body.strategy);
    api_ok(json!({ "strategy": body.strategy }))
}

#[derive(Deserialize)]
pub struct WhitelistExcludeReq {
    pub id: String, // IP or MAC
    #[serde(default)]
    pub ip: Option<String>,
    pub excluded: bool,
}

/// POST /api/whitelist/exclude — добавить или удалить устройство из списка исключений
pub async fn toggle_whitelist_exclude(
    State(state): State<AppState>,
    Json(body): Json<WhitelistExcludeReq>,
) -> Response {
    let id_clean = body.id.trim().to_string();
    if id_clean.is_empty() {
        return api_err("Не указан идентификатор устройства (IP или MAC)");
    }
    let ip_clean = body.ip.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty() && *s != id_clean);

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    if body.excluded {
        if !tx.config().whitelist.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(&id_clean)) {
            tx.config_mut().whitelist.excluded_devices.push(id_clean.clone());
        }
        if let Some(ip) = ip_clean {
            if !tx.config().whitelist.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(ip)) {
                tx.config_mut().whitelist.excluded_devices.push(ip.to_string());
            }
            if !crate::config::is_valid_ip_or_cidr(&id_clean) && crate::config::is_valid_ip_or_cidr(ip) {
                tx.config_mut().whitelist.excluded_last_ips.insert(id_clean.to_lowercase(), ip.to_string());
            }
        }
        if let Ok(arp) = tokio::fs::read_to_string("/proc/net/arp").await {
            crate::routing::refresh_excluded_whitelist_last_ips(tx.config_mut(), &arp);
        }
    } else {
        tx.config_mut().whitelist.excluded_devices.retain(|d| !d.eq_ignore_ascii_case(&id_clean));
        tx.config_mut().whitelist.excluded_last_ips.retain(|k, _| !k.eq_ignore_ascii_case(&id_clean));
        if let Some(ip) = ip_clean {
            tx.config_mut().whitelist.excluded_devices.retain(|d| !d.eq_ignore_ascii_case(ip));
        }
    }

    let conflicts = config::device_conflicts(tx.config());
    if !conflicts.is_empty() {
        return api_err_bad_request(config::format_device_conflicts(&conflicts));
    }

    let raw_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка генерации правил маршрутизации: {e}")),
    };

    if let Err(e) = tx.set_yaml(&new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.apply_and_reload().await {
        return api_err(e);
    }

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    // BS-FIX: список исключений изменил состав перехвата — синхронизируем iptables сразу
    {
        let yaml = tokio::fs::read_to_string(&tx.config().mihomo.config_path).await.unwrap_or_default();
        let ports = routing::redir_ports(&yaml);
        let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
        let dns_port = resolve_active_dns_port(tx.config());
        let _ = sync_full_tunnel_modes(tx.config(), redir_port, tproxy_port, dns_port).await;
    }

    log_i!("Статус исключения устройства {} из белых списков: {}", id_clean, body.excluded);
    api_ok(json!({ "excluded_devices": tx.config().whitelist.excluded_devices }))
}

#[derive(Deserialize, Clone, Debug)]
pub struct WhitelistDevicePatch {
    pub mac: String,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
}

/// Точечное обновление одного устройства БС по MAC без затирания IP остальных устройств (баг 67).
pub fn patch_whitelist_device(
    devices: &mut Vec<config::WhitelistDevice>,
    patch: &WhitelistDevicePatch,
) -> Result<(), String> {
    let mac = patch.mac.trim();
    if mac.is_empty() || !config::is_valid_mac(mac) {
        return Err(format!("Некорректный MAC-адрес устройства: {mac}"));
    }
    if let Some(ref ip) = patch.ip {
        let ip_t = ip.trim();
        if !ip_t.is_empty() && !config::is_valid_ip_or_cidr(ip_t) {
            return Err(format!("Некорректный IP-адрес устройства: {ip_t}"));
        }
    }

    if let Some(existing) = devices.iter_mut().find(|d| d.mac.eq_ignore_ascii_case(mac)) {
        if let Some(enabled) = patch.enabled {
            existing.enabled = enabled;
        }
        if let Some(ref name) = patch.name {
            existing.name = Some(name.clone());
        }
        if let Some(ref srv) = patch.server {
            let s = srv.trim();
            existing.server = if s.is_empty() { None } else { Some(s.to_string()) };
        }
        // Обновляем IP только если у существующего устройства IP пуст (иначе сохраняем актуальный DHCP IP)
        if existing.ip.trim().is_empty() {
            if let Some(ref ip) = patch.ip {
                existing.ip = ip.trim().to_string();
            }
        }
    } else {
        let srv_opt = patch
            .server
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let enabled = patch.enabled.unwrap_or(true);
        if enabled || srv_opt.is_some() {
            devices.push(config::WhitelistDevice {
                mac: mac.to_string(),
                ip: patch.ip.as_deref().map(str::trim).unwrap_or("").to_string(),
                enabled,
                name: patch.name.clone(),
                server: srv_opt,
            });
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct WhitelistUpdateReq {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub auto_by_failover: Option<bool>,
    #[serde(default)]
    pub strategy: Option<config::WhitelistStrategy>,
    #[serde(default)]
    pub target_server: Option<String>,
    #[serde(default)]
    pub follow_failover: Option<bool>,
    #[serde(default)]
    pub excluded_devices: Option<Vec<String>>,
    #[serde(default)]
    pub devices: Option<Vec<config::WhitelistDevice>>,
    #[serde(default)]
    pub device_patch: Option<WhitelistDevicePatch>,
    #[serde(default)]
    pub custom_domains: Option<Vec<String>>,
}

/// POST /api/whitelist — полное сохранение настроек режима белых списков
/// Применить к `WhitelistConfig` только явно переданные поля патча (Bug 66).
/// Сетевая проверка имён узлов Mihomo выполняется отдельно в `save_whitelist`.
pub fn merge_whitelist_patch(
    wl: &mut config::WhitelistConfig,
    body: &WhitelistUpdateReq,
) -> Result<(), String> {
    if let Some(enabled) = body.enabled {
        wl.enabled = enabled;
        if enabled {
            wl.enabled_by = config::WhitelistEnabledBy::Manual;
        }
    }
    if let Some(auto_by_failover) = body.auto_by_failover {
        wl.auto_by_failover = auto_by_failover;
    }
    if let Some(strategy) = body.strategy {
        wl.strategy = strategy;
    }
    if let Some(ref target) = body.target_server {
        let t = target.trim();
        if t.is_empty() || t == "follow_failover" {
            wl.target_server = None;
            wl.follow_failover = true;
        } else {
            wl.target_server = Some(t.to_string());
        }
    }
    if let Some(follow) = body.follow_failover {
        wl.follow_failover = follow;
    }
    if let Some(ref excluded) = body.excluded_devices {
        wl.excluded_devices = excluded.clone();
    }
    if let Some(ref custom_domains) = body.custom_domains {
        let mut clean_domains = Vec::new();
        for d in custom_domains {
            let clean = config::normalize_domain(d);
            if clean.is_empty() {
                continue;
            }
            if !config::is_valid_domain(&clean) {
                return Err(format!("Некорректный домен в белом списке: {d}"));
            }
            if !clean_domains.contains(&clean) {
                clean_domains.push(clean);
            }
        }
        wl.custom_domains = clean_domains;
    }
    Ok(())
}

pub async fn save_whitelist(
    State(state): State<AppState>,
    Json(body): Json<WhitelistUpdateReq>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    if let Err(e) = merge_whitelist_patch(&mut tx.config_mut().whitelist, &body) {
        return api_err(e);
    }
    if let Some(devices) = body.devices {
        for dev in &devices {
            let ip = dev.ip.trim();
            if !ip.is_empty() && !config::is_valid_ip_or_cidr(ip) {
                return api_err(format!("Некорректный IP-адрес устройства: {ip}"));
            }
            let mac = dev.mac.trim();
            if !mac.is_empty() && !config::is_valid_mac(mac) {
                return api_err(format!("Некорректный MAC-адрес устройства: {mac}"));
            }
        }
        // BS-PIN: закреплённая нода недоступна правилам mihomo напрямую (нода живёт
        // в proxy-providers), поэтому пин направляет устройство в его per-device
        // select-группу. Зеркалим закрепление в device_routing — apply_routing
        // создаст группу {name} {ip_underscored}, а ниже выбираем в ней ноду runtime.
        // Валидация: имя ноды должно существовать среди прокси/провайдеров.
        let known: std::collections::HashSet<String> = match mihomo::get_servers(&state.http, tx.config(), &tx.config().failover.priority_chain).await {
            Ok(servers) => servers.iter().map(|s| s.id.clone()).collect(),
            Err(_) => std::collections::HashSet::new(),
        };
        let mut known_providers: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Ok(provs) = mihomo::get_provider_proxies(&state.http, tx.config()).await {
            for (name, _) in provs.iter() {
                known_providers.insert(name.clone());
            }
        }
        for dev in &devices {
            let srv = dev.server.as_deref().map(str::trim).unwrap_or("");
            if srv.is_empty() {
                continue;
            }
            if !known.contains(srv) && !known_providers.contains(srv) {
                return api_err(format!("Неизвестный узел для закрепления устройства: {srv}"));
            }
        }
        let old_devices = tx.config().whitelist.devices.clone();
        let _ = old_devices;
        tx.config_mut().whitelist.devices = devices;
    }
    if let Some(patch) = body.device_patch {
        let srv = patch.server.as_deref().map(str::trim).unwrap_or("");
        if !srv.is_empty() {
            let known: std::collections::HashSet<String> = match mihomo::get_servers(&state.http, tx.config(), &tx.config().failover.priority_chain).await {
                Ok(servers) => servers.iter().map(|s| s.id.clone()).collect(),
                Err(_) => std::collections::HashSet::new(),
            };
            let mut known_providers: std::collections::HashSet<String> = std::collections::HashSet::new();
            if let Ok(provs) = mihomo::get_provider_proxies(&state.http, tx.config()).await {
                for (name, _) in provs.iter() {
                    known_providers.insert(name.clone());
                }
            }
            if !known.contains(srv) && !known_providers.contains(srv) {
                return api_err(format!("Неизвестный узел для закрепления устройства: {srv}"));
            }
        }
        if let Err(e) = patch_whitelist_device(&mut tx.config_mut().whitelist.devices, &patch) {
            return api_err(e);
        }
    }

    let mut raw_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    // BS-PIN: per-device select-группы для закреплённых устройств создаются ДО
    // генерации правил БС — правило SRC-IP-CIDR ссылается на группу по имени
    // (group_name_for), а провайдерные ноды правилам напрямую недоступны.
    // server="Fastest" только гарантирует существование группы; выбор ноды в ней —
    // runtime (switch_group ниже), Failover этот выбор не трогает.
    {
        let pin_assignments: Vec<routing::Assignment> = tx
            .config()
            .whitelist
            .devices
            .iter()
            .filter(|d| {
                d.enabled
                    && !d.ip.trim().is_empty()
                    && !d.server.as_deref().map(str::trim).unwrap_or("").is_empty()
            })
            .map(|d| routing::Assignment {
                ip: d.ip.trim().to_string(),
                name: d.name.clone().unwrap_or_default(),
                server: Some("Fastest".to_string()),
            })
            .collect();
        if !pin_assignments.is_empty() {
            let providers = device_providers_for(tx.config(), &raw_yaml);
            let pinned = routing::pinned_whitelist_ips(tx.config());
            match routing::apply_assignments(&raw_yaml, &pin_assignments, &providers, &pinned) {
                Ok(y) => raw_yaml = y,
                Err(e) => return api_err(format!("Ошибка создания группы закрепления устройства: {e}")),
            }
        }
    }

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка генерации правил маршрутизации: {e}")),
    };

    if let Err(e) = tx.set_yaml(&new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.apply_and_reload().await {
        return api_err(e);
    }

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    sync_whitelist_runtime_after_commit(&state, tx.config()).await;

    // BS-PIN: выбираем закреплённую ноду в per-device группе (runtime, вне Failover)
    // Баг 40: используем switch_group_scoped, чтобы не рвать соединения всей сети
    for dev in &tx.config().whitelist.devices {
        let srv = dev.server.as_deref().map(str::trim).unwrap_or("");
        let ip = dev.ip.trim();
        if !srv.is_empty() && !ip.is_empty() {
            let gname = routing::group_name_for(ip, dev.name.as_deref().unwrap_or(""));
            if let Err(e) = mihomo::switch_group_scoped(&state.http, tx.config(), &gname, srv).await {
                log_w!("[BS-PIN] Не удалось выбрать ноду '{}' в группе '{}': {}", srv, gname, e);
            } else {
                log_i!("[BS-PIN] Устройство {} ({}) закреплено за нодой '{}' (группа '{}')", dev.name.as_deref().unwrap_or("?"), ip, srv, gname);
            }
        }
    }

    log_i!("Настройки режима белых списков успешно сохранены и применены");
    api_ok(json!({ "saved": true, "whitelist": tx.config().whitelist }))
}

#[derive(Deserialize)]
pub struct WhitelistDeviceToggleReq {
    pub ip: Option<String>,
    pub mac: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub name: Option<String>,
}

/// POST /api/whitelist/device/toggle — быстрое переключение устройства в режиме белых списков
/// Синхронизация рантайма БС после коммита конфигурации (баг 38): туннель БС для активного сервера
/// и полный перехват iptables XKEEN_BS. Общая для save_whitelist и toggle_whitelist_device.
pub async fn sync_whitelist_runtime_after_commit(state: &AppState, cfg: &config::AppConfig) {
    if cfg.whitelist.enabled {
        let active_id = mihomo::get_active_server(&state.http, cfg).await.ok().flatten().map(|s| s.id).unwrap_or_default();
        if !active_id.is_empty() {
            let _ = sync_whitelist_mode_tunnel(state, &active_id).await;
        }
    }
    // BS-FIX: полный перехват всех портов (iptables XKEEN_BS) применяется/снимается сразу,
    // а не только после перезапуска панели
    let yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let ports = routing::redir_ports(&yaml);
    let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
    let dns_port = resolve_active_dns_port(cfg);
    sync_full_tunnel_modes(cfg, redir_port, tproxy_port, dns_port).await;
}

/// Действие над глобальным статусом БС после переключения устройства (баг 38).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhitelistModeChange {
    /// Переключить в режим «только указанные устройства»
    PerDeviceList,
    /// Полностью выключить БС (так как выключено последнее устройство, активировавшее режим)
    Disable,
    /// Не менять текущий статус (уже включен вручную, или остались другие устройства)
    Keep,
}

/// Чистая функция расчета изменения режима БС при переключении устройства (баг 38).
pub fn whitelist_mode_after_device_toggle(
    cfg: &config::AppConfig,
    target_dev: &config::WhitelistDevice,
    next_enabled: bool,
) -> WhitelistModeChange {
    if next_enabled {
        if !cfg.whitelist.enabled {
            WhitelistModeChange::PerDeviceList
        } else {
            WhitelistModeChange::Keep
        }
    } else {
        // Выключение устройства
        if !cfg.whitelist.enabled {
            return WhitelistModeChange::Keep;
        }
        if cfg.whitelist.enabled_by == config::WhitelistEnabledBy::Devices {
            // Проверяем, останутся ли ещё активные устройства помимо target_dev
            let has_other_enabled = cfg.whitelist.devices.iter().any(|d| {
                let same_mac = !d.mac.is_empty() && !target_dev.mac.is_empty() && d.mac.eq_ignore_ascii_case(&target_dev.mac);
                let same_ip = !d.ip.is_empty() && !target_dev.ip.is_empty() && d.ip == target_dev.ip;
                let is_target = same_mac || same_ip;
                !is_target && d.enabled
            });
            if !has_other_enabled {
                WhitelistModeChange::Disable
            } else {
                WhitelistModeChange::Keep
            }
        } else {
            // Если включен Manual или Failover — выключение устройства не трогает глобальный статус БС
            WhitelistModeChange::Keep
        }
    }
}

pub async fn toggle_whitelist_device(
    State(state): State<AppState>,
    Json(body): Json<WhitelistDeviceToggleReq>,
) -> Response {
    let mut ip_str = body.ip.as_deref().unwrap_or("").trim().to_string();
    let mut mac_str = body.mac.as_deref().unwrap_or("").trim().to_string();
    let mut name_opt = body.name.clone();

    if ip_str.is_empty() && mac_str.is_empty() {
        return api_err("Не указан IP или MAC адрес устройства");
    }

    let cfg_snap = state.config.read().await.clone();
    if ip_str.is_empty() || mac_str.is_empty() {
        let policies = rci::get_policies(&state.http, &cfg_snap).await.unwrap_or_default();
        if let Ok(devs) = rci::get_devices(&state.http, &cfg_snap, &policies, "").await {
            if let Some(found) = devs.iter().find(|d| {
                (!mac_str.is_empty() && d.mac.eq_ignore_ascii_case(&mac_str))
                    || (!ip_str.is_empty() && d.ip == ip_str)
            }) {
                if ip_str.is_empty() { ip_str = found.ip.clone(); }
                if mac_str.is_empty() { mac_str = found.mac.clone(); }
                if name_opt.is_none() || name_opt.as_deref() == Some("") {
                    name_opt = Some(found.name.clone());
                }
            }
        }
    }

    if !ip_str.is_empty() && !config::is_valid_ip_or_cidr(&ip_str) {
        return api_err("Некорректный IP-адрес устройства");
    }
    if !mac_str.is_empty() && !config::is_valid_mac(&mac_str) {
        return api_err("Некорректный MAC-адрес устройства");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    // Баг 43: устройство идентифицируется строго по MAC-адресу, если он передан.
    // Если MAC передан, ищем ТОЛЬКО по MAC, обновляя IP. Если MAC не передан, ищем по IP,
    // но никогда не перезаписываем чужой MAC при DHCP-смене адресов.
    let mut matched = false;
    for dev in &mut tx.config_mut().whitelist.devices {
        let is_target = if !mac_str.is_empty() {
            dev.mac.eq_ignore_ascii_case(&mac_str)
        } else if !ip_str.is_empty() {
            dev.ip == ip_str
        } else {
            false
        };

        if is_target {
            dev.enabled = body.enabled;
            if !ip_str.is_empty() { dev.ip = ip_str.clone(); }
            if dev.mac.is_empty() && !mac_str.is_empty() { dev.mac = mac_str.clone(); }
            if let Some(ref n) = name_opt {
                if !n.trim().is_empty() { dev.name = Some(n.clone()); }
            }
            matched = true;
            break;
        }
    }

    if !matched && body.enabled {
        tx.config_mut().whitelist.devices.push(config::WhitelistDevice {
            mac: mac_str.clone(),
            ip: ip_str.clone(),
            enabled: true,
            name: name_opt.clone(),
            server: None,
        });
    }

    let target_dev_probe = config::WhitelistDevice {
        mac: mac_str.clone(),
        ip: ip_str.clone(),
        enabled: body.enabled,
        name: name_opt.clone(),
        server: None,
    };
    let mode_change = whitelist_mode_after_device_toggle(tx.config(), &target_dev_probe, body.enabled);
    match mode_change {
        WhitelistModeChange::PerDeviceList => {
            tx.config_mut().whitelist.enabled = true;
            tx.config_mut().whitelist.enabled_by = config::WhitelistEnabledBy::Devices;
        }
        WhitelistModeChange::Disable => {
            tx.config_mut().whitelist.enabled = false;
        }
        WhitelistModeChange::Keep => {}
    }

    let conflicts = config::device_conflicts(tx.config());
    if !conflicts.is_empty() {
        return api_err_bad_request(config::format_device_conflicts(&conflicts));
    }

    let raw_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка генерации правил маршрутизации: {e}")),
    };

    if let Err(e) = tx.set_yaml(&new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.apply_and_reload().await {
        return api_err(e);
    }

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    // Баг 38: та же синхронизация туннеля БС и iptables XKEEN_BS, что и при сохранении настроек БС
    sync_whitelist_runtime_after_commit(&state, tx.config()).await;

    log_i!(
        "Режим белых списков переключен для устройства ip='{}', mac='{}' -> enabled={}",
        ip_str, mac_str, body.enabled
    );
    api_ok(json!({
        "toggled": true,
        "ip": ip_str,
        "mac": mac_str,
        "enabled": body.enabled,
        "whitelist_enabled": tx.config().whitelist.enabled,
    }))
}

/// GET /api/whitelist/catalog — отечественный каталог из 376 базовых сервисов с категориями
pub async fn get_whitelist_catalog() -> Response {
    let mut categories_map: std::collections::BTreeMap<&'static str, Vec<&'static str>> = std::collections::BTreeMap::new();
    for entry in routing::DOMESTIC_CATALOG {
        categories_map.entry(entry.category).or_default().push(entry.domain);
    }

    api_ok(json!({
        "total": routing::DOMESTIC_WHITELIST_DOMAINS.len(),
        "domains": routing::DOMESTIC_WHITELIST_DOMAINS,
        "categories": categories_map,
        "items": routing::DOMESTIC_CATALOG,
    }))
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regression_bug_121b_rci_save_error_is_reported() {
        let err = finish_rci_apply(1, vec![], Some(Err("flash read-only".to_string())))
            .expect_err("rci::save_config failure must return Err instead of silent Ok");
        assert!(err.contains("flash read-only"));

        let ok = finish_rci_apply(2, vec![], Some(Ok(()))).expect("Ok save must succeed");
        assert_eq!(ok["applied"], 2);
        assert_eq!(ok["saved"], true);

        let none_ok = finish_rci_apply(0, vec!["mac1: timeout".to_string()], None)
            .expect_err("zero applied must return Err");
        assert!(none_ok.contains("timeout"));
    }
}
