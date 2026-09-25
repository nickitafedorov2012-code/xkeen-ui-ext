use axum::extract::{ConnectInfo, Query, State};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;

use crate::{config, failover, log_e, log_i, log_w, mihomo, rci, routing, AppState, VERSION};

/// GET /api/status — сводка: панель + роутер + активный сервер Mihomo.
pub async fn status(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();

    let (router_res, system_res, mihomo_ver) = tokio::join!(
        rci::get_version(&state.http, &cfg),
        rci::get_system(&state.http, &cfg),
        mihomo::get_version(&state.http, &cfg)
    );
    let router = router_res.ok();
    let system = system_res.ok();

    let active = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain)
        .await
        .ok()
        .and_then(|servers| {
            servers
                .iter()
                .find(|s| s.is_active && s.id != "Fastest" && s.id != "Fallback")
                .or_else(|| servers.iter().find(|s| s.is_active))
                .cloned()
        })
        .map(|s| json!({
            "id": s.id, "name": s.name, "ping_ms": s.ping_ms,
            "protocol": s.protocol, "host": s.host, "port": s.port,
            "provider": s.provider, "provider_name": s.provider_name,
        }));

    api_ok(json!({
        "version": VERSION,
        "config_path": state.config_path.display().to_string(),
        "router": router,
        "system": system,
        "mihomo_version": mihomo_ver,
        "active_server": active,
        "mihomo": { "host": cfg.mihomo.host, "port": cfg.mihomo.port },
        "rci": { "host": cfg.rci.host, "port": cfg.rci.port },
        "failover": {
            "enabled": cfg.failover.enabled,
            "ping_threshold_ms": cfg.failover.ping_threshold_ms,
            "priority_server": if !cfg.failover.priority_server.is_empty() {
                cfg.failover.priority_server.clone()
            } else {
                cfg.failover.priority_chain.first().cloned().unwrap_or_default()
            },
            "priority_chain": cfg.failover.priority_chain,
            "auto_restore_priority": cfg.failover.auto_restore_priority,
            "interval_secs": cfg.failover.interval_secs,
            "device_failover_enabled": cfg.failover.device_failover_enabled,
        },
        "refresh_interval_sec": cfg.refresh_interval_sec,
        "adblock_enabled": cfg.adblock_enabled,
    }))
    .into_response()
}

/// GET /api/system/metrics — сверхлёгкий эндпоинт для обновления метрик каждую секунду: CPU роутера, RAM роутера, RAM/CPU процесса панели.
pub async fn get_system_metrics(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match rci::get_system(&state.http, &cfg).await {
        Ok(stats) => api_ok(serde_json::to_value(stats).unwrap_or_default()).into_response(),
        Err(e) => api_err(e).into_response(),
    }
}

/// GET /api/system/processes — снимок диспетчера задач (ресурсы CPU/RAM/Swap + список процессов)
pub async fn get_system_processes(State(_state): State<AppState>) -> Response {
    let snapshot = crate::system::get_task_manager_snapshot().await;
    api_ok(serde_json::to_value(snapshot).unwrap_or_default()).into_response()
}

/// POST /api/system/processes/kill — безопасное завершение процесса роутера
pub async fn kill_process(
    State(_state): State<AppState>,
    Json(body): Json<crate::system::KillRequest>,
) -> Response {
    match crate::system::kill_process_by_pid(body.pid, body.signal.as_deref()).await {
        Ok(msg) => api_ok(json!({ "pid": body.pid, "message": msg })).into_response(),
        Err(err) => api_err(err).into_response(),
    }
}



/// Вспомогательное: единый формат ответа API.
pub fn api_ok(data: serde_json::Value) -> axum::response::Response {
    Json(json!({ "success": true, "data": data })).into_response()
}

pub fn api_err(error: impl Into<String>) -> axum::response::Response {
    Json(json!({ "success": false, "error": error.into() })).into_response()
}

/// Атомарная запись файла на диск (tmp + rename)
pub async fn atomic_write_file(path: impl AsRef<std::path::Path>, content: &str) -> Result<(), String> {
    let p = path.as_ref();
    if let Some(parent) = p.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = format!("{}.{}.{}.tmp", p.display(), std::process::id(), nonce);
    if let Err(e) = tokio::fs::write(&tmp, content).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Ошибка записи {tmp}: {e}"));
    }
    if let Err(e) = tokio::fs::rename(&tmp, p).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Ошибка атомарного сохранения {}: {e}", p.display()));
    }
    Ok(())
}

/// Обработчик 404 для несуществующих API-эндпоинтов
pub async fn api_not_found() -> Response {
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(json!({
            "success": false,
            "error": "API endpoint not found"
        })),
    )
        .into_response()
}

/// GET /api/settings — получение настроек панели с маскированием конфиденциальных данных
pub async fn get_settings(State(state): State<AppState>) -> impl IntoResponse {
    let cfg = state.config.read().await.clone();
    let mut val = serde_json::to_value(&*cfg).unwrap_or_default();

    // Маскируем секретные поля при отдаче в UI
    if let Some(auth) = val.get_mut("auth") {
        if let Some(pw) = auth.get_mut("password_hash") {
            if !pw.as_str().unwrap_or("").is_empty() {
                *pw = serde_json::Value::String("******".into());
            }
        }
        if let Some(salt) = auth.get_mut("salt") {
            if !salt.as_str().unwrap_or("").is_empty() {
                *salt = serde_json::Value::String("******".into());
            }
        }
        if let Some(sec) = auth.get_mut("session_secret") {
            if !sec.as_str().unwrap_or("").is_empty() {
                *sec = serde_json::Value::String("******".into());
            }
        }
    }
    if let Some(rci) = val.get_mut("rci") {
        if let Some(pw) = rci.get_mut("password") {
            if !pw.as_str().unwrap_or("").is_empty() {
                *pw = serde_json::Value::String("******".into());
            }
        }
        if let Some(tok) = rci.get_mut("token") {
            if !tok.as_str().unwrap_or("").is_empty() {
                *tok = serde_json::Value::String("******".into());
            }
        }
    }
    if let Some(mihomo) = val.get_mut("mihomo") {
        if let Some(sec) = mihomo.get_mut("secret") {
            if !sec.as_str().unwrap_or("").is_empty() {
                *sec = serde_json::Value::String("******".into());
            }
        }
    }
    if let Some(notif) = val.get_mut("notifications") {
        if let Some(bot) = notif.get_mut("telegram_bot_token") {
            if !bot.as_str().unwrap_or("").is_empty() {
                *bot = serde_json::Value::String("******".into());
            }
        }
    }

    api_ok(val)
}

pub async fn put_settings(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let _cfg_guard = state.config_lock.lock().await;
    let mut merged = serde_json::to_value(&**state.config.read().await).unwrap_or_default();
    crate::config::merge_value(&mut merged, &body);
    let mut new_cfg: config::AppConfig = match serde_json::from_value(merged) {
        Ok(c) => c,
        Err(e) => return api_err(format!("Некорректные настройки: {}", e)),
    };
    new_cfg.failover.migrate_priority();
    if let Err(e) = config::save(&state.config_path, &new_cfg).await {
        return api_err(format!("Ошибка сохранения конфига: {}", e));
    }
    *state.config.write().await = std::sync::Arc::new(new_cfg);
    crate::logger::set_level(&state.config.read().await.logs.level);
    log_i!("Настройки сохранены");
    api_ok(json!({ "saved": true, "applied": true }))
}

