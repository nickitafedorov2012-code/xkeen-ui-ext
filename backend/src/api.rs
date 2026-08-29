use axum::extract::State;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;

use crate::{config, failover, mihomo, rci, routing, AppState, VERSION};

/// GET /api/status — сводка: панель + роутер + активный сервер Mihomo.
pub async fn status(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();

    let router = rci::get_version(&state.http, &cfg).await.ok();
    let active = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_server)
        .await
        .ok()
        .and_then(|servers| servers.into_iter().find(|s| s.is_active))
        .map(|s| json!({ "id": s.id, "name": s.name, "ping_ms": s.ping_ms }));

    api_ok(json!({
        "version": VERSION,
        "config_path": state.config_path.display().to_string(),
        "router": router,
        "active_server": active,
        "mihomo": { "host": cfg.mihomo.host, "port": cfg.mihomo.port },
        "rci": { "host": cfg.rci.host, "port": cfg.rci.port },
        "failover": {
            "enabled": cfg.failover.enabled,
            "ping_threshold_ms": cfg.failover.ping_threshold_ms,
            "priority_server": cfg.failover.priority_server,
            "auto_restore_priority": cfg.failover.auto_restore_priority,
            "interval_secs": cfg.failover.interval_secs,
        },
        "refresh_interval_sec": cfg.refresh_interval_sec,
    }))
    .into_response()
}


/// Вспомогательное: единый формат ответа API.
pub fn api_ok(data: serde_json::Value) -> axum::response::Response {
    Json(json!({ "success": true, "data": data })).into_response()
}

pub fn api_err(error: impl Into<String>) -> axum::response::Response {
    Json(json!({ "success": false, "error": error.into() })).into_response()
}

/// Заготовка настроек: GET/PUT будут добавлены в следующих фазах.
pub async fn get_settings(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await.clone();
    api_ok(serde_json::to_value(cfg).unwrap_or_default())
}

pub async fn put_settings(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let mut merged = serde_json::to_value(state.config.read().await.clone()).unwrap_or_default();
    crate::config::merge_value(&mut merged, &body);
    let new_cfg: config::AppConfig = match serde_json::from_value(merged) {
        Ok(c) => c,
        Err(e) => return api_err(format!("Некорректные настройки: {}", e)),
    };
    if let Err(e) = config::save(&state.config_path, &new_cfg).await {
        return api_err(format!("Ошибка сохранения конфига: {}", e));
    }
    *state.config.write().await = new_cfg;
    api_ok(json!({ "saved": true }))
}

