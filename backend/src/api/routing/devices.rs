use axum::extract::{ConnectInfo, State};
use axum::response::{Json, Response};
use serde::Deserialize;
use serde_json::json;

use crate::api::{
    api_err, api_ok, resolve_active_dns_port, sync_full_tunnel_modes, sync_whitelist_mode_tunnel,
};
use crate::{config, mihomo, rci, routing, transaction::ConfigTx, AppState};
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
    if ok > 0 {
        crate::routing::sync_default_policy_isolation(&state).await;
    }
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
        for dev in &mut tx.config_mut().whitelist.devices {
            if dev.ip.trim() == ip {
                dev.server = None;
            }
        }
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
pub(super) fn device_providers_for(cfg: &config::AppConfig, yaml: &str) -> Vec<String> {
    if cfg.mihomo.device_providers.is_empty() {
        routing::parse_user_provider_names(yaml)
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
        if let Err(e) = sync_full_tunnel_modes(&cfg_guard, redir_port, tproxy_port, dns_port).await {
            return api_err(format!("Маршруты устройств сохранены, но не удалось применить iptables: {e}"));
        }
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

    // В Mihomo 'downloadTotal'/'download' — загрузка клиентом, 'uploadTotal'/'upload' — отдача клиентом (согласовано с gaming.rs, AUDIT-066).
    let download_total = resp_val.get("downloadTotal").and_then(|v| v.as_u64()).unwrap_or(0);
    let upload_total = resp_val.get("uploadTotal").and_then(|v| v.as_u64()).unwrap_or(0);

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

            let dl = conn.get("download").and_then(|v| v.as_u64()).unwrap_or(0);
            let ul = conn.get("upload").and_then(|v| v.as_u64()).unwrap_or(0);
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

fn is_valid_hhmm(s: &str) -> bool {
    let bytes = s.as_bytes();
    if bytes.len() != 5 || bytes[2] != b':' {
        return false;
    }
    let Ok(hh) = s[0..2].parse::<u8>() else {
        return false;
    };
    let Ok(mm) = s[3..5].parse::<u8>() else {
        return false;
    };
    hh <= 23 && mm <= 59
}

/// R38: Валидация доменной модели расписаний до транзакции.
pub fn validate_schedules(schedules: &[config::DeviceSchedule]) -> Result<(), String> {
    let mut seen_ids = std::collections::HashSet::new();
    for (idx, s) in schedules.iter().enumerate() {
        let id = s.id.trim();
        if id.is_empty() {
            return Err(format!("Расписание #{}: пустой id", idx + 1));
        }
        if !seen_ids.insert(id.to_string()) {
            return Err(format!("Повторяющийся id расписания: '{id}'"));
        }
        if !is_valid_hhmm(s.time_start.trim()) || !is_valid_hhmm(s.time_end.trim()) {
            return Err(format!(
                "Расписание '{id}': некорректное время (ожидается HH:MM 00:00–23:59)"
            ));
        }
        if s.days.is_empty() || s.days.iter().any(|&d| !(1..=7).contains(&d)) {
            return Err(format!(
                "Расписание '{id}': дни недели должны быть от 1 до 7 и список не должен быть пустым"
            ));
        }
        match s.action.as_str() {
            "block" | "direct" => {}
            "proxy" => {
                if s.target_server.as_deref().map(|v| v.trim().is_empty()).unwrap_or(true) {
                    return Err(format!(
                        "Расписание '{id}': для действия 'proxy' требуется target_server"
                    ));
                }
            }
            other => {
                return Err(format!("Расписание '{id}': неизвестное действие '{other}'"));
            }
        }
    }
    Ok(())
}

/// POST /api/schedules — сохранение расписаний
pub async fn save_schedules(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<SaveSchedulesReq>,
) -> Response {
    if let Err(e) = validate_schedules(&body.schedules) {
        return api_err(e);
    }
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    // UI-03: Optimistic concurrency check
    if let Some(exp) = &body.expected_revision {
        let cur_rev = calc_schedules_revision(&tx.config().schedules);
        if exp != &cur_rev {
            return api_err("Конфликт параллельного сохранения расписаний: данные были изменены другим клиентом");
        }
    }

    tx.config_mut().schedules = body.schedules;
    let new_rev = calc_schedules_revision(&tx.config().schedules);

    let yaml_exists = std::path::Path::new(&tx.config().mihomo.config_path).exists();
    if yaml_exists {
        let raw_yaml = match tx.read_yaml().await {
            Ok(y) => y,
            Err(e) => return api_err(format!("Ошибка чтения config.yaml: {e}")),
        };
        let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
            Ok(res) => res,
            Err(e) => return api_err(format!("Ошибка генерации правил расписаний: {e}")),
        };
        if let Err(e) = tx.set_yaml(new_yaml) {
            return api_err(format!("Ошибка валидации config.yaml: {e}"));
        }
        if let Err(e) = tx.commit_and_reload().await {
            return api_err(format!("Ошибка сохранения и перезагрузки расписаний: {e}"));
        }
    } else if let Err(e) = tx.commit_without_reload().await {
        return api_err(format!("Ошибка сохранения расписаний: {e}"));
    }

    api_ok(json!({ "saved": true, "revision": new_rev }))
}

// ==================== ИГРОВОЙ РЕЖИМ (GAMING MODE) =============

#[cfg(test)]
mod schedule_validation_tests {
    use super::validate_schedules;
    use crate::config::DeviceSchedule;

    fn sample_sched(id: &str) -> DeviceSchedule {
        DeviceSchedule {
            id: id.into(),
            ip: "192.168.1.50".into(),
            action: "block".into(),
            target_server: None,
            time_start: "23:00".into(),
            time_end: "07:00".into(),
            days: vec![1, 2, 3],
            enabled: true,
        }
    }

    #[test]
    fn regression_r38_validate_schedules_rejects_invalid_times_days_actions_and_dup_ids() {
        assert!(validate_schedules(&[sample_sched("ok1")]).is_ok());

        let mut bad_time = sample_sched("t1");
        bad_time.time_start = "25:90".into();
        assert!(validate_schedules(&[bad_time]).is_err());

        let mut bad_day = sample_sched("d1");
        bad_day.days = vec![8];
        assert!(validate_schedules(&[bad_day]).is_err());

        let mut bad_action = sample_sched("a1");
        bad_action.action = "unknown".into();
        assert!(validate_schedules(&[bad_action]).is_err());

        assert!(validate_schedules(&[sample_sched("dup"), sample_sched("dup")]).is_err());
    }
}