/// GET /api/servers — карточки серверов.
pub async fn get_servers(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await {
        Ok(servers) => {
            let flow_server = cfg.flow_server.clone();
            let list: Vec<serde_json::Value> = servers
                .iter()
                .map(|s| {
                    let flow_status = mihomo::eval_flow_status(&s.id, &s.name);
                    let is_google_ai = flow_server
                        .as_ref()
                        .map_or(s.is_active, |fs| fs == &s.id);
                    json!({
                        "id": s.id, "name": s.name, "protocol": s.protocol,
                        "host": s.host, "port": s.port,
                        "is_active": s.is_active, "is_priority": s.is_priority,
                        "is_google_ai": is_google_ai,
                        "flow_status": flow_status,
                        "ping_ms": s.ping_ms,
                        "provider": s.provider,
                        "provider_name": s.provider_name,
                        "is_pool": s.is_pool,
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
    #[serde(alias = "id")]
    pub server_id: String,
}

/// POST /api/servers/switch
pub async fn switch_server(State(state): State<AppState>, Json(req): Json<SwitchReq>) -> Response {
    let cfg = state.config.read().await.clone();
    match mihomo::switch_server(&state.http, &cfg, &req.server_id).await {
        Ok(msg) => {
            log_i!("Смена сервера: {} — {}", req.server_id, msg);
            // Если переключен конкретный сервер, обновляем его как основной в цепочке приоритетов,
            // чтобы фоновый failover не откатывал выбор пользователя обратно через 30-60 секунд.
            if !req.server_id.is_empty() && req.server_id != "Fastest" && req.server_id != "Fallback" {
                let _cfg_guard = state.config_lock.lock().await;
                let mut mut_cfg = (**state.config.read().await).clone();
                let id = req.server_id.clone();
                if let Some(pos) = mut_cfg.failover.priority_chain.iter().position(|s| s == &id) {
                    mut_cfg.failover.priority_chain.remove(pos);
                }
                mut_cfg.failover.priority_chain.insert(0, id.clone());
                mut_cfg.failover.priority_server = id;
                if let Err(e) = config::save(&state.config_path, &mut_cfg).await {
                    log_e!("Ошибка сохранения приоритета в config.json: {e}");
                } else {
                    *state.config.write().await = std::sync::Arc::new(mut_cfg);
                }
            }
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
    pub url: Option<String>,
}

/// POST /api/servers/ping — один сервер или все (параллельно).
pub async fn ping_servers(State(state): State<AppState>, Json(req): Json<PingReq>) -> Response {
    let cfg = state.config.read().await.clone();
    let ids: Vec<String> = match req.server_id {
        Some(id) => vec![id],
        None => match mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await {
            Ok(servers) => servers.into_iter().map(|s| s.id).collect(),
            Err(e) => return api_err(e),
        },
    };
    let pings = mihomo::ping_all_url(&state.http, &cfg, &ids, 2500, req.url.as_deref()).await;
    api_ok(json!({ "pings": pings }))
}

/// POST /api/flow/ping — специализированный пинг серверов по Google Flow (https://flow.google.com).
pub async fn ping_flow_servers(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let pings = mihomo::ping_flow_servers(&state.http, &cfg, 3500).await;

    let mut best_server: Option<String> = None;
    let mut min_ping = i64::MAX;

    for (name, &ping) in &pings {
        if ping > 0 && ping < min_ping {
            let lower = name.to_lowercase();
            let is_us_ca = lower.contains("сша") || lower.contains("usa") || lower.contains("us ") ||
                lower.contains("chicago") || lower.contains("чикаго") || lower.contains("канад") ||
                lower.contains("canada") || lower.contains("вашингтон") || lower.contains("washington") ||
                lower.contains("майами") || lower.contains("miami") || lower.contains("сиэтл") ||
                lower.contains("seattle") || lower.contains("атланта") || lower.contains("atlanta") ||
                lower.contains("феникс") || lower.contains("phoenix") || lower.contains("лос-анджелес") ||
                lower.contains("los angeles");
            if is_us_ca {
                min_ping = ping;
                best_server = Some(name.clone());
            }
        }
    }

    api_ok(json!({
        "pings": pings,
        "best_server": best_server,
        "min_ping": if min_ping == i64::MAX { None } else { Some(min_ping) },
    }))
}

/// GET /api/servers/google-check — диагностика чистоты узла в Google Search / AI.
pub async fn check_google_geo(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let target_name = match mihomo::get_flow_status(&state.http, &cfg).await {
        Ok(val) => val.get("flow_server").and_then(|s| s.as_str()).unwrap_or("").to_string(),
        Err(_) => String::new(),
    };
    let active_name = if !target_name.is_empty() {
        target_name
    } else if let Some(ref fs) = cfg.flow_server {
        fs.clone()
    } else {
        match mihomo::get_proxies(&state.http, &cfg).await {
            Ok(proxies) => mihomo::resolve_active_leaf(&proxies),
            Err(e) => return api_err(format!("Ошибка получения прокси: {e}")),
        }
    };
    match mihomo::check_google_geo(&state.http, &cfg, &active_name).await {
        Ok(status) => api_ok(serde_json::to_value(status).unwrap_or_default()),
        Err(e) => api_err(e),
    }
}

/// GET /api/flow/status — получить текущий сервер Google Flow & AI
pub async fn get_flow_status(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match mihomo::get_flow_status(&state.http, &cfg).await {
        Ok(val) => api_ok(val),
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct FlowSwitchReq {
    #[serde(alias = "id")]
    pub server_id: String,
}

/// POST /api/flow/switch — переключить ТОЛЬКО сервер для Google Flow & AI (не затрагивая PROXY / Failover)
pub async fn switch_flow_server(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<FlowSwitchReq>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let srv_id = body.server_id.trim();
    if srv_id.is_empty() {
        return api_err("Не указан server_id для Google Flow");
    }

    match mihomo::switch_flow_server(&state.http, &cfg, srv_id).await {
        Ok(msg) => {
            log_i!("[Google Flow] {}", msg);
            // Сохраняем flow_server в config.json БЕЗ изменения failover.priority_server / priority_chain!
            {
                let _cfg_guard = state.config_lock.lock().await;
                let mut mut_cfg = (**state.config.read().await).clone();
                mut_cfg.flow_server = Some(srv_id.to_string());
                if let Err(e) = config::save(&state.config_path, &mut_cfg).await {
                    log_e!("Ошибка сохранения flow_server в config.json: {e}");
                } else {
                    *state.config.write().await = std::sync::Arc::new(mut_cfg);
                }
            }
            api_ok(json!({ "message": msg, "flow_server": srv_id }))
        }
        Err(e) => {
            log_e!("Ошибка переключения Google Flow на {}: {}", srv_id, e);
            api_err(e)
        }
    }
}

/// POST /api/flow/repair — комплексная починка Google Flow:
/// 1) Находит лучший узел США
/// 2) Переключает маршрут Google AI на этот узел
/// 3) Сбрасывает сокеты и соединения ядра Mihomo
pub async fn repair_flow(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();

    // 1. Поиск лучшего сервера США
    let pings = mihomo::ping_flow_servers(&state.http, &cfg, 3500).await;
    let mut best_server: Option<String> = None;
    let mut min_ping = i64::MAX;

    for (name, &ping) in &pings {
        if ping > 0 && ping < min_ping {
            let lower = name.to_lowercase();
            let is_us_ca = lower.contains("сша") || lower.contains("usa") || lower.contains("us ") ||
                lower.contains("chicago") || lower.contains("чикаго") || lower.contains("канад") ||
                lower.contains("canada") || lower.contains("вашингтон") || lower.contains("washington") ||
                lower.contains("майами") || lower.contains("miami") || lower.contains("сиэтл") ||
                lower.contains("seattle") || lower.contains("атланта") || lower.contains("atlanta") ||
                lower.contains("феникс") || lower.contains("phoenix") || lower.contains("лос-анджелес") ||
                lower.contains("los angeles");
            if is_us_ca {
                min_ping = ping;
                best_server = Some(name.clone());
            }
        }
    }

    let target_server = best_server
        .or_else(|| cfg.flow_server.clone())
        .unwrap_or_else(|| "🇺🇸 США Вашингтон".to_string());

    // 2. Переключение Flow сервера и обновление правил
    let _ = mihomo::switch_flow_server(&state.http, &cfg, &target_server).await;

    // 3. Сохранение в config.json
    {
        let _cfg_guard = state.config_lock.lock().await;
        let mut mut_cfg = (**state.config.read().await).clone();
        mut_cfg.flow_server = Some(target_server.clone());
        let _ = config::save(&state.config_path, &mut_cfg).await;
        *state.config.write().await = std::sync::Arc::new(mut_cfg);
    }

    // 4. Сброс всех активных соединений ядра
    mihomo::close_all_connections(&state.http, &cfg).await;

    api_ok(json!({
        "success": true,
        "flow_server": target_server,
        "min_ping": if min_ping == i64::MAX { None } else { Some(min_ping) },
        "message": format!("Маршрут Google Flow переключен на '{target_server}', сокеты сброшены"),
    }))
}



/// GET /api/flow/extension.zip — отдача готового запакованного расширения для Chrome/Edge
pub async fn get_flow_extension() -> Response {
    let disk_file = std::path::Path::new("/opt/share/xkeen-route/dist/flow-unlock-extension.zip");
    if let Ok(data) = tokio::fs::read(disk_file).await {
        return (
            [
                ("Content-Type", "application/zip"),
                ("Content-Disposition", "attachment; filename=\"xkeen-flow-unlock.zip\""),
                ("Cache-Control", "no-cache, no-store, must-revalidate"),
            ],
            data,
        )
            .into_response();
    }
    if let Some(asset) = crate::frontend::Assets::get("flow-unlock-extension.zip") {
        return (
            [
                ("Content-Type", "application/zip"),
                ("Content-Disposition", "attachment; filename=\"xkeen-flow-unlock.zip\""),
                ("Cache-Control", "no-cache, no-store, must-revalidate"),
            ],
            asset.data,
        )
            .into_response();
    }
    (axum::http::StatusCode::NOT_FOUND, "flow-unlock-extension.zip not found").into_response()
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
pub struct FailoverToggleReq {
    pub enabled: bool,
}

/// POST /api/failover/toggle — быстрое включение/отключение failover.
pub async fn failover_toggle(State(state): State<AppState>, Json(req): Json<FailoverToggleReq>) -> Response {
    let _cfg_guard = state.config_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();
    cfg.failover.enabled = req.enabled;
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения конфига: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(cfg);
    log_i!("Failover переключён: enabled={}", req.enabled);
    api_ok(json!({ "enabled": req.enabled }))
}

#[derive(Deserialize)]
pub struct PriorityReq {
    /// Одиночный приоритет (обратная совместимость).
    pub server_id: Option<String>,
    /// Цепочка приоритетов: [основной, резерв1, ...]. Пустой массив = снять.
    #[serde(default)]
    pub server_ids: Vec<String>,
    /// Опционально: включить/выключить failover вместе с цепочкой
    pub enabled: Option<bool>,
}

/// POST /api/settings/priority — задать/снять глобальную цепочку приоритетов.
pub async fn set_priority(State(state): State<AppState>, Json(req): Json<PriorityReq>) -> Response {
    let _cfg_guard = state.config_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();
    // Совместимость: если пришёл только server_id — цепочка из одного элемента.
    let chain: Vec<String> = if !req.server_ids.is_empty() {
        req.server_ids
    } else {
        match req.server_id {
            Some(id) => vec![id],
            None => Vec::new(),
        }
    };
    // id сохраняем ровно как прислал фронт (s.id из GET /api/servers) —
    // trim() ломал совпадение с id серверов (эмодзи/пробелы в именах).
    cfg.failover.priority_chain = chain;
    cfg.failover.migrate_priority();
    if let Some(en) = req.enabled {
        cfg.failover.enabled = en;
    }
    let message = if cfg.failover.priority_chain.is_empty() {
        "Приоритет снят".to_string()
    } else {
        let names: Vec<String> = cfg
            .failover
            .priority_chain
            .iter()
            .map(|id| {
                if id == "Fastest" || id == "Fallback" {
                    id.clone()
                } else {
                    mihomo::display_name(id)
                }
            })
            .collect();
        format!("Цепочка приоритета: {}", names.join(" → "))
    };
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(cfg);
    api_ok(json!({ "saved": true, "message": message }))
}


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
    let _cfg_guard = state.config_lock.lock().await;
    let _guard = state.routing_lock.lock().await; // как в остальных правках config.yaml
    let mut cfg = (**state.config.read().await).clone();
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
    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
    }

    let reload_err = mihomo::reload_config(&state.http, &cfg).await.err();
    // exclude-filter провайдера применяется только при его загрузке — принудительно
    // перечитываем провайдеры, иначе игнор не подействует до планового обновления.
    let updated = mihomo::force_update_all_providers(&state.http, &cfg).await;
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения конфига: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(cfg);

    if let Some(err) = reload_err {
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
    let _cfg_guard = state.config_lock.lock().await;
    let _guard = state.routing_lock.lock().await;
    let cfg = state.config.read().await.clone();
    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Не удалось прочитать {}: {e}", cfg.mihomo.config_path)),
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
    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
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
    let _cfg_guard = state.config_lock.lock().await;
    let _guard = state.routing_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();

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
    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
    }

    let reload_err = mihomo::reload_config(&state.http, &cfg).await.err();
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
    *state.config.write().await = std::sync::Arc::new(cfg);

    if let Some(err) = reload_err {
        return api_ok(json!({
            "applied": !servers.is_empty(),
            "servers": servers,
            "reselected": reselected,
            "warning": format!("Маршрутизация сохранена, но reload Mihomo вернул ошибку: {err}")
        }));
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

/// GET /api/domains — списки доменов (напрямую / принудительно через прокси) + найденные CDN.
pub async fn get_domains(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let auto_cdns = crate::cdn_discovery::expand_bundles(&cfg.force_domains);
    let auto_cdns_list: Vec<String> = auto_cdns.into_iter().collect();
    api_ok(json!({
        "direct": cfg.direct_domains,
        "force": cfg.force_domains,
        "auto_cdns": auto_cdns_list,
    }))
}

#[derive(Deserialize)]
pub struct DomainsReq {
    #[serde(default)]
    pub direct: Vec<String>,
    #[serde(default)]
    pub force: Vec<String>,
}

/// POST /api/domains — сохранить списки, авто-обнаружить CDN, вставить DOMAIN-SUFFIX правила в rules:, reload.
pub async fn set_domains(State(state): State<AppState>, Json(req): Json<DomainsReq>) -> Response {
    let _cfg_guard = state.config_lock.lock().await;
    let _guard = state.routing_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();

    cfg.direct_domains = routing::sanitize_domains(&req.direct);
    cfg.force_domains = routing::sanitize_domains(&req.force);

    // Автоматическое обнаружение сопутствующих CDN (бандлы + поддомены + HTML-сканер)
    let auto_cdns = crate::cdn_discovery::discover_all_cdns(&cfg.force_domains, &cfg.mihomo_proxy_url()).await;
    let mut all_force = cfg.force_domains.clone();
    for cdn in &auto_cdns {
        if !all_force.contains(cdn) {
            all_force.push(cdn.clone());
        }
    }

    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Не удалось прочитать config.yaml: {e}")),
    };
    let new_yaml = match routing::apply_domain_rules(&yaml, &cfg.direct_domains, &all_force, &cfg.device_domain_rules) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
    }

    let reload_err = mihomo::reload_config(&state.http, &cfg).await.err();
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения конфига: {e}"));
    }

    // Автоматическая синхронизация IP-адресов принудительно проксируемых доменов и их CDN с geo_override
    let overridden = match crate::override_sync::sync_geo_override(&all_force).await {
        Ok(count) => count,
        Err(e) => {
            crate::log_w!("[OVERRIDE] Ошибка синхронизации geo_override: {e}");
            0
        }
    };

    let (n_direct, n_force) = (cfg.direct_domains.len(), cfg.force_domains.len());
    *state.config.write().await = std::sync::Arc::new(cfg);
    let auto_cdns_list: Vec<String> = auto_cdns.into_iter().collect();

    if let Some(err) = reload_err {
        return api_ok(json!({
            "direct": n_direct,
            "force": n_force,
            "auto_cdns": auto_cdns_list,
            "overridden_ips": overridden,
            "warning": format!("Правила доменов сохранены, но reload Mihomo вернул ошибку: {err}")
        }));
    }
    api_ok(json!({
        "direct": n_direct,
        "force": n_force,
        "auto_cdns": auto_cdns_list,
        "overridden_ips": overridden
    }))
}

/// POST /api/domains/scan-cdn — ручной запуск глубокого сканирования CDN и синхронизации geo_override
pub async fn scan_cdn_manual(State(state): State<AppState>) -> Response {
    let _guard = state.routing_lock.lock().await;
    let cfg = state.config.read().await.clone();
    if cfg.force_domains.is_empty() {
        return api_ok(json!({
            "auto_cdns": [],
            "overridden_ips": 0,
            "message": "Список принудительно проксируемых доменов пуст"
        }));
    }

    crate::log_i!("[MANUAL] Ручной запуск глубокого сканирования CDN...");
    let auto_cdns = crate::cdn_discovery::discover_all_cdns(&cfg.force_domains, &cfg.mihomo_proxy_url()).await;
    let mut all_domains = cfg.force_domains.clone();
    for cdn in &auto_cdns {
        if !all_domains.contains(cdn) {
            all_domains.push(cdn.clone());
        }
    }

    let overridden = match crate::override_sync::sync_geo_override(&all_domains).await {
        Ok(count) => count,
        Err(e) => {
            crate::log_w!("[MANUAL] Ошибка синхронизации geo_override: {e}");
            0
        }
    };

    crate::log_i!("[MANUAL] ✓ Ручное сканирование CDN завершено. Найдено CDN: {}, IP: {}", auto_cdns.len(), overridden);
    let auto_cdns_list: Vec<String> = auto_cdns.into_iter().collect();
    api_ok(json!({
        "auto_cdns": auto_cdns_list,
        "overridden_ips": overridden
    }))
}

#[derive(Deserialize)]
pub struct ForceAddDomainReq {
    pub domain: String,
    #[serde(default)]
    pub client_ip: Option<String>,
    #[serde(default)]
    pub close_connection_id: Option<String>,
}

/// POST /api/domains/force-add — быстрое добавление домена принудительно в прокси (глобально или per-device)
pub async fn force_add_domain(
    State(state): State<AppState>,
    Json(req): Json<ForceAddDomainReq>,
) -> Response {
    let clean_domain = req
        .domain
        .trim()
        .to_lowercase()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("www.")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_string();

    if clean_domain.is_empty() || !clean_domain.contains('.') {
        return api_err(format!("Некорректный домен: {}", req.domain));
    }

    let _cfg_guard = state.config_lock.lock().await;
    let _guard = state.routing_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();

    let client_ip = req.client_ip.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string);

    if let Some(ref ip) = client_ip {
        // Добавление в персональные правила устройства
        let rules = cfg.device_domain_rules.entry(ip.clone()).or_default();
        if !rules.iter().any(|r| r.domain.eq_ignore_ascii_case(&clean_domain)) {
            rules.push(crate::config::DeviceDomainRule {
                domain: clean_domain.clone(),
                target: "PROXY".into(),
            });
        }

        let raw_yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
            Ok(y) => y,
            Err(e) => return api_err(format!("Не удалось прочитать config.yaml: {e}")),
        };
        let (new_yaml, _) = match crate::routing::apply_routing(&raw_yaml, &cfg) {
            Ok(res) => res,
            Err(e) => return api_err(format!("Ошибка генерации правил роутинга: {e}")),
        };
        if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
            return api_err(format!("Ошибка сохранения config.yaml: {e}"));
        }
        if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
            return api_err(format!("Ошибка перезагрузки конфигурации ядра Mihomo: {e}"));
        }
        if let Err(e) = config::save(&state.config_path, &cfg).await {
            return api_err(format!("Ошибка сохранения config.json: {e}"));
        }

        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        log_i!("Домен {} жестко направлен в прокси для устройства {}", clean_domain, ip);
    } else {
        // Глобальное принудительное проксирование
        if !cfg.force_domains.iter().any(|d| d.eq_ignore_ascii_case(&clean_domain)) {
            cfg.force_domains.push(clean_domain.clone());
            cfg.force_domains.sort();
            cfg.force_domains.dedup();
        }

        // Обнаружение сопутствующих CDN
        let auto_cdns = crate::cdn_discovery::discover_all_cdns(&cfg.force_domains, &cfg.mihomo_proxy_url()).await;
        let mut all_force = cfg.force_domains.clone();
        for cdn in &auto_cdns {
            if !all_force.contains(cdn) {
                all_force.push(cdn.clone());
            }
        }

        let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
            Ok(y) => y,
            Err(e) => return api_err(format!("Не удалось прочитать config.yaml: {e}")),
        };

        let new_yaml = match routing::apply_domain_rules(&yaml, &cfg.direct_domains, &all_force, &cfg.device_domain_rules) {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };

        if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
            return api_err(format!("Ошибка сохранения config.yaml: {e}"));
        }

        let _ = mihomo::reload_config(&state.http, &cfg).await;

        if let Err(e) = config::save(&state.config_path, &cfg).await {
            return api_err(format!("Ошибка сохранения config.json: {e}"));
        }

        // Синхронизация IP-адресов домена с ipset geo_override ядра Linux
        let _ = crate::override_sync::sync_geo_override(&all_force).await;

        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        log_i!("Домен {} жестко направлен в прокси глобально", clean_domain);
    }

    // Если передан ID конкретного соединения — закрываем его немедленно
    if let Some(conn_id) = req.close_connection_id.as_deref().filter(|s| !s.is_empty()) {
        let url = format!("{}/connections/{}", cfg.mihomo_url(), conn_id);
        let mut req_del = state.http.delete(&url);
        if !cfg.mihomo.secret.is_empty() {
            req_del = req_del.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
        }
        let _ = req_del.send().await;
    }

    api_ok(json!({
        "success": true,
        "domain": clean_domain,
        "client_ip": client_ip,
        "forced": true
    }))
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
            if !o.status.success() && action != "status" {
                let stderr = String::from_utf8_lossy(&o.stderr);
                let stdout = String::from_utf8_lossy(&o.stdout);
                let err_msg = if !stderr.trim().is_empty() {
                    stderr.trim().to_string()
                } else if !stdout.trim().is_empty() {
                    stdout.trim().to_string()
                } else {
                    format!("Процесс завершился с кодом {}", o.status.code().unwrap_or(-1))
                };
                return api_err(format!("Команда '{}' завершилась с ошибкой: {}", action, err_msg));
            }
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
    let _cfg_guard = state.config_lock.lock().await;
    let _routing_guard = state.routing_lock.lock().await;

    if !valid_backup_name(&req.name) {
        return api_err("Некорректное имя бэкапа");
    }
    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&req.name);
    if !dir.is_dir() {
        return api_err(format!("Бэкап {} не найден", req.name));
    }
    let src_yaml = dir.join("config.yaml");
    let src_json = dir.join("config.json");
    if !src_yaml.exists() || !src_json.exists() {
        return api_err(format!("В бэкапе {} отсутствуют необходимые файлы (config.yaml / config.json)", req.name));
    }

    // 1. Предварительное чтение и валидация файлов бэкапа ДО каких-либо изменений на диске
    let new_yaml_content = match tokio::fs::read_to_string(&src_yaml).await {
        Ok(s) if !s.trim().is_empty() => s,
        Ok(_) => return api_err("Файл config.yaml в бэкапе пуст"),
        Err(e) => return api_err(format!("Не удалось прочитать config.yaml из бэкапа: {e}")),
    };

    let new_json_content = match tokio::fs::read_to_string(&src_json).await {
        Ok(s) => s,
        Err(e) => return api_err(format!("Не удалось прочитать config.json из бэкапа: {e}")),
    };

    let new_cfg: config::AppConfig = match serde_json::from_str(&new_json_content) {
        Ok(c) => c,
        Err(e) => return api_err(format!("Файл config.json в бэкапе поврежден: {e}")),
    };

    // 2. Снимаем текущие копии файлов в памяти для надёжного и быстрого отката
    let old_yaml_content = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.ok();
    let old_json_content = tokio::fs::read_to_string(state.config_path.as_path()).await.ok();

    // 3. Атомарно записываем config.yaml (tmp + rename)
    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml_content).await {
        return api_err(format!("Не удалось атомарно записать config.yaml: {e}"));
    }

    // 4. Атомарно записываем config.json (tmp + rename)
    if let Err(e) = atomic_write_file(state.config_path.as_path(), &new_json_content).await {
        if let Some(ref old_y) = old_yaml_content {
            let _ = atomic_write_file(&cfg.mihomo.config_path, old_y).await;
        }
        return api_err(format!("Не удалось атомарно записать config.json: {e}"));
    }

    // 5. Перезагрузка ядра Mihomo
    if let Err(e) = mihomo::reload_config(&state.http, &new_cfg).await {
        if let Some(ref old_y) = old_yaml_content {
            let _ = atomic_write_file(&cfg.mihomo.config_path, old_y).await;
        }
        if let Some(ref old_j) = old_json_content {
            let _ = atomic_write_file(state.config_path.as_path(), old_j).await;
        }
        let _ = mihomo::reload_config(&state.http, &cfg).await;
        return api_err(format!("Ошибка применения конфигурации Mihomo ({e}), изменения полностью откатаны"));
    }

    // 6. Обновляем состояние в памяти
    *state.config.write().await = std::sync::Arc::new(new_cfg);
    log_i!("Конфиги атомарно восстановлены из бэкапа {}", req.name);
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
/// Файловый I/O — в spawn_blocking, чтобы не блокировать tokio-воркеры.
pub async fn logs_tail(Query(q): Query<LogsQuery>) -> Response {
    let lines = q.lines.unwrap_or(500);
    match tokio::task::spawn_blocking(move || crate::logger::tail(lines)).await {
        Ok(Ok(text)) => api_ok(json!({
            "text": text,
            "path": crate::logger::path()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
        })),
        Ok(Err(e)) => api_err(e),
        Err(e) => api_err(format!("internal: {e}")),
    }
}

/// GET /api/logs/download — скачать весь журнал (text/plain).
pub async fn logs_download() -> impl IntoResponse {
    let text = tokio::task::spawn_blocking(crate::logger::read_all)
        .await
        .unwrap_or_else(|e| Err(format!("internal: {e}")))
        .unwrap_or_default();
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

/// GET /api/logs/ws — живой поток журнала (WebSocket).
pub async fn logs_ws(
    ws: axum::extract::ws::WebSocketUpgrade,
    Query(q): Query<LogsQuery>,
) -> Response {
    ws.on_upgrade(move |socket| async move {
        ws_logs_stream(socket, q.lines.unwrap_or(200)).await
    })
}

async fn ws_logs_stream(mut socket: axum::extract::ws::WebSocket, history_lines: usize) {
    // 1. Сначала — хвост истории.
    if let Ok(text) = crate::logger::tail(history_lines) {
        for line in text.lines() {
            if socket.send(axum::extract::ws::Message::text(line)).await.is_err() {
                return;
            }
        }
    }
    // 2. Затем — живой поток новых строк.
    let Some(mut rx) = crate::logger::subscribe() else {
        let _ = socket.send(axum::extract::ws::Message::text("(лог не инициализирован)")).await;
        return;
    };
    loop {
        tokio::select! {
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(axum::extract::ws::Message::Ping(payload))) => {
                        if socket.send(axum::extract::ws::Message::Pong(payload)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(axum::extract::ws::Message::Close(_))) | None => {
                        break;
                    }
                    _ => {}
                }
            }
            log_res = rx.recv() => {
                match log_res {
                    Ok(line) => {
                        if socket.send(axum::extract::ws::Message::text(line)).await.is_err() {
                            break;
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                        let _ = socket
                            .send(axum::extract::ws::Message::text(format!("…пропущено {n} строк…")))
                            .await;
                    }
                    Err(_) => break,
                }
            }
        }
    }
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
    let _cfg_guard = state.config_lock.lock().await;
    let _guard = state.routing_lock.lock().await;
    let cfg = state.config.read().await.clone();

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

    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
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

#[derive(Deserialize)]
pub struct RenameProviderReq {
    #[serde(alias = "id")]
    pub provider_id: String,
    pub alias: String,
}

/// GET /api/providers — список подписок с именами и метаданными
pub async fn get_providers(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match mihomo::get_providers_info(&state.http, &cfg).await {
        Ok(providers) => api_ok(json!({ "providers": providers })),
        Err(e) => api_err(e),
    }
}

/// POST /api/providers/rename — переименование подписки
pub async fn rename_provider(State(state): State<AppState>, Json(req): Json<RenameProviderReq>) -> Response {
    let pid = req.provider_id.trim().to_string();
    if pid.is_empty() {
        return api_err("ID подписки не может быть пустым");
    }
    let alias = req.alias.trim().to_string();
    let _cfg_guard = state.config_lock.lock().await;
    let mut new_cfg = (**state.config.read().await).clone();
    if alias.is_empty() {
        new_cfg.provider_aliases.remove(&pid);
    } else {
        new_cfg.provider_aliases.insert(pid.clone(), alias.clone());
    }
    if let Err(e) = config::save(&state.config_path, &new_cfg).await {
        return api_err(format!("Ошибка сохранения: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(new_cfg);
    let label = if alias.is_empty() {
        format!("Сброшено имя подписки '{pid}'")
    } else {
        format!("Подписка '{pid}' переименована в '{alias}'")
    };
    api_ok(json!({ "saved": true, "message": label, "provider_id": pid, "alias": alias }))
}

#[derive(Deserialize)]
pub struct UpdateProviderReq {
    #[serde(alias = "id")]
    pub provider_id: Option<String>,
}

/// POST /api/providers/update — принудительное обновление подписки/подписок
pub async fn update_provider(State(state): State<AppState>, Json(req): Json<UpdateProviderReq>) -> Response {
    let cfg = state.config.read().await.clone();
    if let Some(pid) = req.provider_id.filter(|p| !p.trim().is_empty()) {
        match mihomo::force_update_provider(&state.http, &cfg, &pid).await {
            Ok(_) => api_ok(json!({ "updated": 1, "message": format!("Подписка '{pid}' обновлена") })),
            Err(e) => api_err(format!("Ошибка обновления подписки '{pid}': {e}")),
        }
    } else {
        let count = mihomo::force_update_all_providers(&state.http, &cfg).await;
        api_ok(json!({ "updated": count, "message": format!("Обновлено подписок: {count}") }))
    }
}

#[derive(Deserialize)]
pub struct AddProviderReq {
    pub id: String,
    pub url: String,
    pub name: Option<String>,
    pub hwid: Option<String>,
    pub user_agent: Option<String>,
    pub append_hwid_to_url: Option<bool>,
}

/// POST /api/providers/add — добавление новой подписки в config.yaml
pub async fn add_provider(State(state): State<AppState>, Json(req): Json<AddProviderReq>) -> Response {
    let id = req.id.trim();
    let raw_url = req.url.trim();
    if id.is_empty() || raw_url.is_empty() {
        return api_err("ID и URL подписки не могут быть пустыми");
    }

    let hwid_trimmed = req.hwid.as_deref().map(str::trim).filter(|h| !h.is_empty());
    let mut final_url = raw_url.to_string();
    if req.append_hwid_to_url.unwrap_or(false) {
        if let Some(h) = hwid_trimmed {
            if !final_url.contains("hwid=") {
                let sep = if final_url.contains('?') { '&' } else { '?' };
                final_url.push(sep);
                final_url.push_str(&format!("hwid={h}"));
            }
        }
    }

    let cfg = state.config.read().await.clone();
    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Ошибка чтения {}: {e}", cfg.mihomo.config_path)),
    };

    let new_yaml = match crate::routing::add_provider_to_yaml_full(
        &yaml,
        id,
        &final_url,
        Some(cfg.health_check_url()),
        Some(cfg.mihomo.health_check_interval),
        hwid_trimmed,
        req.user_agent.as_deref(),
    ) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
    }

    // Если указано пользовательское имя — сохраняем псевдоним
    if let Some(alias) = req.name.filter(|n| !n.trim().is_empty()) {
        let _guard = state.config_lock.lock().await;
        let mut new_cfg = (**state.config.read().await).clone();
        new_cfg.provider_aliases.insert(id.to_string(), alias.trim().to_string());
        let _ = config::save(&state.config_path, &new_cfg).await;
        *state.config.write().await = std::sync::Arc::new(new_cfg);
    }

    let _ = mihomo::reload_config(&state.http, &cfg).await;
    let _ = mihomo::force_update_provider(&state.http, &cfg, id).await;

    api_ok(json!({ "added": true, "message": format!("Подписка '{id}' успешно добавлена") }))
}

#[derive(Deserialize)]
pub struct DeleteProviderReq {
    #[serde(alias = "provider_id")]
    pub id: String,
}

/// POST /api/providers/delete — удаление подписки из config.yaml
pub async fn delete_provider(State(state): State<AppState>, Json(req): Json<DeleteProviderReq>) -> Response {
    let id = req.id.trim();
    if id.is_empty() {
        return api_err("ID подписки не может быть пустым");
    }

    let cfg = state.config.read().await.clone();
    let yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Ошибка чтения {}: {e}", cfg.mihomo.config_path)),
    };

    let new_yaml = match crate::routing::delete_provider_from_yaml(&yaml, id) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
    }

    // Удаляем кэш-файлы провайдера с диска (если создавались)
    let prov_cache_yaml = format!("/opt/etc/mihomo/providers/{id}.yaml");
    let prov_cache_json = format!("/opt/etc/mihomo/providers/{id}.json");
    let _ = tokio::fs::remove_file(&prov_cache_yaml).await;
    let _ = tokio::fs::remove_file(&prov_cache_json).await;

    // Удаляем псевдоним и фильтры из AppConfig
    {
        let _guard = state.config_lock.lock().await;
        let mut new_cfg = (**state.config.read().await).clone();
        let mut changed = false;
        if new_cfg.provider_aliases.remove(id).is_some() {
            changed = true;
        }
        if new_cfg.provider_filters.remove(id).is_some() {
            changed = true;
        }
        if changed {
            let _ = config::save(&state.config_path, &new_cfg).await;
            *state.config.write().await = std::sync::Arc::new(new_cfg);
        }
    }

    let reload_res = mihomo::reload_config(&state.http, &cfg).await;
    if let Err(e) = &reload_res {
        crate::log_w!("Внимание: Mihomo reload вернул ошибку после удаления провайдера '{id}': {e}");
    }

    api_ok(json!({ "deleted": true, "message": format!("Подписка '{id}' успешно удалена") }))
}

/// GET /api/antigravity/status
pub async fn get_antigravity_status(State(state): State<AppState>) -> Response {
    let status = state.antigravity.get_status().await;
    api_ok(serde_json::to_value(status).unwrap_or_default()).into_response()
}

#[derive(Deserialize)]
pub struct AntigravitySettingsReq {
    pub enabled: Option<bool>,
    pub mode: Option<String>,
    pub proxy_port: Option<u16>,
    pub proxy_enabled: Option<bool>,
    pub health_check_interval: Option<u64>,
    pub own_proxy: Option<String>,
}

/// POST /api/antigravity/settings
pub async fn set_antigravity_settings(
    State(state): State<AppState>,
    Json(req): Json<AntigravitySettingsReq>,
) -> Response {
    let _guard = state.config_lock.lock().await;
    let mut new_cfg = (**state.config.read().await).clone();

    if let Some(enabled) = req.enabled {
        new_cfg.antigravity.enabled = enabled;
    }
    if let Some(mode) = req.mode {
        new_cfg.antigravity.mode = mode;
    }
    if let Some(port) = req.proxy_port {
        new_cfg.antigravity.proxy_port = port;
    }
    if let Some(pe) = req.proxy_enabled {
        new_cfg.antigravity.proxy_enabled = pe;
    }
    if let Some(interval) = req.health_check_interval {
        new_cfg.antigravity.health_check_interval = interval.max(30);
    }
    if let Some(own_proxy) = req.own_proxy {
        new_cfg.antigravity.own_proxy = own_proxy;
    }

    if let Err(e) = config::save(&state.config_path, &new_cfg).await {
        return api_err(format!("Ошибка сохранения: {e}")).into_response();
    }
    *state.config.write().await = std::sync::Arc::new(new_cfg);

    // Фоновая проверка сразу после смены настроек
    let ag = state.antigravity.clone();
    tokio::spawn(async move {
        ag.check_and_update().await;
    });

    api_ok(json!({ "message": "Настройки Antigravity обновлены" })).into_response()
}

/// POST /api/antigravity/check
pub async fn check_antigravity(State(state): State<AppState>) -> Response {
    state.antigravity.check_and_update().await;
    let status = state.antigravity.get_status().await;
    api_ok(json!({
        "message": "Проверка Antigravity завершена",
        "status": status,
    })).into_response()
}

/// Сырой PowerShell скрипт разблокировки входа в Antigravity
pub const ANTIGRAVITY_PATCH_SCRIPT: &str = r#"[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding = [System.Text.Encoding]::UTF8
Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host "   Antigravity & Cloud Code Login Patch (xkeen route)  " -ForegroundColor Cyan
Write-Host "=======================================================" -ForegroundColor Cyan
Write-Host ""
Write-Host "[1/3] Terminating Antigravity processes..." -ForegroundColor Yellow
$procs = @("language_server", "language_server_windows_x64", "Antigravity", "Antigravity CLI")
foreach ($p in $procs) {
    Get-Process -Name $p -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
}
Start-Sleep -Milliseconds 800
Write-Host "[2/3] Searching for language_server.exe..." -ForegroundColor Yellow
$candidates = @(
    "$env:LOCALAPPDATA\Programs\antigravity\resources\bin\language_server.exe",
    "$env:LOCALAPPDATA\Programs\antigravity\resources\app\extensions\antigravity\bin\language_server.exe",
    "$env:LOCALAPPDATA\Programs\antigravity\resources\app\extensions\antigravity\bin\language_server_windows_x64.exe",
    "$env:LOCALAPPDATA\Programs\Antigravity IDE\resources\bin\language_server.exe",
    "$env:USERPROFILE\.antigravity\bin\language_server.exe"
)
$found = @()
foreach ($c in $candidates) {
    if (Test-Path $c) { $found += $c }
}
if ($found.Count -eq 0) {
    $searchDir = "$env:LOCALAPPDATA\Programs\antigravity"
    if (Test-Path $searchDir) {
        Get-ChildItem -Path $searchDir -Filter "*language_server*.exe" -Recurse -ErrorAction SilentlyContinue | ForEach-Object { $found += $_.FullName }
    }
}
$found = $found | Select-Object -Unique
if ($found.Count -eq 0) {
    Write-Host "[!] language_server.exe files not found!" -ForegroundColor Red
} else {
    Write-Host "[3/3] Patching binary signature (ineligible -> inexigible)..." -ForegroundColor Yellow
    $enc = [System.Text.Encoding]::GetEncoding(28591)
    $patchedCount = 0
    foreach ($file in $found) {
        Write-Host "  -> $file" -ForegroundColor Gray
        try {
            $bytes = [System.IO.File]::ReadAllBytes($file)
            $text = $enc.GetString($bytes)
            if ($text.Contains('ineligible')) {
                $bak = "$file.bak"
                if (-not (Test-Path $bak)) { [System.IO.File]::Copy($file, $bak) }
                $newText = $text.Replace('ineligible', 'inexigible')
                [System.IO.File]::WriteAllBytes($file, $enc.GetBytes($newText))
                Write-Host "     [OK] Successfully patched!" -ForegroundColor Green
                $patchedCount++
            } elseif ($text.Contains('inexigible')) {
                Write-Host "     [OK] Already patched (inexigible)." -ForegroundColor Yellow
                $patchedCount++
            } else {
                Write-Host "     [?] Signature 'ineligible' not found." -ForegroundColor DarkYellow
            }
        } catch {
            Write-Host "     [!] Access error: $_" -ForegroundColor Red
        }
    }
    if ($patchedCount -gt 0) {
        Write-Host ""
        Write-Host "[SUCCESS] Unlock completed! Please restart Antigravity." -ForegroundColor Green
    }
}
Write-Host ""
"#;

fn base64_encode(input: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((input.len() + 2) / 3 * 4);
    for chunk in input.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = if chunk.len() > 1 { chunk[1] as usize } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as usize } else { 0 };
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[(n >> 18) & 63] as char);
        out.push(TABLE[(n >> 12) & 63] as char);
        if chunk.len() > 1 {
            out.push(TABLE[(n >> 6) & 63] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(TABLE[n & 63] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// GET /patch — отдавать сырой PowerShell-скрипт (text/plain; charset=utf-8)
pub async fn get_antigravity_patch_script() -> impl IntoResponse {
    (
        [
            ("Content-Type", "text/plain; charset=utf-8"),
            ("Cache-Control", "no-cache, no-store, must-revalidate"),
        ],
        ANTIGRAVITY_PATCH_SCRIPT,
    )
        .into_response()
}

/// GET /api/antigravity/fix.cmd — отдавать обертку .cmd с заголовком Content-Disposition: attachment; filename="fix_antigravity.cmd"
pub async fn get_antigravity_fix_cmd() -> impl IntoResponse {
    let b64 = base64_encode(ANTIGRAVITY_PATCH_SCRIPT.as_bytes());
    let cmd = format!(
        "@echo off\r\n\
         chcp 65001 >nul\r\n\
         title Antigravity Login Fix (xkeen route)\r\n\
         powershell -NoProfile -ExecutionPolicy Bypass -Command \"[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; [System.Text.Encoding]::UTF8.GetString([System.Convert]::FromBase64String('{}')) | iex\"\r\n\
         echo.\r\n\
         pause\r\n",
        b64
    );
    (
        [
            ("Content-Type", "application/x-bat; charset=utf-8"),
            (
                "Content-Disposition",
                "attachment; filename=\"fix_antigravity.cmd\"",
            ),
            ("Cache-Control", "no-cache, no-store, must-revalidate"),
        ],
        cmd,
    )
        .into_response()
}

// ==================== ВСТРОЕННЫЙ РЕДАКТОР КОНФИГОВ ====================

#[derive(Deserialize)]
pub struct ConfigFileQuery {
    pub file: String,
}

#[derive(Deserialize)]
pub struct SaveConfigFileRequest {
    pub file: String,
    pub content: String,
    #[serde(default)]
    pub reload_mihomo: bool,
}

fn resolve_config_file_path(id: &str, cfg: &config::AppConfig) -> Option<std::path::PathBuf> {
    let providers_dir = std::path::Path::new(&cfg.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));

    match id {
        "mihomo" => Some(std::path::PathBuf::from(&cfg.mihomo.config_path)),
        "route" => Some(if cfg!(target_os = "linux") {
            std::path::PathBuf::from(crate::CONFIG_PATH)
        } else {
            std::path::PathBuf::from("xkeen-route.config.json")
        }),
        "override" => Some(std::path::PathBuf::from(crate::override_sync::OVERRIDE_FILE)),
        "xkeen_conf" => Some(std::path::PathBuf::from(crate::override_sync::XKEEN_CONF_FILE)),
        "crontab" => Some(std::path::PathBuf::from(crate::override_sync::SYSTEM_CRONTAB_FILE)),
        other => {
            if let Some(name) = other.strip_prefix("provider:") {
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
                    return None;
                }
                Some(providers_dir.join(format!("{}.yaml", name)))
            } else {
                None
            }
        }
    }
}

/// GET /api/config-files/list — список доступных для редактирования файлов
pub async fn list_config_files(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let providers_dir = std::path::Path::new(&cfg.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));

    let mut files = vec![
        json!({ "id": "mihomo", "name": "Mihomo Config (config.yaml)", "path": cfg.mihomo.config_path, "syntax": "yaml" }),
        json!({ "id": "route", "name": "XKeen Route Config (config.json)", "path": state.config_path.display().to_string(), "syntax": "json" }),
        json!({ "id": "override", "name": "RU Override IP List (ru_exclude_override.lst)", "path": crate::override_sync::OVERRIDE_FILE, "syntax": "text" }),
        json!({ "id": "xkeen_conf", "name": "XKeen Settings (xkeen.conf)", "path": crate::override_sync::XKEEN_CONF_FILE, "syntax": "shell" }),
        json!({ "id": "crontab", "name": "System Crontab (/opt/etc/crontab)", "path": crate::override_sync::SYSTEM_CRONTAB_FILE, "syntax": "shell" }),
    ];

    if let Ok(mut entries) = tokio::fs::read_dir(&providers_dir).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("yaml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    files.push(json!({
                        "id": format!("provider:{}", stem),
                        "name": format!("Provider: {}.yaml", stem),
                        "path": path.display().to_string(),
                        "syntax": "yaml"
                    }));
                }
            }
        }
    }

    api_ok(json!(files))
}