/// GET /api/servers — карточки серверов.
pub async fn get_servers(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_server).await {
        Ok(servers) => {
            let list: Vec<serde_json::Value> = servers
                .iter()
                .map(|s| {
                    json!({
                        "id": s.id, "name": s.name, "protocol": s.protocol,
                        "host": s.host, "port": s.port,
                        "is_active": s.is_active, "is_priority": s.is_priority,
                        "ping_ms": s.ping_ms,
                    })
                })
                .collect();
            api_ok(json!({ "servers": list }))
        }
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct SwitchReq {
    pub server_id: String,
}

/// POST /api/servers/switch
pub async fn switch_server(State(state): State<AppState>, Json(req): Json<SwitchReq>) -> Response {
    let cfg = state.config.read().await.clone();
    match mihomo::switch_server(&state.http, &cfg, &req.server_id).await {
        Ok(msg) => api_ok(json!({ "message": msg })),
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct PingReq {
    pub server_id: Option<String>,
}

/// POST /api/servers/ping — один сервер или все (параллельно).
pub async fn ping_servers(State(state): State<AppState>, Json(req): Json<PingReq>) -> Response {
    let cfg = state.config.read().await.clone();
    let ids: Vec<String> = match req.server_id {
        Some(id) => vec![id],
        None => match mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_server).await {
            Ok(servers) => servers.into_iter().map(|s| s.id).collect(),
            Err(e) => return api_err(e),
        },
    };
    let pings = mihomo::ping_all(&state.http, &cfg, &ids, 2000).await;
    api_ok(json!({ "pings": pings }))
}

/// GET /api/policies — политики доступа Keenetic.
pub async fn get_policies(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match rci::get_policies(&state.http, &cfg).await {
        Ok(list) => {
            let arr: Vec<serde_json::Value> = list
                .iter()
                .map(|p| json!({ "id": p.id, "name": p.name, "is_default": p.is_default }))
                .collect();
            api_ok(json!({ "policies": arr }))
        }
        Err(e) => api_err(e),
    }
}

/// POST /api/failover/check — ручной прогон проверки.
pub async fn failover_check(State(state): State<AppState>) -> Response {
    match failover::run_check(&state).await {
        Ok(msg) => api_ok(json!({ "message": msg })),
        Err(e) => api_err(e),
    }
}

/// GET /api/failover/events — лента событий.
pub async fn failover_events(State(state): State<AppState>) -> Response {
    let events = state.failover_log.snapshot().await;
    api_ok(json!({ "events": events }))
}

#[derive(Deserialize)]
pub struct PriorityReq {
    pub server_id: String,
}

/// POST /api/settings/priority — назначить/снять приоритетный сервер.
pub async fn set_priority(State(state): State<AppState>, Json(req): Json<PriorityReq>) -> Response {
    let mut cfg = state.config.read().await.clone();
    cfg.failover.priority_server = req.server_id.trim().to_string();
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения: {e}"));
    }
    *state.config.write().await = cfg;
    api_ok(json!({ "saved": true }))
}


/// GET /api/devices — устройства с политиками.
pub async fn get_devices(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let policies = match rci::get_policies(&state.http, &cfg).await {
        Ok(p) => p,
        Err(e) => return api_err(e),
    };
    match rci::get_devices(&state.http, &cfg, &policies, "").await {
        Ok(devices) => {
            let live = mihomo::live_device_servers(&state.http, &cfg).await;
            let arr: Vec<serde_json::Value> = devices
                .iter()
                .map(|d| {
                    json!({
                        "mac": d.mac, "name": d.name, "ip": d.ip,
                        "policy": d.policy, "policy_name": d.policy_name,
                        "online": d.online, "interface": d.interface,
                        "is_current_device": d.is_current_device,
                        "rxbytes": d.rxbytes, "txbytes": d.txbytes,
                        "speed_limit_kbps": d.speed_limit_kbps,
                        "current_server": live.get(&d.ip).cloned().unwrap_or_default(),
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
    if ok > 0 {
        let _ = rci::save_config(&state.http, &cfg).await;
    }
    if ok == 0 {
        api_err(errors.join("; "))
    } else {
        api_ok(json!({ "applied": ok, "errors": errors }))
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
    if ok > 0 {
        let _ = rci::save_config(&state.http, &cfg).await;
    }
    if ok == 0 {
        api_err(errors.join("; "))
    } else {
        api_ok(json!({ "applied": ok, "errors": errors }))
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
    let assignments: Vec<serde_json::Value> = groups
        .iter()
        .map(|(ip, gname)| {
            json!({
                "ip": ip,
                "group": gname,
                "current_server": live.get(ip).cloned().unwrap_or_default(),
            })
        })
        .collect();
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

/// POST /api/routing — применить назначения (merge), reload Mihomo, перевыбор серверов.
pub async fn apply_routing(State(state): State<AppState>, Json(req): Json<RoutingReq>) -> Response {
    let cfg = state.config.read().await.clone();
    let _guard = state.routing_lock.lock().await;

    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Не удалось прочитать config.yaml: {e}")),
    };
    let assignments: Vec<routing::Assignment> = req
        .assignments
        .into_iter()
        .map(|a| routing::Assignment { ip: a.ip, name: a.name, server: a.server })
        .collect();
    let new_yaml = match routing::apply_assignments(&yaml, &assignments) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let tmp = format!("{}.tmp", cfg.mihomo.config_path);
    if let Err(e) = tokio::fs::write(&tmp, &new_yaml).await {
        return api_err(format!("Ошибка записи: {e}"));
    }
    if let Err(e) = tokio::fs::rename(&tmp, &cfg.mihomo.config_path).await {
        return api_err(format!("Ошибка переименования: {e}"));
    }

    // Reload Mihomo и перевыбор серверов в новых группах (порт логики десктопа)
    if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
        return api_err(format!("Конфиг записан, но reload Mihomo не удался: {e}"));
    }
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;
    let mut reselected = 0usize;
    for a in &assignments {
        if let Some(server) = &a.server {
            if server.trim().is_empty() || server.trim() == "default" {
                continue;
            }
            let gname = routing::group_name_for(&a.ip, &a.name);
            if mihomo::switch_group(&state.http, &cfg, &gname, server).await.is_ok() {
                reselected += 1;
            }
        }
    }
    api_ok(json!({ "applied": assignments.len(), "reselected": reselected }))
}



