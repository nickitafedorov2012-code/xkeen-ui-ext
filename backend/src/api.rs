use axum::extract::{Query, State};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;

use crate::{config, failover, log_e, log_i, mihomo, rci, routing, AppState, VERSION};

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
    crate::logger::set_level(&state.config.read().await.logs.level);
    log_i!("Настройки сохранены");
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
        Ok(msg) => {
            log_i!("Смена сервера: {} — {}", req.server_id, msg);
            api_ok(json!({ "message": msg }))
        }
        Err(e) => {
            log_e!("Ошибка смены сервера на {}: {}", req.server_id, e);
            api_err(e)
        }
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
    let _guard = state.routing_lock.lock().await; // как в остальных правках config.yaml
    let mut cfg = state.config.read().await.clone();
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
    cfg.ignore_servers = servers.clone();

    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Не удалось прочитать {}: {e}", cfg.mihomo.config_path)),
    };
    let new_yaml = match routing::apply_ignore_to_groups(&yaml, &servers) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    let mut saved = cfg.provider_filters.clone();
    let new_yaml = routing::apply_ignore_to_providers(&new_yaml, &servers, &mut saved);
    cfg.provider_filters = saved;
    let tmp = format!("{}.tmp", cfg.mihomo.config_path);
    if let Err(e) = tokio::fs::write(&tmp, &new_yaml).await {
        return api_err(format!("Ошибка записи: {e}"));
    }
    if let Err(e) = tokio::fs::rename(&tmp, &cfg.mihomo.config_path).await {
        return api_err(format!("Ошибка переименования: {e}"));
    }
    if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
        return api_err(format!("exclude-filter записан, но reload Mihomo не удался: {e}"));
    }
    // exclude-filter провайдера применяется только при его загрузке — принудительно
    // перечитываем провайдеры, иначе игнор не подействует до планового обновления.
    let updated = mihomo::force_update_all_providers(&state.http, &cfg).await;
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения конфига: {e}"));
    }
    *state.config.write().await = cfg;
    api_ok(json!({ "applied": servers.len(), "providers_updated": updated }))
}

/// POST /api/servers/fix-names — ремонт mojibake-имён статических прокси в config.yaml
/// (глобальная замена битого имени на починенное затрагивает и группы, и правила), reload.
pub async fn fix_names(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let _guard = state.routing_lock.lock().await;
    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Не удалось прочитать {}: {e}", cfg.mihomo.config_path)),
    };
    let mut new_yaml = yaml.clone();
    let mut fixed: Vec<String> = Vec::new();
    for name in routing::parse_static_proxy_names(&yaml) {
        let repaired = mihomo::fix_mojibake_smart(&name);
        if repaired != name {
            new_yaml = new_yaml.replace(&name, &repaired);
            fixed.push(format!("{name} → {repaired}"));
        }
    }
    if fixed.is_empty() {
        return api_ok(json!({ "fixed": 0, "names": fixed }));
    }
    let tmp = format!("{}.tmp", cfg.mihomo.config_path);
    if let Err(e) = tokio::fs::write(&tmp, &new_yaml).await {
        return api_err(format!("Ошибка записи: {e}"));
    }
    if let Err(e) = tokio::fs::rename(&tmp, &cfg.mihomo.config_path).await {
        return api_err(format!("Ошибка переименования: {e}"));
    }
    if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
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
}

fn default_true() -> bool {
    true
}

/// POST /api/device-routing — сохранить цепочку устройства, применить AUTO-DEVICE
/// назначение (основной сервер), reload Mihomo. Пустой servers = снять.
pub async fn set_device_routing(State(state): State<AppState>, Json(req): Json<DeviceRoutingReq>) -> Response {
    let mut cfg = state.config.read().await.clone();
    let _guard = state.routing_lock.lock().await;

    let ip = req.ip.trim().to_string();
    if ip.is_empty() {
        return api_err("Пустой IP устройства");
    }
    let mut servers: Vec<String> = req
        .servers
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    servers.dedup();

    // 1. Сохранить/удалить настройки в конфиге
    if servers.is_empty() {
        cfg.device_routing.remove(&ip);
    } else {
        cfg.device_routing.insert(
            ip.clone(),
            config::DeviceRouting {
                servers: servers.clone(),
                ping_threshold_ms: req.ping_threshold_ms,
                auto_restore: req.auto_restore,
            },
        );
    }

    // 2. Применить AUTO-DEVICE назначение (primary или снятие)
    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Не удалось прочитать config.yaml: {e}")),
    };
    let assignment = routing::Assignment {
        ip: ip.clone(),
        name: req.name.clone(),
        server: servers.first().cloned(),
    };
    let providers = device_providers_for(&cfg, &yaml);
    let new_yaml = match routing::apply_assignments(&yaml, &[assignment], &providers) {
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
    if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
        return api_err(format!("Конфиг записан, но reload Mihomo не удался: {e}"));
    }
    tokio::time::sleep(std::time::Duration::from_millis(800)).await;

    // 3. Перевыбор основного сервера в новой группе
    let mut reselected = false;
    if let Some(primary) = servers.first() {
        let gname = routing::group_name_for(&ip, &req.name);
        if mihomo::switch_group(&state.http, &cfg, &gname, primary).await.is_ok() {
            reselected = true;
        }
    }

    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения конфига: {e}"));
    }
    *state.config.write().await = cfg;
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