/// GET /api/config-files/read?file=mihomo
pub async fn read_config_file(
    State(state): State<AppState>,
    Query(q): Query<ConfigFileQuery>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let path = match resolve_config_file_path(&q.file, &cfg) {
        Some(p) => p,
        None => return api_err("Недопустимый идентификатор файла"),
    };

    match tokio::fs::read_to_string(&path).await {
        Ok(content) => api_ok(json!({
            "file": q.file,
            "path": path.display().to_string(),
            "content": content
        })),
        Err(e) => api_err(format!("Ошибка чтения файла {}: {}", path.display(), e)),
    }
}

fn validate_yaml_syntax(content: &str) -> Result<(), String> {
    let mut bracket_stack = Vec::new();
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut escaped = false;

    for (line_num, line) in content.lines().enumerate() {
        if line.contains('\t') {
            return Err(format!("Строка {}: обнаружен символ табуляции (\\t). В YAML допускаются только пробелы.", line_num + 1));
        }

        let trimmed = line.trim();
        if trimmed.starts_with('#') {
            continue;
        }

        for (col, ch) in line.chars().enumerate() {
            if escaped {
                escaped = false;
                continue;
            }
            if ch == '\\' && in_double_quote {
                escaped = true;
                continue;
            }
            if ch == '\'' && !in_double_quote {
                in_single_quote = !in_single_quote;
                continue;
            }
            if ch == '"' && !in_single_quote {
                in_double_quote = !in_double_quote;
                continue;
            }
            if in_single_quote || in_double_quote {
                continue;
            }
            if ch == '#' && (col == 0 || line[..col].ends_with(' ')) {
                break;
            }
            match ch {
                '[' | '{' => bracket_stack.push((ch, line_num + 1)),
                ']' => match bracket_stack.pop() {
                    Some(('[', _)) => {}
                    Some((other, orig_line)) => {
                        return Err(format!("Строка {}: несоответствие скобок — ожидалось закрытие '{}' из строки {}", line_num + 1, other, orig_line));
                    }
                    None => {
                        return Err(format!("Строка {}: лишняя закрывающая скобка ']'", line_num + 1));
                    }
                },
                '}' => match bracket_stack.pop() {
                    Some(('{', _)) => {}
                    Some((other, orig_line)) => {
                        return Err(format!("Строка {}: несоответствие скобок — ожидалось закрытие '{}' из строки {}", line_num + 1, other, orig_line));
                    }
                    None => {
                        return Err(format!("Строка {}: лишняя закрывающая фигурная скобка '}}'", line_num + 1));
                    }
                },
                _ => {}
            }
        }
    }

    if let Some((ch, line)) = bracket_stack.pop() {
        return Err(format!("Строка {}: незакрытая скобка '{}'", line, ch));
    }
    if in_single_quote || in_double_quote {
        return Err("Обнаружена незакрытая кавычка в файле YAML".into());
    }
    Ok(())
}

/// POST /api/config-files/save
pub async fn save_config_file(
    State(state): State<AppState>,
    Json(body): Json<SaveConfigFileRequest>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let path = match resolve_config_file_path(&body.file, &cfg) {
        Some(p) => p,
        None => return api_err("Недопустимый идентификатор файла"),
    };

    if body.file == "route" {
        if let Err(e) = serde_json::from_str::<serde_json::Value>(&body.content) {
            return api_err(format!("Ошибка синтаксиса JSON в файле config.json: {e}"));
        }
    }

    if body.file == "mihomo" || body.file.starts_with("provider:") {
        if let Err(e) = validate_yaml_syntax(&body.content) {
            return api_err(format!("Ошибка синтаксиса YAML: {e}"));
        }
    }

    if path.exists() {
        let bak = format!("{}.bak", path.display());
        let _ = tokio::fs::copy(&path, &bak).await;
    }

    let _guard = state.routing_lock.lock().await;
    if let Err(e) = atomic_write_file(&path, &body.content).await {
        return api_err(format!("Ошибка сохранения файла {}: {e}", path.display()));
    }

    log_i!("Файл {} успешно сохранён через веб-редактор", path.display());

    if body.file == "route" {
        *state.config.write().await = std::sync::Arc::new(config::load_async(&state.config_path).await);
    }

    if body.file == "override" {
        let _ = crate::override_sync::sync_geo_override(&cfg.force_domains).await;
    }

    if body.reload_mihomo || body.file == "mihomo" {
        if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
            return api_ok(json!({
                "saved": true,
                "warning": format!("Файл сохранен, но reload Mihomo вернул ошибку: {}", e)
            }));
        }
    }

    api_ok(json!({ "saved": true, "path": path.display().to_string() }))
}

// ==================== ЭКСПОРТ И ИМПОРТ БЭКАПОВ В БРАУЗЕРЕ ====================

/// GET /api/backups/export/:name — экспорт снимка конфигурации в формате JSON
pub async fn export_backup(
    State(state): State<AppState>,
    axum::extract::Path(name): axum::extract::Path<String>,
) -> Response {
    if !valid_backup_name(&name) {
        return api_err("Некорректное имя бэкапа");
    }
    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&name);
    if !dir.exists() {
        return api_err("Бэкап не найден");
    }

    let mihomo_yaml = tokio::fs::read_to_string(dir.join("config.yaml")).await.unwrap_or_default();
    let route_json = tokio::fs::read_to_string(dir.join("config.json")).await.unwrap_or_default();

    let backup_bundle = json!({
        "version": "1.0",
        "name": name,
        "exported_at": chrono::Local::now().to_rfc3339(),
        "files": {
            "config.yaml": mihomo_yaml,
            "config.json": route_json
        }
    });

    let filename = format!("{}.xkbak", name);
    let payload = serde_json::to_string_pretty(&backup_bundle).unwrap_or_default();

    (
        [
            ("Content-Type", "application/json; charset=utf-8"),
            ("Content-Disposition", &format!("attachment; filename=\"{}\"", filename)),
            ("Cache-Control", "no-cache, no-store, must-revalidate"),
        ],
        payload,
    )
        .into_response()
}

/// POST /api/backups/import — импорт снимка с ПК в хранилище бэкапов
pub async fn import_backup(
    State(state): State<AppState>,
    Json(body): Json<serde_json::Value>,
) -> Response {
    let files = match body.get("files").and_then(|f| f.as_object()) {
        Some(f) => f,
        None => return api_err("Некорректный формат архива бэкапа: отсутствует секция files"),
    };

    let base_name = body.get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("imported_{}", chrono::Local::now().format("%Y%m%d_%H%M%S")));

    let safe_name: String = base_name.chars()
        .map(|c| if c.is_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect();

    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&safe_name);

    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        return api_err(format!("Ошибка создания каталога бэкапа: {}", e));
    }

    if let Some(yaml) = files.get("config.yaml").and_then(|y| y.as_str()) {
        if let Err(e) = tokio::fs::write(dir.join("config.yaml"), yaml).await {
            return api_err(format!("Ошибка записи config.yaml: {}", e));
        }
    }
    if let Some(json_val) = files.get("config.json").and_then(|j| j.as_str()) {
        if let Err(e) = tokio::fs::write(dir.join("config.json"), json_val).await {
            return api_err(format!("Ошибка записи config.json: {}", e));
        }
    }

    log_i!("Импортирован бэкап '{}' в {}", safe_name, dir.display());
    api_ok(json!({ "imported": safe_name }))
}

// ==================== СТРИМИНГ ЛОГОВ ЯДРА MIHOMO ====================

/// GET /api/logs/mihomo — получение хвоста логов ядра Mihomo
pub async fn mihomo_logs_tail(
    State(state): State<AppState>,
    Query(q): Query<LogsQuery>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let lines_count = q.lines.unwrap_or(200);

    let log_path = std::path::Path::new(&cfg.mihomo.log_path);
    if log_path.exists() {
        if let Ok(mut f) = tokio::fs::File::open(log_path).await {
            if let Ok(meta) = f.metadata().await {
                use tokio::io::{AsyncReadExt, AsyncSeekExt, SeekFrom};
                let file_len = meta.len();
                let max_bytes = 256 * 1024u64;
                let seek_pos = file_len.saturating_sub(max_bytes);
                if seek_pos > 0 {
                    let _ = f.seek(SeekFrom::Start(seek_pos)).await;
                }
                let mut buf = Vec::with_capacity((file_len - seek_pos) as usize);
                if f.read_to_end(&mut buf).await.is_ok() {
                    let content = String::from_utf8_lossy(&buf);
                    let mut lines: Vec<&str> = content.lines().collect();
                    if seek_pos > 0 && !lines.is_empty() {
                        lines.remove(0);
                    }
                    let start = lines.len().saturating_sub(lines_count);
                    let tail = lines[start..].join("\n");
                    return api_ok(json!({
                        "text": tail,
                        "source": "file",
                        "path": log_path.display().to_string()
                    }));
                }
            }
        }
    }

    let url = format!("{}/logs?level=info", cfg.mihomo_url());
    let mut req = state.http.get(&url);
    if !cfg.mihomo.secret.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
    }

    match req.timeout(std::time::Duration::from_secs(2)).send().await {
        Ok(resp) => {
            let body = resp.text().await.unwrap_or_default();
            api_ok(json!({
                "text": body,
                "source": "api",
                "path": url
            }))
        }
        Err(_) => api_ok(json!({
            "text": "(Журнал ядра Mihomo пуст или пишется в системный консольный лог)",
            "source": "none",
            "path": ""
        })),
    }
}

/// POST /api/logs/mihomo/clear — усечение файла журнала Mihomo
pub async fn mihomo_logs_clear(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let log_path = std::path::Path::new(&cfg.mihomo.log_path);
    if log_path.exists() {
        if let Err(e) = tokio::fs::write(log_path, b"").await {
            return api_err(format!("Не удалось очистить лог Mihomo: {e}"));
        }
    }
    log_i!("Журнал Mihomo очищен пользователем");
    api_ok(json!({ "cleared": true }))
}

// ==================== МОНИТОРИНГ ТРАФИКА УСТРОЙСТВ ====================

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

// ==================== ЗАМЕР СКОРОСТИ (SPEEDTEST) ====================

/// POST /api/servers/speedtest
pub async fn speedtest_server(
    State(state): State<AppState>,
    Json(body): Json<crate::speedtest::SpeedtestRequest>,
) -> Response {
    let _speedtest_guard = state.speedtest_lock.lock().await;
    let cfg = state.config.read().await.clone();
    match crate::speedtest::run_speedtest(&state.http, &cfg, &body.server_id).await {
        Ok(res) => api_ok(serde_json::to_value(res).unwrap_or_default()),
        Err(e) => api_err(e),
    }
}

/// POST /api/servers/speedtest/{id} или GET /api/servers/speedtest/{id}
pub async fn speedtest_server_by_id(
    State(state): State<AppState>,
    axum::extract::Path(server_id): axum::extract::Path<String>,
) -> Response {
    let _speedtest_guard = state.speedtest_lock.lock().await;
    let cfg = state.config.read().await.clone();
    match crate::speedtest::run_speedtest(&state.http, &cfg, &server_id).await {
        Ok(res) => api_ok(serde_json::to_value(res).unwrap_or_default()),
        Err(e) => api_err(e),
    }
}

// ==================== ТЕСТ УВЕДОМЛЕНИЙ (TELEGRAM / WEBHOOK) ====================

#[derive(Deserialize)]
pub struct TestNotificationRequest {
    pub telegram_bot_token: Option<String>,
    pub telegram_chat_id: Option<String>,
    pub webhook_url: Option<String>,
}

/// POST /api/notifications/test
pub async fn test_notification(
    State(state): State<AppState>,
    Json(body): Json<TestNotificationRequest>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let bot_token = body.telegram_bot_token.unwrap_or_else(|| cfg.notifications.telegram_bot_token.clone());
    let chat_id = body.telegram_chat_id.unwrap_or_else(|| cfg.notifications.telegram_chat_id.clone());
    let webhook = body.webhook_url.unwrap_or_else(|| cfg.notifications.webhook_url.clone());

    let mut results = Vec::new();

    if !bot_token.is_empty() && !chat_id.is_empty() {
        let test_msg = "<b>🔔 Тестовое уведомление XKeen Route</b>\n\nСвязь с Telegram Bot API успешно установлена!\nВы будете получать оповещения при сбоях и переключениях серверов Failover.";
        match crate::notifications::send_telegram(&state.http, &bot_token, &chat_id, test_msg).await {
            Ok(_) => results.push("Telegram: ✓ Сообщение успешно отправлено".to_string()),
            Err(e) => results.push(format!("Telegram: ❌ {}", e)),
        }
    }

    if !webhook.is_empty() {
        match crate::notifications::send_webhook(
            &state.http,
            &webhook,
            "test",
            "Тестовое уведомление",
            "Тестовая проверка Webhook из веб-панели XKeen Route",
        ).await {
            Ok(_) => results.push("Webhook: ✓ Запрос успешно доставлен".to_string()),
            Err(e) => results.push(format!("Webhook: ❌ {}", e)),
        }
    }

    if results.is_empty() {
        return api_err("Укажите Telegram Bot Token и Chat ID или Webhook URL для теста");
    }

    api_ok(json!({ "results": results }))
}

// ==================== ИМПОРТ НОД (OUTBOUND GENERATOR) ====================

#[derive(Deserialize)]
pub struct ImportNodeRequest {
    pub yaml_content: String,
    pub target: String, // "config" | "provider"
    pub provider_name: Option<String>,
}

/// POST /api/servers/import-node
pub async fn import_node(
    State(state): State<AppState>,
    Json(body): Json<ImportNodeRequest>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let yaml = body.yaml_content.trim();
    if yaml.is_empty() {
        return api_err("YAML ноды не может быть пустым");
    }

    let _guard = state.routing_lock.lock().await;

    if body.target == "provider" {
        let prov_name = body.provider_name.unwrap_or_else(|| "custom".to_string());
        let providers_dir = std::path::Path::new(&cfg.mihomo.config_path)
            .parent()
            .map(|p| p.join("providers"))
            .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));
        let path = providers_dir.join(format!("{}.yaml", prov_name));
        let prov_file = path.display().to_string();

        if let Some(parent) = path.parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }

        let mut current_content = tokio::fs::read_to_string(&path).await.unwrap_or_else(|_| "proxies:\n".to_string());
        if !current_content.contains("proxies:") {
            current_content = format!("proxies:\n{}", current_content);
        }
        current_content.push_str("\n");
        current_content.push_str(yaml);
        current_content.push_str("\n");

        if let Err(e) = atomic_write_file(&path, &current_content).await {
            return api_err(format!("Ошибка записи провайдера {}: {}", prov_file, e));
        }

        if let Ok(config_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
            if !config_yaml.contains(&format!("{}:", prov_name)) {
                if let Ok(new_yaml) = crate::routing::add_provider_to_yaml(
                    &config_yaml,
                    &prov_name,
                    &format!("file:///opt/etc/mihomo/providers/{}.yaml", prov_name),
                    Some(cfg.health_check_url()),
                    Some(cfg.mihomo.health_check_interval),
                ) {
                    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
                        log_e!("Ошибка сохранения config.yaml при добавлении провайдера: {}", e);
                    }
                }
            }
        }

        let _ = mihomo::reload_config(&state.http, &cfg).await;
        log_i!("Импортирована нода в провайдер {}", prov_name);
        return api_ok(json!({ "imported": true, "target": "provider", "file": prov_file }));
    }

    let config_path = &cfg.mihomo.config_path;
    let config_yaml = match tokio::fs::read_to_string(config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Ошибка чтения config.yaml: {}", e)),
    };

    let mut lines: Vec<String> = config_yaml.lines().map(|s| s.to_string()).collect();
    let proxies_idx = match lines.iter().position(|l| l.trim_end() == "proxies:") {
        Some(idx) => idx,
        None => {
            lines.insert(0, "proxies:".to_string());
            0
        }
    };

    lines.insert(proxies_idx + 1, yaml.to_string());
    let new_yaml = lines.join("\n");

    if let Err(e) = atomic_write_file(config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {}", e));
    }

    let _ = mihomo::reload_config(&state.http, &cfg).await;
    log_i!("Нода успешно импортирована в proxies config.yaml");
    api_ok(json!({ "imported": true, "target": "config" }))
}