/// GET /api/domains — списки доменов (напрямую / принудительно через прокси).
pub async fn get_domains(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    api_ok(json!({
        "direct": cfg.direct_domains,
        "force": cfg.force_domains,
    }))
}

#[derive(Deserialize)]
pub struct DomainsReq {
    #[serde(default)]
    pub direct: Vec<String>,
    #[serde(default)]
    pub force: Vec<String>,
}

/// POST /api/domains — сохранить списки, вставить DOMAIN-SUFFIX правила в rules:, reload.
pub async fn set_domains(State(state): State<AppState>, Json(req): Json<DomainsReq>) -> Response {
    let mut cfg = state.config.read().await.clone();
    let _guard = state.routing_lock.lock().await;

    cfg.direct_domains = routing::sanitize_domains(&req.direct);
    cfg.force_domains = routing::sanitize_domains(&req.force);

    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Не удалось прочитать config.yaml: {e}")),
    };
    let new_yaml = match routing::apply_domain_rules(&yaml, &cfg.direct_domains, &cfg.force_domains) {
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
    if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
        return api_err(format!("Правила записаны, но reload Mihomo не удался: {e}"));
    }
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения конфига: {e}"));
    }
    let (n_direct, n_force) = (cfg.direct_domains.len(), cfg.force_domains.len());
    *state.config.write().await = cfg;
    api_ok(json!({ "direct": n_direct, "force": n_force }))
}

// --- Сервис XKeen и бэкапы ---

#[derive(Deserialize)]
pub struct ServiceReq {
    pub action: String,
}

/// POST /api/xkeen/service — start/stop/restart/status сервиса XKeen.
/// Restart перегенерирует config.yaml — настройки возвращаются к исходным.
pub async fn xkeen_service(State(state): State<AppState>, Json(req): Json<ServiceReq>) -> Response {
    let cfg = state.config.read().await.clone();
    let action = req.action.trim().to_string();
    if !matches!(action.as_str(), "start" | "stop" | "restart" | "status") {
        return api_err("Недопустимое действие (start/stop/restart/status)");
    }
    let out = tokio::process::Command::new("sh")
        .arg(&cfg.system.xkeen_init)
        .arg(&action)
        .output()
        .await;
    match out {
        Ok(o) => {
            log_i!(
                "Сервис XKeen: {} (код {})",
                action,
                o.status.code().unwrap_or(-1)
            );
            api_ok(json!({
                "code": o.status.code(),
                "stdout": String::from_utf8_lossy(&o.stdout),
                "stderr": String::from_utf8_lossy(&o.stderr),
            }))
        }
        Err(e) => {
            log_e!("Сервис XKeen: {} не удался: {e}", action);
            api_err(format!(
                "Не удалось выполнить {} {}: {e} (путь настраивается в system.xkeen_init)",
                cfg.system.xkeen_init, action
            ))
        }
    }
}

/// Каталог бэкапов панели: {backup_dir}/xkeen-route.
fn backup_root(cfg: &config::AppConfig) -> std::path::PathBuf {
    std::path::Path::new(&cfg.system.backup_dir).join("xkeen-route")
}