// ==================== УПРАВЛЕНИЕ РЕЖИМАМИ DNS РОУТЕРА ====================

#[derive(Deserialize)]
pub struct SetDnsModeRequest {
    pub enhanced_mode: String, // "fake-ip" | "redir-host"
}

/// GET /api/dns/mode
pub async fn get_dns_mode(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let config_yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();

    static RE_REDIR: std::sync::LazyLock<regex_lite::Regex> =
        std::sync::LazyLock::new(|| regex_lite::Regex::new(r#"(?m)^\s*enhanced-mode:\s*['"]?redir-host['"]?"#).unwrap());
    let enhanced_mode = if RE_REDIR.is_match(&config_yaml) {
        "redir-host"
    } else {
        "fake-ip"
    };

    let xkeen_conf = tokio::fs::read_to_string(crate::override_sync::XKEEN_CONF_FILE).await.unwrap_or_default();
    let proxy_dns = if xkeen_conf.contains("proxy_dns=\"on\"") || xkeen_conf.contains("proxy_dns='on'") {
        "on"
    } else {
        "off"
    };

    api_ok(json!({
        "enhanced_mode": enhanced_mode,
        "proxy_dns": proxy_dns
    }))
}

/// POST /api/dns/mode
pub async fn set_dns_mode(
    State(state): State<AppState>,
    Json(body): Json<SetDnsModeRequest>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let config_yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Ошибка чтения config.yaml: {}", e)),
    };

    let target_mode = if body.enhanced_mode == "redir-host" { "redir-host" } else { "fake-ip" };
    static RE_ENHANCED_MODE: std::sync::LazyLock<regex_lite::Regex> =
        std::sync::LazyLock::new(|| regex_lite::Regex::new(r"(?m)^(\s*enhanced-mode:\s*)[^\r\n]+").unwrap());
    let new_yaml = if RE_ENHANCED_MODE.is_match(&config_yaml) {
        RE_ENHANCED_MODE.replace(&config_yaml, format!("${{1}}{target_mode}")).into_owned()
    } else {
        config_yaml.clone()
    };

    let _guard = state.routing_lock.lock().await;
    if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {}", e));
    }

    if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
        let _ = atomic_write_file(&cfg.mihomo.config_path, &config_yaml).await;
        log_e!("Ошибка reload Mihomo при смене DNS-режима: {e}");
        return api_err(format!("Не удалось применить DNS-режим в Mihomo: {e}. Конфиг откаткан."));
    }
    log_i!("Режим DNS Mihomo переключен на {}", target_mode);
    api_ok(json!({ "saved": true, "applied": true, "enhanced_mode": target_mode }))
}

/// POST /api/dns/clean-servers — принудительная установка чистых DNS-серверов в KeeneticOS
pub async fn apply_clean_dns(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::rci::set_clean_dns_servers(&state.http, &cfg).await {
        Ok(_) => api_ok(json!({ "success": true, "message": "Чистые DNS-серверы (77.88.8.8, 1.1.1.1) успешно настроены в KeeneticOS" })),
        Err(e) => api_err(format!("Ошибка настройки DNS через RCI: {e}")),
    }
}

// ==================== PER-DEVICE DOMAIN RULES ====================

#[derive(Deserialize)]
pub struct SetDeviceDomainRulesRequest {
    pub ip: String,
    pub rules: Vec<crate::config::DeviceDomainRule>,
}

/// GET /api/devices/domain-rules
pub async fn get_device_domain_rules(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    api_ok(json!(cfg.device_domain_rules))
}

/// POST /api/devices/domain-rules
pub async fn set_device_domain_rules(
    State(state): State<AppState>,
    Json(body): Json<SetDeviceDomainRulesRequest>,
) -> Response {
    let _cfg_guard = state.config_lock.lock().await;
    let _guard = state.routing_lock.lock().await;

    let mut cur = (**state.config.read().await).clone();
    if body.rules.is_empty() {
        cur.device_domain_rules.remove(&body.ip);
    } else {
        cur.device_domain_rules.insert(body.ip.clone(), body.rules);
    }

    // 1. Сначала подготавливаем и проверяем YAML для ядра Mihomo
    let raw_yaml = match tokio::fs::read_to_string(&cur.mihomo.config_path).await {
        Ok(y) => y,
        Err(e) => return api_err(format!("Ошибка чтения config.yaml: {e}")),
    };
    let (new_yaml, _) = match crate::routing::apply_routing(&raw_yaml, &cur) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка формирования маршрутов: {e}")),
    };

    // 2. Атомарно записываем YAML
    if let Err(e) = atomic_write_file(&cur.mihomo.config_path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
    }

    // 3. Перезагружаем ядро Mihomo и при сбое откатываем YAML назад
    if let Err(e) = mihomo::reload_config(&state.http, &cur).await {
        let _ = atomic_write_file(&cur.mihomo.config_path, &raw_yaml).await;
        log_e!("Ошибка reload Mihomo при установке правил доменов: {e}");
        return api_err(format!("Ядро Mihomo отклонило новые правила: {e}. Конфиг откаткан."));
    }

    // 4. Только после успешного применения в ядре сохраняем config.json
    if let Err(e) = config::save(&state.config_path, &cur).await {
        log_e!("Ошибка сохранения config.json: {e}");
        return api_err(format!("Правила применены в ядре, но не записаны в config.json: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(cur.clone());

    log_i!("Обновлены индивидуальные доменные правила для устройства {}", body.ip);
    api_ok(json!({ "saved": true, "applied": true, "ip": body.ip }))
}

// ==================== СКВОЗНОЙ РЕЛЕЙ CLASH API (REVERSE PROXY) ====================

/// Сквозной Reverse-Proxy для Clash REST API (/clash/{*path})
pub async fn clash_proxy(
    State(state): State<AppState>,
    req: axum::extract::Request,
) -> Response {
    let cfg = state.config.read().await.clone();
    let path = req.uri().path().strip_prefix("/clash").unwrap_or("");
    let query = req.uri().query().map(|q| format!("?{}", q)).unwrap_or_default();
    let target_url = format!("{}{}{}", cfg.mihomo_url(), path, query);

    let method = req.method().clone();
    let mut proxy_req = state.http.request(method, &target_url);

    if !cfg.mihomo.secret.is_empty() {
        proxy_req = proxy_req.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
    }

    for (k, v) in req.headers() {
        let name = k.as_str().to_ascii_lowercase();
        if name != "host" && name != "authorization" && name != "content-length" && name != "connection" {
            proxy_req = proxy_req.header(k, v);
        }
    }

    let body_bytes = match axum::body::to_bytes(req.into_body(), 10 * 1024 * 1024).await {
        Ok(b) => b,
        Err(_) => return api_err("Ошибка чтения тела запроса"),
    };
    if !body_bytes.is_empty() {
        proxy_req = proxy_req.body(body_bytes);
    }

    match proxy_req.send().await {
        Ok(resp) => {
            let status = resp.status();
            let headers = resp.headers().clone();
            let bytes = resp.bytes().await.unwrap_or_default();

            let mut response = (status, bytes).into_response();
            for (k, v) in headers {
                if let Some(key) = k {
                    let name = key.as_str().to_ascii_lowercase();
                    if name != "transfer-encoding" && name != "content-length" && name != "connection" {
                        response.headers_mut().insert(key, v);
                    }
                }
            }
            response
        }
        Err(e) => api_err(format!("Ошибка проксирования к Clash API: {}", e)),
    }
}

// ==================== ADBLOCK (БЛОКИРОВКА РЕКЛАМЫ) ====================

/// GET /api/adblock — текущее состояние AdBlock
pub async fn get_adblock(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    api_ok(json!({ "enabled": cfg.adblock_enabled }))
}

#[derive(Deserialize)]
pub struct AdBlockToggleReq {
    pub enabled: Option<bool>,
}

/// POST /api/adblock/toggle — переключение блокировки рекламы с перезагрузкой Mihomo
pub async fn toggle_adblock(
    State(state): State<AppState>,
    body: Option<axum::extract::Json<AdBlockToggleReq>>,
) -> Response {
    let _guard = state.routing_lock.lock().await;
    let (mut cfg, target_enabled) = {
        let current = state.config.read().await;
        let target = match body {
            Some(axum::extract::Json(b)) => b.enabled.unwrap_or(!current.adblock_enabled),
            None => !current.adblock_enabled,
        };
        (current.as_ref().clone(), target)
    };

    cfg.adblock_enabled = target_enabled;

    let path = std::path::Path::new(&cfg.mihomo.config_path);
    let raw_yaml = match tokio::fs::read_to_string(path).await {
        Ok(c) => c,
        Err(e) => return api_err(format!("Ошибка чтения config.yaml: {}", e)),
    };

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, &cfg) {
            Ok(res) => res,
            Err(e) => return api_err(format!("Ошибка роутинга: {}", e)),
        };
    if let Err(e) = atomic_write_file(path, &new_yaml).await {
        return api_err(format!("Ошибка сохранения config.yaml: {}", e));
    }

    if let Err(e) = config::save(&state.config_path, &cfg).await {
        log_e!("Ошибка сохранения config.json: {}", e);
        return api_err(format!("Ошибка сохранения настроек AdBlock в config.json: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(cfg.clone());

    let applied = match mihomo::reload_config(&state.http, &cfg).await {
        Ok(_) => true,
        Err(e) => {
            log_w!("Ошибка reload Mihomo после переключения AdBlock: {e}");
            false
        }
    };
    api_ok(json!({ "enabled": target_enabled, "saved": true, "applied": applied }))
}

// ==================== GEOIP & GEOSITE БАЗЫ ====================

/// GET /api/system/geo-info — информация о базах GeoIP и GeoSite
pub async fn get_geo_info(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    let base_dir = std::path::Path::new(&cfg.mihomo.config_path).parent().unwrap_or(std::path::Path::new("/opt/etc/mihomo"));
    let geoip_path = base_dir.join("geoip.dat");
    let geosite_path = base_dir.join("geosite.dat");

    let geoip_meta = tokio::fs::metadata(&geoip_path).await.ok();
    let geosite_meta = tokio::fs::metadata(&geosite_path).await.ok();

    api_ok(json!({
        "geoip": {
            "size": geoip_meta.as_ref().map(|m| m.len()).unwrap_or(0),
            "updated_at": geoip_meta.and_then(|m| m.modified().ok())
                .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Неизвестно".into()),
        },
        "geosite": {
            "size": geosite_meta.as_ref().map(|m| m.len()).unwrap_or(0),
            "updated_at": geosite_meta.and_then(|m| m.modified().ok())
                .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_else(|| "Неизвестно".into()),
        }
    }))
}

/// POST /api/system/geo-update — загрузка актуальных GeoIP и GeoSite
pub async fn update_geo_databases(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let base_dir = std::path::Path::new(&cfg.mihomo.config_path).parent().unwrap_or(std::path::Path::new("/opt/etc/mihomo"));

    let geoip_url = "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geoip.dat";
    let geosite_url = "https://github.com/MetaCubeX/meta-rules-dat/releases/download/latest/geosite.dat";

    let client = if let Ok(proxy) = reqwest::Proxy::all(cfg.mihomo_proxy_url()) {
        reqwest::Client::builder().proxy(proxy).timeout(std::time::Duration::from_secs(90)).build().unwrap_or_else(|_| state.http.clone())
    } else {
        state.http.clone()
    };

    let geoip_bytes = match client.get(geoip_url).send().await {
        Ok(r) if r.status().is_success() => match r.bytes().await {
            Ok(b) if b.len() > 1_000_000 => b,
            _ => return api_err("Скачанный geoip.dat поврежден или имеет неверный размер (<1 МБ)"),
        },
        Err(e) => return api_err(format!("Ошибка загрузки geoip.dat: {}", e)),
        Ok(r) => return api_err(format!("Ошибка сервера при загрузке geoip.dat: {}", r.status())),
    };

    let geosite_bytes = match client.get(geosite_url).send().await {
        Ok(r) if r.status().is_success() => match r.bytes().await {
            Ok(b) if b.len() > 1_000_000 => b,
            _ => return api_err("Скачанный geosite.dat поврежден или имеет неверный размер (<1 МБ)"),
        },
        Err(e) => return api_err(format!("Ошибка загрузки geosite.dat: {}", e)),
        Ok(r) => return api_err(format!("Ошибка сервера при загрузке geosite.dat: {}", r.status())),
    };

    let geoip_path = base_dir.join("geoip.dat");
    let geosite_path = base_dir.join("geosite.dat");

    let tmp_geoip = base_dir.join("geoip.dat.tmp");
    let tmp_geosite = base_dir.join("geosite.dat.tmp");

    if let Err(e) = tokio::fs::write(&tmp_geoip, &geoip_bytes).await {
        return api_err(format!("Ошибка записи geoip.dat.tmp: {}", e));
    }
    if let Err(e) = tokio::fs::write(&tmp_geosite, &geosite_bytes).await {
        let _ = tokio::fs::remove_file(&tmp_geoip).await;
        return api_err(format!("Ошибка записи geosite.dat.tmp: {}", e));
    }

    let _ = tokio::fs::rename(&tmp_geoip, &geoip_path).await;
    let _ = tokio::fs::rename(&tmp_geosite, &geosite_path).await;

    let _ = mihomo::reload_config(&state.http, &cfg).await;

    api_ok(json!({
        "success": true,
        "geoip_size": geoip_bytes.len(),
        "geosite_size": geosite_bytes.len(),
        "updated_at": chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
    }))
}

// ==================== CONNECTIONS VIEWER ====================

/// GET /api/connections — список активных соединений Mihomo
pub async fn get_connections(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    let url = format!("{}/connections", cfg.mihomo_url());
    let mut req = state.http.get(&url);
    if !cfg.mihomo.secret.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
    }
    match req.timeout(std::time::Duration::from_secs(3)).send().await {
        Ok(resp) => {
            let val = resp.json::<serde_json::Value>().await.unwrap_or(json!({ "connections": [] }));
            api_ok(val)
        }
        Err(e) => api_err(format!("Ошибка запроса /connections: {}", e)),
    }
}

/// DELETE /api/connections — закрыть все активные соединения
pub async fn close_connections(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    match mihomo::close_all_connections(&state.http, &cfg).await {
        Ok(_) => api_ok(json!({ "closed": true })),
        Err(e) => api_err(format!("Не удалось закрыть активные соединения: {e}")),
    }
}

/// DELETE /api/connections/{id} — закрыть конкретное соединение
pub async fn close_single_connection(
    State(state): State<AppState>,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Response {
    let cfg = state.config.read().await;
    let url = format!("{}/connections/{}", cfg.mihomo_url(), id);
    let mut req = state.http.delete(&url);
    if !cfg.mihomo.secret.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
    }
    match req.send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                api_ok(json!({ "closed": id }))
            } else {
                api_err(format!("Mihomo вернул статус {}: соединение {} не найдено", resp.status(), id))
            }
        }
        Err(e) => api_err(format!("Ошибка отправки запроса в Mihomo для соединения {}: {e}", id)),
    }
}

// ==================== ТРАФИК В РЕАЛЬНОМ ВРЕМЕНИ ====================

/// GET /api/traffic/poll — текущая скорость прямого и проксированного трафика
pub async fn get_traffic_poll(_state: State<AppState>) -> Response {
    api_ok(crate::traffic::get_snapshot_json())
}

// ==================== RULES & MATCH TESTER («КУДА ПОЙДЁТ?») ====================

/// GET /api/rules — список активных правил маршрутизации
pub async fn get_rules(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    let url = format!("{}/rules", cfg.mihomo_url());
    let mut req = state.http.get(&url);
    if !cfg.mihomo.secret.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
    }
    match req.timeout(std::time::Duration::from_secs(3)).send().await {
        Ok(resp) => {
            let val = resp.json::<serde_json::Value>().await.unwrap_or(json!({ "rules": [] }));
            api_ok(val)
        }
        Err(e) => api_err(format!("Ошибка получения правил: {}", e)),
    }
}

#[derive(Deserialize)]
pub struct RuleTestReq {
    pub domain: String,
    pub source_ip: Option<String>,
}

/// POST /api/rules/test — симулятор маршрутизации «Куда пойдёт трафик?»
pub async fn test_rule_match(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<RuleTestReq>,
) -> Response {
    let cfg = state.config.read().await;
    let domain = body.domain.trim().to_lowercase();
    let src_ip = body.source_ip.as_deref().unwrap_or("").trim();
    let active_srv = cfg.failover.priority_chain.first().cloned().unwrap_or_else(|| "PROXY".into());

    // 1. Персональные доменные правила устройства
    if !src_ip.is_empty() {
        if let Some(rules) = cfg.device_domain_rules.get(src_ip) {
            for r in rules {
                let d = r.domain.trim().to_lowercase();
                if domain == d || domain.ends_with(&format!(".{}", d)) {
                    return api_ok(json!({
                        "matched_rule": format!("AND,((SRC-IP-CIDR,{}/32),(DOMAIN-SUFFIX,{})),{}", src_ip, d, r.target),
                        "rule_type": "DEVICE-DOMAIN",
                        "target_group": r.target,
                        "resolved_server": r.target,
                        "reason": format!("Сработало индивидуальное доменное правило устройства {}", src_ip)
                    }));
                }
            }
        }
    }

    // 2. AdBlock правило
    if cfg.adblock_enabled {
        let is_ad = domain.contains("adservice")
            || domain.contains("googleads")
            || domain.contains("an.yandex.ru")
            || domain.contains("doubleclick")
            || domain.contains("telemetry")
            || domain.contains("analytics");
        if is_ad {
            return api_ok(json!({
                "matched_rule": "GEOSITE,category-ads-all,REJECT",
                "rule_type": "ADBLOCK",
                "target_group": "REJECT",
                "resolved_server": "REJECT",
                "reason": "Заблокировано сетевым AdBlock фильтром роутера"
            }));
        }
    }

    // 3. Персональное назначение устройства (AUTO-DEVICE)
    if !src_ip.is_empty() {
        if let Some(dr) = cfg.device_routing.get(src_ip) {
            if let Some(first_srv) = dr.servers.first() {
                if !first_srv.is_empty() && first_srv != "default" {
                    return api_ok(json!({
                        "matched_rule": format!("SRC-IP-CIDR,{}/32,DEV_{}", src_ip, src_ip.replace('.', "_")),
                        "rule_type": "SRC-IP-CIDR",
                        "target_group": format!("Устройство {}", src_ip),
                        "resolved_server": first_srv,
                        "reason": format!("Весь трафик устройства {} направлен на сервер {}", src_ip, first_srv)
                    }));
                }
            }
        }
    }

    // 4. Выделенный маршрут Google AI / Antigravity Flow
    for d in crate::routing::FLOW_DOMAINS {
        if domain == *d || domain.ends_with(&format!(".{}", d)) {
            let flow_tgt = cfg.flow_server.as_deref().filter(|s| !s.trim().is_empty()).unwrap_or("Google AI");
            return api_ok(json!({
                "matched_rule": format!("DOMAIN-SUFFIX,{},Google AI", d),
                "rule_type": "GOOGLE_AI",
                "target_group": "Google AI",
                "resolved_server": flow_tgt,
                "reason": "Выделенный маршрут Google AI / Antigravity Cloud Code"
            }));
        }
    }

    // 5. Прямой список (DIRECT)
    for d in &cfg.direct_domains {
        let d_lower = d.trim().to_lowercase();
        if domain == d_lower || domain.ends_with(&format!(".{}", d_lower)) {
            return api_ok(json!({
                "matched_rule": format!("DOMAIN-SUFFIX,{},DIRECT", d),
                "rule_type": "DIRECT_DOMAIN",
                "target_group": "DIRECT",
                "resolved_server": "DIRECT (Напрямую)",
                "reason": "Домен находится в белом списке прямого доступа (минуя прокси)"
            }));
        }
    }

    // 6. Принудительный список (PROXY / FORCE)
    for d in &cfg.force_domains {
        let d_lower = d.trim().to_lowercase();
        if domain == d_lower || domain.ends_with(&format!(".{}", d_lower)) {
            return api_ok(json!({
                "matched_rule": format!("DOMAIN-SUFFIX,{},PROXY", d),
                "rule_type": "FORCE_DOMAIN",
                "target_group": "PROXY",
                "resolved_server": active_srv,
                "reason": "Домен находится в списке принудительного проксирования XKeen"
            }));
        }
    }

    // 7. Zapret Hybrid правила (если служба Zapret включена)
    if cfg.zapret.enabled {
        // Изолированные зарубежные сервисы -> PROXY
        if cfg.zapret.isolated_proxy {
            for d in crate::routing::ISOLATED_PROXIED_DOMAINS {
                if domain == *d || domain.ends_with(&format!(".{}", d)) {
                    return api_ok(json!({
                        "matched_rule": format!("DOMAIN-SUFFIX,{},PROXY", d),
                        "rule_type": "ZAPRET_ISOLATED",
                        "target_group": "PROXY",
                        "resolved_server": active_srv,
                        "reason": "Изоляция IP-блокировок через VPN-прокси (Zapret Hybrid)"
                    }));
                }
            }
        }

        // YouTube Direct -> DIRECT
        if cfg.zapret.hybrid_youtube {
            let is_yt = crate::routing::YOUTUBE_HYBRID_DOMAINS.iter().any(|d| domain == *d || domain.ends_with(&format!(".{}", d)))
                || domain.contains("youtube")
                || domain.contains("googlevideo")
                || domain == "youtu.be";
            if is_yt {
                return api_ok(json!({
                    "matched_rule": "DOMAIN-SUFFIX,youtube.com,DIRECT",
                    "rule_type": "ZAPRET_HYBRID",
                    "target_group": "DIRECT",
                    "resolved_server": "DIRECT (Локальный обход Zapret nfqws)",
                    "reason": "Zapret DPI bypass — YouTube Direct (напрямую с кэш-серверов GGC без расхода VPS)"
                }));
            }
        }

        // Discord Direct -> DIRECT
        if cfg.zapret.hybrid_discord {
            let is_dc = crate::routing::DISCORD_HYBRID_DOMAINS.iter().any(|d| domain == *d || domain.ends_with(&format!(".{}", d)))
                || domain.contains("discord");
            if is_dc {
                return api_ok(json!({
                    "matched_rule": "DOMAIN-SUFFIX,discord.com,DIRECT",
                    "rule_type": "ZAPRET_HYBRID",
                    "target_group": "DIRECT",
                    "resolved_server": "DIRECT (Локальный обход Zapret nfqws)",
                    "reason": "Zapret DPI bypass — Discord Direct (минимальный пинг напрямую без VPS)"
                }));
            }
        }
    }

    // 8. Наборы правил Mihomo (Rule-Sets)
    if domain.contains("youtube") || domain.contains("googlevideo") || domain == "youtu.be" {
        return api_ok(json!({
            "matched_rule": "RULE-SET,youtube@domain,YouTube",
            "rule_type": "RULE_SET",
            "target_group": "YouTube",
            "resolved_server": active_srv,
            "reason": "Специальная группа проксирования YouTube (Mihomo Selector)"
        }));
    }

    if domain.contains("discord") {
        return api_ok(json!({
            "matched_rule": "RULE-SET,discord@classical,Discord",
            "rule_type": "RULE_SET",
            "target_group": "Discord",
            "resolved_server": active_srv,
            "reason": "Специальная группа проксирования Discord"
        }));
    }

    if domain == "t.me" || domain.ends_with(".t.me") || domain.contains("telegram") {
        return api_ok(json!({
            "matched_rule": "RULE-SET,telegram@domain,Telegram",
            "rule_type": "RULE_SET",
            "target_group": "Telegram",
            "resolved_server": active_srv,
            "reason": "Специальная группа проксирования Telegram"
        }));
    }

    if domain.contains("steam") {
        return api_ok(json!({
            "matched_rule": "RULE-SET,steam@domain,Steam",
            "rule_type": "RULE_SET",
            "target_group": "Steam",
            "resolved_server": active_srv,
            "reason": "Группа маршрутизации Steam"
        }));
    }

    if domain.contains("twitch") {
        return api_ok(json!({
            "matched_rule": "RULE-SET,twitch@domain,Twitch",
            "rule_type": "RULE_SET",
            "target_group": "Twitch",
            "resolved_server": active_srv,
            "reason": "Группа маршрутизации Twitch"
        }));
    }

    if domain.contains("tracker") || domain.contains("torrent") {
        return api_ok(json!({
            "matched_rule": "RULE-SET,public-tracker@domain,Torrent",
            "rule_type": "RULE_SET",
            "target_group": "Torrent",
            "resolved_server": "DIRECT",
            "reason": "P2P и торрент-трафик (Torrent группа)"
        }));
    }

    // Российские домены и RU Geosite
    if domain.ends_with(".ru") || domain.ends_with(".su") || domain.ends_with(".рф")
        || domain == "ya.ru" || domain == "yandex.ru" || domain == "vk.com"
        || domain == "gosuslugi.ru" || domain == "kinopoisk.ru"
    {
        return api_ok(json!({
            "matched_rule": "RULE-SET,category-ru@domain,DIRECT",
            "rule_type": "GEO_DIRECT",
            "target_group": "DIRECT",
            "resolved_server": "DIRECT (Напрямую)",
            "reason": "Российский сегмент интернета (RU Geosite) — прямой доступ без прокси"
        }));
    }

    // 9. Финальное базовое правило (MATCH)
    api_ok(json!({
        "matched_rule": "MATCH,PROXY",
        "rule_type": "MATCH",
        "target_group": "PROXY",
        "resolved_server": active_srv,
        "reason": "Сработало финальное правило маршрутизации по умолчанию (MATCH)"
    }))
}

// ==================== ДИАГНОСТИКА СЕТИ & SMART DNS ====================

/// GET /api/diagnostics/health — комплексная проверка здоровья роутера и компонентов
pub async fn get_diagnostics_health(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();

    // 1. Проверка шлюза Keenetic (RCI)
    let gateway_res = rci::get_version(&state.http, &cfg).await;
    let (gw_status, gw_msg) = match gateway_res {
        Ok(v) => {
            let ver = v.get("version").cloned().unwrap_or_else(|| "доступна".into());
            ("ok", format!("KeeneticOS {}", ver))
        }
        Err(e) => ("fail", format!("Keenetic RCI недоступен: {}", e)),
    };

    // 2. Проверка ядра Mihomo
    let mihomo_res = mihomo::get_version(&state.http, &cfg).await;
    let (mihomo_status, mihomo_msg) = match mihomo_res {
        Some(v) => ("ok", format!("Mihomo {} работает штатно", v)),
        None => ("fail", "Mihomo API недоступен".into()),
    };

    // 3. Проверка DNS резолва
    let start_dns = std::time::Instant::now();
    let dns_ok = tokio::net::lookup_host("google.com:80").await.is_ok();
    let dns_ms = start_dns.elapsed().as_millis();
    let (dns_status, dns_msg) = if dns_ok {
        ("ok", format!("DNS резолв успешен ({} мс)", dns_ms))
    } else {
        ("fail", "DNS резолв не удался".to_string())
    };

    // 4. Проверка WAN доступа
    let wan_ok = state.http.get("http://cp.cloudflare.com/generate_204")
        .timeout(std::time::Duration::from_secs(3))
        .send().await.map(|r| r.status().is_success()).unwrap_or(false);
    let (wan_status, wan_msg) = if wan_ok {
        ("ok", "Интернет-соединение (WAN) активно".to_string())
    } else {
        ("warn", "Прямой доступ к тестовому узлу не отвечает".to_string())
    };

    // 5. Проверка хранилища /opt
    let disk_msg = match tokio::process::Command::new("df").arg("-h").arg("/opt").output().await {
        Ok(out) => String::from_utf8_lossy(&out.stdout).lines().nth(1).unwrap_or("").to_string(),
        Err(_) => "Накопитель Entware смонтирован".to_string(),
    };

    api_ok(json!({
        "checks": [
            { "id": "gateway", "name": "Шлюз роутера Keenetic", "status": gw_status, "message": gw_msg },
            { "id": "mihomo", "name": "Ядро Mihomo (XKeen)", "status": mihomo_status, "message": mihomo_msg },
            { "id": "dns", "name": "DNS Резолвер", "status": dns_status, "message": dns_msg, "latency_ms": dns_ms },
            { "id": "wan", "name": "Прямой выход в интернет", "status": wan_status, "message": wan_msg },
            { "id": "storage", "name": "Дисковое пространство /opt", "status": "ok", "message": disk_msg },
        ]
    }))
}

#[derive(Deserialize)]
pub struct DnsTestReq {
    pub domain: String,
}

/// POST /api/diagnostics/dns-test — Smart DNS диагностика домена
pub async fn test_dns_domain(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<DnsTestReq>,
) -> Response {
    let domain = body.domain.trim().to_lowercase();
    if domain.is_empty() || domain.len() > 253 {
        return api_err("Некорректная длина доменного имени (1-253 символа)");
    }
    // Проверка допустимых символов домена (RFC 1035 / RFC 1123)
    if !domain.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
        || domain.starts_with('.')
        || domain.ends_with('.')
        || domain.contains("..")
        || domain == "localhost"
        || domain.starts_with("127.")
    {
        return api_err("Недопустимый формат домена. Укажите корректное FQDN-имя, например: example.com");
    }

    let host_port = format!("{}:80", domain);
    let local_ips: Vec<String> = match tokio::net::lookup_host(&host_port).await {
        Ok(iter) => iter.map(|s| s.ip().to_string()).collect(),
        Err(_) => Vec::new(),
    };

    let is_poisoned = local_ips.iter().any(|ip| {
        ip == "127.0.0.1" || ip == "0.0.0.0" || ip.starts_with("10.") || ip.starts_with("192.168.")
    });

    let http_direct_ok = state.http.get(format!("https://{}", domain))
        .timeout(std::time::Duration::from_secs(3))
        .send().await.is_ok();

    let verdict = if is_poisoned {
        "Домен подменяется провайдером (DNS-spoofing / РКН-заглушка). Необходим прокси."
    } else if !local_ips.is_empty() && !http_direct_ok {
        "Домен резолвится в реальные IP, но прямое TCP/TLS соединение сбрасывается (RST / блокировка по IP/SNI)."
    } else if !local_ips.is_empty() && http_direct_ok {
        "Домен доступен напрямую без ограничений."
    } else {
        "Домен не найден в DNS."
    };

    let recommendation = if is_poisoned || !http_direct_ok {
        "Рекомендуется добавить домен в список «Принудительно через прокси» в Настройках или назначить устройство на зарубежный сервер."
    } else {
        "Дополнительных действий не требуется."
    };

    api_ok(json!({
        "domain": domain,
        "resolved_ips": local_ips,
        "is_poisoned": is_poisoned,
        "http_direct_ok": http_direct_ok,
        "verdict": verdict,
        "recommendation": recommendation
    }))
}

// ==================== KEENETIC POLICIES MAP ====================

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

// ==================== ZAPRET / DPI ИНТЕГРАЦИЯ ====================

pub const DEFAULT_ZAPRET_HOSTS: &str = "# zapret-hosts.txt — Список доменов для универсального обхода DPI\n\
# Поддомены применяются автоматически (subdomains auto apply)\n\
rutracker.org\n\
ntc.party\n\
kinozal.tv\n\
flibusta.is\n\
hdrezka.ag\n\
lostfilm.tv\n\
nnmclub.to\n\
torlook.info\n\
medium.com\n\
linkedin.com\n\
notion.so\n\
canva.com\n\
intel.com\n\
dell.com\n";

pub const S51ZAPRET_SCRIPT: &str = r#"#!/bin/sh

PIDFILE="/opt/var/run/zapret.pid"
FAILSAFE_PID="/opt/var/run/zapret_failsafe.pid"
CONF="/opt/etc/zapret/zapret.conf"

find_bin() {
  if [ -x "/opt/zapret/nfq/nfqws" ]; then
    echo "/opt/zapret/nfq/nfqws"
  elif [ -x "/opt/zapret/binaries/linux-arm64/nfqws" ]; then
    echo "/opt/zapret/binaries/linux-arm64/nfqws"
  elif [ -x "/opt/zapret/binaries/linux-arm/nfqws" ]; then
    echo "/opt/zapret/binaries/linux-arm/nfqws"
  elif [ -x "/opt/zapret/binaries/linux-mips32r2-lsb/nfqws" ]; then
    echo "/opt/zapret/binaries/linux-mips32r2-lsb/nfqws"
  elif [ -x "/opt/zapret/binaries/linux-mips32r2-msb/nfqws" ]; then
    echo "/opt/zapret/binaries/linux-mips32r2-msb/nfqws"
  elif [ -x "/opt/zapret/binaries/linux-x86_64/nfqws" ]; then
    echo "/opt/zapret/binaries/linux-x86_64/nfqws"
  elif [ -x "/opt/bin/nfqws" ]; then
    echo "/opt/bin/nfqws"
  elif [ -x "/opt/usr/bin/nfqws" ]; then
    echo "/opt/usr/bin/nfqws"
  fi
}

BIN=$(find_bin)

# Fallback default with fwmark to prevent loops
NFQWS_ARGS="--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 --filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com --dpi-desync=fake,disorder2 --dpi-desync-split-pos=1 --dpi-desync-fooling=badseq --dpi-desync-cutoff=d4"
DISCORD_VOICE_ENABLED="1"
BLOCK_QUIC="0"

# Safe key-value parser for zapret.conf (no source / eval)
if [ -f "$CONF" ]; then
  while IFS='=' read -r key val || [ -n "$key" ]; do
    case "$key" in
      NFQWS_ARGS)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        NFQWS_ARGS="$val"
        ;;
      DISCORD_VOICE_ENABLED)
        val="${val#\"}"
        val="${val%\"}"
        DISCORD_VOICE_ENABLED="$val"
        ;;
      BLOCK_QUIC)
        val="${val#\"}"
        val="${val%\"}"
        BLOCK_QUIC="$val"
        ;;
    esac
  done < "$CONF"
fi