/// Валидация имени бэкапа (защита от path traversal).
fn valid_backup_name(name: &str) -> bool {
    !name.is_empty()
        && name.starts_with("xr-")
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// GET /api/backups — список бэкапов.
pub async fn list_backups(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let root = backup_root(&cfg);
    let mut items: Vec<serde_json::Value> = Vec::new();
    if let Ok(mut rd) = tokio::fs::read_dir(&root).await {
        let mut names: Vec<String> = Vec::new();
        while let Ok(Some(e)) = rd.next_entry().await {
            if e.path().is_dir() {
                names.push(e.file_name().to_string_lossy().to_string());
            }
        }
        names.sort();
        names.reverse();
        for n in names {
            items.push(json!({ "name": n }));
        }
    }
    api_ok(json!({ "backups": items, "dir": root.display().to_string() }))
}

/// POST /api/backups — создать бэкап (config.yaml Mihomo + config.json панели).
pub async fn create_backup(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let name = format!("xr-{ts}");
    let dir = backup_root(&cfg).join(&name);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return api_err(format!("Не удалось создать {}: {e}", dir.display()));
    }
    // 1. config.yaml Mihomo
    if let Err(e) = tokio::fs::copy(&cfg.mihomo.config_path, dir.join("config.yaml")).await {
        return api_err(format!("Не удалось скопировать {}: {e}", cfg.mihomo.config_path));
    }
    // 2. config.json панели
    if let Err(e) = tokio::fs::copy(state.config_path.as_path(), dir.join("config.json")).await {
        return api_err(format!("Не удалось скопировать {}: {e}", state.config_path.display()));
    }
    log_i!("Бэкап создан: {} ({})", name, dir.display());
    api_ok(json!({ "name": name, "dir": dir.display().to_string() }))
}

#[derive(Deserialize)]
pub struct BackupReq {
    pub name: String,
}

/// POST /api/backups/restore — восстановить конфиги из бэкапа, reload Mihomo.
pub async fn restore_backup(State(state): State<AppState>, Json(req): Json<BackupReq>) -> Response {
    if !valid_backup_name(&req.name) {
        return api_err("Некорректное имя бэкапа");
    }
    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&req.name);
    if !dir.is_dir() {
        return api_err(format!("Бэкап {} не найден", req.name));
    }
    if let Err(e) = tokio::fs::copy(dir.join("config.yaml"), &cfg.mihomo.config_path).await {
        return api_err(format!("Не удалось восстановить config.yaml: {e}"));
    }
    if let Err(e) = tokio::fs::copy(dir.join("config.json"), state.config_path.as_path()).await {
        return api_err(format!("Не удалось восстановить config.json: {e}"));
    }
    if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
        return api_err(format!("Конфиги восстановлены, но reload Mihomo не удался: {e}"));
    }
    // Перечитать конфиг панели в состояние.
    *state.config.write().await = config::load(&state.config_path);
    log_i!("Конфиги восстановлены из бэкапа {}", req.name);
    api_ok(json!({ "restored": req.name }))
}

/// POST /api/backups/delete — удалить бэкап.
pub async fn delete_backup(State(state): State<AppState>, Json(req): Json<BackupReq>) -> Response {
    if !valid_backup_name(&req.name) {
        return api_err("Некорректное имя бэкапа");
    }
    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&req.name);
    match tokio::fs::remove_dir_all(&dir).await {
        Ok(_) => {
            log_i!("Бэкап {} удалён", req.name);
            api_ok(json!({ "deleted": req.name }))
        }
        Err(e) => api_err(format!("Не удалось удалить {}: {e}", req.name)),
    }
}

// ---------- Логи ----------

#[derive(Deserialize)]
pub struct LogsQuery {
    pub lines: Option<usize>,
}

/// GET /api/logs?lines=500 — хвост журнала.
pub async fn logs_tail(Query(q): Query<LogsQuery>) -> Response {
    match crate::logger::tail(q.lines.unwrap_or(500)) {
        Ok(text) => api_ok(json!({
            "text": text,
            "path": crate::logger::path()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        })),
        Err(e) => api_err(e),
    }
}

/// GET /api/logs/download — скачать весь журнал (text/plain).
pub async fn logs_download() -> impl IntoResponse {
    let text = crate::logger::read_all().unwrap_or_default();
    (
        [
            ("Content-Type", "text/plain; charset=utf-8"),
            (
                "Content-Disposition",
                "attachment; filename=\"xkeen-route.log\"",
            ),
        ],
        text,
    )
        .into_response()
}

/// POST /api/logs/clear — очистить журнал.
pub async fn logs_clear() -> Response {
    match crate::logger::clear() {
        Ok(_) => {
            log_i!("Журнал очищен");
            api_ok(json!({ "cleared": true }))
        }
        Err(e) => api_err(e),
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
    let new_yaml = match routing::apply_assignments(&yaml, &assignments, &device_providers_for(&cfg, &yaml)) {
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