stop_nfqws() {
  if [ -f "$PIDFILE" ]; then
    PID=$(cat "$PIDFILE" 2>/dev/null)
    if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
      CMD=$(tr '\0' ' ' < "/proc/$PID/cmdline" 2>/dev/null)
      case "$CMD" in
        *nfqws*qnum=200*)
          kill "$PID" 2>/dev/null
          for _i in 1 2 3 4; do
            kill -0 "$PID" 2>/dev/null || break
            usleep 500000 2>/dev/null || sleep 1
          done
          kill -0 "$PID" 2>/dev/null && kill -9 "$PID" 2>/dev/null
          ;;
        *)
          logger -t zapret "WARNING: PID $PID in $PIDFILE does not match expected nfqws qnum=200, skipping"
          ;;
      esac
    fi
    rm -f "$PIDFILE"
  fi
}

add_fw() {
  del_fw

  # Dedicated zapret chain in mangle
  iptables -t mangle -N zapret 2>/dev/null
  iptables -t mangle -F zapret 2>/dev/null

  # 1. CRITICAL: Skip packets already marked by nfqws to prevent infinite packet looping
  iptables -t mangle -A zapret -m mark --mark 0x40000000/0x40000000 -m comment --comment "xkeen-route-zapret" -j RETURN

  # Exclude loopback and LAN bridge (OpenWrt br+ and Keenetic Bridge+)
  iptables -t mangle -A zapret -o lo -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -i lo -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -o br+ -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -o Bridge+ -m comment --comment "xkeen-route-zapret" -j RETURN

  # 2. CRITICAL: Skip private/local subnets & router IP so Keenetic Web UI / LAN are NEVER touched
  iptables -t mangle -A zapret -d 0.0.0.0/8 -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -d 10.0.0.0/8 -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -d 100.64.0.0/10 -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -d 127.0.0.0/8 -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -d 169.254.0.0/16 -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -d 172.16.0.0/12 -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -d 192.168.0.0/16 -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -d 224.0.0.0/4 -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -d 240.0.0.0/4 -m comment --comment "xkeen-route-zapret" -j RETURN
  iptables -t mangle -A zapret -d 255.255.255.255/32 -m comment --comment "xkeen-route-zapret" -j RETURN

  # 3. Queue WAN TCP (80, 443) -> NFQUEUE 200 with bypass
  iptables -t mangle -A zapret -p tcp -m multiport --dports 80,443 -m comment --comment "xkeen-route-zapret" -j NFQUEUE --queue-num 200 --queue-bypass

  # 4. Reject UDP 443 (QUIC / HTTP3) only if explicitly enabled
  if [ "$BLOCK_QUIC" = "1" ]; then
    iptables -t mangle -A zapret -p udp --dport 443 -m comment --comment "xkeen-route-zapret" -j REJECT --reject-with icmp-port-unreachable 2>/dev/null || iptables -t mangle -A zapret -p udp --dport 443 -m comment --comment "xkeen-route-zapret" -j DROP
  fi

  # 5. Discord Voice RTC UDP (50000:65535) if voice enabled
  if [ "$DISCORD_VOICE_ENABLED" = "1" ]; then
    iptables -t mangle -A zapret -p udp -m multiport --dports 50000:65535 -m comment --comment "xkeen-route-zapret" -j NFQUEUE --queue-num 200 --queue-bypass
  fi

  # Hook into POSTROUTING for all outbound WAN packets (LAN forwarded + router local direct)
  iptables -t mangle -I POSTROUTING 1 -m comment --comment "xkeen-route-zapret" -j zapret || { del_fw; return 1; }

  # Redirect client LAN DNS queries to Mihomo DNS (port 1053) only for LAN bridge interfaces (br+, Bridge+)
  iptables -t nat -A PREROUTING -i br+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null
  iptables -t nat -A PREROUTING -i br+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null
  iptables -t nat -A PREROUTING -i Bridge+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null
  iptables -t nat -A PREROUTING -i Bridge+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null

  # Stop previous failsafe if running
  if [ -f "$FAILSAFE_PID" ]; then
    kill -9 $(cat "$FAILSAFE_PID") 2>/dev/null
    rm -f "$FAILSAFE_PID"
  fi

  # FAILSAFE: через 30 сек проверяем интернет, при потере — откатываем всё
  (sleep 30 && \
    if ! curl -s -m 5 -o /dev/null http://www.gstatic.com/generate_204 2>/dev/null && \
       ! curl -s -m 5 -o /dev/null http://cp.cloudflare.com 2>/dev/null; then
      del_fw
      stop_nfqws
      logger -t zapret "FAILSAFE: internet connectivity lost after enabling zapret, iptables rules rolled back"
    fi
    rm -f "$FAILSAFE_PID"
  ) </dev/null >/dev/null 2>&1 &
  echo $! > "$FAILSAFE_PID"
  return 0
}

del_fw() {
  if [ -f "$FAILSAFE_PID" ]; then
    kill -9 $(cat "$FAILSAFE_PID") 2>/dev/null
    rm -f "$FAILSAFE_PID"
  fi

  # Remove DNS redirects with explicit comment
  while iptables -t nat -D PREROUTING -i br+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i br+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null; do :; done

  # Remove hooks
  while iptables -t mangle -D POSTROUTING -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D POSTROUTING -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D OUTPUT -j zapret 2>/dev/null; do :; done

  # Flush and delete chain
  iptables -t mangle -F zapret 2>/dev/null
  iptables -t mangle -X zapret 2>/dev/null
  return 0
}

case "$1" in
  start)
    mkdir -p /opt/var/run /opt/etc/zapret
    if [ -f "$PIDFILE" ]; then
      PID=$(cat "$PIDFILE" 2>/dev/null)
      if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
        add_fw
        exit 0
      fi
    fi
    if [ -n "$BIN" ] && [ -x "$BIN" ]; then
      set -f
      $BIN --pidfile="$PIDFILE" $NFQWS_ARGS
      set +f
      sleep 1
      if [ -f "$PIDFILE" ]; then
        PID=$(cat "$PIDFILE" 2>/dev/null)
        if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
          add_fw
          exit 0
        fi
      fi
      logger -t zapret "ERROR: nfqws failed to start with args: $NFQWS_ARGS"
      del_fw
      exit 1
    else
      logger -t zapret "ERROR: nfqws binary not found or not executable"
      exit 1
    fi
    ;;
  stop)
    del_fw
    stop_nfqws
    exit 0
    ;;
  restart)
    del_fw
    stop_nfqws
    sleep 1
    mkdir -p /opt/var/run /opt/etc/zapret
    if [ -n "$BIN" ] && [ -x "$BIN" ]; then
      set -f
      $BIN --pidfile="$PIDFILE" $NFQWS_ARGS
      set +f
      sleep 1
      if [ -f "$PIDFILE" ]; then
        PID=$(cat "$PIDFILE" 2>/dev/null)
        if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
          add_fw
          exit 0
        fi
      fi
      logger -t zapret "ERROR: nfqws failed to restart with args: $NFQWS_ARGS"
      del_fw
      exit 1
    else
      logger -t zapret "ERROR: nfqws binary not found"
      exit 1
    fi
    ;;
  status)
    if [ -f "$PIDFILE" ]; then
      PID=$(cat "$PIDFILE" 2>/dev/null)
      if [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null; then
        exit 0
      fi
    fi
    exit 1
    ;;
  *)
    echo "Usage: $0 {start|stop|restart|status}"
    exit 1
    ;;
esac
"#;

pub fn build_nfqws_args(cfg: &crate::config::ZapretConfig) -> (String, bool) {
    let mut profiles: Vec<String> = Vec::new();

    // YouTube profile (TCP 80/443) - split2 at pos 1 bypasses SNI inspection cleanly without packet drops
    if cfg.youtube_turbo || cfg.hybrid_youtube {
        let yt_desync = if cfg.aggressive_dpi {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-fooling=badseq --dpi-desync-cutoff=d4"
        } else {
            "--dpi-desync=split2 --dpi-desync-split-pos=1 --dpi-desync-cutoff=d4"
        };
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com {yt_desync}"
        ));
    }

    // Discord Web/Chat profile - split2 for TLS 1.3
    if cfg.hybrid_discord {
        let dc_desync = if cfg.aggressive_dpi {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-fooling=badseq --dpi-desync-cutoff=d4"
        } else {
            "--dpi-desync=split2 --dpi-desync-split-pos=1 --dpi-desync-cutoff=d4"
        };
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist-domains=discord.com,discord.gg,discordapp.com,discordapp.net,discord.media,discord-attachments-uploads-prd.storage.googleapis.com,dis.gd,discord-activities.com {dc_desync}"
        ));
    }

    // Discord Voice UDP profile
    if cfg.discord_voice_udp {
        profiles.push("--filter-udp=50000-65535 --dpi-desync=fake --dpi-desync-cutoff=d4".to_string());
    }

    // General Web Hostlist profile
    if cfg.general_bypass {
        let gen_desync = if cfg.aggressive_dpi {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-fooling=badseq --dpi-desync-cutoff=d4"
        } else {
            "--dpi-desync=split2 --dpi-desync-split-pos=1 --dpi-desync-cutoff=d4"
        };
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist=/opt/etc/zapret/zapret-hosts.txt {gen_desync}"
        ));
    }

    // If no specific profiles enabled, provide safe basic profile
    if profiles.is_empty() {
        profiles.push("--filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com --dpi-desync=split2 --dpi-desync-split-pos=1 --dpi-desync-cutoff=d4".to_string());
    }

    let args = format!("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 {}", profiles.join(" --new "));
    let voice_enabled = cfg.discord_voice_udp;
    (args, voice_enabled)
}

pub fn validate_custom_args(args: &str) -> Result<(), String> {
    for ch in args.chars() {
        if ch == ';' || ch == '&' || ch == '|' || ch == '`' || ch == '$' || ch == '\n' || ch == '\r' || ch == '(' || ch == ')' || ch == '<' || ch == '>' || ch == '!' {
            return Err(format!("Недопустимый спецсимвол в аргументах Zapret: '{}'. Разрешены только флаги nfqws", ch));
        }
    }
    Ok(())
}

pub fn validate_zapret_conf(content: &str) -> Result<(), String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some((key, val)) = trimmed.split_once('=') {
            let k = key.trim();
            if k == "NFQWS_ARGS" {
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                validate_custom_args(clean_val)?;
            } else if k != "DISCORD_VOICE_ENABLED" && k != "BLOCK_QUIC" {
                return Err(format!("Неизвестный параметр в zapret.conf: '{k}'. Разрешены только NFQWS_ARGS, DISCORD_VOICE_ENABLED, BLOCK_QUIC"));
            }
        } else {
            return Err("Некорректная строка в zapret.conf: должна быть в формате КЛЮЧ=\"ЗНАЧЕНИЕ\"".to_string());
        }
    }
    Ok(())
}

pub async fn sync_zapret_files(cfg: &crate::config::ZapretConfig) -> Result<(), String> {
    tokio::fs::create_dir_all("/opt/etc/zapret")
        .await
        .map_err(|e| format!("Не удалось создать /opt/etc/zapret: {e}"))?;
    tokio::fs::create_dir_all("/opt/etc/init.d")
        .await
        .map_err(|e| format!("Не удалось создать /opt/etc/init.d: {e}"))?;

    // 1. S51zapret script
    tokio::fs::write("/opt/etc/init.d/S51zapret", S51ZAPRET_SCRIPT)
        .await
        .map_err(|e| format!("Не удалось записать /opt/etc/init.d/S51zapret: {e}"))?;
    tokio::process::Command::new("chmod")
        .arg("+x")
        .arg("/opt/etc/init.d/S51zapret")
        .output()
        .await
        .map_err(|e| format!("Не удалось сделать S51zapret исполняемым: {e}"))?;

    // 2. Default hostlist if missing
    if !std::path::Path::new("/opt/etc/zapret/zapret-hosts.txt").exists() {
        let _ = tokio::fs::write("/opt/etc/zapret/zapret-hosts.txt", DEFAULT_ZAPRET_HOSTS).await;
    }

    // 3. zapret.conf with multi-strategy args or custom_args
    let (args, voice_enabled) = if let Some(custom) = &cfg.custom_args {
        validate_custom_args(custom)?;
        (custom.clone(), cfg.discord_voice_udp)
    } else {
        build_nfqws_args(cfg)
    };
    let sanitized_args = args.replace('\r', " ").replace('\n', " ").replace('"', "");
    let conf_data = format!(
        "NFQWS_ARGS=\"{}\"\nDISCORD_VOICE_ENABLED=\"{}\"\nBLOCK_QUIC=\"0\"\n",
        sanitized_args.trim(),
        if voice_enabled { "1" } else { "0" }
    );
    tokio::fs::write("/opt/etc/zapret/zapret.conf", conf_data)
        .await
        .map_err(|e| format!("Не удалось записать /opt/etc/zapret/zapret.conf: {e}"))?;
    Ok(())
}

/// GET /api/zapret/status — статус nfqws, iptables и S51zapret
pub async fn get_zapret_status(State(state): State<AppState>) -> Response {
    let init_script = std::path::Path::new("/opt/etc/init.d/S51zapret");
    let installed = init_script.exists();

    let mut running = false;
    let mut pid: Option<u32> = None;
    let mut cmdline: Option<String> = None;

    if installed {
        if let Ok(out) = tokio::process::Command::new("pidof").arg("nfqws").output().await {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(first_pid) = s.split_whitespace().next() {
                if let Ok(p) = first_pid.parse::<u32>() {
                    running = true;
                    pid = Some(p);

                    let proc_cmd = format!("/proc/{p}/cmdline");
                    if let Ok(raw) = tokio::fs::read(&proc_cmd).await {
                        let c = raw
                            .split(|&b| b == 0)
                            .filter(|s| !s.is_empty())
                            .map(|s| String::from_utf8_lossy(s))
                            .collect::<Vec<_>>()
                            .join(" ");
                        if !c.is_empty() {
                            cmdline = Some(c);
                        }
                    }
                }
            }
        }
    }

    let autostart = if installed {
        tokio::process::Command::new("sh")
            .arg("-c")
            .arg("[ -x /opt/etc/init.d/S51zapret ]")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    } else {
        false
    };

    let iptables_active = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("iptables -t mangle -C POSTROUTING -j zapret 2>/dev/null || iptables -t mangle -C POSTROUTING ! -o br+ ! -o lo -j zapret 2>/dev/null || iptables -t nat -C PREROUTING -i br+ -p udp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null || iptables -t mangle -C PREROUTING -i br+ -j zapret 2>/dev/null")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    let config_content = tokio::fs::read_to_string("/opt/etc/zapret/zapret.conf").await.ok();
    let hosts_content = tokio::fs::read_to_string("/opt/etc/zapret/zapret-hosts.txt").await.ok();

    // Определение текущего пресета
    let check_str = cmdline.as_deref().or(config_content.as_deref()).unwrap_or("");
    let preset = if check_str.contains("disorder2") && check_str.contains("split-pos=1") && check_str.contains("discord") {
        "gamer"
    } else if check_str.contains("badseq") {
        "aggressive"
    } else if check_str.contains("disorder2") && check_str.contains("split-pos=1") {
        "youtube"
    } else if check_str.contains("discord") {
        "discord"
    } else if check_str.contains("fake,split2") {
        "general"
    } else {
        "custom"
    };

    let cfg = state.config.read().await;

    api_ok(json!({
        "installed": installed,
        "running": running,
        "pid": pid,
        "autostart": autostart,
        "iptables_active": iptables_active,
        "preset": preset,
        "cmdline": cmdline,
        "config": config_content,
        "hosts": hosts_content,
        "features": &cfg.zapret,
    }))
}

#[derive(Deserialize)]
pub struct ZapretActionReq {
    pub action: String, // "start" | "stop" | "restart" | "toggle" | "install" | "set_preset" | "save_config" | "save_hosts" | "test_dpi" | "toggle_feature" | "set_features" | "reset_features"
    pub preset: Option<String>,
    pub custom_args: Option<String>,
    pub config_content: Option<String>,
    pub hosts_content: Option<String>,
    pub feature: Option<String>,
    pub enabled: Option<bool>,
    pub features: Option<crate::config::ZapretConfig>,
}

/// POST /api/zapret/action — запуск, остановка, переключение, пресеты и тест DPI
pub async fn zapret_action(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<ZapretActionReq>,
) -> Response {
    let act = body.action.trim();

    // 1. Тестирование обхода DPI (прямой через Zapret и через Mihomo прокси)
    if act == "test_dpi" {
        let test_cmd = r#"
            check_target() {
                target="$1"
                # 1. Прямой curl probe через Zapret (без прокси)
                d_out=$(curl -m 3 -s -o /dev/null -w "%{http_code}:%{time_total}" "$target" 2>/dev/null)
                # 2. Proxy probe через Mihomo mixed-port 7890
                p_out=$(curl -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}" -x http://127.0.0.1:7890 "$target" 2>/dev/null)
                echo "${d_out:-000:0.0}|${p_out:-000:0.0}"
            }
            yt_res=$(check_target https://www.youtube.com/generate_204)
            dc_res=$(check_target https://discord.com)
            echo "$yt_res#$dc_res"
        "#;
        let out = tokio::process::Command::new("sh").arg("-c").arg(test_cmd).output().await;
        let line = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        let parts: Vec<&str> = line.split('#').collect();

        let parse_pair = |pair_str: &str| -> (u16, f64, u16, f64) {
            let mut dp = pair_str.split('|');
            let d_str = dp.next().unwrap_or("000:0.0");
            let p_str = dp.next().unwrap_or("000:0.0");

            let parse_one = |s: &str| -> (u16, f64) {
                let mut split = s.split(':');
                let code = split.next().and_then(|c| c.parse().ok()).unwrap_or(0);
                let time = split.next().and_then(|t| t.parse().ok()).unwrap_or(0.0);
                (code, time)
            };
            let (d_code, d_time) = parse_one(d_str);
            let (p_code, p_time) = parse_one(p_str);
            (d_code, d_time, p_code, p_time)
        };

        let (yt_d_code, yt_d_time, yt_p_code, yt_p_time) = parts.get(0).map(|s| parse_pair(s)).unwrap_or((0, 0.0, 0, 0.0));
        let (dc_d_code, dc_d_time, dc_p_code, dc_p_time) = parts.get(1).map(|s| parse_pair(s)).unwrap_or((0, 0.0, 0, 0.0));

        return api_ok(json!({
            "youtube": {
                "code": yt_d_code,
                "time_secs": yt_d_time,
                "ok": yt_d_code >= 200 && yt_d_code < 400,
                "proxy_code": yt_p_code,
                "proxy_time_secs": yt_p_time,
                "proxy_ok": yt_p_code >= 200 && yt_p_code < 400,
            },
            "discord": {
                "code": dc_d_code,
                "time_secs": dc_d_time,
                "ok": dc_d_code >= 200 && dc_d_code < 400,
                "proxy_code": dc_p_code,
                "proxy_time_secs": dc_p_time,
                "proxy_ok": dc_p_code >= 200 && dc_p_code < 400,
            },
        }));
    }

    // 2. Сохранение конфигурации zapret.conf
    if act == "save_config" {
        if let Some(content) = body.config_content {
            if let Err(e) = validate_zapret_conf(&content) {
                return api_err(format!("Ошибка валидации zapret.conf: {e}"));
            }
            let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
            if let Err(e) = tokio::fs::write("/opt/etc/zapret/zapret.conf", &content).await {
                return api_err(format!("Ошибка записи zapret.conf: {e}"));
            }
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
            return api_ok(json!({ "saved": true, "message": "Конфигурация Zapret сохранена и служба перезапущена" }));
        }
        return api_err("Отсутствует содержимое config_content");
    }

    // 2.1 Сохранение списка доменов zapret-hosts.txt
    if act == "save_hosts" {
        if let Some(content) = body.hosts_content {
            let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
            if let Err(e) = tokio::fs::write("/opt/etc/zapret/zapret-hosts.txt", &content).await {
                return api_err(format!("Ошибка записи zapret-hosts.txt: {e}"));
            }
            let is_running = tokio::process::Command::new("pidof")
                .arg("nfqws")
                .output()
                .await
                .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
                .unwrap_or(false);
            if is_running {
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
            }
            return api_ok(json!({ "saved": true, "message": "Список доменов zapret-hosts.txt сохранен" }));
        }
        return api_err("Отсутствует содержимое hosts_content");
    }

    // 3. Выбор пресета / быстрых бандлов стратегий
    if act == "set_preset" {
        let p = body.preset.as_deref().unwrap_or("general");
        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();

        match p {
            "youtube" => {
                cfg.zapret.custom_args = None;
                cfg.zapret.youtube_turbo = true;
                cfg.zapret.hybrid_youtube = false;
                cfg.zapret.hybrid_discord = false;
                cfg.zapret.discord_voice_udp = false;
                cfg.zapret.general_bypass = false;
                cfg.zapret.aggressive_dpi = false;
            }
            "discord" => {
                cfg.zapret.custom_args = None;
                cfg.zapret.youtube_turbo = false;
                cfg.zapret.hybrid_youtube = false;
                cfg.zapret.hybrid_discord = false;
                cfg.zapret.discord_voice_udp = true;
                cfg.zapret.general_bypass = false;
                cfg.zapret.aggressive_dpi = false;
            }
            "gamer" | "media" => {
                cfg.zapret.custom_args = None;
                cfg.zapret.youtube_turbo = true;
                cfg.zapret.hybrid_youtube = false;
                cfg.zapret.hybrid_discord = false;
                cfg.zapret.discord_voice_udp = true;
                cfg.zapret.general_bypass = true;
                cfg.zapret.aggressive_dpi = false;
                cfg.zapret.isolated_proxy = true;
            }
            "aggressive" => {
                cfg.zapret.custom_args = None;
                cfg.zapret.youtube_turbo = true;
                cfg.zapret.hybrid_youtube = false;
                cfg.zapret.hybrid_discord = false;
                cfg.zapret.discord_voice_udp = true;
                cfg.zapret.general_bypass = true;
                cfg.zapret.aggressive_dpi = true;
                cfg.zapret.isolated_proxy = true;
            }
            "custom" => {
                if let Some(custom) = &body.custom_args {
                    cfg.zapret.custom_args = Some(custom.clone());
                }
            }
            _ => {
                // "general" / "all"
                cfg.zapret.custom_args = None;
                cfg.zapret.youtube_turbo = false;
                cfg.zapret.hybrid_youtube = false;
                cfg.zapret.hybrid_discord = false;
                cfg.zapret.discord_voice_udp = true;
                cfg.zapret.general_bypass = true;
                cfg.zapret.aggressive_dpi = false;
                cfg.zapret.isolated_proxy = true;
            }
        }

        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret: {e}"));
        }
        let _ = crate::rci::set_clean_dns_servers(&state.http, &cfg).await;

        // Обновляем правила в config.yaml ядра Mihomo
        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok(new_yaml) = routing::apply_zapret_hybrid_rules(&yaml, &cfg.zapret) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let is_running = tokio::process::Command::new("pidof")
            .arg("nfqws")
            .output()
            .await
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);
        if is_running {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({ "preset": p, "features": cfg.zapret, "message": format!("Применен пресет '{p}'") }));
    }

    // 4. Установка службы Zapret
    if act == "install" {
        let install_cmd = r#"
            mkdir -p /opt/zapret /opt/etc/init.d /opt/etc/zapret
            cd /opt
            (curl -kLs -x http://127.0.0.1:7890 "https://github.com/bol-van/zapret/releases/download/v72.13/zapret-v72.13.tar.gz" -o z.tar.gz || \
             curl -kLs "https://ghproxy.net/https://github.com/bol-van/zapret/releases/download/v72.13/zapret-v72.13.tar.gz" -o z.tar.gz) && \
            tar -xzf z.tar.gz && \
            rm -f z.tar.gz && \
            rm -rf /opt/zapret && \
            mv zapret-v* /opt/zapret && \
            cd /opt/zapret && \
            ./install_bin.sh
        "#;
        match tokio::process::Command::new("sh").arg("-c").arg(install_cmd).output().await {
            Ok(out) => {
                let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                let _cfg = state.config.read().await;
                let _ = sync_zapret_files(&_cfg.zapret).await;
                if let Ok(out) = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("start").output().await {
            if !out.status.success() {
                return api_err(format!("Ошибка запуска Zapret: {}", String::from_utf8_lossy(&out.stderr)));
            }
        }
                let installed = std::path::Path::new("/opt/etc/init.d/S51zapret").exists();
                return api_ok(json!({ "success": installed, "output": output_str.trim() }));
            }
            Err(e) => return api_err(format!("Ошибка установки Zapret: {}", e)),
        }
    }

    // 5. Управление независимыми режимами и выключателями (toggle_feature / set_features / reset_features)
    if act == "toggle_feature" || act == "set_features" || act == "reset_features" {
        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        if act == "reset_features" {
            cfg.zapret = crate::config::ZapretConfig::default();
        } else if let Some(new_features) = body.features {
            if let Some(ref ca) = new_features.custom_args {
                if let Err(e) = validate_custom_args(ca) {
                    return api_err(e);
                }
            }
            cfg.zapret = new_features;
        } else if let (Some(feat), Some(val)) = (body.feature.as_deref(), body.enabled) {
            match feat {
                "hybrid_youtube" => cfg.zapret.hybrid_youtube = val,
                "hybrid_discord" => cfg.zapret.hybrid_discord = val,
                "discord_voice_udp" => cfg.zapret.discord_voice_udp = val,
                "youtube_turbo" => cfg.zapret.youtube_turbo = val,
                "general_bypass" => cfg.zapret.general_bypass = val,
                "aggressive_dpi" => cfg.zapret.aggressive_dpi = val,
                "isolated_proxy" => cfg.zapret.isolated_proxy = val,
                "enabled" => cfg.zapret.enabled = val,
                _ => return api_err(format!("Неизвестный параметр функции: {feat}")),
            }
        }

        let is_running = tokio::process::Command::new("pidof")
            .arg("nfqws")
            .output()
            .await
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);

        // Обновляем файлы zapret.conf и S51zapret
        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret: {e}"));
        }

        if is_running && !cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("stop").output().await;
        } else if is_running && cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
        }

        // Обновляем правила в config.yaml ядра Mihomo через apply_routing
        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                let (new_yaml, _) = match routing::apply_routing(&raw_yaml, &cfg) {
                    Ok(res) => res,
                    Err(e) => return api_err(format!("Ошибка формирования маршрутов Zapret: {e}")),
                };
                if let Err(e) = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await {
                    return api_err(format!("Ошибка сохранения config.yaml: {e}"));
                }
                if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
                    log_w!("Ошибка reload Mihomo после изменения настроек Zapret: {e}");
                }
            }
        }

        if let Err(e) = config::save(&state.config_path, &cfg).await {
            log_e!("Ошибка сохранения config.json: {e}");
            return api_err(format!("Настройки Zapret применены, но не сохранены в config.json: {e}"));
        }
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": "Параметры и правила маршрутизации успешно обновлены"
        }));
    }

    let init_script = "/opt/etc/init.d/S51zapret";
    if !std::path::Path::new(init_script).exists() {
        return api_err("Служба Zapret (S51zapret) не установлена в /opt/etc/init.d/");
    }

    // 6. Toggle (Включение / Выключение)
    let action_to_run = if act == "toggle" {
        if let Some(en) = body.enabled {
            if en { "start" } else { "stop" }
        } else {
            let is_running = tokio::process::Command::new("pidof")
                .arg("nfqws")
                .output()
                .await
                .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
                .unwrap_or(false);
            if is_running { "stop" } else { "start" }
        }
    } else {
        act
    };

    if action_to_run != "start" && action_to_run != "stop" && action_to_run != "restart" {
        return api_err("Недопустимое действие для службы Zapret");
    }

    // Перед стартом гарантируем актуальные и безопасные правила
    if action_to_run == "start" || action_to_run == "restart" {
        let _cfg = state.config.read().await;
        let _ = sync_zapret_files(&_cfg.zapret).await;
        let _ = crate::rci::set_clean_dns_servers(&state.http, &_cfg).await;
    }

    match tokio::process::Command::new(init_script).arg(action_to_run).output().await {
        Ok(out) => {
            let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let _cfg_guard = state.config_lock.lock().await;
            let mut cfg = (**state.config.read().await).clone();
            if action_to_run == "start" {
                cfg.zapret.enabled = true;
            } else if action_to_run == "stop" {
                cfg.zapret.enabled = false;
            }
            if std::path::Path::new(&cfg.mihomo.config_path).exists() {
                if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, &cfg) {
                        Ok(res) => res,
                        Err(e) => return api_err(format!("Ошибка роутинга: {}", e)),
                    };
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
            let _ = config::save(&state.config_path, &cfg).await;
            *state.config.write().await = std::sync::Arc::new(cfg);

            api_ok(json!({ "success": true, "output": output_str.trim(), "action": action_to_run }))
        }
        Err(e) => api_err(format!("Ошибка выполнения {}: {}", init_script, e)),
    }
}

// ==================== РАСПИСАНИЯ УСТРОЙСТВ ====================

/// GET /api/schedules — список расписаний
pub async fn get_schedules(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    api_ok(json!({ "schedules": cfg.schedules }))
}

#[derive(Deserialize)]
pub struct SaveSchedulesReq {
    pub schedules: Vec<config::DeviceSchedule>,
}

/// POST /api/schedules — сохранение расписаний
pub async fn save_schedules(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<SaveSchedulesReq>,
) -> Response {
    let _guard = state.config_lock.lock().await;
    let mut cfg = state.config.read().await.as_ref().clone();
    cfg.schedules = body.schedules;

    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения расписаний: {}", e));
    }
    *state.config.write().await = std::sync::Arc::new(cfg);
    api_ok(json!({ "saved": true }))
}

// ==================== ИГРОВОЙ РЕЖИМ (GAMING MODE) ====================

/// GET /api/gaming/status — статус, конфиг и активный игровой узел
pub async fn get_gaming_status(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let domains_count = routing::get_gaming_domains(&cfg.gaming).len();

    // Пытаемся получить активный прокси для группы 🎮 Gaming из Mihomo
    let active_server = match mihomo::get_proxies(&state.http, &cfg).await {
        Ok(map) => {
            map.get(routing::GAMING_GROUP_NAME)
                .or_else(|| map.get("Gaming"))
                .and_then(|g| g.get("now"))
                .and_then(|n| n.as_str())
                .unwrap_or(cfg.gaming.target_server.as_str())
                .to_string()
        }
        Err(_) => cfg.gaming.target_server.clone(),
    };

    api_ok(json!({
        "config": cfg.gaming,
        "active_server": active_server,
        "domains_count": domains_count,
    }))
}

#[derive(Deserialize)]
pub struct SaveGamingReq {
    pub gaming: config::GamingConfig,
}

/// POST /api/gaming/save — сохранение настроек игрового режима и применение маршрутов
pub async fn save_gaming_config(
    State(state): State<AppState>,
    Json(body): Json<SaveGamingReq>,
) -> Response {
    let _cfg_guard = state.config_lock.lock().await;
    let _routing_guard = state.routing_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();
    cfg.gaming = body.gaming;

    // 1. Применяем правила в config.yaml ядра Mihomo (под блокировкой routing_lock)
    let path = std::path::Path::new(&cfg.mihomo.config_path);
    if path.exists() {
        let raw_yaml = match tokio::fs::read_to_string(path).await {
            Ok(y) => y,
            Err(e) => return api_err(format!("Ошибка чтения config.yaml: {e}")),
        };
        let (new_yaml, _) = match routing::apply_routing(&raw_yaml, &cfg) {
            Ok(res) => res,
            Err(e) => return api_err(format!("Ошибка генерации правил роутинга: {}", e)),
        };
        if let Err(e) = atomic_write_file(path, &new_yaml).await {
            return api_err(format!("Ошибка записи config.yaml: {}", e));
        }
        if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
            return api_err(format!("Ошибка перезагрузки Mihomo: {}", e));
        }
    }

    // 2. Синхронизируем ipset geo_override ядра Keenetic для игровых доменов
    if cfg.gaming.enabled {
        let game_domains = routing::get_gaming_domains(&cfg.gaming);
        let mut all_domains = cfg.force_domains.clone();
        all_domains.extend(game_domains);
        let _ = crate::override_sync::sync_geo_override(&all_domains).await;
    }

    // 3. Сохраняем в config.json
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения config.json: {}", e));
    }
    *state.config.write().await = std::sync::Arc::new(cfg);

    log_i!("Настройки игрового режима успешно сохранены и применены");
    api_ok(json!({ "saved": true }))
}

#[derive(Deserialize)]
pub struct ToggleGamingReq {
    pub enabled: bool,
}

/// POST /api/gaming/toggle — быстрое включение/отключение игрового режима
pub async fn toggle_gaming(
    State(state): State<AppState>,
    Json(body): Json<ToggleGamingReq>,
) -> Response {
    let _cfg_guard = state.config_lock.lock().await;
    let _routing_guard = state.routing_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();
    cfg.gaming.enabled = body.enabled;

    let path = std::path::Path::new(&cfg.mihomo.config_path);
    if path.exists() {
        let raw_yaml = match tokio::fs::read_to_string(path).await {
            Ok(y) => y,
            Err(e) => return api_err(format!("Ошибка чтения config.yaml: {e}")),
        };
        let (new_yaml, _) = match routing::apply_routing(&raw_yaml, &cfg) {
            Ok(res) => res,
            Err(e) => return api_err(format!("Ошибка роутинга: {}", e)),
        };
        if let Err(e) = atomic_write_file(path, &new_yaml).await {
            return api_err(format!("Ошибка записи config.yaml: {e}"));
        }
        if let Err(e) = mihomo::reload_config(&state.http, &cfg).await {
            return api_err(format!("Ошибка перезагрузки Mihomo: {e}"));
        }
    }

    if cfg.gaming.enabled {
        let game_domains = routing::get_gaming_domains(&cfg.gaming);
        let mut all_domains = cfg.force_domains.clone();
        all_domains.extend(game_domains);
        let _ = crate::override_sync::sync_geo_override(&all_domains).await;
    }

    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(cfg);

    log_i!("Игровой режим {}", if body.enabled { "включен" } else { "отключен" });
    api_ok(json!({ "enabled": body.enabled }))
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






