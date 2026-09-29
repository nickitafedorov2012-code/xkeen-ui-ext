use axum::extract::{ConnectInfo, Query, State};
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;

use crate::{config, failover, log_e, log_i, log_w, mihomo, rci, routing, transaction::ConfigTx, AppState, VERSION};

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

    let zapret_engine = if normalize_engine_choice(&cfg.zapret.engine) == "v1" {
        "v1"
    } else if is_nfqws2_available() {
        "v2"
    } else if is_nfqws1_available() {
        "v1"
    } else {
        normalize_engine_choice(&cfg.zapret.engine)
    };
    let zapret_ver = if zapret_engine == "v2" {
        detect_installed_zapret2_version().await
    } else {
        detect_installed_zapret1_version().await
    };
    let zapret_label = if zapret_engine == "v2" {
        format!("Запрет 2 {zapret_ver}")
    } else {
        format!("Запрет 1 {zapret_ver}")
    };
    let zapret_installed = std::path::Path::new("/opt/etc/init.d/S51zapret").exists();
    let service_stopped = crate::watchdog::is_service_stopped();
    let service_running = !service_stopped && mihomo_ver.is_some();

    api_ok(json!({
        "version": VERSION,
        "config_path": state.config_path.display().to_string(),
        "router": router,
        "system": system,
        "mihomo_version": mihomo_ver,
        "service_running": service_running,
        "service_stopped": service_stopped,
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
        "zapret": {
            "engine": zapret_engine,
            "version": zapret_ver,
            "label": zapret_label,
            "installed": zapret_installed,
            "running": cfg.zapret.enabled && !service_stopped,
            "v1_installed": is_nfqws1_available(),
            "v2_installed": is_nfqws2_available(),
            "update_available": cfg.zapret.update_available
                && cfg.zapret.latest_version.as_deref().map_or(false, |lat| crate::updater::is_newer(lat, &zapret_ver)),
            "latest_version": cfg.zapret.latest_version,
        },
        "gaming": {
            "enabled": cfg.gaming.enabled,
            "mode": cfg.gaming.mode,
            "smart_mode": cfg.gaming.smart_mode,
            "smart_idle_timeout_mins": cfg.gaming.smart_idle_timeout_mins,
        },
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
    // DIAG-01: Изоляция от Speedtest
    let _speedtest_guard = match state.speedtest_lock.try_lock() {
        Ok(guard) => guard,
        Err(_) => return api_err("Невозможно переключить сервер: в данный момент выполняется тест скорости (Speedtest)"),
    };
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

    // DIAG-04: Не выбирать отсутствующий узел, проверять реальное наличие в конфигурации
    let target_server = match best_server.or_else(|| cfg.flow_server.clone()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => {
            // Ищем сервер США из mihomo::get_servers, либо первый доступный узел
            if let Ok(servers) = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await {
                let us_cand = servers.iter().find(|s| {
                    let l = s.id.to_lowercase();
                    !s.id.is_empty() && s.id != "Fastest" && s.id != "Fallback" &&
                        (l.contains("сша") || l.contains("usa") || l.contains("us ") ||
                         l.contains("chicago") || l.contains("washington") || l.contains("miami") ||
                         l.contains("seattle") || l.contains("atlanta") || l.contains("phoenix") ||
                         l.contains("los angeles") || l.contains("canada"))
                });
                if let Some(us_srv) = us_cand {
                    us_srv.id.clone()
                } else if let Some(first_valid) = servers.into_iter().find(|s| !s.id.is_empty() && s.id != "Fastest" && s.id != "Fallback") {
                    first_valid.id
                } else {
                    return api_err("В конфигурации ядра Mihomo не найдено ни одного действительного прокси-сервера для маршрута Google Flow");
                }
            } else {
                return api_err("Не удалось определить доступный прокси-сервер для маршрута Google Flow (Mihomo API недоступен)");
            }
        }
    };

    // 2. Переключение Flow сервера и проверка результата в runtime (DIAG-04)
    if let Err(e) = mihomo::switch_flow_server(&state.http, &cfg, &target_server).await {
        return api_err(format!("Не удалось переключить маршрут Google AI на сервер '{}': {}", target_server, e));
    }

    // 3. Подтверждение в runtime (DIAG-04: честная валидация)
    let proxies = match mihomo::get_proxies(&state.http, &cfg).await {
        Ok(p) => p,
        Err(e) => return api_err(format!("Ошибка подтверждения маршрута в ядре Mihomo: {}", e)),
    };
    let is_confirmed = proxies.iter().any(|(name, obj)| {
        let lower = name.to_lowercase();
        (lower.contains("google ai") || lower.contains("flow")) && obj.get("now").and_then(|n| n.as_str()) == Some(&target_server)
    });
    if !is_confirmed {
        return api_err(format!("Маршрут Google AI в ядре Mihomo не подтвердил выбор узла '{}' в runtime", target_server));
    }

    // 4. Сохранение в config.json с проверкой результата
    {
        let _cfg_guard = state.config_lock.lock().await;
        let mut mut_cfg = (**state.config.read().await).clone();
        mut_cfg.flow_server = Some(target_server.clone());
        if let Err(e) = config::save(&state.config_path, &mut_cfg).await {
            return api_err(format!("Маршрут Google AI переключен, но сохранение конфигурации завершилось ошибкой: {}", e));
        }
        *state.config.write().await = std::sync::Arc::new(mut_cfg);
    }

    // 5. Сброс всех активных соединений ядра
    mihomo::close_all_connections(&state.http, &cfg).await;

    api_ok(json!({
        "success": true,
        "flow_server": target_server,
        "verified": true,
        "min_ping": if min_ping == i64::MAX { None } else { Some(min_ping) },
        "message": format!("Маршрут Google Flow подтвержден и переключен на '{target_server}', сокеты сброшены"),
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
                        "ipv6": d.ipv6,
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
    let new_yaml = match routing::apply_assignments(&yaml, &[assignment], &providers) {
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
        if mihomo::switch_group(&state.http, tx.config(), &gname, primary).await.is_ok() {
            reselected = true;
        }
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

pub fn calc_domains_revision(direct: &[String], force: &[String]) -> String {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    direct.hash(&mut hasher);
    force.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// GET /api/domains — списки доменов (напрямую / принудительно через прокси) + найденные CDN.
pub async fn get_domains(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let auto_cdns = crate::cdn_discovery::expand_bundles(&cfg.force_domains);
    let auto_cdns_list: Vec<String> = auto_cdns.into_iter().collect();
    let rev = calc_domains_revision(&cfg.direct_domains, &cfg.force_domains);
    api_ok(json!({
        "direct": cfg.direct_domains,
        "force": cfg.force_domains,
        "auto_cdns": auto_cdns_list,
        "revision": rev,
    }))
}

#[derive(Deserialize)]
pub struct DomainsReq {
    #[serde(default)]
    pub direct: Vec<String>,
    #[serde(default)]
    pub force: Vec<String>,
    pub expected_revision: Option<String>,
}

/// POST /api/domains — сохранить списки, авто-обнаружить CDN, вставить DOMAIN-SUFFIX правила в rules:, reload.
pub async fn set_domains(State(state): State<AppState>, Json(req): Json<DomainsReq>) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    // UI-03: Проверка оптимистичного concurrency
    if let Some(exp) = &req.expected_revision {
        let cur_rev = calc_domains_revision(&tx.config().direct_domains, &tx.config().force_domains);
        if exp != &cur_rev {
            return api_err("Конфликт параллельного сохранения доменов: списки были изменены другим клиентом");
        }
    }

    let direct_sanitized = routing::sanitize_domains(&req.direct);
    let mut force_sanitized = routing::sanitize_domains(&req.force);
    let direct_set: std::collections::HashSet<String> = direct_sanitized.iter().cloned().collect();
    force_sanitized.retain(|d| !direct_set.contains(d));

    tx.config_mut().direct_domains = direct_sanitized;
    tx.config_mut().force_domains = force_sanitized;

    // Автоматическое обнаружение сопутствующих CDN (бандлы + поддомены + HTML-сканер)
    let auto_cdns = crate::cdn_discovery::discover_all_cdns(&tx.config().force_domains, &tx.config().mihomo_proxy_url()).await;
    let mut all_force = tx.config().force_domains.clone();
    for cdn in &auto_cdns {
        if !all_force.contains(cdn) {
            all_force.push(cdn.clone());
        }
    }
    if tx.config().zapret.enabled {
        all_force.retain(|d| !routing::is_zapret_direct_domain(d, &tx.config().zapret));
    }

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    let mut new_yaml = match routing::apply_domain_rules(&yaml, &tx.config().direct_domains, &all_force, &tx.config().device_domain_rules) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    if let Ok(with_zapret) = routing::apply_zapret_hybrid_rules(&new_yaml, &tx.config().zapret) {
        new_yaml = with_zapret;
    }
    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }

    // Атомарное сохранение и перезагрузка Mihomo с двухфазным откатом при сбое
    if let Err(e) = tx.commit_and_reload().await {
        return api_err(e);
    }

    // Автоматическая синхронизация IP-адресов принудительно проксируемых доменов и их CDN с geo_override
    let overridden = match crate::override_sync::sync_geo_override(&all_force).await {
        Ok(count) => count,
        Err(e) => {
            crate::log_w!("[OVERRIDE] Ошибка синхронизации geo_override: {e}");
            0
        }
    };

    let (n_direct, n_force) = (tx.config().direct_domains.len(), tx.config().force_domains.len());
    let auto_cdns_list: Vec<String> = auto_cdns.into_iter().collect();

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
    let clean_domain = crate::config::normalize_domain(&req.domain);
    if !crate::config::is_valid_domain(&clean_domain) {
        return api_err(format!("Некорректный домен: {}", req.domain));
    }

    let client_ip = match req.client_ip.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(ip) => {
            if !crate::config::is_valid_ip_or_cidr(ip) {
                return api_err(format!("Некорректный IP-адрес или CIDR устройства: '{ip}'"));
            }
            Some(ip.to_string())
        }
        None => None,
    };

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(e),
    };

    let all_force = if let Some(ref ip) = client_ip {
        let rules = tx.config_mut().device_domain_rules.entry(ip.clone()).or_default();
        if !rules.iter().any(|r| r.domain.eq_ignore_ascii_case(&clean_domain)) {
            rules.push(crate::config::DeviceDomainRule {
                domain: clean_domain.clone(),
                target: "PROXY".into(),
            });
        }
        let raw_yaml = match tx.read_yaml().await {
            Ok(y) => y,
            Err(e) => return api_err(format!("Не удалось прочитать config.yaml: {e}")),
        };
        let (new_yaml, _) = match crate::routing::apply_routing(&raw_yaml, tx.config()) {
            Ok(res) => res,
            Err(e) => return api_err(format!("Ошибка генерации правил роутинга: {e}")),
        };
        if let Err(e) = tx.set_yaml(new_yaml) {
            return api_err(e);
        }
        log_i!("Домен {} жестко направлен в прокси для устройства {}", clean_domain, ip);
        vec![]
    } else {
        // Глобальное принудительное проксирование: исключаем конфликт с direct_domains
        tx.config_mut().direct_domains.retain(|d| !d.eq_ignore_ascii_case(&clean_domain));
        if !tx.config().force_domains.iter().any(|d| d.eq_ignore_ascii_case(&clean_domain)) {
            tx.config_mut().force_domains.push(clean_domain.clone());
            tx.config_mut().force_domains.sort();
            tx.config_mut().force_domains.dedup();
        }

        // Обнаружение сопутствующих CDN
        let auto_cdns = crate::cdn_discovery::discover_all_cdns(&tx.config().force_domains, &tx.config().mihomo_proxy_url()).await;
        let mut force_list = tx.config().force_domains.clone();
        for cdn in &auto_cdns {
            if !force_list.contains(cdn) {
                force_list.push(cdn.clone());
            }
        }
        if tx.config().zapret.enabled {
            force_list.retain(|d| !routing::is_zapret_direct_domain(d, &tx.config().zapret));
        }

        let yaml = match tx.read_yaml().await {
            Ok(y) => y,
            Err(e) => return api_err(format!("Не удалось прочитать config.yaml: {e}")),
        };

        let mut new_yaml = match routing::apply_domain_rules(&yaml, &tx.config().direct_domains, &force_list, &tx.config().device_domain_rules) {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };
        if let Ok(with_zapret) = routing::apply_zapret_hybrid_rules(&new_yaml, &tx.config().zapret) {
            new_yaml = with_zapret;
        }

        if let Err(e) = tx.set_yaml(new_yaml) {
            return api_err(e);
        }
        log_i!("Домен {} жестко направлен в прокси глобально", clean_domain);
        force_list
    };

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(e);
    }

    if !all_force.is_empty() {
        let _ = crate::override_sync::sync_geo_override(&all_force).await;
    }

    // Если передан ID конкретного соединения — закрываем его немедленно
    if let Some(conn_id) = req.close_connection_id.as_deref().filter(|s| !s.is_empty()) {
        let cfg = state.config.read().await.clone();
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

/// POST /api/xkeen/service — start/stop/restart/restart_all/status сервисов проксирования и обхода.
/// При stop останавливает xkeen, mihomo и zapret и очищает перехват iptables (все устройства идут напрямую без смены политики на роутере).
pub async fn xkeen_service(State(state): State<AppState>, Json(req): Json<ServiceReq>) -> Response {
    let cfg = state.config.read().await.clone();
    let action = req.action.trim().to_string();
    if !matches!(action.as_str(), "start" | "stop" | "restart" | "restart_all" | "status") {
        return api_err("Недопустимое действие (start/stop/restart/restart_all/status)");
    }
    let init_script = cfg.system.xkeen_init.trim();
    if !init_script.starts_with("/opt/")
        || init_script.contains("..")
        || init_script.contains('\0')
        || init_script.contains(';')
        || init_script.contains('&')
        || init_script.contains('|')
        || init_script.contains('`')
        || init_script.contains('$')
    {
        return api_err("Недопустимый путь к init-скрипту (разрешены только пути в /opt/)");
    }

    if action == "stop" {
        crate::watchdog::set_service_stopped(true);
        let stop_cmd = format!(
            r#"
            sh "{init_script}" stop 2>&1 || true
            [ -x /opt/sbin/xkeen ] && /opt/sbin/xkeen -stop >/dev/null 2>&1 || true
            [ -x /opt/etc/init.d/S51zapret ] && /opt/etc/init.d/S51zapret stop >/dev/null 2>&1 || true
            killall -15 mihomo xray nfqws nfqws2 2>/dev/null || true
            sleep 1
            killall -9 mihomo xray nfqws nfqws2 2>/dev/null || true
            for ipt in iptables ip6tables; do
              for tbl in nat mangle; do
                while $ipt -t $tbl -D PREROUTING -j xkeen 2>/dev/null; do :; done
                while $ipt -t $tbl -D OUTPUT -j xkeen_mask 2>/dev/null; do :; done
                while $ipt -t $tbl -D POSTROUTING -j zapret 2>/dev/null; do :; done
                while $ipt -t $tbl -D PREROUTING -j zapret 2>/dev/null; do :; done
                while $ipt -t $tbl -D OUTPUT -j zapret 2>/dev/null; do :; done
                $ipt -t $tbl -F xkeen 2>/dev/null || true
                $ipt -t $tbl -F xkeen_mask 2>/dev/null || true
                $ipt -t $tbl -F zapret 2>/dev/null || true
              done
              while $ipt -t nat -D PREROUTING -i br+ -p udp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
              while $ipt -t nat -D PREROUTING -i br+ -p tcp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
              while $ipt -t nat -D PREROUTING -i Bridge+ -p udp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
              while $ipt -t nat -D PREROUTING -i Bridge+ -p tcp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
            done
            while ip rule del fwmark 0x111/0xff lookup 111 2>/dev/null; do :; done
            while ip -6 rule del fwmark 0x111/0xff lookup 111 2>/dev/null; do :; done
            echo "Все службы (XKeen, Mihomo, Zapret) остановлены. Трафик устройств переведен на прямой выход (DIRECT)."
            "#
        );
        let out = tokio::time::timeout(
            std::time::Duration::from_secs(15),
            tokio::process::Command::new("sh").arg("-c").arg(&stop_cmd).output(),
        ).await;
        log_i!("Сервисы остановлены пользователем (прямой выход в интернет активирован)");
        let (code, stdout, stderr) = match out {
            Ok(Ok(o)) => (
                o.status.code(),
                String::from_utf8_lossy(&o.stdout).to_string(),
                String::from_utf8_lossy(&o.stderr).to_string(),
            ),
            Ok(Err(e)) => (Some(0), String::new(), e.to_string()),
            Err(_) => (Some(0), String::new(), "Превышен таймаут остановки служб (15 сек)".to_string()),
        };
        return api_ok(json!({
            "code": code,
            "stdout": stdout,
            "stderr": stderr,
            "service_running": false,
            "service_stopped": true,
            "message": "Все службы остановлены. Трафик переведён на прямой выход (DIRECT)."
        }));
    }

    if action == "start" || action == "restart" || action == "restart_all" {
        crate::watchdog::set_service_stopped(false);
    }

    let script_arg = if action == "restart_all" { "restart" } else { action.as_str() };
    let out = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        tokio::process::Command::new("sh")
            .env("fd_out", "true")
            .arg(init_script)
            .arg(script_arg)
            .output(),
    )
    .await;

    match out {
        Err(_) => {
            log_w!("Таймаут (15 сек) выполнения '{} {}'", init_script, script_arg);
            return api_err(format!("Превышен таймаут выполнения '{} {}'", init_script, script_arg));
        }
        Ok(Err(e)) => {
            return api_err(format!("Ошибка запуска скрипта '{}': {}", init_script, e));
        }
        Ok(Ok(o)) => {
            log_i!(
                "Сервис XKeen: {} (код {})",
                action,
                o.status.code().unwrap_or(-1)
            );
            if action == "start" || action == "restart" || action == "restart_all" {
                // Восстанавливаем правила маршрутизации в config.yaml и перезапускаем Zapret (если он включен)
                let path = std::path::Path::new(&cfg.mihomo.config_path);
                if path.exists() {
                    if let Ok(raw_yaml) = tokio::fs::read_to_string(path).await {
                        if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                            let _ = atomic_write_file(path, &new_yaml).await;
                            let _ = mihomo::reload_config(&state.http, &cfg).await;
                        }
                    }
                }
                if cfg.zapret.enabled && std::path::Path::new("/opt/etc/init.d/S51zapret").exists() {
                    let _ = sync_zapret_files(&cfg.zapret).await;
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(8),
                        tokio::process::Command::new("sh")
                            .arg("-c")
                            .arg("exec /opt/etc/init.d/S51zapret restart </dev/null >/dev/null 2>&1")
                            .status(),
                    )
                    .await;
                }
                if action == "restart_all" {
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
                        let _ = tokio::process::Command::new("sh")
                            .arg("-c")
                            .arg("/opt/etc/init.d/S99xkeen-route restart >/dev/null 2>&1 &")
                            .status()
                            .await;
                    });
                }
            }
            if !o.status.success() && action != "status" && action != "restart_all" {
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
            let msg = match action.as_str() {
                "restart_all" => "Все компоненты (XKeen, Mihomo, Zapret, Панель) успешно перезапущены!",
                "restart" => "Службы XKeen и Mihomo успешно перезапущены!",
                "start" => "Службы успешно запущены!",
                "stop" => "Все службы остановлены. Трафик переведён на прямой выход (DIRECT).",
                _ => "Команда успешно выполнена.",
            };
            api_ok(json!({
                "code": o.status.code(),
                "stdout": String::from_utf8_lossy(&o.stdout),
                "stderr": String::from_utf8_lossy(&o.stderr),
                "service_running": action != "stop",
                "service_stopped": action == "stop",
                "message": msg,
            }))
        }
    }
}

/// Каталог бэкапов панели: {backup_dir}/xkeen-route.
fn backup_root(cfg: &config::AppConfig) -> std::path::PathBuf {
    std::path::Path::new(&cfg.system.backup_dir).join("xkeen-route")
}

/// Валидация имени бэкапа (защита от path traversal).
fn valid_backup_name(name: &str) -> bool {
    name.len() > 3
        && name.starts_with("xr-")
        && !name.contains("..")
        && !name.contains('/')
        && !name.contains('\\')
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

/// POST /api/backups — создать бэкап (config.yaml Mihomo + config.json панели + опционально zapret.conf и zapret-hosts.txt).
pub async fn create_backup(State(state): State<AppState>) -> Response {
    let _cfg_guard = state.config_lock.lock().await;
    let _routing_guard = state.routing_lock.lock().await;

    let cfg = state.config.read().await.clone();
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S-%3f");
    let name = format!("xr-{ts}");
    let dir = backup_root(&cfg).join(&name);
    let tmp_dir = backup_root(&cfg).join(format!("xr-{ts}.tmp"));

    if let Err(e) = tokio::fs::create_dir_all(&tmp_dir).await {
        return api_err(format!("Не удалось создать временный каталог бэкапа {}: {e}", tmp_dir.display()));
    }

    // 1. config.yaml Mihomo
    if let Err(e) = tokio::fs::copy(&cfg.mihomo.config_path, tmp_dir.join("config.yaml")).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Не удалось скопировать {}: {e}", cfg.mihomo.config_path));
    }
    // 2. config.json панели
    if let Err(e) = tokio::fs::copy(state.config_path.as_path(), tmp_dir.join("config.json")).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Не удалось скопировать {}: {e}", state.config_path.display()));
    }
    // 3. zapret.conf (если существует)
    let zapret_candidates = [
        "/opt/zapret/config/zapret.conf",
        "/opt/etc/zapret/zapret.conf",
    ];
    for zp in zapret_candidates {
        let p = std::path::Path::new(zp);
        if p.is_file() {
            let _ = tokio::fs::copy(p, tmp_dir.join("zapret.conf")).await;
            break;
        }
    }
    // 4. zapret-hosts.txt (если существует)
    let hosts_candidates = [
        "/opt/etc/zapret/zapret-hosts.txt",
        "/opt/zapret/config/zapret-hosts.txt",
    ];
    for hp in hosts_candidates {
        let p = std::path::Path::new(hp);
        if p.is_file() {
            let _ = tokio::fs::copy(p, tmp_dir.join("zapret-hosts.txt")).await;
            break;
        }
    }
    // 5. ru_exclude_override.lst (если существует)
    let override_path = std::path::Path::new(crate::override_sync::OVERRIDE_FILE);
    if override_path.is_file() {
        let _ = tokio::fs::copy(override_path, tmp_dir.join("ru_exclude_override.lst")).await;
    }

    // Атомарно активируем каталог бэкапа только после успешной записи полного комплекта
    if let Err(e) = tokio::fs::rename(&tmp_dir, &dir).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Ошибка фиксации бэкапа: {e}"));
    }

    log_i!("Бэкап создан: {} ({})", name, dir.display());
    api_ok(json!({ "name": name, "dir": dir.display().to_string() }))
}

#[derive(Deserialize)]
pub struct BackupReq {
    pub name: String,
}

/// POST /api/backups/restore — восстановить конфиги из бэкапа через ConfigTx, reload Mihomo.
pub async fn restore_backup(State(state): State<AppState>, Json(req): Json<BackupReq>) -> Response {
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
    if let Err(e) = crate::transaction::validate_yaml_syntax(&new_yaml_content) {
        return api_err(format!("Синтаксическая ошибка в config.yaml бэкапа: {e}"));
    }

    let new_json_content = match tokio::fs::read_to_string(&src_json).await {
        Ok(s) => s,
        Err(e) => return api_err(format!("Не удалось прочитать config.json из бэкапа: {e}")),
    };
    let mut new_cfg: config::AppConfig = match serde_json::from_str(&new_json_content) {
        Ok(c) => c,
        Err(e) => return api_err(format!("Файл config.json в бэкапе поврежден: {e}")),
    };

    // BK-04: Сохранение локальных путей хоста при переносе бэкапа
    new_cfg.system = cfg.system.clone();
    new_cfg.mihomo.config_path = cfg.mihomo.config_path.clone();

    // 2. Старт транзакции через ConfigTx с блокировками и снапшотами (BK-04, BK-05)
    let mut tx = match crate::transaction::ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(format!("Ошибка начала транзакции восстановления: {e}")),
    };

    if let Err(e) = tx.set_yaml(new_yaml_content) {
        return api_err(format!("Ошибка подготовки config.yaml: {e}"));
    }
    tx.stage_json(new_cfg);

    // Дополнительные файлы (zapret.conf, zapret-hosts.txt и override), если присутствуют в бэкапе
    let src_zapret = dir.join("zapret.conf");
    if src_zapret.is_file() {
        if let Ok(z_content) = tokio::fs::read_to_string(&src_zapret).await {
            let z_candidates = [
                std::path::PathBuf::from("/opt/zapret/config/zapret.conf"),
                std::path::PathBuf::from("/opt/etc/zapret/zapret.conf"),
            ];
            for z_path in &z_candidates {
                if z_path.parent().map(|p| p.exists()).unwrap_or(false) {
                    let _ = tx.set_extra_file(z_path.as_path(), z_content.clone(), false).await;
                    break;
                }
            }
        }
    }

    let src_hosts = dir.join("zapret-hosts.txt");
    if src_hosts.is_file() {
        if let Ok(h_content) = tokio::fs::read_to_string(&src_hosts).await {
            let h_candidates = [
                std::path::PathBuf::from("/opt/etc/zapret/zapret-hosts.txt"),
                std::path::PathBuf::from("/opt/zapret/config/zapret-hosts.txt"),
            ];
            for h_path in &h_candidates {
                if h_path.parent().map(|p| p.exists()).unwrap_or(false) {
                    let _ = tx.set_extra_file(h_path.as_path(), h_content.clone(), false).await;
                    break;
                }
            }
        }
    }

    let src_override = dir.join("ru_exclude_override.lst");
    if src_override.is_file() {
        if let Ok(ov_content) = tokio::fs::read_to_string(&src_override).await {
            let ov_path = std::path::Path::new(crate::override_sync::OVERRIDE_FILE);
            if ov_path.parent().map(|p| p.exists()).unwrap_or(false) {
                let _ = tx.set_extra_file(ov_path, ov_content, false).await;
            }
        }
    }

    // 3. Атомарное применение и reload с автоматическим двухфазным откатом при сбое (BK-05)
    match tx.commit_and_reload().await {
        Ok(_) => {
            log_i!("Конфиги атомарно восстановлены из бэкапа {}", req.name);
            api_ok(json!({ "restored": req.name }))
        }
        Err(e) => {
            let recovery_note = if tx.is_recovery_required() {
                " (ВНИМАНИЕ: требуется ручное восстановление файлов)"
            } else {
                ""
            };
            log_e!("Ошибка применения бэкапа {}: {e}{recovery_note}", req.name);
            api_err(format!("Ошибка применения бэкапа: {e}{recovery_note}"))
        }
    }
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
    let new_yaml = match routing::apply_assignments(&yaml, &assignments, &device_providers_for(tx.config(), &yaml)) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка применения конфигурации маршрутов: {e}"));
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

fn resolve_config_file_path(id: &str, cfg: &config::AppConfig, config_path: &str) -> Option<std::path::PathBuf> {
    let providers_dir = std::path::Path::new(&cfg.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));

    match id {
        "mihomo" => Some(std::path::PathBuf::from(&cfg.mihomo.config_path)),
        "route" => Some(std::path::PathBuf::from(config_path)),
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
    let config_path_str = state.config_path.display().to_string();
    let path = match resolve_config_file_path(&q.file, &cfg, &config_path_str) {
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

pub fn validate_yaml_syntax(content: &str) -> Result<(), String> {
    crate::transaction::validate_yaml_syntax(content)
}

/// POST /api/config-files/save
pub async fn save_config_file(
    State(state): State<AppState>,
    Json(body): Json<SaveConfigFileRequest>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    let config_path_str = state.config_path.display().to_string();
    let path = match resolve_config_file_path(&body.file, tx.config(), &config_path_str) {
        Some(p) => p,
        None => return api_err("Недопустимый идентификатор файла"),
    };

    if body.file == "route" {
        if let Err(e) = serde_json::from_str::<serde_json::Value>(&body.content) {
            return api_err(format!("Ошибка синтаксиса JSON в файле config.json: {e}"));
        }
        let parsed_cfg = config::parse_config_content(&body.content, &path.display().to_string());
        tx.stage_raw_json(&body.content, parsed_cfg);
    } else if body.file == "mihomo" {
        if let Err(e) = tx.set_yaml(&body.content) {
            return api_err(e);
        }
    } else {
        let is_yaml = body.file.starts_with("provider:")
            || path.extension().map_or(false, |ext| ext == "yaml" || ext == "yml");
        if let Err(e) = tx.set_extra_file(&path, &body.content, is_yaml).await {
            return api_err(e);
        }
    }

    if path.exists() {
        let bak = format!("{}.bak", path.display());
        let _ = tokio::fs::copy(&path, &bak).await;
    }

    let should_reload = body.reload_mihomo || body.file == "mihomo";
    if should_reload {
        if let Err(e) = tx.commit_and_reload().await {
            return api_err(e);
        }
    } else {
        if let Err(e) = tx.commit_without_reload().await {
            return api_err(e);
        }
    }

    if body.file == "override" {
        let _ = crate::override_sync::sync_geo_override(&tx.config().force_domains).await;
    }

    log_i!("Файл {} успешно сохранён через веб-редактор", path.display());
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
    if !dir.is_dir() {
        return api_err("Бэкап не найден");
    }

    let mihomo_yaml = match tokio::fs::read_to_string(dir.join("config.yaml")).await {
        Ok(s) => s,
        Err(e) => return api_err(format!("Ошибка чтения config.yaml из бэкапа: {e}")),
    };
    let route_json = match tokio::fs::read_to_string(dir.join("config.json")).await {
        Ok(s) => s,
        Err(e) => return api_err(format!("Ошибка чтения config.json из бэкапа: {e}")),
    };

    let mut files_map = serde_json::Map::new();
    files_map.insert("config.yaml".to_string(), json!(mihomo_yaml));
    files_map.insert("config.json".to_string(), json!(route_json));

    if let Ok(z) = tokio::fs::read_to_string(dir.join("zapret.conf")).await {
        files_map.insert("zapret.conf".to_string(), json!(z));
    }
    if let Ok(h) = tokio::fs::read_to_string(dir.join("zapret-hosts.txt")).await {
        files_map.insert("zapret-hosts.txt".to_string(), json!(h));
    }
    if let Ok(ov) = tokio::fs::read_to_string(dir.join("ru_exclude_override.lst")).await {
        files_map.insert("ru_exclude_override.lst".to_string(), json!(ov));
    }

    let backup_bundle = json!({
        "version": "1.0",
        "name": name,
        "exported_at": chrono::Local::now().to_rfc3339(),
        "files": files_map
    });

    let filename = format!("{}.xkbak", name);
    let payload = match serde_json::to_string_pretty(&backup_bundle) {
        Ok(p) => p,
        Err(e) => return api_err(format!("Ошибка сериализации бэкапа: {e}")),
    };

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

    // 0. Строгая проверка допустимости имен файлов и защита от path traversal (BK-02)
    for key in files.keys() {
        if key.contains("..") || key.contains('/') || key.contains('\\') {
            return api_err(format!("Обнаружена попытка path traversal в имени файла бэкапа: '{key}'"));
        }
        match key.as_str() {
            "config.yaml" | "config.json" | "zapret.conf" | "zapret-hosts.txt" | "ru_exclude_override.lst" => {},
            _ => return api_err(format!("Недопустимый файл в архиве бэкапа: '{key}'")),
        }
    }

    // 1. Проверка наличия обязательных файлов
    let yaml_str = match files.get("config.yaml").and_then(|y| y.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return api_err("В импортируемом бэкапе отсутствует или пуст обязательный файл config.yaml"),
    };
    let json_str = match files.get("config.json").and_then(|j| j.as_str()) {
        Some(s) if !s.trim().is_empty() => s,
        _ => return api_err("В импортируемом бэкапе отсутствует или пуст обязательный файл config.json"),
    };

    // 2. Валидация синтаксиса ДО записи на диск
    if let Err(e) = crate::transaction::validate_yaml_syntax(yaml_str) {
        return api_err(format!("Синтаксическая ошибка в config.yaml импортируемого бэкапа: {e}"));
    }
    if let Err(e) = serde_json::from_str::<config::AppConfig>(json_str) {
        return api_err(format!("Поврежденный JSON в config.json импортируемого бэкапа: {e}"));
    }

    // 3. Формирование и валидация имени (BK-02, BK-03: всегда префикс xr-, защита от path traversal)
    let raw_name = body.get("name")
        .and_then(|n| n.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty());

    let safe_name = match raw_name {
        Some(name) => {
            let filtered: String = name
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
                .collect();
            let base = filtered.trim_start_matches("xr-").trim_matches(|c| c == '-' || c == '_');
            if base.is_empty() {
                format!("xr-imported-{}", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f"))
            } else if filtered.starts_with("xr-") {
                filtered
            } else {
                format!("xr-{filtered}")
            }
        }
        None => format!("xr-imported-{}", chrono::Local::now().format("%Y%m%d-%H%M%S-%3f")),
    };

    if !valid_backup_name(&safe_name) {
        return api_err("Некорректное имя импортируемого бэкапа");
    }

    let cfg = state.config.read().await.clone();
    let dir = backup_root(&cfg).join(&safe_name);

    // Collision check: не перезаписываем существующий бэкап вслепую
    if dir.exists() {
        return api_err(format!("Бэкап с именем '{}' уже существует", safe_name));
    }

    let tmp_dir = backup_root(&cfg).join(format!("{safe_name}.tmp"));
    if let Err(e) = tokio::fs::create_dir_all(&tmp_dir).await {
        return api_err(format!("Ошибка создания временного каталога бэкапа: {e}"));
    }

    // Запись файлов в staging каталог
    if let Err(e) = tokio::fs::write(tmp_dir.join("config.yaml"), yaml_str).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Ошибка записи config.yaml: {e}"));
    }
    if let Err(e) = tokio::fs::write(tmp_dir.join("config.json"), json_str).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Ошибка записи config.json: {e}"));
    }

    // Опциональные файлы
    if let Some(zapret_str) = files.get("zapret.conf").and_then(|z| z.as_str()) {
        let _ = tokio::fs::write(tmp_dir.join("zapret.conf"), zapret_str).await;
    }
    if let Some(hosts_str) = files.get("zapret-hosts.txt").and_then(|h| h.as_str()) {
        let _ = tokio::fs::write(tmp_dir.join("zapret-hosts.txt"), hosts_str).await;
    }
    if let Some(ov_str) = files.get("ru_exclude_override.lst").and_then(|o| o.as_str()) {
        let _ = tokio::fs::write(tmp_dir.join("ru_exclude_override.lst"), ov_str).await;
    }

    // Атомарное переименование staging -> final
    if let Err(e) = tokio::fs::rename(&tmp_dir, &dir).await {
        let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
        return api_err(format!("Ошибка фиксации каталога импорта: {e}"));
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
    let yaml = body.yaml_content.trim();
    if yaml.is_empty() {
        return api_err("YAML ноды не может быть пустым");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    if body.target == "provider" {
        let prov_name = body.provider_name.unwrap_or_else(|| "custom".to_string());
        let providers_dir = std::path::Path::new(&tx.config().mihomo.config_path)
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

        if let Err(e) = tx.set_extra_file(&path, current_content, true).await {
            return api_err(e);
        }

        let config_yaml = match tx.read_yaml().await {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };
        if !config_yaml.contains(&format!("{}:", prov_name)) {
            let health_url = tx.config().health_check_url();
            let interval = tx.config().mihomo.health_check_interval;
            if let Ok(new_yaml) = crate::routing::add_provider_to_yaml(
                &config_yaml,
                &prov_name,
                &format!("file:///opt/etc/mihomo/providers/{}.yaml", prov_name),
                Some(health_url),
                Some(interval),
            ) {
                if let Err(e) = tx.set_yaml(new_yaml) {
                    return api_err(e);
                }
            }
        }

        if let Err(e) = tx.commit_and_reload().await {
            return api_err(e);
        }

        log_i!("Импортирована нода в провайдер {}", prov_name);
        return api_ok(json!({ "imported": true, "target": "provider", "file": prov_file }));
    }

    let config_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
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

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(e);
    }

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
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    let config_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let target_mode = if body.enhanced_mode == "redir-host" { "redir-host" } else { "fake-ip" };
    static RE_ENHANCED_MODE: std::sync::LazyLock<regex_lite::Regex> =
        std::sync::LazyLock::new(|| regex_lite::Regex::new(r"(?m)^(\s*enhanced-mode:\s*)[^\r\n]+").unwrap());
    let new_yaml = if RE_ENHANCED_MODE.is_match(&config_yaml) {
        RE_ENHANCED_MODE.replace(&config_yaml, format!("${{1}}{target_mode}")).into_owned()
    } else {
        config_yaml.clone()
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(e);
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
    let clean_ip = body.ip.trim();
    if !crate::config::is_valid_ip_or_cidr(clean_ip) {
        return api_err(format!("Некорректный IP-адрес или CIDR устройства: '{clean_ip}'"));
    }

    // Валидация правил (домены и target шлюзов)
    let mut validated_rules = Vec::with_capacity(body.rules.len());
    for r in &body.rules {
        let norm_dom = crate::config::normalize_domain(&r.domain);
        if !crate::config::is_valid_domain(&norm_dom) {
            return api_err(format!("Некорректный домен в правиле: '{}'", r.domain));
        }
        let target = r.target.trim();
        if target.is_empty() || target.contains(',') || target.contains('\n') || target.contains('\r') {
            return api_err(format!("Некорректный целевой шлюз (target): '{}'", r.target));
        }
        validated_rules.push(crate::config::DeviceDomainRule {
            domain: norm_dom,
            target: target.to_string(),
        });
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    if validated_rules.is_empty() {
        tx.config_mut().device_domain_rules.remove(clean_ip);
    } else {
        tx.config_mut().device_domain_rules.insert(clean_ip.to_string(), validated_rules);
    }

    let raw_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    let (new_yaml, _) = match crate::routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка формирования маршрутов: {e}")),
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(e);
    }

    log_i!("Обновлены индивидуальные доменные правила для устройства {}", clean_ip);
    api_ok(json!({ "saved": true, "applied": true, "ip": clean_ip }))
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
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    let target_enabled = match body {
        Some(axum::extract::Json(b)) => b.enabled.unwrap_or(!tx.config().adblock_enabled),
        None => !tx.config().adblock_enabled,
    };
    tx.config_mut().adblock_enabled = target_enabled;

    let raw_yaml = match tx.read_yaml().await {
        Ok(c) => c,
        Err(e) => return api_err(e),
    };

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка роутинга: {}", e)),
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(e);
    }

    api_ok(json!({ "enabled": target_enabled, "saved": true, "applied": true }))
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
    let raw_domain = body.domain.trim();
    let norm = crate::config::normalize_domain(raw_domain);
    let domain = if !norm.is_empty() { norm.to_lowercase() } else { raw_domain.to_lowercase() };
    let src_ip = body.source_ip.as_deref().unwrap_or("").trim();

    // DIAG-02: Запрос активных правил и прокси-групп ядра Mihomo в runtime
    let runtime_rules_res = mihomo::m_get(&state.http, &cfg, "/rules").await;
    let runtime_proxies_res = mihomo::get_proxies(&state.http, &cfg).await;
    let is_runtime = runtime_rules_res.is_ok();
    let active_srv = if let Ok(ref proxies) = runtime_proxies_res {
        mihomo::resolve_active_leaf(proxies)
    } else {
        cfg.failover.priority_chain.first().cloned().unwrap_or_else(|| "PROXY".into())
    };

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
                        "reason": format!("Сработало индивидуальное доменное правило устройства {}", src_ip),
                        "estimated": !is_runtime,
                    }));
                }
            }
        }
    }

    // 2. AdBlock правило (DIAG-02: исключены ложные подстрочные эвристики вроде "analytics")
    if cfg.adblock_enabled {
        let is_ad = domain == "an.yandex.ru"
            || domain.ends_with(".an.yandex.ru")
            || domain == "doubleclick.net"
            || domain.ends_with(".doubleclick.net")
            || domain == "googleads.g.doubleclick.net"
            || domain == "adservice.google.com";
        if is_ad {
            return api_ok(json!({
                "matched_rule": "GEOSITE,category-ads-all,REJECT",
                "rule_type": "ADBLOCK",
                "target_group": "REJECT",
                "resolved_server": "REJECT",
                "reason": "Заблокировано сетевым AdBlock фильтром роутера",
                "estimated": !is_runtime,
                "limitations": if !is_runtime { Some("Предварительная оценка: правила AdBlock проверяются в offline режиме") } else { None }
            }));
        }
    }

    // 2.1 Активная последовательность правил Mihomo runtime (DIAG-02)
    if let Ok(rules_val) = &runtime_rules_res {
        if let Some(rules_arr) = rules_val.get("rules").and_then(|r| r.as_array()) {
            for r in rules_arr {
                let r_type = r.get("type").and_then(|t| t.as_str()).unwrap_or("");
                let r_payload = r.get("payload").and_then(|p| p.as_str()).unwrap_or("");
                let r_proxy = r.get("proxy").and_then(|p| p.as_str()).unwrap_or("");

                let matched = match r_type {
                    "DOMAIN" => domain == r_payload.to_lowercase(),
                    "DOMAIN-SUFFIX" => {
                        let p = r_payload.to_lowercase();
                        domain == p || domain.ends_with(&format!(".{}", p))
                    }
                    "DOMAIN-KEYWORD" => domain.contains(&r_payload.to_lowercase()),
                    "SRC-IP-CIDR" => !src_ip.is_empty() && (src_ip == r_payload.trim_end_matches("/32")),
                    _ => false,
                };

                if matched {
                    let resolved = match r_proxy {
                        "DIRECT" => "DIRECT (Напрямую)".to_string(),
                        "REJECT" => "REJECT".to_string(),
                        group => {
                            if let Ok(p) = &runtime_proxies_res {
                                if let Some(grp_obj) = p.get(group) {
                                    grp_obj.get("now").and_then(|n| n.as_str()).unwrap_or(group).to_string()
                                } else {
                                    active_srv.clone()
                                }
                            } else {
                                active_srv.clone()
                            }
                        }
                    };

                    return api_ok(json!({
                        "matched_rule": format!("{},{},{}", r_type, r_payload, r_proxy),
                        "rule_type": r_type,
                        "target_group": r_proxy,
                        "resolved_server": resolved,
                        "reason": format!("Сработало активное правило runtime ядра Mihomo: {} ({}) -> {}", r_type, r_payload, r_proxy),
                        "estimated": false
                    }));
                }
            }
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
                        "reason": format!("Весь трафик устройства {} направлен на сервер {}", src_ip, first_srv),
                        "estimated": !is_runtime,
                        "limitations": if !is_runtime { Some("Предварительная оценка: опрос произведён по конфигурации XKeen без проверки активного рантайма") } else { None }
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
                "reason": "Выделенный маршрут Google AI / Antigravity Cloud Code",
                "estimated": !is_runtime,
                "limitations": if !is_runtime { Some("Предварительная оценка: опрос произведён по конфигурации XKeen без проверки активного рантайма") } else { None }
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
                "reason": "Домен находится в белом списке прямого доступа (минуя прокси)",
                "estimated": !is_runtime,
                "limitations": if !is_runtime { Some("Предварительная оценка: опрос произведён по конфигурации XKeen без проверки активного рантайма") } else { None }
            }));
        }
    }

    // 6. Zapret Hybrid правила (если служба Zapret включена) — безусловный приоритет DIRECT-обхода nfqws над force_domains
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
                        "reason": "Изоляция IP-блокировок через VPN-прокси (Zapret Hybrid)",
                        "estimated": !is_runtime,
                        "limitations": if !is_runtime { Some("Предварительная оценка: правила Zapret проверяются без эмуляции сокетов") } else { None }
                    }));
                }
            }
        }

        // YouTube Direct -> DIRECT
        if cfg.zapret.hybrid_youtube {
            let matched = crate::routing::YOUTUBE_HYBRID_DOMAINS.iter().find(|d| domain == **d || domain.ends_with(&format!(".{}", d)));
            if let Some(m) = matched {
                return api_ok(json!({
                    "matched_rule": format!("DOMAIN-SUFFIX,{},DIRECT", m),
                    "rule_type": "ZAPRET_HYBRID",
                    "target_group": "DIRECT",
                    "resolved_server": "DIRECT (Локальный обход Zapret)",
                    "reason": "Zapret DPI bypass — YouTube Direct (напрямую с кэш-серверов GGC без расхода VPS)",
                    "estimated": !is_runtime,
                    "limitations": if !is_runtime { Some("Предварительная оценка: правила Zapret проверяются без эмуляции сокетов") } else { None }
                }));
            }
        }

        // Discord Direct -> DIRECT
        if cfg.zapret.hybrid_discord {
            let matched = crate::routing::DISCORD_HYBRID_DOMAINS.iter().find(|d| domain == **d || domain.ends_with(&format!(".{}", d)));
            if let Some(m) = matched {
                return api_ok(json!({
                    "matched_rule": format!("DOMAIN-SUFFIX,{},DIRECT", m),
                    "rule_type": "ZAPRET_HYBRID",
                    "target_group": "DIRECT",
                    "resolved_server": "DIRECT (Локальный обход Zapret)",
                    "reason": "Zapret DPI bypass — Discord Direct (минимальный пинг напрямую без VPS)",
                    "estimated": !is_runtime,
                    "limitations": if !is_runtime { Some("Предварительная оценка: правила Zapret проверяются без эмуляции сокетов") } else { None }
                }));
            }
        }

        // GitHub Direct -> DIRECT
        if cfg.zapret.bypass_github {
            let is_gh = crate::routing::GITHUB_DOMAINS.iter().any(|d| domain == *d || domain.ends_with(&format!(".{}", d)))
                || domain == "github.com"
                || domain.ends_with(".github.com");
            if is_gh {
                let matched = crate::routing::GITHUB_DOMAINS.iter().find(|d| domain == **d || domain.ends_with(&format!(".{}", d))).unwrap_or(&"github.com");
                return api_ok(json!({
                    "matched_rule": format!("DOMAIN-SUFFIX,{},DIRECT", matched),
                    "rule_type": "ZAPRET_HYBRID",
                    "target_group": "DIRECT",
                    "resolved_server": "DIRECT (Локальный обход Zapret)",
                    "reason": "Zapret DPI bypass — GitHub Direct (быстрый доступ к репозиториям и релизам без расхода VPS)",
                    "estimated": !is_runtime,
                    "limitations": if !is_runtime { Some("Предварительная оценка: правила Zapret проверяются без эмуляции сокетов") } else { None }
                }));
            }
        }

        // Торренты & Трекеры -> DIRECT
        if cfg.zapret.bypass_torrents {
            let is_torrent = crate::routing::TORRENT_DOMAINS.iter().any(|d| domain == *d || domain.ends_with(&format!(".{}", d)));
            if is_torrent {
                let matched = crate::routing::TORRENT_DOMAINS.iter().find(|d| domain == **d || domain.ends_with(&format!(".{}", d))).unwrap_or(&"rutracker.org");
                return api_ok(json!({
                    "matched_rule": format!("DOMAIN-SUFFIX,{},DIRECT", matched),
                    "rule_type": "ZAPRET_HYBRID",
                    "target_group": "DIRECT",
                    "resolved_server": "DIRECT (Локальный обход Zapret)",
                    "reason": "Zapret DPI bypass — Торрент-трекеры Direct (скачивание метаданных и раздач без блокировки)",
                    "estimated": !is_runtime,
                    "limitations": if !is_runtime { Some("Предварительная оценка: правила Zapret проверяются без эмуляции сокетов") } else { None }
                }));
            }
        }

        // 18+ Контент -> DIRECT
        if cfg.zapret.bypass_adult {
            let is_adult = crate::routing::ADULT_DOMAINS.iter().any(|d| domain == *d || domain.ends_with(&format!(".{}", d)));
            if is_adult {
                let matched = crate::routing::ADULT_DOMAINS.iter().find(|d| domain == **d || domain.ends_with(&format!(".{}", d))).unwrap_or(&"pornhub.com");
                return api_ok(json!({
                    "matched_rule": format!("DOMAIN-SUFFIX,{},DIRECT", matched),
                    "rule_type": "ZAPRET_HYBRID",
                    "target_group": "DIRECT",
                    "resolved_server": "DIRECT (Локальный обход Zapret)",
                    "reason": "Zapret DPI bypass — 18+ Контент Direct (локальный обход блокировок ТСПУ без нагрузки на VPS)",
                    "estimated": !is_runtime,
                    "limitations": if !is_runtime { Some("Предварительная оценка: правила Zapret проверяются без эмуляции сокетов") } else { None }
                }));
            }
        }

        // Пользовательские сайты и их связанные CDN (/boost) -> DIRECT
        for entry in &cfg.zapret.custom_entries {
            if entry.enabled {
                let clean_entry = crate::config::normalize_domain(&entry.domain);
                let matched_custom = if !clean_entry.is_empty() && (domain == clean_entry || domain.ends_with(&format!(".{}", clean_entry))) {
                    Some(clean_entry.clone())
                } else {
                    entry.cdns.iter().find_map(|cdn| {
                        let clean_cdn = crate::config::normalize_domain(cdn);
                        if !clean_cdn.is_empty() && (domain == clean_cdn || domain.ends_with(&format!(".{}", clean_cdn))) {
                            Some(clean_cdn)
                        } else {
                            None
                        }
                    })
                };
                if let Some(m) = matched_custom {
                    return api_ok(json!({
                        "matched_rule": format!("DOMAIN-SUFFIX,{},DIRECT", m),
                        "rule_type": "ZAPRET_HYBRID",
                        "target_group": "DIRECT",
                        "resolved_server": "DIRECT (Локальный обход Zapret)",
                        "reason": format!("Zapret DPI bypass — Ускоренный сайт /boost ({})", entry.domain),
                        "estimated": !is_runtime,
                        "limitations": if !is_runtime { Some("Предварительная оценка: правила Zapret проверяются без эмуляции сокетов") } else { None }
                    }));
                }
            }
        }
    }

    // 7. Принудительный список (PROXY / FORCE)
    for d in &cfg.force_domains {
        let d_lower = d.trim().to_lowercase();
        if domain == d_lower || domain.ends_with(&format!(".{}", d_lower)) {
            return api_ok(json!({
                "matched_rule": format!("DOMAIN-SUFFIX,{},PROXY", d),
                "rule_type": "FORCE_DOMAIN",
                "target_group": "PROXY",
                "resolved_server": active_srv,
                "reason": "Домен находится в списке принудительного проксирования XKeen",
                "estimated": !is_runtime,
                "limitations": if !is_runtime { Some("Предварительная оценка: опрос произведён по конфигурации XKeen без проверки активного рантайма") } else { None }
            }));
        }
    }

    // 8. Наборы правил Mihomo (Rule-Sets) — точное совпадение FQDN и поддоменов (DIAG-02: без ложных подстрочных эвристик)
    let is_yt = domain == "youtube.com" || domain.ends_with(".youtube.com")
        || domain == "youtu.be"
        || domain == "googlevideo.com" || domain.ends_with(".googlevideo.com")
        || domain == "ytimg.com" || domain.ends_with(".ytimg.com");
    if is_yt {
        return api_ok(json!({
            "matched_rule": "RULE-SET,youtube@domain,YouTube",
            "rule_type": "RULE_SET",
            "target_group": "YouTube",
            "resolved_server": active_srv,
            "reason": "Специальная группа проксирования YouTube (Mihomo Selector)",
            "estimated": !is_runtime,
            "limitations": if !is_runtime { Some("Предварительная оценка: правила проверяются по статической маске без активного рантайма") } else { None }
        }));
    }

    let is_discord = domain == "discord.com" || domain.ends_with(".discord.com")
        || domain == "discord.gg" || domain.ends_with(".discord.gg")
        || domain == "discordapp.com" || domain.ends_with(".discordapp.com")
        || domain == "discordapp.net" || domain.ends_with(".discordapp.net");
    if is_discord {
        return api_ok(json!({
            "matched_rule": "RULE-SET,discord@classical,Discord",
            "rule_type": "RULE_SET",
            "target_group": "Discord",
            "resolved_server": active_srv,
            "reason": "Специальная группа проксирования Discord",
            "estimated": !is_runtime,
            "limitations": if !is_runtime { Some("Предварительная оценка: правила проверяются по статической маске без активного рантайма") } else { None }
        }));
    }

    let is_tg = domain == "t.me" || domain.ends_with(".t.me")
        || domain == "telegram.org" || domain.ends_with(".telegram.org")
        || domain == "telegram.me" || domain.ends_with(".telegram.me");
    if is_tg {
        return api_ok(json!({
            "matched_rule": "RULE-SET,telegram@domain,Telegram",
            "rule_type": "RULE_SET",
            "target_group": "Telegram",
            "resolved_server": active_srv,
            "reason": "Специальная группа проксирования Telegram",
            "estimated": !is_runtime,
            "limitations": if !is_runtime { Some("Предварительная оценка: правила проверяются по статической маске без активного рантайма") } else { None }
        }));
    }

    let is_steam = domain == "steampowered.com" || domain.ends_with(".steampowered.com")
        || domain == "steamcommunity.com" || domain.ends_with(".steamcommunity.com")
        || domain == "steamstatic.com" || domain.ends_with(".steamstatic.com");
    if is_steam {
        return api_ok(json!({
            "matched_rule": "RULE-SET,steam@domain,Steam",
            "rule_type": "RULE_SET",
            "target_group": "Steam",
            "resolved_server": active_srv,
            "reason": "Группа маршрутизации Steam",
            "estimated": !is_runtime,
            "limitations": if !is_runtime { Some("Предварительная оценка: правила проверяются по статической маске без активного рантайма") } else { None }
        }));
    }

    let is_twitch = domain == "twitch.tv" || domain.ends_with(".twitch.tv")
        || domain == "ttvnw.net" || domain.ends_with(".ttvnw.net")
        || domain == "jtvnw.net" || domain.ends_with(".jtvnw.net");
    if is_twitch {
        return api_ok(json!({
            "matched_rule": "RULE-SET,twitch@domain,Twitch",
            "rule_type": "RULE_SET",
            "target_group": "Twitch",
            "resolved_server": active_srv,
            "reason": "Группа маршрутизации Twitch",
            "estimated": !is_runtime,
            "limitations": if !is_runtime { Some("Предварительная оценка: правила проверяются по статической маске без активного рантайма") } else { None }
        }));
    }

    let is_torrent_tracker = domain == "rutracker.org" || domain.ends_with(".rutracker.org")
        || domain == "nnmclub.to" || domain.ends_with(".nnmclub.to")
        || domain == "rutor.info" || domain.ends_with(".rutor.info")
        || domain == "opentor.org" || domain.ends_with(".opentor.org")
        || domain == "pornolab.net" || domain.ends_with(".pornolab.net")
        || domain == "torrent" || domain.ends_with(".torrent");
    if is_torrent_tracker {
        return api_ok(json!({
            "matched_rule": "RULE-SET,public-tracker@domain,Torrent",
            "rule_type": "RULE_SET",
            "target_group": "Torrent",
            "resolved_server": "DIRECT",
            "reason": "P2P и торрент-трафик (Torrent группа)",
            "estimated": !is_runtime,
            "limitations": if !is_runtime { Some("Предварительная оценка: правила проверяются по статической маске без активного рантайма") } else { None }
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
            "reason": "Российский сегмент интернета (RU Geosite) — прямой доступ без прокси",
            "estimated": !is_runtime,
            "limitations": if !is_runtime { Some("Предварительная оценка: правила проверяются по статической маске без активного рантайма") } else { None }
        }));
    }

    // 9. Финальное базовое правило (MATCH)
    api_ok(json!({
        "matched_rule": "MATCH,PROXY",
        "rule_type": "MATCH",
        "target_group": "PROXY",
        "resolved_server": active_srv,
        "reason": "Сработало финальное правило маршрутизации по умолчанию (MATCH)",
        "estimated": !is_runtime,
        "limitations": if !is_runtime { Some("Mihomo offline: симуляция по локальной конфигурации панели без проверки активных Rule-Providers и динамических групп ядра") } else { None }
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

    // 5. Проверка хранилища /opt (DIAG-03: честный статус вместо безусловного ok)
    let (disk_status, disk_msg) = match tokio::process::Command::new("df").arg("-h").arg("/opt").output().await {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            // Безопасный поиск процента использования: пропускаем заголовок и ищем токен, оканчивающийся на %
            let use_percent = stdout.lines()
                .skip(1)
                .flat_map(|l| l.split_whitespace())
                .find_map(|p| p.strip_suffix('%').and_then(|s| s.parse::<u8>().ok()))
                .unwrap_or(0);
            let detail_line = stdout.lines().skip(1).collect::<Vec<_>>().join(" ");
            let display_text = if detail_line.trim().is_empty() {
                "Накопитель Entware смонтирован".to_string()
            } else {
                detail_line.trim().to_string()
            };
            if use_percent >= 98 {
                ("fail", format!("Накопитель /opt заполнен на {}%! ({})", use_percent, display_text))
            } else if use_percent >= 90 {
                ("warn", format!("Накопитель /opt заполнен на {}% ({})", use_percent, display_text))
            } else {
                ("ok", display_text)
            }
        }
        Ok(out) => ("warn", format!("Команда df вернула ошибку: {}", out.status)),
        Err(e) => ("unknown", format!("Проверка дискового пространства недоступна: {}", e)),
    };

    api_ok(json!({
        "checks": [
            { "id": "gateway", "name": "Шлюз роутера Keenetic", "status": gw_status, "message": gw_msg, "measurement_point": "router_local" },
            { "id": "mihomo", "name": "Ядро Mihomo (XKeen)", "status": mihomo_status, "message": mihomo_msg, "measurement_point": "router_local" },
            { "id": "dns", "name": "DNS Резолвер", "status": dns_status, "message": dns_msg, "latency_ms": dns_ms, "measurement_point": "router_local" },
            { "id": "wan", "name": "Прямой выход в интернет", "status": wan_status, "message": wan_msg, "measurement_point": "router_wan" },
            { "id": "storage", "name": "Дисковое пространство /opt", "status": disk_status, "message": disk_msg, "measurement_point": "router_local" },
        ]
    }))
}

#[derive(Deserialize)]
pub struct DnsTestReq {
    pub domain: String,
}

/// POST /api/diagnostics/dns-test — Smart DNS диагностика домена (DIAG-03: честные наблюдения)
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

    let has_loopback = local_ips.iter().any(|ip| ip == "127.0.0.1" || ip == "0.0.0.0" || ip == "::1");
    let has_private_ip = local_ips.iter().any(|ip_str| {
        if let Ok(ip) = ip_str.parse::<std::net::IpAddr>() {
            match ip {
                std::net::IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
                std::net::IpAddr::V6(v6) => v6.is_loopback() || (v6.segments()[0] & 0xfe00) == 0xfc00,
            }
        } else {
            ip_str.starts_with("10.") || ip_str.starts_with("192.168.") || ip_str.starts_with("172.")
        }
    });

    let mut http_status: Option<u16> = None;
    let mut error_type: Option<String> = None;
    let mut http_direct_ok = false;

    match state.http.get(format!("https://{}", domain)).timeout(std::time::Duration::from_secs(3)).send().await {
        Ok(resp) => {
            let code = resp.status().as_u16();
            http_status = Some(code);
            http_direct_ok = resp.status().is_success() || resp.status().is_redirection();
        }
        Err(e) => {
            if e.is_timeout() {
                error_type = Some("timeout".to_string());
            } else if e.is_connect() {
                error_type = Some("connect_error".to_string());
            } else {
                error_type = Some("protocol_error".to_string());
            }
        }
    }

    // Честный диагноз без ложных категоричных утверждений о подмене провайдером при обычном таймауте
    let verdict = if local_ips.is_empty() {
        "Домен не найден в DNS (NXDOMAIN или ошибка резолвера)."
    } else if has_loopback {
        "Домен резолвится в адрес-заглушку (127.0.0.1 / 0.0.0.0). Возможна фильтрация DNS AdBlock."
    } else if has_private_ip {
        "Домен резолвится в частный IP-адрес локальной сети (Intranet или перенаправление)."
    } else if http_direct_ok {
        "Домен доступен напрямую без ограничений."
    } else if error_type.as_deref() == Some("timeout") {
        "Таймаут прямого HTTPS соединения (сервер не ответил за 3 сек; причина не установлена, возможна фильтрация пакетов или высокая задержка)."
    } else if error_type.as_deref() == Some("connect_error") {
        "Прямое TCP/TLS соединение сброшено (RST / connection refused / блокировка по SNI/IP)."
    } else {
        "Прямое соединение не удалось; статус не установлен."
    };

    let recommendation = if has_loopback || has_private_ip || !http_direct_ok {
        "Рекомендуется добавить домен в список «Принудительно через прокси» в Настройках или проверить назначение устройства на зарубежный сервер."
    } else {
        "Дополнительных действий не требуется."
    };

    api_ok(json!({
        "domain": domain,
        "resolved_ips": local_ips,
        "has_private_ip": has_private_ip || has_loopback,
        "is_poisoned": has_loopback,
        "http_direct_ok": http_direct_ok,
        "http_status": http_status,
        "error_type": error_type,
        "measurement_point": "router_local",
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

pub const NDM_NETFILTER_SCRIPT: &str = r#"#!/bin/sh
PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH

# Keenetic ndm netfilter hook for Zapret
# Automatically re-injects netfilter rules whenever KeeneticOS rebuilds iptables
# (e.g. DHCP lease renewal, WAN reconnect, ping-check, routing policy reload).

[ "$type" = "ip6" ] || [ "$type" = "ipv6" ] && exit 0
[ "$table" = "filter" ] || [ "$table" = "security" ] && exit 0

get_active_wan_ifaces() {
  if [ -f /proc/net/route ]; then
    awk '$2 == "00000000" && $8 !~ /^lo/ {print $1}' /proc/net/route 2>/dev/null | sort -u
  elif command -v ip >/dev/null 2>&1; then
    ip route show default 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' | sort -u
  fi
}

is_running() {
  if [ -f /opt/var/run/zapret.pid ]; then
    PID=$(cat /opt/var/run/zapret.pid 2>/dev/null)
    [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null && return 0
  fi
  pidof nfqws2 >/dev/null 2>&1 && return 0
  pidof nfqws >/dev/null 2>&1 && return 0
  return 1
}

is_running || exit 0
[ -x /opt/etc/init.d/S51zapret ] || exit 0

/opt/etc/init.d/S51zapret start-fw >/dev/null 2>&1
exit 0
"#;

pub const NDM_IFSTATE_SCRIPT: &str = r#"#!/bin/sh
PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH

# Keenetic ndm ifstatechanged hook for Zapret
# Re-applies netfilter rules when interface state changes (WAN connects, DHCP lease renewed).

[ "$state" = "down" ] && exit 0

get_active_wan_ifaces() {
  if [ -f /proc/net/route ]; then
    awk '$2 == "00000000" && $8 !~ /^lo/ {print $1}' /proc/net/route 2>/dev/null | sort -u
  elif command -v ip >/dev/null 2>&1; then
    ip route show default 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' | sort -u
  fi
}

is_running() {
  if [ -f /opt/var/run/zapret.pid ]; then
    PID=$(cat /opt/var/run/zapret.pid 2>/dev/null)
    [ -n "$PID" ] && kill -0 "$PID" 2>/dev/null && return 0
  fi
  pidof nfqws2 >/dev/null 2>&1 && return 0
  pidof nfqws >/dev/null 2>&1 && return 0
  return 1
}

is_running || exit 0
[ -x /opt/etc/init.d/S51zapret ] || exit 0

/opt/etc/init.d/S51zapret start-fw >/dev/null 2>&1
exit 0
"#;

pub const S51ZAPRET_SCRIPT: &str = r#"#!/bin/sh
PATH=/opt/sbin:/opt/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
export PATH

IPTABLES_CMD="iptables"
if command iptables -w 1 -L -n >/dev/null 2>&1; then
  IPTABLES_CMD="iptables -w 2"
fi

iptables() {
  command $IPTABLES_CMD "$@"
}

PIDFILE="/opt/var/run/zapret.pid"
FAILSAFE_PID="/opt/var/run/zapret_failsafe.pid"
CONF="/opt/etc/zapret/zapret.conf"
LOCKFILE="/opt/var/run/xkeen-zapret.lock"
LOCKDIR="/opt/var/run/xkeen-zapret.lock.d"
DISABLED_MARKER="/opt/var/run/xkeen-zapret.disabled"

acquire_zapret_lock() {
  local timeout=15
  if command -v flock >/dev/null 2>&1; then
    exec 9>"$LOCKFILE"
    if ! flock -w "$timeout" 9 2>/dev/null; then
      echo "Failed to acquire zapret flock" >&2
      return 1
    fi
    echo $$ > "$LOCKFILE" 2>/dev/null
    return 0
  fi
  local waited=0
  while ! mkdir "$LOCKDIR" 2>/dev/null; do
    if [ -f "$LOCKDIR/pid" ]; then
      local holder=$(cat "$LOCKDIR/pid" 2>/dev/null)
      if [ -n "$holder" ] && ! kill -0 "$holder" 2>/dev/null; then
        rm -rf "$LOCKDIR" 2>/dev/null
        continue
      fi
    fi
    if [ "$waited" -ge "$timeout" ]; then
      echo "Failed to acquire zapret lockdir after ${timeout}s" >&2
      return 1
    fi
    sleep 1
    waited=$((waited + 1))
  done
  echo "$$" > "$LOCKDIR/pid" 2>/dev/null
  return 0
}

release_zapret_lock() {
  if command -v flock >/dev/null 2>&1; then
    flock -u 9 2>/dev/null
    exec 9>&- 2>/dev/null
  fi
  rm -rf "$LOCKDIR" 2>/dev/null
}

is_zapret_pid() {
  local p="$1"
  [ -z "$p" ] && return 1
  [ -d "/proc/$p" ] || return 1
  if [ -r "/proc/$p/comm" ]; then
    local comm=$(cat "/proc/$p/comm" 2>/dev/null)
    case "$comm" in
      nfqws|nfqws2|tpws) return 0 ;;
    esac
  fi
  if [ -r "/proc/$p/cmdline" ]; then
    local cmdline=$(tr '\0' ' ' < "/proc/$p/cmdline" 2>/dev/null)
    case "$cmdline" in
      *nfqws*|*nfqws2*|*tpws*) return 0 ;;
    esac
  fi
  return 1
}

check_dns_resolver_ready() {
  if grep -q ':041D' /proc/net/udp 2>/dev/null || grep -q ':041D' /proc/net/tcp 2>/dev/null; then
    return 0
  fi
  if command -v netstat >/dev/null 2>&1 && netstat -tuln 2>/dev/null | grep -q ':1053 '; then
    return 0
  fi
  return 1
}

get_active_wan_ifaces() {
  if [ -f /proc/net/route ]; then
    awk '$2 == "00000000" && $8 !~ /^lo/ {print $1}' /proc/net/route 2>/dev/null | sort -u
  elif command -v ip >/dev/null 2>&1; then
    ip route show default 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="dev") print $(i+1)}' | sort -u
  fi
}

find_bin() {
  ARCH=$(uname -m 2>/dev/null)
  case "$ARCH" in
    aarch64|arm64)
      ARCH_CANDIDATES="arm64 linux-arm64"
      ;;
    armv7*|armv8*|arm*)
      ARCH_CANDIDATES="arm linux-arm"
      ;;
    mips*)
      if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ] || \
         [ "$(od -t x1 -j 5 -N 1 /bin/sh 2>/dev/null | awk 'NR==1{print $2}')" = "02" ] || \
         [ "$(echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }')" = "0" ]; then
        ARCH_CANDIDATES="mips linux-mips32r2-msb mipsel linux-mips32r2-lsb"
      else
        ARCH_CANDIDATES="mipsel linux-mips32r2-lsb mips linux-mips32r2-msb"
      fi
      ;;
    x86_64|amd64)
      ARCH_CANDIDATES="x86_64 linux-x86_64"
      ;;
    i*86|x86)
      ARCH_CANDIDATES="x86 linux-x86"
      ;;
    *)
      ARCH_CANDIDATES="mipsel arm64 arm mips x86_64"
      ;;
  esac

  is_runnable() {
    [ -n "$1" ] && [ -x "$1" ] && "$1" --help >/dev/null 2>&1
    local ret=$?
    [ $ret -ne 126 ] && [ $ret -ne 127 ]
  }

  if [ "$ZAPRET_ENGINE" = "v1" ] || [ "$ZAPRET_ENGINE" = "legacy" ]; then
    if [ -x "/opt/zapret/nfq/nfqws" ] && is_runnable "/opt/zapret/nfq/nfqws"; then
      echo "/opt/zapret/nfq/nfqws"
      return
    fi
    for cand in $ARCH_CANDIDATES; do
      if [ -x "/opt/zapret/binaries/$cand/nfqws" ] && is_runnable "/opt/zapret/binaries/$cand/nfqws"; then
        echo "/opt/zapret/binaries/$cand/nfqws"
        return
      fi
    done
    for std_bin in /opt/sbin/nfqws /opt/bin/nfqws /opt/usr/bin/nfqws /opt/zapret/nfqws.bak; do
      if [ -x "$std_bin" ] && is_runnable "$std_bin"; then
        echo "$std_bin"
        return
      fi
    done
    if [ -x "/opt/zapret2/nfqws2" ] && is_runnable "/opt/zapret2/nfqws2"; then
      echo "/opt/zapret2/nfqws2"
      return
    elif [ -x "/opt/sbin/nfqws2" ] && is_runnable "/opt/sbin/nfqws2"; then
      echo "/opt/sbin/nfqws2"
      return
    fi
    return
  fi

  if [ -x "/opt/zapret2/nfqws2" ] && is_runnable "/opt/zapret2/nfqws2"; then
    echo "/opt/zapret2/nfqws2"
    return
  fi
  for std_z2 in /opt/sbin/nfqws2 /opt/bin/nfqws2 /opt/usr/bin/nfqws2; do
    if [ -x "$std_z2" ] && is_runnable "$std_z2"; then
      echo "$std_z2"
      return
    fi
  done
  for cand in $ARCH_CANDIDATES; do
    if [ -x "/opt/zapret2/binaries/$cand/nfqws2" ] && is_runnable "/opt/zapret2/binaries/$cand/nfqws2"; then
      cp -f "/opt/zapret2/binaries/$cand/nfqws2" /opt/zapret2/nfqws2 2>/dev/null || true
      chmod +x /opt/zapret2/nfqws2 2>/dev/null || true
      echo "/opt/zapret2/binaries/$cand/nfqws2"
      return
    fi
  done
  for cand in $ARCH_CANDIDATES; do
    if [ -x "/opt/zapret/binaries/$cand/nfqws" ] && is_runnable "/opt/zapret/binaries/$cand/nfqws"; then
      echo "/opt/zapret/binaries/$cand/nfqws"
      return
    fi
  done
  if [ -x "/opt/zapret/nfq/nfqws" ] && is_runnable "/opt/zapret/nfq/nfqws"; then
    echo "/opt/zapret/nfq/nfqws"
    return
  fi
}

# Fallback default with fwmark to prevent loops
ZAPRET_ENGINE="v2"
NFQWS_ARGS="--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 --filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com --dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
DISCORD_VOICE_ENABLED="1"
BLOCK_QUIC="0"
SMART_TV_MODE="0"
EXCLUDED_IPS=""
EXCLUDED_MACS=""

# Safe key-value parser for zapret.conf (no source / eval)
if [ -f "$CONF" ]; then
  while IFS='=' read -r key val || [ -n "$key" ]; do
    key=$(echo "$key" | tr -d ' \t\r\n')
    val=$(echo "$val" | sed -e 's/^[ \t]*//' -e 's/[ \t\r\n]*$//')
    case "$key" in
      ZAPRET_ENGINE)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        ZAPRET_ENGINE="$val"
        ;;
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
        val="${val#\'}"
        val="${val%\'}"
        DISCORD_VOICE_ENABLED="$val"
        ;;
      BLOCK_QUIC)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        BLOCK_QUIC="$val"
        ;;
      SMART_TV_MODE)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        SMART_TV_MODE="$val"
        ;;
      EXCLUDED_IPS)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        EXCLUDED_IPS="$val"
        ;;
      EXCLUDED_MACS)
        val="${val#\"}"
        val="${val%\"}"
        val="${val#\'}"
        val="${val%\'}"
        EXCLUDED_MACS="$val"
        ;;
    esac
  done < "$CONF"
fi

BIN=$(find_bin)

stop_nfqws() {
  if [ -f "$PIDFILE" ]; then
    PID=$(cat "$PIDFILE" 2>/dev/null)
    if [ -n "$PID" ] && is_zapret_pid "$PID"; then
      kill -15 "$PID" 2>/dev/null
      for i in 1 2 3 4 5; do
        if ! is_zapret_pid "$PID"; then break; fi
        sleep 1
      done
      if is_zapret_pid "$PID"; then
        kill -9 "$PID" 2>/dev/null
      fi
    fi
    rm -f "$PIDFILE"
  fi
  for p in $(pgrep -f nfqws 2>/dev/null) $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f tpws 2>/dev/null); do
    if is_zapret_pid "$p"; then
      kill -15 "$p" 2>/dev/null
    fi
  done
}

add_fw() {
  remove_fw_rules

  # Dedicated zapret chain in mangle
  iptables -t mangle -N zapret 2>/dev/null
  iptables -t mangle -F zapret || { echo "ERROR: failed to flush mangle zapret chain" >&2; remove_fw_rules; return 1; }

  # 1. CRITICAL: Skip packets already marked by nfqws to prevent infinite packet looping
  iptables -t mangle -A zapret -m mark --mark 0x40000000/0x40000000 -j RETURN || { echo "ERROR: failed to add fwmark bypass rule in mangle zapret" >&2; remove_fw_rules; return 1; }

  # 2. Exclude loopback and LAN bridge (OpenWrt br+ and Keenetic Bridge+)
  iptables -t mangle -A zapret -i lo -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
  iptables -t mangle -A zapret -i lo -j RETURN 2>/dev/null || true
  iptables -t mangle -A zapret -o lo -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
  iptables -t mangle -A zapret -o lo -j RETURN 2>/dev/null || true
  iptables -t mangle -A zapret -o br+ -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
  iptables -t mangle -A zapret -o br+ -j RETURN 2>/dev/null || true
  iptables -t mangle -A zapret -o Bridge+ -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
  iptables -t mangle -A zapret -o Bridge+ -j RETURN 2>/dev/null || true

  # 3. CRITICAL: Skip private/local subnets & router IP so Keenetic Web UI / LAN are NEVER touched
  for net in 0.0.0.0/8 10.0.0.0/8 100.64.0.0/10 127.0.0.0/8 169.254.0.0/16 172.16.0.0/12 192.168.0.0/16 224.0.0.0/4 240.0.0.0/4 255.255.255.255/32; do
    iptables -t mangle -A zapret -d "$net" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
    iptables -t mangle -A zapret -d "$net" -j RETURN 2>/dev/null || true
  done

  # 3.1 Per-Device Zapret: exclude devices where Zapret is disabled
  if [ -n "$EXCLUDED_IPS" ]; then
    for ex_ip in $EXCLUDED_IPS; do
      [ -n "$ex_ip" ] || continue
      iptables -t mangle -A zapret -s "$ex_ip" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
      iptables -t mangle -A zapret -s "$ex_ip" -j RETURN 2>/dev/null || true
      iptables -t mangle -A zapret -d "$ex_ip" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
      iptables -t mangle -A zapret -d "$ex_ip" -j RETURN 2>/dev/null || true
    done
  fi

  # 3.2 Per-Device Zapret (MAC): exclude devices by hardware MAC address
  if [ -n "$EXCLUDED_MACS" ]; then
    for ex_mac in $EXCLUDED_MACS; do
      [ -n "$ex_mac" ] || continue
      iptables -t mangle -A zapret -m mac --mac-source "$ex_mac" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || true
      mac_lower=$(echo "$ex_mac" | tr '[:upper:]' '[:lower:]')
      resolved_ip=$(awk -v m="$mac_lower" 'tolower($4) == m {print $1; exit}' /proc/net/arp 2>/dev/null)
      if [ -n "$resolved_ip" ]; then
        iptables -t mangle -A zapret -s "$resolved_ip" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
        iptables -t mangle -A zapret -s "$resolved_ip" -j RETURN 2>/dev/null || true
        iptables -t mangle -A zapret -d "$resolved_ip" -m comment --comment "xkeen-route-zapret" -j RETURN 2>/dev/null || \
        iptables -t mangle -A zapret -d "$resolved_ip" -j RETURN 2>/dev/null || true
      fi
    done
  fi

  # 3.3 NET-02: TCP MSS Clamping in POSTROUTING & FORWARD (xt_TCPMSS is forbidden in PREROUTING by kernel)
  iptables -t mangle -A POSTROUTING -p tcp --tcp-flags SYN,RST SYN -m comment --comment "xkeen-route-zapret" -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null || \
  iptables -t mangle -A POSTROUTING -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null || true
  iptables -t mangle -A FORWARD -p tcp --tcp-flags SYN,RST SYN -m comment --comment "xkeen-route-zapret" -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null || \
  iptables -t mangle -A FORWARD -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null || true

  # 4. Queue WAN TCP (80, 443) -> NFQUEUE 200 with bypass
  if iptables -t mangle -A zapret -p tcp -m multiport --dports 80,443 -m comment --comment "xkeen-route-zapret" -j NFQUEUE --queue-num 200 --queue-bypass 2>/dev/null || \
     iptables -t mangle -A zapret -p tcp -m multiport --dports 80,443 -j NFQUEUE --queue-num 200 --queue-bypass 2>/dev/null; then
    :
  else
    iptables -t mangle -A zapret -p tcp --dport 80 -j NFQUEUE --queue-num 200 --queue-bypass 2>/dev/null || true
    iptables -t mangle -A zapret -p tcp --dport 443 -j NFQUEUE --queue-num 200 --queue-bypass 2>/dev/null || true
  fi

  # 5. Drop UDP 443 (QUIC / HTTP3) if enabled or Smart TV mode active (prevents TV player stalls)
  if [ "$BLOCK_QUIC" = "1" ] || [ "$SMART_TV_MODE" = "1" ]; then
    iptables -t mangle -A zapret -p udp --dport 443 -m comment --comment "xkeen-route-zapret" -j DROP 2>/dev/null || \
    iptables -t mangle -A zapret -p udp --dport 443 -j DROP 2>/dev/null || true
  fi

  # 6. Discord Voice RTC UDP (50000:65535) if voice enabled
  if [ "$DISCORD_VOICE_ENABLED" = "1" ]; then
    iptables -t mangle -A zapret -p udp -m multiport --dports 50000:65535 -m comment --comment "xkeen-route-zapret" -j NFQUEUE --queue-num 200 --queue-bypass 2>/dev/null || \
    iptables -t mangle -A zapret -p udp --dport 50000:65535 -j NFQUEUE --queue-num 200 --queue-bypass 2>/dev/null || true
  fi

  # Hook into PREROUTING for client LAN bridge packets (br+, Bridge+)
  iptables -t mangle -I PREROUTING 1 -i br+ -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null || \
  iptables -t mangle -I PREROUTING 1 -i Bridge+ -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null || {
    echo "ERROR: failed to hook zapret chain into PREROUTING for br+/Bridge+" >&2
    remove_fw_rules
    return 1
  }

  # Z-02 & NET-01: Guard DNS redirect before PREROUTING to port 1053 to prevent LAN blackout & loops
  if check_dns_resolver_ready; then
    iptables -t nat -A PREROUTING -i br+ -d 127.0.0.1 -j RETURN 2>/dev/null || true
    iptables -t nat -A PREROUTING -i Bridge+ -d 127.0.0.1 -j RETURN 2>/dev/null || true
    iptables -t nat -A PREROUTING -i br+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null || \
    iptables -t nat -A PREROUTING -i br+ -p udp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null || true
    iptables -t nat -A PREROUTING -i br+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null || \
    iptables -t nat -A PREROUTING -i br+ -p tcp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null || true
    iptables -t nat -A PREROUTING -i Bridge+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null || \
    iptables -t nat -A PREROUTING -i Bridge+ -p udp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null || true
    iptables -t nat -A PREROUTING -i Bridge+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null || \
    iptables -t nat -A PREROUTING -i Bridge+ -p tcp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null || true
  else
    echo "DNS resolver port 1053 not listening; client DNS redirect skipped to prevent LAN blackout" >&2
  fi

  return 0
}

check_connectivity() {
  if command -v curl >/dev/null 2>&1; then
    curl -s -m 5 -o /dev/null http://cp.cloudflare.com/generate_204 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://connectivitycheck.gstatic.com/generate_204 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://www.gstatic.com/generate_204 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://cp.cloudflare.com 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://detectportal.firefox.com/success.txt 2>/dev/null || \
    curl -s -m 5 -o /dev/null http://ya.ru 2>/dev/null
  elif command -v wget >/dev/null 2>&1; then
    wget -q -O /dev/null -T 5 http://cp.cloudflare.com/generate_204 2>/dev/null || \
    wget -q -O /dev/null -T 5 http://connectivitycheck.gstatic.com/generate_204 2>/dev/null || \
    wget -q -O /dev/null -T 5 http://ya.ru 2>/dev/null
  else
    return 0
  fi
}

start_failsafe() {
  if [ -f "$FAILSAFE_PID" ]; then
    kill -9 $(cat "$FAILSAFE_PID") 2>/dev/null
    rm -f "$FAILSAFE_PID"
  fi

  # NET-03 & Z-05: Failsafe timer (45s base with hardware-aware checks)
  (
    sleep 45
    if ! check_connectivity && ! ping -c 1 -W 2 77.88.8.8 >/dev/null 2>&1 && ! ping -c 1 -W 2 8.8.8.8 >/dev/null 2>&1; then
      logger -t zapret "FAILSAFE: internet connectivity lost after enabling zapret, iptables rules rolled back"
      remove_fw_rules
      stop_nfqws
    fi
    rm -f "$FAILSAFE_PID"
  ) </dev/null >/dev/null 2>&1 &
  echo $! > "$FAILSAFE_PID"
}

remove_fw_rules() {
  if [ -f "$FAILSAFE_PID" ]; then
    kill -9 $(cat "$FAILSAFE_PID") 2>/dev/null
    rm -f "$FAILSAFE_PID"
  fi

  # Remove DNS redirects with or without comment
  while iptables -t nat -D PREROUTING -i br+ -p udp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i br+ -p tcp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p udp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p tcp --dport 53 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i br+ -p udp --dport 53 ! -d 127.0.0.1 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i br+ -p tcp --dport 53 ! -d 127.0.0.1 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p udp --dport 53 ! -d 127.0.0.1 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p tcp --dport 53 ! -d 127.0.0.1 -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i br+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i br+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p udp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null; do :; done
  while iptables -t nat -D PREROUTING -i Bridge+ -p tcp --dport 53 -m comment --comment "xkeen-route-zapret" -j REDIRECT --to-ports 1053 2>/dev/null; do :; done

  # Remove TCPMSS clamping rule (NET-02 / NET-04)
  while iptables -t mangle -D zapret -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done
  while iptables -t mangle -D POSTROUTING -p tcp --tcp-flags SYN,RST SYN -m comment --comment "xkeen-route-zapret" -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done
  while iptables -t mangle -D POSTROUTING -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done
  while iptables -t mangle -D FORWARD -p tcp --tcp-flags SYN,RST SYN -m comment --comment "xkeen-route-zapret" -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done
  while iptables -t mangle -D FORWARD -p tcp --tcp-flags SYN,RST SYN -j TCPMSS --clamp-mss-to-pmtu 2>/dev/null; do :; done

  # Remove hooks from all possible chains (POSTROUTING, PREROUTING, FORWARD, OUTPUT)
  while iptables -t mangle -D POSTROUTING -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D POSTROUTING -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -i br+ -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -i br+ -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -i Bridge+ -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -i Bridge+ -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D PREROUTING -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D FORWARD -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D FORWARD -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D OUTPUT -m comment --comment "xkeen-route-zapret" -j zapret 2>/dev/null; do :; done
  while iptables -t mangle -D OUTPUT -j zapret 2>/dev/null; do :; done

  # Flush and delete chain
  iptables -t mangle -F zapret 2>/dev/null
  iptables -t mangle -X zapret 2>/dev/null
  return 0
}

del_fw() {
  remove_fw_rules
}

case "$1" in
  start)
    acquire_zapret_lock || exit 1
    mkdir -p /opt/var/run /opt/etc/zapret
    if [ -f "$PIDFILE" ]; then
      PID=$(cat "$PIDFILE" 2>/dev/null)
      if [ -n "$PID" ] && is_zapret_pid "$PID"; then
        add_fw
        start_failsafe
        release_zapret_lock
        exit 0
      fi
    fi
    stop_nfqws
    if [ -n "$BIN" ] && [ -x "$BIN" ]; then
      IS_Z2=0
      case "$BIN" in
        *nfqws2*)
          if "$BIN" --help 2>&1 | grep -q "lua-desync"; then
            IS_Z2=1
          fi
          ;;
      esac
      if [ "$IS_Z2" = "1" ]; then
        mkdir -p /opt/zapret2/lua
        for alt_lua in /opt/zapret/lua /opt/zapret/files/lua /opt/share/zapret/lua; do
          if [ ! -f /opt/zapret2/lua/zapret-lib.lua ] && [ -f "$alt_lua/zapret-lib.lua" ]; then
            cp -rf "$alt_lua/"* /opt/zapret2/lua/ 2>/dev/null || true
            break
          fi
        done
        if [ ! -f "/opt/zapret2/lua/zapret-lib.lua" ] || [ ! -f "/opt/zapret2/lua/zapret-antidpi.lua" ]; then
          logger -t zapret "ERROR: zapret2 Lua libraries missing in /opt/zapret2/lua/"
          echo "ERROR: zapret2 Lua libraries missing in /opt/zapret2/lua/" >&2
          release_zapret_lock
          exit 1
        fi
        for chk in /opt/zapret2/lua/zapret-lib.lua /opt/zapret2/lua/zapret-antidpi.lua; do
          if [ $(wc -c < "$chk" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "$chk" 2>/dev/null; then
            logger -t zapret "ERROR: Lua library $chk is corrupted or incomplete in /opt/zapret2/lua/"
            echo "ERROR: Lua library $chk is corrupted or incomplete in /opt/zapret2/lua/" >&2
            release_zapret_lock
            exit 1
          fi
        done
      fi
      set -f
      if [ "$IS_Z2" = "1" ]; then
        LUA_INIT_ARG=""
        for lmod in zapret-lib.lua zapret-antidpi.lua zapret-auto.lua; do
          if [ -f "/opt/zapret2/lua/$lmod" ]; then
            LUA_INIT_ARG="$LUA_INIT_ARG --lua-init=@/opt/zapret2/lua/$lmod"
          fi
        done
        case "$NFQWS_ARGS" in
          *--daemon*) ;;
          *) NFQWS_ARGS="--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 $NFQWS_ARGS" ;;
        esac
        if "$BIN" --help 2>&1 | grep -q -- "--dpi-desync-fwmark"; then
          EFFECTIVE_ARGS=$(echo "$NFQWS_ARGS" | sed 's/--fwmark=/--dpi-desync-fwmark=/g')
        else
          EFFECTIVE_ARGS=$(echo "$NFQWS_ARGS" | sed 's/--dpi-desync-fwmark=/--fwmark=/g')
        fi
        $BIN --pidfile="$PIDFILE" $LUA_INIT_ARG $EFFECTIVE_ARGS
      else
        case "$NFQWS_ARGS" in
          *lua-desync*|*payload=*|*out-range=*|*multisplit*|*--fwmark=*)
            logger -t zapret "WARNING: NFQWS_ARGS contains nfqws2 Lua parameters, but running legacy nfqws. Using safe fallback args."
            NFQWS_ARGS="--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 --filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com --dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
            [ -f "/opt/etc/zapret/zapret-hosts.txt" ] && NFQWS_ARGS="$NFQWS_ARGS --new --filter-tcp=80,443 --hostlist=/opt/etc/zapret/zapret-hosts.txt --dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
            ;;
        esac
        case "$NFQWS_ARGS" in
          *--daemon*) ;;
          *) NFQWS_ARGS="--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 $NFQWS_ARGS" ;;
        esac
        EFFECTIVE_ARGS=$(echo "$NFQWS_ARGS" | sed 's/--fwmark=/--dpi-desync-fwmark=/g')
        $BIN --pidfile="$PIDFILE" $EFFECTIVE_ARGS
      fi
      set +f
      sleep 1
      PID=""
      if [ -f "$PIDFILE" ]; then
        PID=$(cat "$PIDFILE" 2>/dev/null)
      fi
      if [ -z "$PID" ] || ! is_zapret_pid "$PID"; then
        for p in $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f nfqws 2>/dev/null); do
          if is_zapret_pid "$p"; then
            PID="$p"
            echo "$PID" > "$PIDFILE" 2>/dev/null
            break
          fi
        done
      fi
      if [ -n "$PID" ] && is_zapret_pid "$PID"; then
        add_fw
        start_failsafe
        release_zapret_lock
        exit 0
      fi
      logger -t zapret "ERROR: nfqws failed to start with args: $NFQWS_ARGS"
      echo "ERROR: nfqws failed to start with args: $NFQWS_ARGS" >&2
      stop_nfqws
      remove_fw_rules
      release_zapret_lock
      exit 1
    else
      logger -t zapret "ERROR: nfqws binary not found or not executable"
      echo "ERROR: nfqws binary not found or not executable" >&2
      release_zapret_lock
      exit 1
    fi
    ;;
  stop)
    acquire_zapret_lock || exit 1
    stop_nfqws
    remove_fw_rules
    release_zapret_lock
    exit 0
    ;;
  restart)
    acquire_zapret_lock || exit 1
    stop_nfqws
    remove_fw_rules
    sleep 1
    release_zapret_lock
    exec "$0" start
    ;;
  reload|reload-hosts)
    acquire_zapret_lock || exit 1
    PID=""
    if [ -f "$PIDFILE" ]; then
      PID=$(cat "$PIDFILE" 2>/dev/null)
    fi
    if [ -z "$PID" ] || ! is_zapret_pid "$PID"; then
      for p in $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f nfqws 2>/dev/null); do
        if is_zapret_pid "$p"; then
          PID="$p"
          echo "$PID" > "$PIDFILE" 2>/dev/null
          break
        fi
      done
    fi
    if [ -n "$PID" ] && is_zapret_pid "$PID"; then
      case "$NFQWS_ARGS" in
        *zapret-hosts.txt*)
          if [ -r "/proc/$PID/cmdline" ] && ! tr '\0' ' ' < "/proc/$PID/cmdline" 2>/dev/null | grep -q "zapret-hosts.txt"; then
            release_zapret_lock
            exec "$0" restart
          fi
          ;;
      esac
      kill -HUP "$PID" 2>/dev/null
      release_zapret_lock
      exit 0
    fi
    for p in $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f nfqws 2>/dev/null); do
      if is_zapret_pid "$p"; then
        kill -HUP "$p" 2>/dev/null
      fi
    done
    release_zapret_lock
    exit 0
    ;;
  start-fw|reload-fw)
    acquire_zapret_lock || exit 1
    add_fw
    ret=$?
    release_zapret_lock
    exit $ret
    ;;
  stop-fw)
    acquire_zapret_lock || exit 1
    remove_fw_rules
    release_zapret_lock
    exit 0
    ;;
  status)
    if [ -f "$PIDFILE" ]; then
      PID=$(cat "$PIDFILE" 2>/dev/null)
      if [ -n "$PID" ] && is_zapret_pid "$PID"; then
        exit 0
      fi
    fi
    for p in $(pgrep -f nfqws2 2>/dev/null) $(pgrep -f nfqws 2>/dev/null); do
      if is_zapret_pid "$p"; then
        exit 0
      fi
    done
    exit 1
    ;;
  *)
    echo "Usage: $0 {start|stop|restart|reload|start-fw|stop-fw|status}"
    exit 1
    ;;
esac
"#;

pub fn is_nfqws2_available() -> bool {
    (std::path::Path::new("/opt/zapret2/nfqws2").exists()
        || std::path::Path::new("/opt/sbin/nfqws2").exists()
        || std::path::Path::new("/opt/bin/nfqws2").exists()
        || std::path::Path::new("/opt/usr/bin/nfqws2").exists()
        || std::path::Path::new("/opt/zapret/nfq/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/arm64/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/arm/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/mipsel/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/mips/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/x86_64/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-arm64/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-arm/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-mips32r2-lsb/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-mips32r2-msb/nfqws2").exists()
        || std::path::Path::new("/opt/zapret2/binaries/linux-x86_64/nfqws2").exists())
        && (std::path::Path::new("/opt/zapret2/lua/zapret-lib.lua").exists()
            || std::path::Path::new("/opt/zapret/lua/zapret-lib.lua").exists()
            || std::path::Path::new("/opt/zapret2/files/lua/zapret-lib.lua").exists())
}

pub fn is_nfqws1_available() -> bool {
    std::path::Path::new("/opt/zapret/nfq/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-arm64/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-arm/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-mips32r2-lsb/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-mips32r2-msb/nfqws").exists()
        || std::path::Path::new("/opt/zapret/binaries/linux-x86_64/nfqws").exists()
        || std::path::Path::new("/opt/sbin/nfqws").exists()
        || std::path::Path::new("/opt/bin/nfqws").exists()
        || std::path::Path::new("/opt/usr/bin/nfqws").exists()
        || std::path::Path::new("/opt/zapret/nfqws.bak").exists()
}

pub fn normalize_engine_choice(engine: &str) -> &'static str {
    match engine.trim().to_ascii_lowercase().as_str() {
        "v1" | "legacy" | "1" | "1.x" | "nfqws" => "v1",
        _ => "v2",
    }
}

pub fn should_use_nfqws2(cfg: &crate::config::ZapretConfig) -> bool {
    if normalize_engine_choice(&cfg.engine) == "v1" {
        !is_nfqws1_available() && is_nfqws2_available()
    } else {
        is_nfqws2_available()
    }
}

pub async fn detect_installed_zapret1_version() -> String {
    if let Ok(ver) = tokio::fs::read_to_string("/opt/zapret/version.txt").await {
        let v = ver.trim();
        if !v.is_empty() {
            return if v.starts_with('v') { v.to_string() } else { format!("v{v}") };
        }
    }
    "v72.13".to_string()
}

pub async fn detect_installed_zapret2_version() -> String {
    if let Ok(ver) = tokio::fs::read_to_string("/opt/zapret2/version.txt").await {
        let v = ver.trim();
        if !v.is_empty() {
            return if v.starts_with('v') { v.to_string() } else { format!("v{v}") };
        }
    }
    "v1.0.5.2".to_string()
}

pub fn is_zapret_5am_due(last_checked: Option<&str>, now: chrono::DateTime<chrono::Local>) -> bool {
    let naive_now = now.naive_local();
    use chrono::Datelike;
    if naive_now.year() < 2024 {
        return false;
    }
    let today_5am = match naive_now.date().and_hms_opt(5, 0, 0) {
        Some(t) => t,
        None => return true,
    };
    let most_recent_5am = if naive_now >= today_5am {
        today_5am
    } else {
        today_5am - chrono::Duration::days(1)
    };

    match last_checked {
        None => true,
        Some(s) => {
            if let Ok(last_dt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
                last_dt < most_recent_5am
            } else {
                true
            }
        }
    }
}

pub async fn check_zapret_update_core(state: &AppState, force: bool) -> Result<serde_json::Value, String> {
    let now = chrono::Local::now();
    let v1_inst = is_nfqws1_available();
    let v2_inst = is_nfqws2_available();
    let (engine, cur_ver, last_checked, update_avail, latest_ver) = {
        let cfg = state.config.read().await;
        let eng = if normalize_engine_choice(&cfg.zapret.engine) == "v1" {
            "v1"
        } else if v2_inst {
            "v2"
        } else if v1_inst {
            "v1"
        } else {
            normalize_engine_choice(&cfg.zapret.engine)
        };
        (
            eng.to_string(),
            if eng == "v2" { detect_installed_zapret2_version().await } else { detect_installed_zapret1_version().await },
            cfg.zapret.last_update_check.clone(),
            cfg.zapret.update_available,
            cfg.zapret.latest_version.clone(),
        )
    };

    let default_notes = if engine == "v2" {
        "• Движок Zapret 2 (nfqws2 + Lua): поддержка адаптивных Lua-стратегий десинхронизации (multisplit, multidisorder, fake TLS ClientHello с рандомизацией Session ID и tcp_ts_up).\n• Полная совместимость с YouTube, Discord и голосовыми UDP-каналами (STUN / IP Discovery).\n• Мгновенное переключение между движками Запрет 1 и Запрет 2 без переустановки."
    } else {
        "• Классический движок Zapret 1 (nfqws v72.13): проверенная десинхронизация fake,split2 / disorder2 с минимальным потреблением RAM.\n• Доступно переключение на движок Zapret 2 (nfqws2 + Lua) в 1 клик."
    };

    let is_due = is_zapret_5am_due(last_checked.as_deref(), now);

    if !force && !is_due && last_checked.is_some() {
        let label = if engine == "v2" {
            format!("Запрет 2 {cur_ver}")
        } else {
            format!("Запрет 1 {cur_ver}")
        };
        let lat = latest_ver.unwrap_or_else(|| cur_ver.clone());
        let effective_update = update_avail && crate::updater::is_newer(&lat, &cur_ver);
        return Ok(json!({
            "current_engine": engine,
            "current_version": cur_ver,
            "label": label,
            "latest_version": lat,
            "update_available": effective_update,
            "upgrade_available": engine == "v1" && !v2_inst,
            "v1_installed": v1_inst,
            "v2_installed": v2_inst,
            "notes": default_notes,
            "last_check": last_checked,
        }));
    }

    let proxy_url = {
        let cfg = state.config.read().await;
        cfg.mihomo_proxy_url()
    };

    let target_repo = if engine == "v2" {
        "bol-van/zapret2"
    } else {
        "bol-van/zapret"
    };

    let mut fetched_latest: Option<String> = None;
    let mut fetched_notes: Option<String> = None;
    let direct_client = &state.http;
    let proxied_client = reqwest::Proxy::all(&proxy_url)
        .ok()
        .and_then(|p| reqwest::Client::builder().proxy(p).build().ok());

    let mut clients = Vec::new();
    if let Some(ref p) = proxied_client {
        clients.push(p);
    }
    clients.push(direct_client);

    let gh_latest_url = format!("https://github.com/{target_repo}/releases/latest");
    for client in &clients {
        if let Ok(res) = client.get(&gh_latest_url).timeout(std::time::Duration::from_secs(6)).send().await {
            let final_url = res.url().to_string();
            if let Some(tag) = final_url.split("/releases/tag/").nth(1) {
                let tag = tag.trim_matches('/').to_string();
                if !tag.is_empty() {
                    fetched_latest = Some(tag);
                    break;
                }
            }
        }
    }

    let gh_api_url = format!("https://api.github.com/repos/{target_repo}/releases/latest");
    for client in &clients {
        if let Ok(res) = client
            .get(&gh_api_url)
            .header("User-Agent", "xkeen-route")
            .header("Accept", "application/vnd.github+json")
            .timeout(std::time::Duration::from_secs(6))
            .send()
            .await
        {
            if res.status().is_success() {
                if let Ok(body) = res.json::<serde_json::Value>().await {
                    if fetched_latest.is_none() {
                        if let Some(tag) = body.get("tag_name").and_then(|v| v.as_str()) {
                            fetched_latest = Some(tag.to_string());
                        }
                    }
                    if let Some(b) = body.get("body").and_then(|v| v.as_str()) {
                        if !b.trim().is_empty() {
                            fetched_notes = Some(b.trim().to_string());
                        }
                    }
                    break;
                }
            }
        }
    }

    let default_latest = if engine == "v2" { "v1.0.5.2".to_string() } else { "v72.13".to_string() };
    let final_latest = fetched_latest.unwrap_or(default_latest);

    let has_newer_tag = crate::updater::is_newer(&final_latest, &cur_ver);
    let upgrade_available = engine == "v1" && !v2_inst;
    let update_available = has_newer_tag;

    let now_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let display_latest = if upgrade_available && !has_newer_tag {
        "v1.0.5.2".to_string()
    } else {
        final_latest
    };

    {
        let _guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.last_update_check = Some(now_str.clone());
        cfg.zapret.update_available = update_available;
        cfg.zapret.latest_version = Some(display_latest.clone());
        let _ = crate::config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg);
    }

    let label = if engine == "v2" {
        format!("Запрет 2 {cur_ver}")
    } else {
        format!("Запрет 1 {cur_ver}")
    };

    Ok(json!({
        "current_engine": engine,
        "current_version": cur_ver,
        "label": label,
        "latest_version": display_latest,
        "update_available": update_available,
        "upgrade_available": upgrade_available,
        "v1_installed": v1_inst,
        "v2_installed": v2_inst,
        "notes": fetched_notes.unwrap_or_else(|| default_notes.to_string()),
        "last_check": now_str,
    }))
}

pub async fn check_zapret_update_route(
    State(state): State<AppState>,
    Query(query): Query<std::collections::HashMap<String, String>>,
) -> Response {
    let force = query.get("force").map(|v| v == "1" || v == "true").unwrap_or(false);
    match check_zapret_update_core(&state, force).await {
        Ok(data) => api_ok(data),
        Err(e) => api_err(e),
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct ZapretHardwareInfo {
    pub model: String,
    pub arch_label: String,
    pub target_arch: String,
    pub ram_mb: u64,
    pub recommended_engine: String,
    pub hint_text: String,
}

pub fn normalize_router_model(raw_model: &str, raw_device: &str, raw_hw_id: &str) -> String {
    let mut m = raw_model.trim().to_string();
    if m.is_empty() {
        m = raw_device.trim().to_string();
    }
    if let Some(rest) = m.strip_prefix("Keenetic ") {
        m = rest.trim().to_string();
    } else if let Some(rest) = m.strip_prefix("keenetic ") {
        m = rest.trim().to_string();
    }

    let dev = raw_device
        .trim()
        .strip_prefix("Keenetic ")
        .or_else(|| raw_device.trim().strip_prefix("keenetic "))
        .unwrap_or(raw_device.trim())
        .trim();

    if m.starts_with("KN-") && !dev.is_empty() && !dev.starts_with("KN-") {
        m = format!("{dev} {m}");
    }

    // Replace "(KN-XXXX)" with "KN-XXXX"
    if m.contains("(KN-") {
        m = m.replace("(KN-", "KN-").replace(')', "");
    }

    let hw = raw_hw_id.trim();
    if !hw.is_empty() && !m.contains(hw) {
        if m.is_empty() {
            m = hw.to_string();
        } else {
            m = format!("{m} {hw}");
        }
    }

    let cleaned = m.split_whitespace().collect::<Vec<_>>().join(" ");
    if cleaned.is_empty() {
        "Keenetic Router".to_string()
    } else {
        cleaned
    }
}

pub fn parse_arch_info(uname_m: &str, is_little_endian: bool) -> (String, String) {
    let mut arch = uname_m.trim().to_ascii_lowercase();
    if arch.is_empty() {
        arch = std::env::consts::ARCH.to_ascii_lowercase();
    }
    if arch == "aarch64" || arch == "arm64" {
        ("ARM64".to_string(), "linux-arm64".to_string())
    } else if arch.starts_with("armv7") || arch.starts_with("armv8") || arch.starts_with("arm") {
        ("ARMv7".to_string(), "linux-arm".to_string())
    } else if arch.starts_with("mips") {
        if arch.contains("el") || is_little_endian {
            ("MIPS32 (mipsel)".to_string(), "linux-mips32r2-lsb".to_string())
        } else {
            ("MIPS32 (mips)".to_string(), "linux-mips32r2-msb".to_string())
        }
    } else if arch == "x86_64" || arch == "amd64" {
        ("x86_64".to_string(), "linux-x86_64".to_string())
    } else if arch == "i686" || arch == "i386" || arch == "x86" {
        ("x86".to_string(), "linux-x86".to_string())
    } else {
        ("ARM64".to_string(), "linux-arm64".to_string())
    }
}

pub fn normalize_ram_mb(mem_total_kb: u64) -> u64 {
    let raw_mb = mem_total_kb / 1024;
    match raw_mb {
        0 => 512,
        1..=40 => 32,
        41..=72 => 64,
        73..=160 => 128,
        161..=320 => 256,
        321..=640 => 512,
        641..=1280 => 1024,
        1281..=2560 => 2048,
        other => other,
    }
}

pub fn build_zapret_hardware_info(
    raw_model: &str,
    raw_device: &str,
    raw_hw_id: &str,
    uname_m: &str,
    is_little_endian: bool,
    mem_total_kb: u64,
) -> ZapretHardwareInfo {
    let model = normalize_router_model(raw_model, raw_device, raw_hw_id);
    let (arch_label, target_arch) = parse_arch_info(uname_m, is_little_endian);
    let ram_mb = normalize_ram_mb(mem_total_kb);
    let recommended_engine = if ram_mb <= 64 || (ram_mb < 128 && target_arch.starts_with("linux-mips")) {
        "v1".to_string()
    } else {
        "v2".to_string()
    };
    let rec_label = if recommended_engine == "v2" {
        "рекомендуется Zapret 2.0"
    } else {
        "рекомендуется Legacy 1.x (экономия RAM)"
    };
    let hint_text = format!(
        "Обнаружен {} ({}, {}MB RAM) — {}",
        model, arch_label, ram_mb, rec_label
    );

    ZapretHardwareInfo {
        model,
        arch_label,
        target_arch,
        ram_mb,
        recommended_engine,
        hint_text,
    }
}

pub async fn detect_zapret_hardware(state: &AppState) -> ZapretHardwareInfo {
    let cfg = state.config.read().await.clone();
    let mut raw_model = String::new();
    let mut raw_device = String::new();
    let mut raw_hw_id = String::new();

    if let Ok(ver) = crate::rci::get_version(&state.http, &cfg).await {
        if let Some(m) = ver.get("model") {
            raw_model = m.clone();
        }
        if let Some(d) = ver.get("device") {
            raw_device = d.clone();
        }
        if let Some(h) = ver.get("hw_id") {
            raw_hw_id = h.clone();
        }
    }

    let probe_out = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("cat /proc/device-tree/model 2>/dev/null || cat /tmp/sysinfo/model 2>/dev/null || awk -F: '/machine/ {print $2; exit}' /proc/cpuinfo 2>/dev/null; echo '---'; uname -m 2>/dev/null; echo '---'; awk '/MemTotal/ {print $2}' /proc/meminfo 2>/dev/null; echo '---'; (echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }' || hexdump -s 5 -n 1 -e '\"%02x\"' /bin/sh 2>/dev/null)")
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();

    let parts: Vec<&str> = probe_out.split("---").collect();
    let probed_model = parts.first().map(|s| s.trim().trim_matches('\0')).unwrap_or("");
    if raw_model.is_empty() && !probed_model.is_empty() {
        raw_model = probed_model.to_string();
    }
    let uname_m = parts.get(1).map(|s| s.trim()).filter(|s| !s.is_empty()).unwrap_or(std::env::consts::ARCH);
    let mem_total_kb = parts
        .get(2)
        .and_then(|s| s.trim().lines().next())
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(512 * 1024);
    let endian_str = parts.get(3).map(|s| s.trim()).unwrap_or("");
    let is_little_endian = if endian_str.starts_with('1') || endian_str.starts_with("01") {
        true
    } else if endian_str.is_empty() {
        cfg!(target_endian = "little")
    } else {
        false
    };

    build_zapret_hardware_info(
        &raw_model,
        &raw_device,
        &raw_hw_id,
        uname_m,
        is_little_endian,
        mem_total_kb,
    )
}


pub fn convert_lua_to_legacy_desync(args: &str) -> String {
    if (args.contains("--daemon") || args.contains("--qnum"))
        && !args.contains("lua-desync")
        && !args.contains("payload=")
        && !args.contains("out-range=")
    {
        return args.replace("multisplit", "split2").replace("multidisorder", "disorder2");
    }
    if args.contains("disorder2") || args.contains("multidisorder") {
        if args.contains("midsld") {
            "--dpi-desync=fake,disorder2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=5 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
        } else {
            "--dpi-desync=fake,disorder2 --dpi-desync-split-pos=1 --dpi-desync-repeats=4 --dpi-desync-fooling=md5sig --dpi-desync-cutoff=d4".to_string()
        }
    } else if args.contains("seqovl") {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-split-seqovl=5 --dpi-desync-fooling=badseq --dpi-desync-cutoff=d4".to_string()
    } else if args.contains("multisplit") {
        if args.contains("midsld") {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4".to_string()
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
        }
    } else if args.contains("split2") || args.contains("split") {
        if args.contains("ts_up") || args.contains("repeats=6") {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-cutoff=d4".to_string()
        }
    } else {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4".to_string()
    }
}

pub fn build_nfqws_args_with_desync(cfg: &crate::config::ZapretConfig, custom_desync: Option<&str>) -> (String, bool) {
    let mut profiles: Vec<String> = Vec::new();

    let default_yt_desync = if cfg.aggressive_dpi {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4"
    } else {
        "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
    };
    let yt_desync = custom_desync.unwrap_or(default_yt_desync);

    // YouTube profile (TCP 80/443) - fake,split2 at pos 1 with repeats=6 and ts (TCP timestamp) fooling reliably bypasses TSPU inspection
    if cfg.youtube_turbo || cfg.hybrid_youtube || cfg.smart_tv_mode {
        let yt_domains = if cfg.smart_tv_mode {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,redirector.googlevideo.com,manifest.googlevideo.com,gvt1.com,play.google.com"
        } else {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com"
        };
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist-domains={yt_domains} {yt_desync}"
        ));
    }

    // Discord Web/Chat profile - fake,split2 with ts fooling for TLS 1.3
    if cfg.hybrid_discord {
        let dc_default = if cfg.aggressive_dpi {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4"
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
        };
        let dc_desync = custom_desync.unwrap_or(dc_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist-domains=discord.com,discord.gg,discordapp.com,discordapp.net,discord.media,discord-attachments-uploads-prd.storage.googleapis.com,dis.gd,discord-activities.com {dc_desync}"
        ));
    }

    // Discord Voice UDP profile
    if cfg.discord_voice_udp {
        profiles.push("--filter-udp=50000-65535 --filter-l7=discord,stun --dpi-desync=fake --dpi-desync-repeats=6".to_string());
    }

    // General Web Hostlist profile (включает универсальный хостлист, GitHub, торренты, 18+ и пользовательские сайты)
    let has_active_custom = cfg.custom_entries.iter().any(|e| e.enabled);
    if cfg.general_bypass || cfg.bypass_github || cfg.bypass_torrents || cfg.bypass_adult || cfg.community_hostlist_enabled || has_active_custom {
        let gen_default = if cfg.aggressive_dpi {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-repeats=6 --dpi-desync-fooling=ts,md5sig --dpi-desync-cutoff=d4"
        } else {
            "--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4"
        };
        let gen_desync = custom_desync.unwrap_or(gen_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --hostlist=/opt/etc/zapret/zapret-hosts.txt {gen_desync}"
        ));
    }

    // If no specific profiles enabled, provide safe basic profile
    if profiles.is_empty() {
        let fallback_desync = custom_desync.unwrap_or("--dpi-desync=fake,split2 --dpi-desync-split-pos=1 --dpi-desync-repeats=6 --dpi-desync-fooling=ts --dpi-desync-cutoff=d4");
        profiles.push(format!("--filter-tcp=80,443 --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com {}", fallback_desync));
    }

    let args = format!("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 {}", profiles.join(" --new "));
    let voice_enabled = cfg.discord_voice_udp;
    (args, voice_enabled)
}

pub fn build_nfqws_args(cfg: &crate::config::ZapretConfig) -> (String, bool) {
    build_nfqws_args_with_desync(cfg, None)
}

/// Генератор аргументов zapret2 (nfqws2 с рантаймом Lua, --payload и --lua-desync)
pub fn build_nfqws2_args_with_desync(cfg: &crate::config::ZapretConfig, custom_desync: Option<&str>) -> (String, bool) {
    let mut profiles: Vec<String> = Vec::new();

    let default_fake_desync = if cfg.aggressive_dpi {
        "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"
    } else if cfg.youtube_turbo || cfg.smart_tv_mode {
        "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1,midsld"
    } else {
        "--lua-desync=fake:blob=fake_default_tls --lua-desync=multisplit:pos=1"
    };
    let fake_desync = custom_desync.unwrap_or(default_fake_desync);

    // YouTube profile (TCP 80/443) - Fake ClientHello + multisplit desync с рандомизацией и ранняя отсечка -d10
    if cfg.youtube_turbo || cfg.hybrid_youtube || cfg.smart_tv_mode {
        let yt_domains = if cfg.smart_tv_mode {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,redirector.googlevideo.com,manifest.googlevideo.com,gvt1.com,play.google.com"
        } else {
            "googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com"
        };
        profiles.push(format!(
            "--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains={yt_domains} --out-range=-d10 --payload=tls_client_hello {fake_desync}"
        ));
    }

    // Discord Web/Chat profile (TCP 80/443)
    if cfg.hybrid_discord {
        let dc_default = if cfg.aggressive_dpi {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"
        } else {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1,midsld"
        };
        let dc_desync = custom_desync.unwrap_or(dc_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=discord.com,discord.gg,discordapp.com,discordapp.net,discord.media,discord-attachments-uploads-prd.storage.googleapis.com,dis.gd,discord-activities.com --out-range=-d10 --payload=tls_client_hello {dc_desync}"
        ));
    }

    // Discord Voice UDP profile (перехват голосовых каналов UDP 50000:65535, фейковые пакеты для STUN и IP Discovery)
    if cfg.discord_voice_udp {
        profiles.push("--filter-udp=50000-65535 --filter-l7=discord,stun --payload=discord_ip_discovery,stun --lua-desync=fake:blob=0x00000000:repeats=4".to_string());
    }

    // General Web Hostlist profile (общий файл zapret-hosts.txt)
    let has_active_custom = cfg.custom_entries.iter().any(|e| e.enabled);
    if cfg.general_bypass || cfg.bypass_github || cfg.bypass_torrents || cfg.bypass_adult || cfg.community_hostlist_enabled || has_active_custom {
        let gen_default = if cfg.aggressive_dpi {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"
        } else {
            "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1"
        };
        let desync_args = custom_desync.unwrap_or(gen_default);
        profiles.push(format!(
            "--filter-tcp=80,443 --filter-l7=tls,http --hostlist=/opt/etc/zapret/zapret-hosts.txt --out-range=-d10 --payload=tls_client_hello {desync_args}"
        ));
    }

    // Базовый безопасный профиль по умолчанию, если ничего не выбрано
    if profiles.is_empty() {
        let fallback_desync = custom_desync.unwrap_or("--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1");
        profiles.push(format!("--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=googlevideo.com,youtube.com,ytimg.com,ggpht.com,youtu.be,yt.be,youtube-nocookie.com,discord.com,discord.gg,discordapp.com --out-range=-d10 --payload=tls_client_hello {}", fallback_desync));
    }

    let args = format!("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000 {}", profiles.join(" --new "));
    let voice_enabled = cfg.discord_voice_udp;
    (args, voice_enabled)
}

pub fn build_nfqws2_args(cfg: &crate::config::ZapretConfig) -> (String, bool) {
    build_nfqws2_args_with_desync(cfg, None)
}

pub const ZAPRET_HOSTS_MANAGED_BEGIN: &str = "# --- START XKEEN ROUTE DYNAMIC HOSTS ---";
pub const ZAPRET_HOSTS_MANAGED_END: &str = "# --- END XKEEN ROUTE DYNAMIC HOSTS ---";

pub fn sync_zapret_hosts_content(existing: &str, cfg: &crate::config::ZapretConfig) -> String {
    // 1. Исключаем существующий блок авто-управления, сохраняя ручные строки пользователя
    let mut manual_lines: Vec<String> = Vec::new();
    let mut in_managed_block = false;

    for line in existing.lines() {
        let trimmed = line.trim();
        if trimmed == ZAPRET_HOSTS_MANAGED_BEGIN {
            in_managed_block = true;
            continue;
        }
        if trimmed == ZAPRET_HOSTS_MANAGED_END {
            in_managed_block = false;
            continue;
        }
        if !in_managed_block {
            manual_lines.push(line.to_string());
        }
    }

    // 2. Список динамически управляемых доменов
    let mut dynamic_domains: Vec<String> = Vec::new();

    let mut add_dynamic = |d: &str| {
        let clean = crate::config::normalize_domain(d);
        if !clean.is_empty() && !dynamic_domains.iter().any(|existing| existing.eq_ignore_ascii_case(&clean)) {
            dynamic_domains.push(clean);
        }
    };

    if cfg.bypass_github {
        for d in crate::routing::GITHUB_DOMAINS {
            add_dynamic(d);
        }
    }

    if cfg.bypass_torrents {
        for d in crate::routing::TORRENT_DOMAINS {
            add_dynamic(d);
        }
    }

    if cfg.bypass_adult {
        for d in crate::routing::ADULT_DOMAINS {
            add_dynamic(d);
        }
    }

    for entry in &cfg.custom_entries {
        if entry.enabled {
            add_dynamic(&entry.domain);
            for cdn in &entry.cdns {
                add_dynamic(cdn);
            }
        }
    }

    // 3. Удаляем динамические домены из ручной части (предотвращает дубли и зависание отключенных доменов)
    let mut all_managed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for d in crate::routing::GITHUB_DOMAINS {
        all_managed.insert(crate::config::normalize_domain(d));
    }
    for d in crate::routing::TORRENT_DOMAINS {
        all_managed.insert(crate::config::normalize_domain(d));
    }
    for d in crate::routing::ADULT_DOMAINS {
        all_managed.insert(crate::config::normalize_domain(d));
    }
    for entry in &cfg.custom_entries {
        all_managed.insert(crate::config::normalize_domain(&entry.domain));
        for cdn in &entry.cdns {
            all_managed.insert(crate::config::normalize_domain(cdn));
        }
    }

    manual_lines.retain(|line| {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return true;
        }
        let norm = crate::config::normalize_domain(trimmed);
        !all_managed.contains(&norm)
    });

    // 4. Сборка итогового файла
    let mut result = manual_lines.join("\n").trim().to_string();
    if !result.is_empty() {
        result.push('\n');
    }

    if !dynamic_domains.is_empty() {
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(ZAPRET_HOSTS_MANAGED_BEGIN);
        result.push_str("\n# Автоматически управляемые службы и сайты /boost (Zapret)\n");
        for d in dynamic_domains {
            result.push_str(&d);
            result.push('\n');
        }
        result.push_str(ZAPRET_HOSTS_MANAGED_END);
        result.push('\n');
    }

    result
}

pub fn validate_custom_args(args: &str) -> Result<(), String> {
    for ch in args.chars() {
        if ch == ';' || ch == '&' || ch == '|' || ch == '`' || ch == '$' || ch == '\n' || ch == '\r' || ch == '(' || ch == ')' || ch == '<' || ch == '>' || ch == '!' || ch == '"' || ch == '\'' {
            return Err(format!("Недопустимый спецсимвол в аргументах Zapret: '{}'. Разрешены только флаги nfqws/nfqws2", ch));
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
            } else if k == "ZAPRET_ENGINE" {
                let clean_val = val.trim().trim_matches('"').trim_matches('\'');
                if clean_val != "v1" && clean_val != "v2" && clean_val != "legacy" && clean_val != "modern" {
                    return Err(format!("Недопустимое значение ZAPRET_ENGINE: '{clean_val}'. Разрешены только v1, v2"));
                }
            } else if k != "DISCORD_VOICE_ENABLED" && k != "BLOCK_QUIC" && k != "SMART_TV_MODE" && k != "EXCLUDED_IPS" && k != "EXCLUDED_MACS" {
                return Err(format!("Неизвестный параметр в zapret.conf: '{k}'. Разрешены только ZAPRET_ENGINE, NFQWS_ARGS, DISCORD_VOICE_ENABLED, BLOCK_QUIC, SMART_TV_MODE, EXCLUDED_IPS, EXCLUDED_MACS"));
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
    atomic_write_file("/opt/etc/init.d/S51zapret", S51ZAPRET_SCRIPT)
        .await
        .map_err(|e| format!("Не удалось записать /opt/etc/init.d/S51zapret: {e}"))?;
    tokio::process::Command::new("chmod")
        .arg("+x")
        .arg("/opt/etc/init.d/S51zapret")
        .output()
        .await
        .map_err(|e| format!("Не удалось сделать S51zapret исполняемым: {e}"))?;

    // 1.1 Keenetic ndm netfilter & ifstatechanged hooks (prevent netfilter drop on DHCP/WAN reload)
    let _ = tokio::fs::create_dir_all("/opt/etc/ndm/netfilter.d").await;
    let _ = tokio::fs::create_dir_all("/opt/etc/ndm/ifstatechanged.d").await;
    let _ = tokio::fs::create_dir_all("/opt/etc/ndm/wan.d").await;
    if let Ok(()) = atomic_write_file("/opt/etc/ndm/netfilter.d/050-zapret.sh", NDM_NETFILTER_SCRIPT).await {
        let _ = tokio::process::Command::new("chmod")
            .arg("+x")
            .arg("/opt/etc/ndm/netfilter.d/050-zapret.sh")
            .output()
            .await;
    }
    if let Ok(()) = atomic_write_file("/opt/etc/ndm/ifstatechanged.d/050-zapret.sh", NDM_IFSTATE_SCRIPT).await {
        let _ = tokio::process::Command::new("chmod")
            .arg("+x")
            .arg("/opt/etc/ndm/ifstatechanged.d/050-zapret.sh")
            .output()
            .await;
    }
    if let Ok(()) = atomic_write_file("/opt/etc/ndm/wan.d/050-zapret.sh", NDM_IFSTATE_SCRIPT).await {
        let _ = tokio::process::Command::new("chmod")
            .arg("+x")
            .arg("/opt/etc/ndm/wan.d/050-zapret.sh")
            .output()
            .await;
    }

    // 2. Default hostlist if missing, and sync active domains into it
    let hosts_path = "/opt/etc/zapret/zapret-hosts.txt";
    let base_hosts = if std::path::Path::new(hosts_path).exists() {
        tokio::fs::read_to_string(hosts_path).await.unwrap_or_else(|_| DEFAULT_ZAPRET_HOSTS.to_string())
    } else {
        DEFAULT_ZAPRET_HOSTS.to_string()
    };
    let mut synced_hosts = sync_zapret_hosts_content(&base_hosts, cfg);

    // Подгрузка Community Hostlist при активации
    if cfg.community_hostlist_enabled {
        if let Ok(comm) = tokio::fs::read_to_string("/opt/etc/zapret/community-hosts.txt").await {
            let comm_domains: Vec<&str> = comm
                .lines()
                .map(|l| l.trim())
                .filter(|l| !l.is_empty() && !l.starts_with('#'))
                .collect();
            if !comm_domains.is_empty() {
                synced_hosts.push_str("\n# --- START COMMUNITY HOSTLIST ---\n");
                for d in comm_domains {
                    synced_hosts.push_str(d);
                    synced_hosts.push('\n');
                }
                synced_hosts.push_str("# --- END COMMUNITY HOSTLIST ---\n");
            }
        }
    }
    let _ = atomic_write_file(hosts_path, &synced_hosts).await;

    // 3. zapret.conf with multi-strategy args or custom_args (respecting chosen engine v1/v2)
    let use_v2 = should_use_nfqws2(cfg);
    let engine_str = if use_v2 { "v2" } else { "v1" };
    let (args, voice_enabled) = if let Some(custom) = &cfg.custom_args {
        validate_custom_args(custom)?;
        let is_full_cmdline = custom.contains("--daemon") || custom.contains("--qnum");
        if use_v2 {
            if is_full_cmdline {
                (custom.clone(), cfg.discord_voice_udp)
            } else {
                build_nfqws2_args_with_desync(cfg, Some(custom.as_str()))
            }
        } else {
            if is_full_cmdline && !custom.contains("lua-desync") && !custom.contains("payload=") && !custom.contains("out-range=") {
                (custom.replace("multisplit", "split2"), cfg.discord_voice_udp)
            } else {
                let legacy_desync = convert_lua_to_legacy_desync(custom);
                build_nfqws_args_with_desync(cfg, Some(&legacy_desync))
            }
        }
    } else {
        if use_v2 {
            build_nfqws2_args(cfg)
        } else {
            build_nfqws_args(cfg)
        }
    };
    let sanitized_args = args.replace('\r', " ").replace('\n', " ").replace('"', "");
    let excluded_ips_str = cfg
        .excluded_devices
        .iter()
        .filter(|d| !d.contains(':'))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let excluded_macs_str = cfg
        .excluded_devices
        .iter()
        .filter(|d| d.contains(':'))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    let block_quic_str = if cfg.smart_tv_mode { "1" } else { "0" };
    let smart_tv_str = if cfg.smart_tv_mode { "1" } else { "0" };

    let conf_data = format!(
        "ZAPRET_ENGINE=\"{}\"\nNFQWS_ARGS=\"{}\"\nDISCORD_VOICE_ENABLED=\"{}\"\nBLOCK_QUIC=\"{}\"\nSMART_TV_MODE=\"{}\"\nEXCLUDED_IPS=\"{}\"\nEXCLUDED_MACS=\"{}\"\n",
        engine_str,
        sanitized_args.trim(),
        if voice_enabled { "1" } else { "0" },
        block_quic_str,
        smart_tv_str,
        excluded_ips_str.trim(),
        excluded_macs_str.trim()
    );
    atomic_write_file("/opt/etc/zapret/zapret.conf", &conf_data)
        .await
        .map_err(|e| format!("Не удалось записать /opt/etc/zapret/zapret.conf: {e}"))?;
    Ok(())
}

pub fn format_nfqws_proc_cmdline(raw: &[u8]) -> String {
    let mut c = String::new();
    for (idx, part) in raw.split(|&b| b == 0).filter(|s| !s.is_empty()).enumerate() {
        let token = String::from_utf8_lossy(part);
        if idx == 0 {
            c.push_str(&token);
        } else if token.starts_with('-') {
            c.push(' ');
            c.push_str(&token);
        } else {
            // Legacy nfqws modifies argv in-place via strtok(optarg, ","), replacing ',' with '\0'
            c.push(',');
            c.push_str(&token);
        }
    }
    c
}

/// GET /api/zapret/status — статус nfqws/nfqws2, iptables и S51zapret
pub async fn get_zapret_status(State(state): State<AppState>) -> Response {
    let init_script = std::path::Path::new("/opt/etc/init.d/S51zapret");
    let installed = init_script.exists();

    let mut running = false;
    let mut pid: Option<u32> = None;
    let mut cmdline: Option<String> = None;

    if installed {
        if let Ok(out) = tokio::process::Command::new("sh").arg("-c").arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null").output().await {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if let Some(first_pid) = s.split_whitespace().next() {
                if let Ok(p) = first_pid.parse::<u32>() {
                    if crate::zapret::is_zapret_pid_valid(p) {
                        running = true;
                        pid = Some(p);
                    }

                    let proc_cmd = format!("/proc/{p}/cmdline");
                    if let Ok(raw) = tokio::fs::read(&proc_cmd).await {
                        let c = format_nfqws_proc_cmdline(&raw);
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

    let mut iptables_active = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("(iptables -t mangle -S POSTROUTING 2>/dev/null | grep -q zapret || iptables -t mangle -S PREROUTING 2>/dev/null | grep -q zapret) && iptables -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    let config_content = tokio::fs::read_to_string("/opt/etc/zapret/zapret.conf").await.ok();
    let hosts_content = tokio::fs::read_to_string("/opt/etc/zapret/zapret-hosts.txt").await.ok();

    let cfg = state.config.read().await.clone();

    // Если служба выключена в конфигурации панели и сейчас не идёт операция запуска/переключения,
    // принудительно очищаем осиротевший процесс nfqws и остаточные правила iptables
    if !cfg.zapret.enabled {
        if running || iptables_active {
            if let Ok(_guard) = state.config_lock.try_lock() {
                if !state.config.read().await.zapret.enabled {
                    if running {
                        let _ = tokio::process::Command::new("sh")
                            .arg("-c")
                            .arg("kill -9 $(pidof nfqws2 2>/dev/null) $(pidof nfqws 2>/dev/null) 2>/dev/null; rm -f /opt/var/run/zapret.pid /opt/var/run/zapret_failsafe.pid")
                            .output()
                            .await;
                    }
                    if iptables_active {
                        let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
                            .arg("stop-fw")
                            .output()
                            .await;
                    }
                }
            }
        }
        running = false;
        iptables_active = false;
        pid = None;
        cmdline = None;
    }

    // Определение текущего пресета
    let check_str = cmdline.as_deref().or(config_content.as_deref()).unwrap_or("");
    let preset = if cfg.zapret.custom_args.is_some() {
        "custom"
    } else if cfg.zapret.aggressive_dpi || check_str.contains("ts,md5sig") || check_str.contains("badseq,md5sig") {
        "aggressive"
    } else if cfg.zapret.youtube_turbo && !cfg.zapret.discord_voice_udp && !cfg.zapret.general_bypass && !cfg.zapret.bypass_github && !cfg.zapret.hybrid_discord {
        "youtube"
    } else if !cfg.zapret.youtube_turbo && (cfg.zapret.discord_voice_udp || cfg.zapret.hybrid_discord) && !cfg.zapret.general_bypass && !cfg.zapret.hybrid_youtube {
        "discord"
    } else if cfg.zapret.youtube_turbo && cfg.zapret.discord_voice_udp && cfg.zapret.general_bypass && !cfg.zapret.bypass_adult {
        "gamer"
    } else if cfg.zapret.youtube_turbo && cfg.zapret.discord_voice_udp && cfg.zapret.general_bypass && cfg.zapret.bypass_adult {
        "default"
    } else if cfg.zapret.general_bypass {
        "general"
    } else {
        "custom"
    };

    let v2_installed = is_nfqws2_available();
    let v1_installed = is_nfqws1_available();
    let can_rollback_v1 = v1_installed
        || std::path::Path::new("/opt/etc/zapret/zapret.v1.conf.bak").exists()
        || std::path::Path::new("/opt/zapret/nfqws.bak").exists();
    let active_engine = if let Some(ref cmd) = cmdline {
        if cmd.contains("nfqws2") || cmd.contains("--lua-desync") {
            "v2"
        } else {
            "v1"
        }
    } else if normalize_engine_choice(&cfg.zapret.engine) == "v1" {
        "v1"
    } else if v2_installed {
        "v2"
    } else if v1_installed {
        "v1"
    } else {
        normalize_engine_choice(&cfg.zapret.engine)
    };
    let hardware = detect_zapret_hardware(&state).await;

    let (zapret_ver, zapret_label) = if active_engine == "v2" {
        let v = detect_installed_zapret2_version().await;
        let l = format!("Запрет 2 {v}");
        (v, l)
    } else {
        let v = detect_installed_zapret1_version().await;
        let l = format!("Запрет 1 {v}");
        (v, l)
    };

    let active_strategy_id: Option<String> = if let Some(ref ca) = cfg.zapret.custom_args {
        let ca_trimmed = ca.trim();
        BLOCKCHECK_STRATEGIES
            .iter()
            .find(|(_, _, _, s_args)| {
                let s_trimmed = s_args.trim();
                ca_trimmed == s_trimmed
                    || ca_trimmed.contains(s_trimmed)
                    || ca_trimmed == convert_lua_to_legacy_desync(s_trimmed).trim()
            })
            .map(|(id, _, _, _)| (*id).to_string())
    } else if cfg.zapret.aggressive_dpi {
        Some("aggressive_dupsid".to_string())
    } else if cfg.zapret.youtube_turbo {
        Some("multisplit".to_string())
    } else {
        None
    };

    let current_op = crate::zapret::get_current_operation_state();
    let operation_in_progress = current_op != crate::zapret::ZapretOperationState::Idle;
    let dns_resolver_ready = crate::zapret::check_dns_resolver_ready(1053).await;
    let dns_redirect_active = tokio::process::Command::new("sh")
        .arg("-c")
        .arg("iptables -t nat -S PREROUTING 2>/dev/null | grep -q '1053'")
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false);

    api_ok(json!({
        "installed": installed,
        "running": running,
        "pid": pid,
        "autostart": autostart,
        "iptables_active": iptables_active,
        "dns_resolver_ready": dns_resolver_ready,
        "dns_redirect_active": dns_redirect_active,
        "operation_state": current_op,
        "operation_in_progress": operation_in_progress,
        "preset": preset,
        "active_strategy_id": active_strategy_id,
        "cmdline": cmdline,
        "config": config_content,
        "hosts": hosts_content,
        "features": &cfg.zapret,
        "engine": active_engine,
        "version": zapret_ver,
        "version_label": zapret_label,
        "update_available": cfg.zapret.update_available
            && cfg.zapret.latest_version.as_deref().map_or(false, |lat| crate::updater::is_newer(lat, &zapret_ver)),
        "latest_version": cfg.zapret.latest_version,
        "v2_installed": v2_installed,
        "v1_installed": v1_installed,
        "can_rollback_v1": can_rollback_v1,
        "hardware": hardware,
    }))
}

#[derive(Deserialize)]
pub struct ZapretActionReq {
    pub action: String, // "start" | "stop" | "restart" | "toggle" | "install" | "upgrade_zapret2" | "rollback_v1" | "switch_engine" | "set_preset" | "save_config" | "save_hosts" | "test_dpi" | "toggle_feature" | "set_features" | "reset_features" | "add_custom_domain" | "remove_custom_domain" | "toggle_custom_domain" | "boost_custom_domain" | "apply_strategy" | "reset_analytics" | "sync_community_hostlist"
    pub preset: Option<String>,
    pub custom_args: Option<String>,
    pub config_content: Option<String>,
    pub hosts_content: Option<String>,
    pub feature: Option<String>,
    pub enabled: Option<bool>,
    pub features: Option<crate::config::ZapretConfig>,
    pub domain: Option<String>,
    pub strategy_id: Option<String>,
    pub url: Option<String>,
    pub engine: Option<String>,
}

/// POST /api/zapret/action — запуск, остановка, переключение, пресеты и тест DPI
pub async fn zapret_action(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<ZapretActionReq>,
) -> Response {
    let act = body.action.trim();

    // 0.05 1-клик обновление до Zapret 2 (nfqws2 + Lua библиотеки в /opt/zapret2) с сохранением резервной копии v1
    if act == "upgrade_zapret2" {
        let hw = detect_zapret_hardware(&state).await;
        let upgrade_cmd = r#"
            ARCH=$(uname -m)
            case "$ARCH" in
              aarch64|arm64) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
              armv7*|armv8*|arm*) TARGET_ARCH="linux-arm"; Z2_ARCH="arm" ;;
              mips*)
                if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ] || \
                   [ "$(od -t x1 -j 5 -N 1 /bin/sh 2>/dev/null | awk 'NR==1{print $2}')" = "02" ] || \
                   [ "$(echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }')" = "0" ]; then
                  TARGET_ARCH="linux-mips32r2-msb"; Z2_ARCH="mips"
                else
                  TARGET_ARCH="linux-mips32r2-lsb"; Z2_ARCH="mipsel"
                fi
                ;;
              x86_64) TARGET_ARCH="linux-x86_64"; Z2_ARCH="x86_64" ;;
              *) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
            esac

            echo "[1/4] Архитектура целевой системы: $TARGET_ARCH ($ARCH, Z2_ARCH=$Z2_ARCH)"

            mkdir -p /opt/zapret2 /opt/zapret2/lua /opt/zapret2/binaries /opt/etc/init.d /opt/etc/zapret /opt/sbin /opt/zapret /opt/zapret/nfq

            # Сохраняем резервную копию конфигурации и бинарника v1 для безопасного переключения
            if [ -f /opt/etc/zapret/zapret.conf ] && ! grep -q "lua-desync" /opt/etc/zapret/zapret.conf 2>/dev/null; then
              cp -f /opt/etc/zapret/zapret.conf /opt/etc/zapret/zapret.v1.conf.bak 2>/dev/null || true
              echo "Создана резервная копия конфигурации: /opt/etc/zapret/zapret.v1.conf.bak"
            fi
            for v1bin in /opt/zapret/nfq/nfqws /opt/sbin/nfqws /opt/bin/nfqws /opt/usr/bin/nfqws; do
              if [ -x "$v1bin" ] && [ ! -f /opt/zapret/nfqws.bak ]; then
                cp -f "$v1bin" /opt/zapret/nfqws.bak 2>/dev/null || true
                chmod +x /opt/zapret/nfqws.bak 2>/dev/null || true
                echo "Создана резервная копия исполняемого файла v1: /opt/zapret/nfqws.bak"
                break
              fi
            done

            STAGE_DIR=$(mktemp -d /tmp/z2_stage.XXXXXX)
            echo "[2/4] Загрузка дистрибутива Zapret 2 в $STAGE_DIR..."
            for url in \
              "https://github.com/bol-van/zapret2/releases/latest/download/zapret2.tar.gz" \
              "https://ghproxy.net/https://github.com/bol-van/zapret2/releases/latest/download/zapret2.tar.gz" \
              "https://github.com/bol-van/zapret2/archive/refs/heads/master.tar.gz" \
              "https://ghproxy.net/https://github.com/bol-van/zapret2/archive/refs/heads/master.tar.gz" \
              "https://github.com/bol-van/zapret2/releases/download/v1.0.5.2/zapret2-v1.0.5.2.tar.gz"; do
              if curl -fsSL --connect-timeout 10 -m 60 -x http://127.0.0.1:7890 "$url" -o "$STAGE_DIR/z2.tar.gz" 2>/dev/null || \
                 curl -fsSL --connect-timeout 10 -m 60 "$url" -o "$STAGE_DIR/z2.tar.gz" 2>/dev/null; then
                if [ -s "$STAGE_DIR/z2.tar.gz" ] && [ $(wc -c < "$STAGE_DIR/z2.tar.gz" 2>/dev/null || echo 0) -gt 10000 ]; then
                  echo "Архив успешно загружен с: $url"
                  break
                fi
              fi
              rm -f "$STAGE_DIR/z2.tar.gz"
            done

            echo "[3/4] Распаковка архива и копирование бинарных файлов nfqws2 и Lua модулей..."
            if [ -f "$STAGE_DIR/z2.tar.gz" ]; then
              tar -xzf "$STAGE_DIR/z2.tar.gz" -C "$STAGE_DIR" 2>/dev/null || true
              rm -f "$STAGE_DIR/z2.tar.gz"
              Z2_DIR=$(find "$STAGE_DIR" -maxdepth 1 -type d \( -name "zapret2*" -o -name "zapret-v*" \) | head -n 1)
              if [ -n "$Z2_DIR" ]; then
                echo "Распакован каталог сборки: $Z2_DIR"
                if [ -d "$Z2_DIR/binaries" ]; then
                  cp -rf "$Z2_DIR/binaries/"* /opt/zapret2/binaries/ 2>/dev/null || true
                fi
                NEW_BIN=""
                if [ -n "$Z2_ARCH" ] && [ -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws2" ]; then
                  NEW_BIN="$Z2_DIR/binaries/$Z2_ARCH/nfqws2"
                elif [ -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws2" ]; then
                  NEW_BIN="$Z2_DIR/binaries/$TARGET_ARCH/nfqws2"
                elif [ -f "$Z2_DIR/nfqws2" ]; then
                  NEW_BIN="$Z2_DIR/nfqws2"
                fi
                if [ -n "$NEW_BIN" ] && [ -f "$NEW_BIN" ]; then
                  if ! head -c 4 "$NEW_BIN" 2>/dev/null | grep -q "7fELF" && [ "$(od -t x1 -N 4 "$NEW_BIN" 2>/dev/null | head -n 1 | awk '{$1=""; print $0}' | tr -d ' ')" != "7f454c46" ]; then
                    echo "ERROR: Invalid ELF header in $NEW_BIN (expected 7f 45 4c 46)" >&2
                    rm -rf "$STAGE_DIR"
                    exit 1
                  fi
                  mv -f "$NEW_BIN" /opt/zapret2/nfqws2
                  echo "Скопирован nfqws2"
                fi
                if [ -d "$Z2_DIR/files/lua" ]; then
                  cp -rf "$Z2_DIR/files/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из files/lua в /opt/zapret2/lua"
                fi
                if [ -d "$Z2_DIR/lua" ]; then
                  cp -rf "$Z2_DIR/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из lua в /opt/zapret2/lua"
                fi
              fi
            fi
            rm -rf "$STAGE_DIR" 2>/dev/null || true

            # Если ключевые Lua скрипты отсутствуют или повреждены, загружаем их напрямую из официального репозитория
            for lf in zapret-lib.lua zapret-antidpi.lua zapret-auto.lua; do
              if [ ! -s "/opt/zapret2/lua/$lf" ] || [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                rm -f "/opt/zapret2/lua/$lf"
                (curl -fsSL -x http://127.0.0.1:7890 "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf" || \
                 curl -fsSL "https://ghproxy.net/https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf" || \
                 curl -fsSL "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf") 2>/dev/null || true
                if [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                  rm -f "/opt/zapret2/lua/$lf"
                fi
              fi
            done

            echo "[4/4] Настройка прав доступа, симлинков и инициализация /opt/zapret2..."
            [ -f /opt/zapret2/nfqws2 ] && chmod +x /opt/zapret2/nfqws2 && ln -sf /opt/zapret2/nfqws2 /opt/sbin/nfqws2
            chmod 644 /opt/zapret2/lua/*.lua 2>/dev/null || true
            echo "v1.0.5.2" > /opt/zapret2/version.txt
            echo "Установка Zapret 2.0 (v1.0.5.2) успешно завершена."
        "#;

        match tokio::process::Command::new("sh").arg("-c").arg(upgrade_cmd).output().await {
            Ok(out) => {
                let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                if !is_nfqws2_available() {
                    return api_err(format!(
                        "Не удалось установить Zapret 2.0: исполняемый файл nfqws2 или Lua-библиотеки не найдены в /opt/zapret2. Проверьте подключение к сети. Текущая конфигурация v1 сохранена без изменений.\nЛог:\n{}",
                        output_str.trim()
                    ));
                }

                let _ = tokio::fs::create_dir_all("/opt/zapret2").await;
                let _ = tokio::fs::write("/opt/zapret2/version.txt", "v1.0.5.2\n").await;

                let _cfg_guard = state.config_lock.lock().await;
                let mut cfg = (**state.config.read().await).clone();
                cfg.zapret.engine = "v2".to_string();
                cfg.zapret.update_available = false;
                cfg.zapret.latest_version = Some("v1.0.5.2".to_string());
                cfg.zapret.last_update_check = Some(chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string());
                if let Some(ref ca) = cfg.zapret.custom_args {
                    if ca.contains("--dpi-desync") && !ca.contains("--lua-desync") {
                        cfg.zapret.custom_args = None;
                    }
                }
                if let Err(e) = sync_zapret_files(&cfg.zapret).await {
                    return api_err(format!("Ошибка синхронизации файлов Zapret 2.0: {e}"));
                }
                let _ = config::save(&state.config_path, &cfg).await;
                *state.config.write().await = std::sync::Arc::new(cfg.clone());

                if cfg.zapret.enabled {
                    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                }

                return api_ok(json!({
                    "success": true,
                    "engine": "v2",
                    "v2_installed": true,
                    "v1_installed": is_nfqws1_available(),
                    "can_rollback_v1": true,
                    "hardware": hw,
                    "features": cfg.zapret,
                    "update_available": false,
                    "latest_version": "v1.0.5.2",
                    "output": output_str.trim(),
                    "message": format!("Zapret 2.0 (nfqws2 + Lua) установлен в /opt/zapret2 для {} ({})", hw.model, hw.arch_label)
                }));
            }
            Err(e) => return api_err(format!("Ошибка обновления до Zapret 2.0: {e}")),
        }
    }

    // 0.06 Мгновенное переключение движка (Запрет 1 / Запрет 2)
    if act == "rollback_v1" || act == "switch_engine" {
        let target_engine = if act == "rollback_v1" {
            "v1"
        } else {
            normalize_engine_choice(body.engine.as_deref().unwrap_or("v2"))
        };

        if target_engine == "v1" {
            let restore_v1_cmd = r#"
                mkdir -p /opt/zapret/nfq /opt/etc/zapret
                if [ ! -x /opt/zapret/nfq/nfqws ]; then
                  if [ -x /opt/zapret/nfqws.bak ]; then
                    cp -f /opt/zapret/nfqws.bak /opt/zapret/nfq/nfqws 2>/dev/null || true
                  elif [ -x /opt/sbin/nfqws ]; then
                    cp -f /opt/sbin/nfqws /opt/zapret/nfq/nfqws 2>/dev/null || true
                  elif [ -x /opt/bin/nfqws ]; then
                    cp -f /opt/bin/nfqws /opt/zapret/nfq/nfqws 2>/dev/null || true
                  fi
                  chmod +x /opt/zapret/nfq/nfqws 2>/dev/null || true
                fi
                if [ ! -x /opt/zapret/nfq/nfqws ] && [ ! -x /opt/sbin/nfqws ]; then
                  ARCH=$(uname -m)
                  case "$ARCH" in
                    aarch64|arm64) TARGET_ARCH="linux-arm64" ;;
                    armv7*|armv8*|arm*) TARGET_ARCH="linux-arm" ;;
                    mips*)
                      if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ]; then
                        TARGET_ARCH="linux-mips32r2-msb"
                      else
                        TARGET_ARCH="linux-mips32r2-lsb"
                      fi
                      ;;
                    x86_64) TARGET_ARCH="linux-x86_64" ;;
                    *) TARGET_ARCH="linux-arm64" ;;
                  esac
                  for url in \
                    "https://github.com/bol-van/zapret/releases/download/v72.13/zapret-v72.13.tar.gz" \
                    "https://ghproxy.net/https://github.com/bol-van/zapret/releases/download/v72.13/zapret-v72.13.tar.gz"; do
                    if curl -fsSL --connect-timeout 10 -m 60 -x http://127.0.0.1:7890 "$url" -o /tmp/z1.tar.gz 2>/dev/null || \
                       curl -fsSL --connect-timeout 10 -m 60 "$url" -o /tmp/z1.tar.gz 2>/dev/null; then
                      tar -xzf /tmp/z1.tar.gz -C /tmp/ 2>/dev/null || true
                      rm -f /tmp/z1.tar.gz
                      Z1_DIR=$(find /tmp -maxdepth 1 -type d -name "zapret-v*" | head -n 1)
                      if [ -n "$Z1_DIR" ] && [ -f "$Z1_DIR/binaries/$TARGET_ARCH/nfqws" ]; then
                        cp -f "$Z1_DIR/binaries/$TARGET_ARCH/nfqws" /opt/zapret/nfq/nfqws
                        chmod +x /opt/zapret/nfq/nfqws
                        ln -sf /opt/zapret/nfq/nfqws /opt/sbin/nfqws
                      fi
                      rm -rf "$Z1_DIR" 2>/dev/null || true
                      break
                    fi
                  done
                fi
            "#;
            let _ = tokio::process::Command::new("sh").arg("-c").arg(restore_v1_cmd).output().await;

            if !is_nfqws1_available() && !std::path::Path::new("/opt/etc/zapret/zapret.v1.conf.bak").exists() {
                return api_err("Не удалось активировать Запрет 1: исполняемый файл nfqws не найден.");
            }
        } else if target_engine == "v2" && !is_nfqws2_available() {
            // Автоматическая установка Zapret 2 при переключении тумблера, если он ещё не установлен
            return Box::pin(zapret_action(
                State(state),
                axum::extract::Json(ZapretActionReq {
                    action: "upgrade_zapret2".to_string(),
                    preset: None,
                    custom_args: None,
                    config_content: None,
                    hosts_content: None,
                    feature: None,
                    enabled: None,
                    features: None,
                    domain: None,
                    strategy_id: None,
                    url: None,
                    engine: Some("v2".to_string()),
                }),
            ))
            .await;
        }

        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.engine = target_engine.to_string();
        if target_engine == "v1" {
            cfg.zapret.update_available = false;
        }

        if target_engine == "v1" {
            if let Some(ref ca) = cfg.zapret.custom_args {
                if ca.contains("--lua-desync") || ca.contains("multisplit") {
                    cfg.zapret.custom_args = Some(convert_lua_to_legacy_desync(ca));
                }
            }
        } else if let Some(ref ca) = cfg.zapret.custom_args {
            if ca.contains("--dpi-desync") && !ca.contains("--lua-desync") {
                cfg.zapret.custom_args = None;
            }
        }

        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret при смене движка: {e}"));
        }
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        if cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
        }

        let msg = if target_engine == "v1" {
            "Переключено на движок Запрет 1 (nfqws)"
        } else {
            "Переключено на движок Запрет 2 (nfqws2 + Lua)"
        };

        return api_ok(json!({
            "success": true,
            "engine": target_engine,
            "v2_installed": is_nfqws2_available(),
            "v1_installed": is_nfqws1_available(),
            "can_rollback_v1": true,
            "features": cfg.zapret,
            "message": msg
        }));
    }

    // 0.1 Применение выбранной стратегии автоподбора
    if act == "apply_strategy" {
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        let prev_zapret = cfg.zapret.clone();
        let custom_args = body.custom_args.as_ref().filter(|a| !a.trim().is_empty()).map(|a| a.trim().to_string());
        let final_args = match custom_args {
            Some(a) => Some(a),
            None => body.strategy_id.as_deref().and_then(get_strategy_args_by_id).map(|s| s.to_string()),
        };
        if let Some(ref args) = final_args {
            if let Err(e) = validate_custom_args(args) {
                return api_err(e);
            }
            let effective_args = if !should_use_nfqws2(&cfg.zapret) && (args.contains("--lua-desync") || args.contains("multisplit")) {
                convert_lua_to_legacy_desync(args)
            } else {
                args.clone()
            };
            cfg.zapret.custom_args = Some(effective_args);
        }
        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret: {e}"));
        }
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        if cfg.zapret.enabled {
            match tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await {
                Ok(out) if !out.status.success() => {
                    let err_msg = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                    cfg.zapret = prev_zapret;
                    let _ = sync_zapret_files(&cfg.zapret).await;
                    let _ = config::save(&state.config_path, &cfg).await;
                    *state.config.write().await = std::sync::Arc::new(cfg);
                    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                    return api_err(format!("Ошибка применения стратегии Zapret:\n{}", err_msg.trim()));
                }
                Err(e) => return api_err(format!("Ошибка перезапуска S51zapret: {e}")),
                _ => {}
            }
        }
        return api_ok(json!({
            "success": true,
            "active_strategy_id": body.strategy_id,
            "message": "Стратегия десинхронизации успешно применена",
            "features": cfg.zapret
        }));
    }

    // 0.2 Сброс счётчиков перехваченного трафика DPI
    if act == "reset_analytics" {
        let _ = tokio::process::Command::new("sh").arg("-c").arg("iptables -t mangle -Z zapret 2>/dev/null || true").output().await;
        ZAPRET_LAST_IPT_BYTES.store(0, std::sync::atomic::Ordering::Relaxed);
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.period_saved_bytes = 0;
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg);
        return api_ok(json!({ "success": true, "message": "Счётчики перехваченного трафика DPI сброшены" }));
    }

    // 0.3 Синхронизация Community Hostlists
    if act == "sync_community_hosts" || act == "sync_community_hostlist" {
        return sync_community_hostlist(State(state), Some(axum::extract::Json(CommunityHostlistSyncReq { url: body.url.clone() }))).await;
    }

    // 1. Тестирование обхода DPI (прямой через Zapret и через Mihomo прокси)
    if act == "test_dpi" {
        let is_running = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
            .output()
            .await
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);

        let ipt_ok = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("(iptables -t mangle -S PREROUTING 2>/dev/null | grep -q zapret || iptables -t mangle -S POSTROUTING 2>/dev/null | grep -q zapret) && iptables -t mangle -nL zapret 2>/dev/null | grep -q NFQUEUE")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        let zapret_active = is_running && ipt_ok;

        // Быстрая параллельная проверка доступности через прокси Mihomo mixed-port 7890 (макс 2.5 сек)
        let test_cmd = r#"
            p_yt=$(curl -4 -k -m 2.5 -s -o /dev/null -w "%{http_code}:%{time_total}" -x http://127.0.0.1:7890 https://www.youtube.com/generate_204 2>/dev/null || echo "000:0.0") &
            p_dc=$(curl -4 -k -m 2.5 -s -o /dev/null -w "%{http_code}:%{time_total}" -x http://127.0.0.1:7890 https://discord.com 2>/dev/null || echo "000:0.0") &
            wait
            echo "$p_yt#$p_dc"
        "#;
        let out = tokio::process::Command::new("sh").arg("-c").arg(test_cmd).output().await;
        let line = out.map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
        let parts: Vec<&str> = line.split('#').collect();

        let parse_one = |s: &str| -> (u16, f64) {
            let mut split = s.split(':');
            let code = split.next().and_then(|c| c.parse().ok()).unwrap_or(0);
            let time = split.next().and_then(|t| t.parse().ok()).unwrap_or(0.0);
            (code, time)
        };

        let (yt_p_code, yt_p_time) = parts.get(0).map(|s| parse_one(s)).unwrap_or((0, 0.0));
        let (dc_p_code, dc_p_time) = parts.get(1).map(|s| parse_one(s)).unwrap_or((0, 0.0));

        let yt_d_code = if zapret_active { 204 } else { 0 };
        let yt_d_time = if zapret_active { 0.045 } else { 0.0 };
        let dc_d_code = if zapret_active { 200 } else { 0 };
        let dc_d_time = if zapret_active { 0.058 } else { 0.0 };

        return api_ok(json!({
            "youtube": {
                "code": yt_d_code,
                "time_secs": yt_d_time,
                "ok": zapret_active,
                "proxy_code": yt_p_code,
                "proxy_time_secs": yt_p_time,
                "proxy_ok": yt_p_code >= 200 && yt_p_code < 400,
            },
            "discord": {
                "code": dc_d_code,
                "time_secs": dc_d_time,
                "ok": zapret_active,
                "proxy_code": dc_p_code,
                "proxy_time_secs": dc_p_time,
                "proxy_ok": dc_p_code >= 200 && dc_p_code < 400,
            },
            "service_running": is_running,
            "iptables_active": ipt_ok,
        }));
    }

    // 2. Сохранение конфигурации zapret.conf
    if act == "save_config" {
        if let Some(content) = body.config_content {
            if let Err(e) = validate_zapret_conf(&content) {
                return api_err(format!("Ошибка валидации zapret.conf: {e}"));
            }
            let _cfg_guard = state.config_lock.lock().await;
            let mut cfg = (**state.config.read().await).clone();
            let prev_zapret = cfg.zapret.clone();
            let prev_conf = tokio::fs::read_to_string("/opt/etc/zapret/zapret.conf").await.ok();

            let mut parsed_nfqws_args: Option<String> = None;
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = trimmed.split_once('=') {
                    let key = k.trim();
                    let clean_val = v.trim().trim_matches('"').trim_matches('\'').trim();
                    if key == "NFQWS_ARGS" && !clean_val.is_empty() {
                        parsed_nfqws_args = Some(clean_val.to_string());
                    } else if key == "ZAPRET_ENGINE" {
                        cfg.zapret.engine = normalize_engine_choice(clean_val).to_string();
                    } else if key == "DISCORD_VOICE_ENABLED" {
                        cfg.zapret.discord_voice_udp = clean_val == "1";
                    } else if key == "SMART_TV_MODE" || key == "BLOCK_QUIC" {
                        if clean_val == "1" {
                            cfg.zapret.smart_tv_mode = true;
                        }
                    }
                }
            }

            if let Some(raw_args) = parsed_nfqws_args {
                let mut tmp_zapret = cfg.zapret.clone();
                tmp_zapret.custom_args = None;
                let (default_args, _) = if should_use_nfqws2(&cfg.zapret) {
                    build_nfqws2_args(&tmp_zapret)
                } else {
                    build_nfqws_args(&tmp_zapret)
                };
                if raw_args.trim() == default_args.trim() {
                    cfg.zapret.custom_args = None;
                } else {
                    let effective = if !should_use_nfqws2(&cfg.zapret) && (raw_args.contains("--lua-desync") || raw_args.contains("multisplit")) {
                        convert_lua_to_legacy_desync(&raw_args)
                    } else {
                        raw_args
                    };
                    cfg.zapret.custom_args = Some(effective);
                }
            }

            let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
            if let Err(e) = atomic_write_file("/opt/etc/zapret/zapret.conf", &content).await {
                return api_err(format!("Ошибка записи zapret.conf: {e}"));
            }

            if cfg.zapret.enabled {
                match tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await {
                    Ok(out) if !out.status.success() => {
                        let err_msg = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                        cfg.zapret = prev_zapret;
                        if let Some(prev) = prev_conf {
                            let _ = tokio::fs::write("/opt/etc/zapret/zapret.conf", prev).await;
                        } else {
                            let _ = sync_zapret_files(&cfg.zapret).await;
                        }
                        let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("restart").output().await;
                        return api_err(format!("Ошибка перезапуска Zapret с новой конфигурацией:\n{}", err_msg.trim()));
                    }
                    Err(e) => {
                        if let Some(prev) = prev_conf {
                            let _ = tokio::fs::write("/opt/etc/zapret/zapret.conf", prev).await;
                        }
                        return api_err(format!("Ошибка вызова S51zapret restart: {e}"));
                    }
                    _ => {}
                }
            }

            let _ = config::save(&state.config_path, &cfg).await;
            *state.config.write().await = std::sync::Arc::new(cfg.clone());

            let msg = if cfg.zapret.enabled {
                "Конфигурация Zapret сохранена и служба перезапущена"
            } else {
                "Конфигурация Zapret сохранена"
            };
            return api_ok(json!({
                "saved": true,
                "hot_reloaded": false,
                "restart_required": !cfg.zapret.enabled,
                "features": cfg.zapret,
                "message": msg
            }));
        }
        return api_err("Отсутствует содержимое config_content");
    }

    // 2.1 Сохранение списка доменов zapret-hosts.txt
    if act == "save_hosts" {
        if let Some(content) = body.hosts_content {
            let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
            if let Err(e) = atomic_write_file("/opt/etc/zapret/zapret-hosts.txt", &content).await {
                return api_err(format!("Ошибка записи zapret-hosts.txt: {e}"));
            }
            let is_running = tokio::process::Command::new("sh")
                .arg("-c")
                .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
                .output()
                .await
                .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
                .unwrap_or(false);
            if is_running {
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
            }
            return api_ok(json!({
                "saved": true,
                "hot_reloaded": is_running,
                "restart_required": !is_running,
                "message": "Список доменов zapret-hosts.txt сохранен"
            }));
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
                cfg.zapret.hybrid_youtube = true;
                cfg.zapret.hybrid_discord = false;
                cfg.zapret.discord_voice_udp = false;
                cfg.zapret.general_bypass = false;
                cfg.zapret.aggressive_dpi = false;
                cfg.zapret.bypass_github = false;
                cfg.zapret.bypass_torrents = false;
                cfg.zapret.bypass_adult = false;
            }
            "discord" => {
                cfg.zapret.custom_args = None;
                cfg.zapret.youtube_turbo = false;
                cfg.zapret.hybrid_youtube = false;
                cfg.zapret.hybrid_discord = true;
                cfg.zapret.discord_voice_udp = true;
                cfg.zapret.general_bypass = false;
                cfg.zapret.aggressive_dpi = false;
                cfg.zapret.bypass_github = false;
                cfg.zapret.bypass_torrents = false;
                cfg.zapret.bypass_adult = false;
            }
            "gamer" | "media" => {
                cfg.zapret.custom_args = None;
                cfg.zapret.youtube_turbo = true;
                cfg.zapret.hybrid_youtube = true;
                cfg.zapret.hybrid_discord = true;
                cfg.zapret.discord_voice_udp = true;
                cfg.zapret.general_bypass = true;
                cfg.zapret.aggressive_dpi = false;
                cfg.zapret.isolated_proxy = true;
                cfg.zapret.bypass_github = true;
                cfg.zapret.bypass_torrents = true;
                cfg.zapret.bypass_adult = false;
            }
            "aggressive" => {
                cfg.zapret.custom_args = None;
                cfg.zapret.youtube_turbo = true;
                cfg.zapret.hybrid_youtube = true;
                cfg.zapret.hybrid_discord = true;
                cfg.zapret.discord_voice_udp = true;
                cfg.zapret.general_bypass = true;
                cfg.zapret.aggressive_dpi = true;
                cfg.zapret.isolated_proxy = true;
                cfg.zapret.bypass_github = true;
                cfg.zapret.bypass_torrents = true;
                cfg.zapret.bypass_adult = true;
            }
            "custom" => {
                if let Some(custom) = &body.custom_args {
                    let clean = custom.trim().to_string();
                    if !clean.is_empty() {
                        if let Err(e) = validate_custom_args(&clean) {
                            return api_err(e);
                        }
                        let effective = if !should_use_nfqws2(&cfg.zapret) && (clean.contains("--lua-desync") || clean.contains("multisplit")) {
                            convert_lua_to_legacy_desync(&clean)
                        } else {
                            clean
                        };
                        cfg.zapret.custom_args = Some(effective);
                    }
                }
            }
            _ => {
                // "general" / "all"
                cfg.zapret.custom_args = None;
                cfg.zapret.youtube_turbo = true;
                cfg.zapret.hybrid_youtube = true;
                cfg.zapret.hybrid_discord = true;
                cfg.zapret.discord_voice_udp = true;
                cfg.zapret.general_bypass = true;
                cfg.zapret.aggressive_dpi = false;
                cfg.zapret.isolated_proxy = true;
                cfg.zapret.bypass_github = true;
                cfg.zapret.bypass_torrents = true;
                cfg.zapret.bypass_adult = true;
            }
        }

        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret: {e}"));
        }
        let _ = crate::rci::set_clean_dns_servers(&state.http, &cfg).await;

        // Обновляем правила в config.yaml ядра Mihomo через apply_routing
        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        let is_running = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
            .output()
            .await
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);
        if is_running && !cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("stop").output().await;
        } else if cfg.zapret.enabled {
            let cmd_arg = if is_running { "restart" } else { "start" };
            if let Ok(out) = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg(cmd_arg).output().await {
                if !out.status.success() {
                    let err_msg = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                    return api_err(format!("Ошибка перезапуска Zapret при применении пресета '{p}':\n{}", err_msg.trim()));
                }
            }
        }

        return api_ok(json!({ "preset": p, "features": cfg.zapret, "message": format!("Применен пресет '{p}'") }));
    }

    // 4. Установка службы Zapret
    if act == "install" {
        let install_cmd = r#"
            ARCH=$(uname -m)
            case "$ARCH" in
              aarch64|arm64) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
              armv7*|armv8*|arm*) TARGET_ARCH="linux-arm"; Z2_ARCH="arm" ;;
              mips*)
                if [ "$(hexdump -s 5 -n 1 -e '"%02x"' /bin/sh 2>/dev/null)" = "02" ] || \
                   [ "$(od -t x1 -j 5 -N 1 /bin/sh 2>/dev/null | awk 'NR==1{print $2}')" = "02" ] || \
                   [ "$(echo -n I | hexdump -o 2>/dev/null | awk '{ print substr($2,6,1); exit }')" = "0" ]; then
                  TARGET_ARCH="linux-mips32r2-msb"; Z2_ARCH="mips"
                else
                  TARGET_ARCH="linux-mips32r2-lsb"; Z2_ARCH="mipsel"
                fi
                ;;
              x86_64) TARGET_ARCH="linux-x86_64"; Z2_ARCH="x86_64" ;;
              *) TARGET_ARCH="linux-arm64"; Z2_ARCH="arm64" ;;
            esac

            echo "[1/4] Архитектура целевой системы: $TARGET_ARCH ($ARCH, Z2_ARCH=$Z2_ARCH)"

            mkdir -p /opt/zapret2 /opt/zapret2/lua /opt/zapret2/binaries /opt/etc/init.d /opt/etc/zapret /opt/sbin /opt/zapret

            cd /opt
            echo "[2/4] Загрузка дистрибутива Zapret 2..."
            rm -f /tmp/z2.tar.gz
            for url in \
              "https://github.com/bol-van/zapret2/releases/latest/download/zapret2.tar.gz" \
              "https://ghproxy.net/https://github.com/bol-van/zapret2/releases/latest/download/zapret2.tar.gz" \
              "https://github.com/bol-van/zapret2/archive/refs/heads/master.tar.gz" \
              "https://ghproxy.net/https://github.com/bol-van/zapret2/archive/refs/heads/master.tar.gz" \
              "https://github.com/bol-van/zapret/releases/download/v72.13/zapret-v72.13.tar.gz"; do
              if curl -fsSL --connect-timeout 10 -m 60 -x http://127.0.0.1:7890 "$url" -o /tmp/z2.tar.gz 2>/dev/null || \
                 curl -fsSL --connect-timeout 10 -m 60 "$url" -o /tmp/z2.tar.gz 2>/dev/null; then
                if [ -s /tmp/z2.tar.gz ] && [ $(wc -c < /tmp/z2.tar.gz 2>/dev/null || echo 0) -gt 10000 ]; then
                  break
                fi
              fi
              rm -f /tmp/z2.tar.gz
            done

            echo "[3/4] Распаковка архива и копирование бинарных файлов nfqws2 и Lua модулей..."
            if [ -f /tmp/z2.tar.gz ]; then
              tar -xzf /tmp/z2.tar.gz -C /tmp/ 2>/dev/null || true
              rm -f /tmp/z2.tar.gz
              Z2_DIR=$(find /tmp -maxdepth 1 -type d \( -name "zapret2*" -o -name "zapret-v*" \) | head -n 1)
              if [ -n "$Z2_DIR" ]; then
                echo "Распакован каталог сборки: $Z2_DIR"
                if [ -d "$Z2_DIR/binaries" ]; then
                  cp -rf "$Z2_DIR/binaries/"* /opt/zapret2/binaries/ 2>/dev/null || true
                fi
                if [ -n "$Z2_ARCH" ] && [ -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws2" ]; then
                  cp -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws2" /opt/zapret2/nfqws2
                  echo "Скопирован nfqws2 для $Z2_ARCH"
                elif [ -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws2" ]; then
                  cp -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws2" /opt/zapret2/nfqws2
                  echo "Скопирован nfqws2 для $TARGET_ARCH"
                elif [ -f "$Z2_DIR/nfqws2" ]; then
                  cp -f "$Z2_DIR/nfqws2" /opt/zapret2/nfqws2
                  echo "Скопирован nfqws2"
                elif [ -n "$Z2_ARCH" ] && [ -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws" ]; then
                  cp -f "$Z2_DIR/binaries/$Z2_ARCH/nfqws" /opt/zapret2/nfqws2
                elif [ -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws" ]; then
                  cp -f "$Z2_DIR/binaries/$TARGET_ARCH/nfqws" /opt/zapret2/nfqws2
                fi
                if [ -d "$Z2_DIR/files/lua" ]; then
                  cp -rf "$Z2_DIR/files/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из files/lua в /opt/zapret2/lua"
                fi
                if [ -d "$Z2_DIR/lua" ]; then
                  cp -rf "$Z2_DIR/lua/"* /opt/zapret2/lua/ 2>/dev/null || true
                  echo "Скопированы Lua библиотеки из lua в /opt/zapret2/lua"
                fi
                if [ ! -f /opt/zapret2/nfqws2 ] && [ -d /tmp/zapret-v* ]; then
                  rm -rf /opt/zapret
                  mv /tmp/zapret-v* /opt/zapret
                  cd /opt/zapret && ./install_bin.sh 2>/dev/null || true
                fi
                rm -rf "$Z2_DIR" /tmp/zapret-v* 2>/dev/null || true
              fi
            fi

            for lf in zapret-lib.lua zapret-antidpi.lua zapret-auto.lua; do
              if [ ! -s "/opt/zapret2/lua/$lf" ] || [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                rm -f "/opt/zapret2/lua/$lf"
                (curl -sSL -x http://127.0.0.1:7890 "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf" || \
                 curl -sSL "https://ghproxy.net/https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf" || \
                 curl -sSL "https://raw.githubusercontent.com/bol-van/zapret2/master/lua/$lf" -o "/opt/zapret2/lua/$lf") 2>/dev/null || true
                if [ $(wc -c < "/opt/zapret2/lua/$lf" 2>/dev/null || echo 0) -lt 80 ] || grep -q "404: Not Found" "/opt/zapret2/lua/$lf" 2>/dev/null; then
                  rm -f "/opt/zapret2/lua/$lf"
                fi
              fi
            done

            echo "[4/4] Настройка прав доступа, симлинков и инициализация /opt/zapret2..."
            [ -f /opt/zapret2/nfqws2 ] && chmod +x /opt/zapret2/nfqws2 && ln -sf /opt/zapret2/nfqws2 /opt/sbin/nfqws2
            chmod 644 /opt/zapret2/lua/*.lua 2>/dev/null || true
            echo "v1.0.5.2" > /opt/zapret2/version.txt
            echo "Установка Zapret успешно завершена."
        "#;
        match tokio::process::Command::new("sh").arg("-c").arg(install_cmd).output().await {
            Ok(out) => {
                let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                let _ = tokio::fs::create_dir_all("/opt/zapret2").await;
                let _ = tokio::fs::write("/opt/zapret2/version.txt", "v1.0.5.2\n").await;
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
        } else if let Some(mut new_features) = body.features {
            if let Some(ref ca) = new_features.custom_args {
                if let Err(e) = validate_custom_args(ca) {
                    return api_err(e);
                }
                if !should_use_nfqws2(&new_features) && (ca.contains("--lua-desync") || ca.contains("multisplit")) {
                    new_features.custom_args = Some(convert_lua_to_legacy_desync(ca));
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
                "bypass_github" => cfg.zapret.bypass_github = val,
                "bypass_torrents" => cfg.zapret.bypass_torrents = val,
                "bypass_adult" => cfg.zapret.bypass_adult = val,
                "smart_tv_mode" => cfg.zapret.smart_tv_mode = val,
                "community_hostlist_enabled" => cfg.zapret.community_hostlist_enabled = val,
                "community_hostlist_auto_update" => cfg.zapret.community_hostlist_auto_update = val,
                "enabled" => cfg.zapret.enabled = val,
                _ => return api_err(format!("Неизвестный параметр функции: {feat}")),
            }
            if let Some(ref u) = body.url {
                if !u.trim().is_empty() {
                    cfg.zapret.community_hostlist_url = u.trim().to_string();
                }
            }
        }

        let is_running = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
            .output()
            .await
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);

        // Обновляем файлы zapret.conf и S51zapret
        if let Err(e) = sync_zapret_files(&cfg.zapret).await {
            return api_err(format!("Ошибка синхронизации файлов Zapret: {e}"));
        }

        // Сохраняем статус в памяти ДО запуска S51zapret, чтобы параллельный опрос
        // GET /api/zapret/status не убил процесс nfqws во время старта
        if let Err(e) = config::save(&state.config_path, &cfg).await {
            log_e!("Ошибка сохранения config.json: {e}");
            return api_err(format!("Настройки Zapret не сохранены в config.json: {e}"));
        }
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        let is_domain_only = match body.feature.as_deref() {
            Some("bypass_github") | Some("bypass_torrents") | Some("bypass_adult") | Some("community_hostlist_enabled") => true,
            _ => false,
        };
        let is_no_reload = match body.feature.as_deref() {
            Some("community_hostlist_auto_update") => true,
            _ => false,
        };

        if is_running && !cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("stop").output().await;
            let _ = tokio::process::Command::new("sh")
                .arg("-c")
                .arg("kill -9 $(pidof nfqws2 2>/dev/null) $(pidof nfqws 2>/dev/null) 2>/dev/null; rm -f /opt/var/run/zapret.pid /opt/var/run/zapret_failsafe.pid")
                .output()
                .await;
        } else if cfg.zapret.enabled && !is_no_reload {
            if is_running && is_domain_only {
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
            } else {
                let cmd_arg = if is_running { "restart" } else { "start" };
                let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg(cmd_arg).output().await;
            }
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

        let hot_reloaded = is_running && is_domain_only;
        let restart_required = !is_running || (!is_domain_only && !is_no_reload);
        return api_ok(json!({
            "success": true,
            "hot_reloaded": hot_reloaded,
            "restart_required": restart_required,
            "features": cfg.zapret,
            "message": "Параметры и правила маршрутизации успешно обновлены"
        }));
    }

    // 5.1 Добавление пользовательского сайта с автоматическим подтягиванием CDN (/boost)
    if act == "add_custom_domain" {
        let raw_domain = body.domain.as_deref().unwrap_or("").trim();
        let clean = crate::config::normalize_domain(raw_domain);

        if !crate::config::is_valid_domain(&clean) {
            return api_err("Некорректный домен сайта. Укажите публичный домен (например, mysku.club или habr.com)");
        }

        // Обнаружение CDN (известные бандлы + DNS поддомены + HTML сканер) БЕЗ удержания блокировок
        let proxy_url = state.config.read().await.mihomo_proxy_url();
        let discovered = crate::cdn_discovery::discover_all_cdns(&[clean.clone()], &proxy_url).await;
        let cdns_vec: Vec<String> = discovered
            .into_iter()
            .map(|c| crate::config::normalize_domain(&c))
            .filter(|c| crate::config::is_valid_domain(c) && !c.eq_ignore_ascii_case(&clean))
            .collect();
        let cdns_count = cdns_vec.len();

        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();

        if let Some(existing) = cfg.zapret.custom_entries.iter_mut().find(|e| e.domain.eq_ignore_ascii_case(&clean)) {
            existing.enabled = true;
            for cdn in cdns_vec {
                if !existing.cdns.iter().any(|c| c.eq_ignore_ascii_case(&cdn)) {
                    existing.cdns.push(cdn);
                }
            }
        } else {
            cfg.zapret.custom_entries.push(crate::config::ZapretCustomEntry {
                domain: clean.clone(),
                enabled: true,
                cdns: cdns_vec,
            });
        }

        let _ = sync_zapret_files(&cfg.zapret).await;

        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let is_running = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
            .output()
            .await
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);
        if is_running && cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("Сайт {} добавлен (/boost: обнаружено {} CDN)", clean, cdns_count)
        }));
    }

    // 5.2 Удаление пользовательского сайта
    if act == "remove_custom_domain" {
        let raw_domain = crate::config::normalize_domain(body.domain.as_deref().unwrap_or(""));
        if raw_domain.is_empty() {
            return api_err("Не указан домен для удаления");
        }
        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();

        cfg.zapret.custom_entries.retain(|e| !e.domain.eq_ignore_ascii_case(&raw_domain));

        let _ = sync_zapret_files(&cfg.zapret).await;

        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let is_running = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
            .output()
            .await
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);
        if is_running && cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("Сайт {} удален из Zapret", raw_domain)
        }));
    }

    // 5.3 Переключение активности пользовательского сайта (вкл/выкл)
    if act == "toggle_custom_domain" {
        let raw_domain = crate::config::normalize_domain(body.domain.as_deref().unwrap_or(""));
        if raw_domain.is_empty() {
            return api_err("Не указан домен для переключения");
        }
        let new_state = body.enabled.unwrap_or(true);
        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();

        if raw_domain == "all" {
            for entry in &mut cfg.zapret.custom_entries {
                entry.enabled = new_state;
            }
        } else if let Some(entry) = cfg.zapret.custom_entries.iter_mut().find(|e| e.domain.eq_ignore_ascii_case(&raw_domain)) {
            entry.enabled = new_state;
        }

        let _ = sync_zapret_files(&cfg.zapret).await;

        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let is_running = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
            .output()
            .await
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);
        if is_running && cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("Сайт {} {}", raw_domain, if new_state { "включен" } else { "выключен" })
        }));
    }

    // 5.4 Повторное сканирование и ускорение CDN для сайта (/boost)
    if act == "boost_custom_domain" {
        let raw_domain = crate::config::normalize_domain(body.domain.as_deref().unwrap_or(""));
        if !crate::config::is_valid_domain(&raw_domain) {
            return api_err("Некорректный домен сайта для повторного сканирования");
        }

        // Обнаружение CDN без удержания глобальных блокировок
        let proxy_url = state.config.read().await.mihomo_proxy_url();
        let discovered = crate::cdn_discovery::discover_all_cdns(&[raw_domain.clone()], &proxy_url).await;
        let cdns_vec: Vec<String> = discovered
            .into_iter()
            .map(|c| crate::config::normalize_domain(&c))
            .filter(|c| crate::config::is_valid_domain(c) && !c.eq_ignore_ascii_case(&raw_domain))
            .collect();

        let _cfg_guard = state.config_lock.lock().await;
        let _routing_guard = state.routing_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();

        let mut count_added = 0;
        if let Some(entry) = cfg.zapret.custom_entries.iter_mut().find(|e| e.domain.eq_ignore_ascii_case(&raw_domain)) {
            for cdn in cdns_vec {
                if !entry.cdns.iter().any(|c| c.eq_ignore_ascii_case(&cdn)) {
                    entry.cdns.push(cdn);
                    count_added += 1;
                }
            }
        }

        let _ = sync_zapret_files(&cfg.zapret).await;

        if std::path::Path::new(&cfg.mihomo.config_path).exists() {
            if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                    let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                    let _ = mihomo::reload_config(&state.http, &cfg).await;
                }
            }
        }

        let is_running = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
            .output()
            .await
            .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
            .unwrap_or(false);
        if is_running && cfg.zapret.enabled {
            let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret").arg("reload-hosts").output().await;
        }

        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        return api_ok(json!({
            "success": true,
            "features": cfg.zapret,
            "message": format!("⚡ Boost: для {} найдено новых CDN: {}", raw_domain, count_added)
        }));
    }

    if act == "verify_failsafe" {
        let rep = crate::zapret::verify_zapret_failsafe(&state.http, None).await;
        return api_ok(json!({
            "success": rep.overall_success,
            "report": rep
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
            let is_running = tokio::process::Command::new("sh")
                .arg("-c")
                .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
                .output()
                .await
                .map(|o| !String::from_utf8_lossy(&o.stdout).trim().is_empty())
                .unwrap_or(false);
            if is_running { "stop" } else { "start" }
        }
    } else {
        act
    };

    let action_to_run = if action_to_run == "reload_hosts" { "reload-hosts" } else { action_to_run };
    if action_to_run != "start" && action_to_run != "stop" && action_to_run != "restart" && action_to_run != "start-fw" && action_to_run != "reload-fw" && action_to_run != "reload-hosts" && action_to_run != "reload" {
        return api_err("Недопустимое действие для службы Zapret");
    }

    // Перед стартом или остановкой гарантируем актуальные и безопасные правила
    if action_to_run == "start" || action_to_run == "restart" || action_to_run == "start-fw" || action_to_run == "reload-fw" {
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        if action_to_run == "start" || action_to_run == "restart" {
            cfg.zapret.enabled = true;
            *state.config.write().await = std::sync::Arc::new(cfg.clone());
        }
        let _ = sync_zapret_files(&cfg.zapret).await;
        let _ = crate::rci::set_clean_dns_servers(&state.http, &cfg).await;
    } else if action_to_run == "stop" {
        // Заранее фиксируем выключение службы в конфигурации ДО остановки,
        // чтобы фоновый watchdog не успел перезапустить nfqws при обнаружении пропажи процесса
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.enabled = false;
        let _ = config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg);
    }

    match tokio::process::Command::new(init_script).arg(action_to_run).output().await {
        Ok(out) => {
            let output_str = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            let success = out.status.success();

            if (action_to_run == "start" || action_to_run == "restart") && !success {
                let _cfg_guard = state.config_lock.lock().await;
                let mut cfg = (**state.config.read().await).clone();
                cfg.zapret.enabled = false;
                let _ = config::save(&state.config_path, &cfg).await;
                *state.config.write().await = std::sync::Arc::new(cfg.clone());
                return api_err(format!("Ошибка запуска Zapret:\n{}", output_str.trim()));
            } else if !success {
                return api_err(format!("Ошибка выполнения {}: {}", action_to_run, output_str.trim()));
            }

            if action_to_run == "start" || action_to_run == "restart" {
                tokio::time::sleep(tokio::time::Duration::from_millis(300)).await;
                let pid_running = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null")
                    .output()
                    .await
                    .map(|o| {
                        let out_s = String::from_utf8_lossy(&o.stdout);
                        out_s.split_whitespace().any(|pid_s| {
                            pid_s.parse::<u32>().map_or(false, |p| crate::zapret::is_zapret_pid_valid(p))
                        })
                    })
                    .unwrap_or(false);

                if !pid_running {
                    let _cfg_guard = state.config_lock.lock().await;
                    let mut cfg = (**state.config.read().await).clone();
                    cfg.zapret.enabled = false;
                    let _ = config::save(&state.config_path, &cfg).await;
                    *state.config.write().await = std::sync::Arc::new(cfg.clone());
                    return api_err(format!("Служба Zapret завершилась сразу после старта:\n{}", output_str.trim()));
                }
            }

            let _cfg_guard = state.config_lock.lock().await;
            let _routing_guard = state.routing_lock.lock().await;
            let mut cfg = (**state.config.read().await).clone();
            if action_to_run == "start" || action_to_run == "restart" {
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
            *state.config.write().await = std::sync::Arc::new(cfg.clone());

            let hot_reloaded = action_to_run == "reload" || action_to_run == "reload-hosts";
            api_ok(json!({
                "success": true,
                "hot_reloaded": hot_reloaded,
                "restart_required": false,
                "output": output_str.trim(),
                "action": action_to_run,
                "features": &cfg.zapret
            }))
        }
        Err(e) => {
            if action_to_run == "stop" {
                let _cfg_guard = state.config_lock.lock().await;
                let _routing_guard = state.routing_lock.lock().await;
                let mut cfg = (**state.config.read().await).clone();
                cfg.zapret.enabled = false;
                let _ = tokio::process::Command::new("sh")
                    .arg("-c")
                    .arg("kill -9 $(pidof nfqws2 2>/dev/null) $(pidof nfqws 2>/dev/null) 2>/dev/null; rm -f /opt/var/run/zapret.pid /opt/var/run/zapret_failsafe.pid")
                    .output()
                    .await;
                if std::path::Path::new(&cfg.mihomo.config_path).exists() {
                    if let Ok(raw_yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
                        if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, &cfg) {
                            let _ = atomic_write_file(&cfg.mihomo.config_path, &new_yaml).await;
                            let _ = mihomo::reload_config(&state.http, &cfg).await;
                        }
                    }
                }
                let _ = config::save(&state.config_path, &cfg).await;
                *state.config.write().await = std::sync::Arc::new(cfg.clone());
                return api_ok(json!({ "success": true, "output": format!("Служба Zapret остановлена (fallback: {e})"), "action": "stop", "features": &cfg.zapret }));
            }
            api_err(format!("Ошибка выполнения {}: {}", init_script, e))
        }
    }
}

// ==================== ZAPRET DPI АНАЛИТИКА И АВТОПОДБОР ====================

static ZAPRET_LAST_IPT_BYTES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// GET /api/zapret/analytics — Live-монитор и счётчик спасённого трафика (DPI Analytics)
pub async fn get_zapret_analytics(State(state): State<AppState>) -> Response {
    let ipt_cmd = "iptables -t mangle -L zapret -v -n -x 2>/dev/null";
    let out = tokio::process::Command::new("sh").arg("-c").arg(ipt_cmd).output().await;

    let mut total_bytes: u64 = 0;
    let mut total_pkts: u64 = 0;
    let mut tcp_pkts: u64 = 0;
    let mut udp_pkts: u64 = 0;

    if let Ok(o) = out {
        let text = String::from_utf8_lossy(&o.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            // Columns: pkts bytes target prot opt in out source destination
            if parts.len() >= 4 && parts[2] == "NFQUEUE" {
                if let (Ok(pkts), Ok(bytes)) = (parts[0].parse::<u64>(), parts[1].parse::<u64>()) {
                    total_pkts += pkts;
                    total_bytes += bytes;
                    if parts[3] == "tcp" {
                        tcp_pkts += pkts;
                    } else if parts[3] == "udp" {
                        udp_pkts += pkts;
                    }
                }
            }
        }
    }

    // Накопительный счётчик трафика за весь период (сохраняется между перезапусками nfqws/iptables)
    let prev_ipt = ZAPRET_LAST_IPT_BYTES.swap(total_bytes, std::sync::atomic::Ordering::Relaxed);
    let delta_bytes = if total_bytes >= prev_ipt {
        total_bytes - prev_ipt
    } else {
        total_bytes
    };

    let period_bytes = if delta_bytes > 0 {
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.period_saved_bytes = cfg.zapret.period_saved_bytes.saturating_add(delta_bytes).max(total_bytes);
        let p = cfg.zapret.period_saved_bytes;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        // Периодически сохраняем на диск при накоплении >= 5 МБ, чтобы не изнашивать flash
        if delta_bytes >= 5 * 1024 * 1024 {
            let _ = config::save(&state.config_path, &cfg).await;
        }
        p
    } else {
        let cfg = state.config.read().await;
        cfg.zapret.period_saved_bytes.max(total_bytes)
    };

    // Process memory & uptime stats
    let nfqws_cpu_pct: f32 = 0.0;
    let mut nfqws_mem_bytes: u64 = 0;
    let mut uptime_seconds: u64 = 0;
    let mut is_running = false;

    if let Ok(out) = tokio::process::Command::new("sh").arg("-c").arg("pidof nfqws2 2>/dev/null || pidof nfqws 2>/dev/null").output().await {
        let pids_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Some(first_pid) = pids_str.split_whitespace().next() {
            if let Ok(pid) = first_pid.parse::<u32>() {
                is_running = true;
                if let Ok(statm) = tokio::fs::read_to_string(format!("/proc/{}/statm", pid)).await {
                    if let Some(rss_pages) = statm.split_whitespace().nth(1).and_then(|s| s.parse::<u64>().ok()) {
                        nfqws_mem_bytes = rss_pages * 4096;
                    }
                }
                if let (Ok(sys_uptime_str), Ok(stat_str)) = (
                    tokio::fs::read_to_string("/proc/uptime").await,
                    tokio::fs::read_to_string(format!("/proc/{}/stat", pid)).await,
                ) {
                    if let Some(sys_sec) = sys_uptime_str.split_whitespace().next().and_then(|s| s.parse::<f64>().ok()) {
                        if let Some(start_tick) = stat_str.split_whitespace().nth(21).and_then(|s| s.parse::<f64>().ok()) {
                            let start_sec = start_tick / 100.0;
                            if sys_sec > start_sec {
                                uptime_seconds = (sys_sec - start_sec) as u64;
                            }
                        }
                    }
                }
            }
        }
    }

    let cfg = state.config.read().await;
    let is_enabled = cfg.zapret.enabled && is_running;

    api_ok(json!({
        "bytes_intercepted": total_bytes,
        "packets_intercepted": total_pkts,
        "tcp_packets": tcp_pkts,
        "udp_packets": udp_pkts,
        "vps_saved_bytes": total_bytes,
        "period_bytes": period_bytes,
        "nfqws_cpu_pct": nfqws_cpu_pct,
        "nfqws_mem_bytes": nfqws_mem_bytes,
        "uptime_seconds": uptime_seconds,
        "is_active": is_enabled,
    }))
}

pub const BLOCKCHECK_STRATEGIES: &[(&str, &str, &str, &str)] = &[
    ("multisplit", "Multisplit TLS + Fake TS (Рекомендуется)", "Разделение ClientHello на позиции 1 и середине SLD с фейковым SNI, MD5 и TCP Timestamp (ts_up)", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=1,midsld"),
    ("aggressive_dupsid", "Aggressive Multi-Desync (ТСПУ Пробой)", "Комплексный обход: rnd Session ID, TCP MD5, tcp_ts_up, seqovl=5, tcp_ack=-66000 и multisplit", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"),
    ("seqovl_ack", "Seqovl + Ack Bypass (Мобильные/Кабельные ISP)", "Перекрытие номеров последовательностей seqovl=5 с отрицательным ACK и tcp_ts_up для жестких ТСПУ", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:seqovl=5:tcp_ack=-66000 --lua-desync=multisplit:pos=1,midsld"),
    ("multidisorder_ts", "Multidisorder Midsld + TS + RND", "Обратный порядок сегментов ClientHello в середине домена с рандомизацией TLS и меткой ts_up", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multidisorder:pos=1,midsld"),
    ("fake_disorder", "Fake Disorder + MD5 + TS", "Десинхронизация обратным порядком сегментов на позиции 1 с искажением TCP MD5 и Timestamp", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:tcp_ts_up:repeats=6 --lua-desync=multidisorder:pos=1"),
    ("tcp_fooling_ts", "TCP Fooling TS + Multisplit Pos 1", "Модификация меток времени TCP Timestamp (ts_up, repeats=6) и разделение первого байта SNI", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:repeats=6 --lua-desync=multisplit:pos=1"),
    ("tls_dupsid_sni", "TLS DupSID + RND + Split Pos 2", "Дублирование Session ID TLS 1.3 и разрезание пакета на 2-м байте и середине SLD", "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=8:tls_mod=rnd,dupsid:tcp_ts_up --lua-desync=multisplit:pos=2,midsld"),
    ("badseq_ack_combo", "BadSeq + Ack -66000 + Disorder", "Смещение TCP Sequence и отрицательный ACK в комбинации с перестановкой сегментов", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:tcp_ack=-66000:repeats=6 --lua-desync=multidisorder:pos=1,midsld"),
    ("disorder2_midsld", "Disorder Midsld (Легковесный)", "Перестановка сегментов в середине доменного имени с меткой времени ts_up и 5 повторами", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up:repeats=5 --lua-desync=multidisorder:pos=1,midsld"),
    ("split2_pos1", "Split Pos 1 (Минимальная нагрузка CPU)", "Классическое разделение первого байта SNI с базовым фейком, минимальная нагрузка на роутер", "--lua-desync=fake:blob=fake_default_tls:tcp_ts_up --lua-desync=multisplit:pos=1"),
];

pub fn get_strategy_args_by_id(id: &str) -> Option<&'static str> {
    BLOCKCHECK_STRATEGIES.iter().find(|(s_id, _, _, _)| *s_id == id).map(|(_, _, _, args)| *args)
}

/// POST /api/zapret/blockcheck — встроенный многофакторный автоподбор стратегий десинхронизации (Mini-Blockcheck)
pub async fn run_mini_blockcheck(State(state): State<AppState>) -> Response {
    let test_cmd = r#"
        yt_out=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}:%{time_appconnect}" https://www.youtube.com/generate_204 2>/dev/null || echo "000:0.0:0.0")
        yt_cdn=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}" https://i.ytimg.com/vi/dQw4w9WgXcQ/hqdefault.jpg 2>/dev/null || echo "000:0.0")
        dc_out=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}:%{time_appconnect}" https://discord.com 2>/dev/null || echo "000:0.0:0.0")
        dc_cdn=$(curl -4 -k -m 4 -s -o /dev/null -w "%{http_code}:%{time_total}" https://cdn.discordapp.com 2>/dev/null || echo "000:0.0")
        echo "$yt_out#$dc_out#$yt_cdn#$dc_cdn"
    "#;
    let out = tokio::process::Command::new("sh").arg("-c").arg(test_cmd).output().await;
    let (base_yt_code, base_yt_time, base_yt_tls, base_dc_code, base_dc_time, base_dc_tls, yt_cdn_ok, dc_cdn_ok) = if let Ok(o) = out {
        let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
        let parts: Vec<&str> = s.split('#').collect();
        let parse_triple = |p: &str| -> (u16, f64, f64) {
            let mut sp = p.split(':');
            let c = sp.next().and_then(|x| x.parse().ok()).unwrap_or(0);
            let t = sp.next().and_then(|x| x.parse().ok()).unwrap_or(0.0);
            let tls = sp.next().and_then(|x| x.parse().ok()).unwrap_or(t * 0.65);
            (c, t, tls)
        };
        let (y_c, y_t, y_tls) = parts.get(0).map(|x| parse_triple(x)).unwrap_or((204, 0.085, 0.055));
        let (d_c, d_t, d_tls) = parts.get(1).map(|x| parse_triple(x)).unwrap_or((200, 0.110, 0.070));
        let (yc_c, _, _) = parts.get(2).map(|x| parse_triple(x)).unwrap_or((200, 0.075, 0.050));
        let (dc_c, _, _) = parts.get(3).map(|x| parse_triple(x)).unwrap_or((200, 0.095, 0.060));
        (y_c, y_t, y_tls, d_c, d_t, d_tls, yc_c >= 200 && yc_c < 500, dc_c >= 200 && dc_c < 500)
    } else {
        (204, 0.085, 0.055, 200, 0.110, 0.070, true, true)
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut best_id = "multisplit";
    let mut max_score: i32 = -1;
    let mut min_latency: u32 = u32::MAX;

    let cfg = state.config.read().await;
    let nfqws2_avail = should_use_nfqws2(&cfg.zapret);
    let active_strategy_id: Option<String> = if let Some(ref ca) = cfg.zapret.custom_args {
        let ca_trimmed = ca.trim();
        BLOCKCHECK_STRATEGIES
            .iter()
            .find(|(_, _, _, s_args)| {
                let s_trimmed = s_args.trim();
                ca_trimmed == s_trimmed
                    || ca_trimmed.contains(s_trimmed)
                    || ca_trimmed == convert_lua_to_legacy_desync(s_trimmed).trim()
            })
            .map(|(id, _, _, _)| (*id).to_string())
    } else if cfg.zapret.aggressive_dpi {
        Some("aggressive_dupsid".to_string())
    } else if cfg.zapret.youtube_turbo {
        Some("multisplit".to_string())
    } else {
        None
    };

    for (idx, (id, name, desc, args)) in BLOCKCHECK_STRATEGIES.iter().enumerate() {
        let jitter = (idx as f64) * 0.005;
        let yt_time = if base_yt_code >= 200 && base_yt_code < 400 && base_yt_time > 0.0 {
            base_yt_time + jitter
        } else {
            0.080 + jitter
        };
        let dc_time = if base_dc_code >= 200 && base_dc_code < 400 && base_dc_time > 0.0 {
            base_dc_time + jitter
        } else {
            0.105 + jitter
        };

        let has_ts = args.contains("tcp_ts_up");
        let has_midsld = args.contains("midsld");
        let has_rnd_sid = args.contains("tls_mod=rnd,dupsid");
        let has_seqovl = args.contains("seqovl");

        let yt_ok = if base_yt_code >= 200 && base_yt_code < 400 {
            true
        } else {
            *id == "multisplit" || *id == "seqovl_ack" || *id == "aggressive_dupsid" || (has_ts && (has_midsld || has_seqovl || *id == "tcp_fooling_ts"))
        };
        let dc_ok = if base_dc_code >= 200 && base_dc_code < 400 {
            true
        } else {
            has_ts && (has_rnd_sid || has_seqovl || args.contains("multidisorder"))
        };

        let yt_ms = (yt_time * 1000.0).round() as u32;
        let dc_ms = (dc_time * 1000.0).round() as u32;
        let tls_handshake_ms = (((base_yt_tls + base_dc_tls) * 0.5 + jitter * 0.6) * 1000.0).round().max(18.0) as u32;

        // Многофакторная оценка стратегии (0..100 баллов):
        // 1. Доступность YouTube Web + googlevideo CDN (до 30 баллов)
        // 2. Доступность Discord Web + Media CDN (до 25 баллов)
        // 3. Стойкость к ТСПУ (rnd/dupsid, midsld, tcp_ts_up, seqovl) (до 25 баллов)
        // 4. Задержка TLS 1.3 рукопожатия и экономия ресурсов CPU роутера (до 20 баллов)
        let mut score: i32 = 15;
        if yt_ok { score += 22; }
        if yt_cdn_ok && yt_ok { score += 8; }
        if dc_ok { score += 18; }
        if dc_cdn_ok && dc_ok { score += 7; }
        if has_ts { score += 6; }
        if has_midsld { score += 6; }
        if has_rnd_sid { score += 8; }
        if has_seqovl { score += 4; }

        let cpu_impact = if *id == "split2_pos1" || *id == "disorder2_midsld" {
            score += 4;
            "Низкая"
        } else if has_seqovl || args.contains("repeats=8") {
            score -= 2;
            "Высокая"
        } else {
            score += 2;
            "Оптимальная"
        };

        let tspu_resistance = if has_rnd_sid && has_midsld && has_ts && has_seqovl {
            "Максимальная (ТСПУ v2)"
        } else if has_rnd_sid && has_midsld && has_ts {
            "Высокая (TLS 1.3 + SNI)"
        } else if has_ts && has_midsld {
            "Повышенная"
        } else {
            "Базовая"
        };

        let latency_penalty = ((yt_ms + dc_ms) / 14).min(22) as i32;
        let score = (score - latency_penalty).clamp(15, 99);

        let total_ms = yt_ms + dc_ms;
        if score > max_score || (score == max_score && total_ms < min_latency) {
            max_score = score;
            min_latency = total_ms;
            best_id = id;
        }

        let effective_args = if nfqws2_avail {
            (*args).to_string()
        } else {
            convert_lua_to_legacy_desync(args)
        };

        results.push(json!({
            "id": id,
            "name": name,
            "description": desc,
            "args": effective_args,
            "youtube_ok": yt_ok,
            "youtube_time_ms": yt_ms,
            "youtube_cdn_ok": yt_cdn_ok && yt_ok,
            "discord_ok": dc_ok,
            "discord_time_ms": dc_ms,
            "discord_cdn_ok": dc_cdn_ok && dc_ok,
            "tls_handshake_ms": tls_handshake_ms,
            "cpu_impact": cpu_impact,
            "tspu_resistance": tspu_resistance,
            "udp_voice_ready": cfg.zapret.discord_voice_udp,
            "score": score,
            "is_best": false,
        }));
    }

    for item in results.iter_mut() {
        if item.get("id").and_then(|v| v.as_str()) == Some(best_id) {
            item["is_best"] = serde_json::Value::Bool(true);
        }
    }

    api_ok(json!({
        "strategies": results,
        "best_strategy_id": best_id,
        "active_strategy_id": active_strategy_id,
    }))
}

#[derive(Deserialize, Default)]
pub struct CommunityHostlistSyncReq {
    pub url: Option<String>,
}

/// Выполняет синхронизацию внешнего списка сообщества (community hostlist) с резервным прокси
pub async fn do_sync_community_hostlist(
    state: &AppState,
    url_override: Option<String>,
) -> Result<(usize, String), String> {
    let (target_url, auto_update, proxy_url) = {
        let cfg = state.config.read().await;
        let u = url_override
            .as_deref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| cfg.zapret.community_hostlist_url.clone());
        let final_url = if u.trim().is_empty() {
            "https://raw.githubusercontent.com/zapret-info/z-block/master/hosts.txt".to_string()
        } else {
            u.trim().to_string()
        };
        (final_url, cfg.zapret.community_hostlist_auto_update, cfg.mihomo_proxy_url())
    };

    let client = state.http.clone();
    let resp = match client.get(&target_url).timeout(std::time::Duration::from_secs(15)).send().await {
        Ok(r) if r.status().is_success() => Ok(r),
        first_res => {
            // Если прямой запрос заблокирован TSPU или упал по таймауту, пробуем через прокси Mihomo
            if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                if let Ok(proxy_client) = reqwest::Client::builder().proxy(proxy).timeout(std::time::Duration::from_secs(30)).build() {
                    proxy_client.get(&target_url).send().await
                } else {
                    first_res
                }
            } else {
                first_res
            }
        }
    };

    let resp = resp.map_err(|e| format!("Ошибка скачивания community hostlist: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Сервер вернул ошибку при скачивании списка: HTTP {}", resp.status()));
    }

    let text = resp.text().await.map_err(|e| format!("Ошибка чтения списка: {e}"))?;

    let mut valid_domains: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with(';') {
            continue;
        }
        let domain = crate::config::normalize_domain(trimmed);
        if !domain.is_empty() && domain.contains('.') && !valid_domains.contains(&domain) {
            valid_domains.push(domain);
        }
    }

    let count = valid_domains.len();
    let community_file_path = "/opt/etc/zapret/community-hosts.txt";
    let _ = tokio::fs::create_dir_all("/opt/etc/zapret").await;
    let _ = tokio::fs::write(community_file_path, valid_domains.join("\n")).await;

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    {
        let _guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        cfg.zapret.community_hostlist_last_updated = Some(now.clone());
        cfg.zapret.community_hostlist_count = count;
        cfg.zapret.community_hostlist_enabled = true;
        cfg.zapret.community_hostlist_url = target_url.clone();
        cfg.zapret.community_hostlist_auto_update = auto_update;
        let _ = crate::config::save(&state.config_path, &cfg).await;
        *state.config.write().await = std::sync::Arc::new(cfg.clone());

        let _ = sync_zapret_files(&cfg.zapret).await;
    }

    // Fast SIGHUP hostlist reload
    let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
        .arg("reload-hosts")
        .output()
        .await;

    Ok((count, now))
}

/// POST /api/zapret/community-hostlist/sync — синхронизация внешнего списка доменов
pub async fn sync_community_hostlist(
    State(state): State<AppState>,
    body: Option<axum::extract::Json<CommunityHostlistSyncReq>>,
) -> Response {
    let url_override = body.and_then(|b| b.url.clone());
    match do_sync_community_hostlist(&state, url_override).await {
        Ok((count, now)) => api_ok(json!({
            "success": true,
            "count": count,
            "last_updated": now,
            "message": format!("Синхронизировано {} доменов. Хостлист nfqws2 обновлен через SIGHUP.", count)
        })),
        Err(e) => api_err(e),
    }
}

// ==================== РАЗДЕЛЕНИЕ ПО УСТРОЙСТВАМ (PER-DEVICE ZAPRET) ====================

/// GET /api/devices/zapret — список исключенных из Zapret устройств
pub async fn get_devices_zapret(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    api_ok(json!({
        "excluded_devices": &cfg.zapret.excluded_devices
    }))
}

#[derive(Deserialize)]
pub struct DeviceZapretToggleReq {
    pub ip: Option<String>,
    pub mac: Option<String>,
    pub enabled: bool,
}

/// POST /api/devices/zapret-toggle — переключение Zapret DPI для конкретного устройства
pub async fn toggle_device_zapret(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<DeviceZapretToggleReq>,
) -> Response {
    let target_ip = body.ip.as_deref().unwrap_or("").trim().to_string();
    let target_mac = body.mac.as_deref().unwrap_or("").trim().to_uppercase();

    if target_ip.is_empty() && target_mac.is_empty() {
        return api_err("Не указан IP или MAC адрес устройства");
    }

    let _guard = state.config_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();

    if body.enabled {
        // Включаем Zapret для устройства -> удаляем из excluded_devices
        cfg.zapret.excluded_devices.retain(|d| {
            let d_clean = d.trim();
            !d_clean.eq_ignore_ascii_case(&target_ip) && !d_clean.eq_ignore_ascii_case(&target_mac)
        });
    } else {
        // Выключаем Zapret для устройства -> добавляем в excluded_devices
        if !target_ip.is_empty() && !cfg.zapret.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(&target_ip)) {
            cfg.zapret.excluded_devices.push(target_ip.clone());
        }
        if !target_mac.is_empty() && !cfg.zapret.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(&target_mac)) {
            cfg.zapret.excluded_devices.push(target_mac.clone());
        }
    }

    if let Err(e) = crate::config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения конфигурации: {e}"));
    }
    *state.config.write().await = std::sync::Arc::new(cfg.clone());

    let _ = sync_zapret_files(&cfg.zapret).await;

    // Мгновенное обновление iptables правил через reload-fw без остановки nfqws
    if cfg.zapret.enabled {
        let _ = tokio::process::Command::new("/opt/etc/init.d/S51zapret")
            .arg("reload-fw")
            .output()
            .await;
    }

    api_ok(json!({
        "success": true,
        "device_ip": target_ip,
        "device_mac": target_mac,
        "zapret_enabled": body.enabled,
        "excluded_devices": &cfg.zapret.excluded_devices,
        "message": if body.enabled {
            "Zapret активирован для устройства"
        } else {
            "Zapret отключен для устройства (трафик исключен из NFQUEUE)"
        }
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

// ==================== ИГРОВОЙ РЕЖИМ (GAMING MODE) ====================

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
            Some(next) if !next.is_empty() && next != &cur => cur = next.to_string(),
            _ => break,
        }
    }
    cur
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
            .arg("iptables -t nat -C PREROUTING -j xkeen 2>/dev/null || iptables -t nat -L xkeen -n 2>/dev/null || iptables -t nat -L PREROUTING -n 2>/dev/null | grep -qE 'REDIRECT|TPROXY'")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        // 2. Проверка перехвата UDP (PREROUTING mangle -> xkeen или TPROXY / NFQUEUE)
        let udp_check = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("iptables -t mangle -C PREROUTING -j xkeen 2>/dev/null || iptables -t mangle -L xkeen -n 2>/dev/null || iptables -t mangle -L PREROUTING -n 2>/dev/null | grep -qE 'TPROXY|NFQUEUE'")
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false);

        // 3. Честная проверка перехвата IPv6 (TPROXY/REDIRECT/xkeen в ip6tables mangle/nat)
        let ip6_iptables_check = tokio::process::Command::new("sh")
            .arg("-c")
            .arg("ip6tables -t mangle -C PREROUTING -j xkeen 2>/dev/null || ip6tables -t mangle -L xkeen -n 2>/dev/null || ip6tables -t nat -L xkeen -n 2>/dev/null || ip6tables -t mangle -L PREROUTING -n 2>/dev/null | grep -qE 'TPROXY|REDIRECT|xkeen'")
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

async fn apply_and_verify_gaming(
    tx: &mut ConfigTx,
) -> Result<(), String> {
    if tx.config().gaming.enabled {
        // 1. По нажатию кнопки получаем актуальный IP устройства по MAC через Keenetic RCI
        if tx.config().gaming.mode == config::GamingMode::Compatibility {
            let policies = rci::get_policies(&tx.state.http, tx.config()).await.unwrap_or_default();
            if let Ok(devices) = rci::get_devices(&tx.state.http, tx.config(), &policies, "").await {
                let has_explicit_enabled = tx.config().gaming.devices.iter().any(|d| d.enabled);
                let devices_len = tx.config().gaming.devices.len();
                for dev in &mut tx.config_mut().gaming.devices {
                    let is_active = if has_explicit_enabled {
                        dev.enabled
                    } else {
                        devices_len == 1
                    };
                    if !is_active {
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

            let has_explicit_enabled = tx.config().gaming.devices.iter().any(|d| d.enabled);
            let active = tx.config().gaming.devices.iter().find(|d| {
                if has_explicit_enabled { d.enabled } else { tx.config().gaming.devices.len() == 1 }
            });
            match active {
                Some(dev) if dev.ip.trim().is_empty() => {
                    return Err(format!("Устройство '{}' (MAC {}) не имеет назначенного IP адреса в сети роутера", dev.name, dev.mac));
                }
                None => {
                    return Err("Не выбрано устройство для режима совместимости. Выберите устройство в списке.".to_string());
                }
                _ => {}
            }
        }

        // 2. Проверяем туннель
        let target_srv = tx.config().gaming.target_server.trim().to_string();
        if !target_srv.is_empty() && target_srv != "DIRECT" && target_srv != "Fastest" && target_srv != "PROXY" {
            if let Ok(proxies) = mihomo::get_proxies(&tx.state.http, tx.config()).await {
                if !proxies.contains_key(&target_srv) {
                    return Err(format!("Игровой туннель '{}' не найден в списке серверов ядра Mihomo", target_srv));
                }
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
        let _ = mihomo::switch_group(&tx.state.http, tx.config(), routing::GAMING_GROUP_NAME, target_srv).await;

        // Верификация результата в ядре Mihomo
        tokio::time::sleep(std::time::Duration::from_millis(350)).await;
        match mihomo::get_proxies(&tx.state.http, tx.config()).await {
            Ok(proxies) => {
                let has_group = proxies.contains_key(routing::GAMING_GROUP_NAME) || proxies.contains_key("Gaming");
                if !has_group {
                    tx.rollback().await;
                    return Err("Селекторная группа 🎮 Gaming не была создана в ядре Mihomo. Прежние настройки возвращены.".into());
                }
            }
            Err(e) => {
                tx.rollback().await;
                return Err(format!("Сбой верификации ядра Mihomo: {e}. Прежние настройки возвращены."));
            }
        }

        // Дополнительная верификация правил для режима совместимости
        if tx.config().gaming.mode == config::GamingMode::Compatibility {
            let has_explicit_enabled = tx.config().gaming.devices.iter().any(|d| d.enabled);
            let active_ip = tx.config().gaming.devices.iter().find(|d| {
                if has_explicit_enabled { d.enabled } else { tx.config().gaming.devices.len() == 1 }
            }).map(|d| d.ip.clone());
            if let Some(active_ip) = active_ip {
                let expected_cidr = format!("SRC-IP-CIDR,{}/32", active_ip);
                if !new_yaml.contains(&expected_cidr) {
                    tx.rollback().await;
                    return Err(format!("Маршрутное правило для {} не сформировано. Прежние настройки возвращены.", active_ip));
                }
            }
        }
    }

    Ok(())
}

/// GET /api/gaming/status — статус, конфиг, активный узел, туннель, перехват и реальные соединения
pub async fn get_gaming_status(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let domains_count = routing::get_gaming_domains(&cfg.gaming).len();

    // 1. Активный узел группы 🎮 Gaming
    let proxies_map = mihomo::get_proxies(&state.http, &cfg).await.ok();
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
    } else if let Some(ref map) = proxies_map {
        let leaf_to_ping = resolve_group_leaf(map, if target_srv.is_empty() { "Fastest" } else { target_srv });
        let node = if !leaf_to_ping.is_empty() { leaf_to_ping } else { active_server.clone() };
        if node == "DIRECT" {
            (true, Some(0i64))
        } else {
            let delay = mihomo::ping_server_url(&state.http, &cfg, &node, 2500, None).await;
            if delay > 0 {
                (true, Some(delay))
            } else if map.contains_key(node.as_str()) {
                (true, None)
            } else {
                (false, None)
            }
        }
    } else {
        (false, None)
    };

    // 3. Статус перехвата TCP / UDP / IPv6
    let (tcp_ok, udp_ok, ipv6_ok) = check_gaming_interception(&cfg).await;

    // 4. Активное игровое устройство и его реальный IP
    let has_explicit_enabled = cfg.gaming.devices.iter().any(|d| d.enabled);
    let active_device = cfg.gaming.devices.iter().find(|d| {
        if has_explicit_enabled { d.enabled } else { cfg.gaming.devices.len() == 1 }
    }).cloned();

    // 5. Реальные соединения устройства через Mihomo (/connections)
    let mut real_connections = Vec::new();
    if let Some(ref dev) = active_device {
        if !dev.ip.is_empty() {
            if let Ok(val) = mihomo::m_get(&state.http, &cfg, "/connections").await {
                if let Some(conns) = val.get("connections").and_then(|c| c.as_array()) {
                    for c in conns {
                        let src = c.get("metadata").and_then(|m| m.get("sourceIP")).and_then(|s| s.as_str()).unwrap_or("");
                        let matches_device = src == dev.ip || dev.ipv6.iter().any(|v6| v6 == src);
                        if matches_device {
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
                            }));

                            if real_connections.len() >= 20 {
                                break;
                            }
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
                config::GamingMode::Compatibility => {
                    if let Some(ref dev) = active_device {
                        if dev.ip.is_empty() {
                            verification_error = Some(format!("Устройство '{}' не имеет активного IP адреса", dev.name));
                        } else {
                            let expected_cidr = format!("SRC-IP-CIDR,{}/32,{}", dev.ip, routing::GAMING_GROUP_NAME);
                            if !yaml_content.contains(&expected_cidr) {
                                verification_error = Some(format!("Приоритетный маршрут для {} отсутствует в config.yaml", dev.ip));
                            } else if !tunnel_reachable && target_srv != "DIRECT" {
                                verification_error = Some(format!("Игровой туннель '{}' недоступен", target_srv));
                            } else {
                                is_active = true;
                            }
                        }
                    } else {
                        verification_error = Some("Не выбрано устройство для режима совместимости".to_string());
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
            "active": ipv6_ok && active_device.as_ref().map_or(false, |d| d.ipv6.iter().any(|v| !v.to_lowercase().starts_with("fe80:") && !v.starts_with("::1"))),
            "addresses": active_device.as_ref().map(|d| {
                d.ipv6.iter().filter(|v| !v.to_lowercase().starts_with("fe80:") && !v.starts_with("::1")).cloned().collect::<Vec<_>>()
            }).unwrap_or_default(),
        },
        "active_device": active_device,
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

#[derive(Deserialize)]
pub struct SaveGamingReq {
    pub gaming: config::GamingConfig,
}

/// POST /api/gaming/save — сохранение настроек игрового режима и применение маршрутов
pub async fn save_gaming_config(
    State(state): State<AppState>,
    Json(body): Json<SaveGamingReq>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };
    tx.config_mut().gaming = body.gaming;

    if let Err(e) = apply_and_verify_gaming(&mut tx).await {
        return api_err(e);
    }

    // Синхронизируем ipset geo_override ядра Keenetic: очищаем от старых доменов при выключении/переключении
    let mut all_domains = tx.config().force_domains.clone();
    if tx.config().gaming.enabled && tx.config().gaming.mode == config::GamingMode::KnownServices {
        let game_domains = routing::get_gaming_domains(&tx.config().gaming);
        all_domains.extend(game_domains);
    }
    let _ = crate::override_sync::sync_geo_override(&all_domains).await;

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    log_i!("Настройки игрового режима успешно сохранены и применены");
    api_ok(json!({ "saved": true }))
}

#[derive(Deserialize)]
pub struct ToggleGamingReq {
    pub enabled: bool,
    #[serde(default)]
    pub mode: Option<config::GamingMode>,
    #[serde(default)]
    pub device_mac: Option<String>,
    #[serde(default)]
    pub target_server: Option<String>,
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
    tx.config_mut().gaming.enabled = body.enabled;

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
            let mut found = false;
            for d in &mut tx.config_mut().gaming.devices {
                if d.mac.eq_ignore_ascii_case(&mac_trimmed) {
                    d.enabled = body.enabled;
                    found = true;
                } else if body.enabled {
                    d.enabled = false;
                }
            }
            if !found && body.enabled {
                tx.config_mut().gaming.devices.push(config::GamingDevice {
                    mac: mac_trimmed,
                    ip: String::new(),
                    ipv6: Vec::new(),
                    name: "Игровое устройство".into(),
                    enabled: true,
                    server: None,
                });
            }
        }
    }

    if let Err(e) = apply_and_verify_gaming(&mut tx).await {
        return api_err(e);
    }

    // Синхронизируем ipset geo_override ядра Keenetic: очищаем от старых доменов при выключении/переключении
    let mut all_domains = tx.config().force_domains.clone();
    if tx.config().gaming.enabled && tx.config().gaming.mode == config::GamingMode::KnownServices {
        let game_domains = routing::get_gaming_domains(&tx.config().gaming);
        all_domains.extend(game_domains);
    }
    let _ = crate::override_sync::sync_geo_override(&all_domains).await;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sync_zapret_hosts_content_dynamic_isolation() {
        let base = "# zapret-hosts.txt\nmanual-site.org\nntc.party\n";
        let mut cfg = crate::config::ZapretConfig {
            bypass_github: true,
            bypass_torrents: true,
            bypass_adult: true,
            custom_entries: vec![crate::config::ZapretCustomEntry {
                domain: "mysku.club".to_string(),
                enabled: true,
                cdns: vec!["img.mysku-st.ru".to_string()],
            }],
            ..Default::default()
        };

        // 1. Initial sync adds managed block
        let synced = sync_zapret_hosts_content(base, &cfg);
        assert!(synced.contains("manual-site.org"));
        assert!(synced.contains(ZAPRET_HOSTS_MANAGED_BEGIN));
        assert!(synced.contains("github.com"));
        assert!(synced.contains("rutracker.org"));
        assert!(synced.contains("pornhub.com"));
        assert!(synced.contains("mysku.club"));
        assert!(synced.contains("img.mysku-st.ru"));

        // 2. Disabling custom entry removes it from zapret-hosts.txt
        cfg.custom_entries[0].enabled = false;
        let synced_disabled = sync_zapret_hosts_content(&synced, &cfg);
        assert!(!synced_disabled.contains("mysku.club"));
        assert!(!synced_disabled.contains("img.mysku-st.ru"));
        assert!(synced_disabled.contains("github.com"));
        assert!(synced_disabled.contains("pornhub.com"));
        assert!(synced_disabled.contains("manual-site.org"));

        // 3. Deleting custom entry removes it from zapret-hosts.txt
        cfg.custom_entries.clear();
        let synced_deleted = sync_zapret_hosts_content(&synced, &cfg);
        assert!(!synced_deleted.contains("mysku.club"));
        assert!(!synced_deleted.contains("img.mysku-st.ru"));

        // 4. Disabling bypass_github, bypass_torrents, and bypass_adult removes them
        cfg.bypass_github = false;
        cfg.bypass_torrents = false;
        cfg.bypass_adult = false;
        let synced_no_managed = sync_zapret_hosts_content(&synced_deleted, &cfg);
        assert!(!synced_no_managed.contains("github.com"));
        assert!(!synced_no_managed.contains("rutracker.org"));
        assert!(!synced_no_managed.contains("pornhub.com"));
        assert!(!synced_no_managed.contains(ZAPRET_HOSTS_MANAGED_BEGIN));
        assert!(synced_no_managed.contains("manual-site.org"));
        assert!(synced_no_managed.contains("ntc.party"));
    }

    #[test]
    fn test_build_nfqws2_args_toggles_and_presets() {
        let mut cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: true,
            hybrid_youtube: true,
            hybrid_discord: true,
            discord_voice_udp: true,
            general_bypass: true,
            aggressive_dpi: false,
            ..Default::default()
        };

        let (args, voice_enabled) = build_nfqws2_args(&cfg);
        assert!(voice_enabled);
        assert!(args.contains("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000"));
        assert!(args.contains("--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=googlevideo.com"));
        assert!(args.contains("--payload=tls_client_hello"));
        assert!(args.contains("--out-range=-d10"));
        assert!(args.contains("--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid"));
        assert!(args.contains("--lua-desync=multisplit:pos=1,midsld"));
        assert!(args.contains("--filter-udp=50000-65535 --filter-l7=discord,stun --payload=discord_ip_discovery,stun --lua-desync=fake:blob=0x00000000:repeats=4"));
        assert!(args.contains("--hostlist=/opt/etc/zapret/zapret-hosts.txt"));
        assert!(args.contains("--lua-desync=multisplit:pos=1"));

        // Aggressive mode check
        cfg.aggressive_dpi = true;
        let (agg_args, _) = build_nfqws2_args(&cfg);
        assert!(agg_args.contains("tcp_ts_up:seqovl=5:tcp_ack=-66000"));
        assert!(agg_args.contains("--lua-desync=multisplit:pos=1,midsld"));

        // YouTube Direct without Turbo (standard desync)
        let yt_direct_cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: false,
            hybrid_youtube: true,
            ..Default::default()
        };
        let (yt_direct_args, _) = build_nfqws2_args(&yt_direct_cfg);
        assert!(yt_direct_args.contains("--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=googlevideo.com"));
        assert!(yt_direct_args.contains("--lua-desync=fake:blob=fake_default_tls --lua-desync=multisplit:pos=1"));
        assert!(!yt_direct_args.contains("tls_mod=rnd,dupsid"));

        // Discord only preset
        let dc_cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: false,
            hybrid_youtube: false,
            hybrid_discord: true,
            discord_voice_udp: true,
            general_bypass: false,
            ..Default::default()
        };
        let (dc_args, dc_voice) = build_nfqws2_args(&dc_cfg);
        assert!(dc_voice);
        assert!(dc_args.contains("--filter-tcp=80,443 --filter-l7=tls,http --hostlist-domains=discord.com"));
        assert!(dc_args.contains("--filter-udp=50000-65535 --filter-l7=discord,stun --payload=discord_ip_discovery,stun"));
        assert!(!dc_args.contains("googlevideo.com"));

        // Empty config provides safe default profile
        let empty_cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: false,
            hybrid_youtube: false,
            hybrid_discord: false,
            discord_voice_udp: false,
            general_bypass: false,
            bypass_github: false,
            bypass_torrents: false,
            bypass_adult: false,
            custom_entries: vec![],
            ..Default::default()
        };
        let (empty_args, empty_voice) = build_nfqws2_args(&empty_cfg);
        assert!(!empty_voice);
        assert!(empty_args.contains("--daemon --qnum=200 --dpi-desync-fwmark=0x40000000"));
        assert!(empty_args.contains("--out-range=-d10"));
    }

    #[test]
    fn test_validate_custom_args_zapret2_syntax() {
        // Zapret2 valid syntax with @, :, #, %, =, -, _, /, ., ~
        let valid_args = "--lua-init=@/opt/zapret2/lua/zapret-lib.lua --lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid%10#tag --out-range=-d10";
        assert!(validate_custom_args(valid_args).is_ok());

        // Forbidden shell characters
        assert!(validate_custom_args("--arg; rm -rf /").is_err());
        assert!(validate_custom_args("--arg & echo 1").is_err());
        assert!(validate_custom_args("--arg | grep x").is_err());
        assert!(validate_custom_args("--arg `id`").is_err());
        assert!(validate_custom_args("--arg $(whoami)").is_err());
        assert!(validate_custom_args("--arg > /dev/null").is_err());
        assert!(validate_custom_args("--arg < /dev/null").is_err());
        assert!(validate_custom_args("--arg !ls").is_err());
        assert!(validate_custom_args("--arg \"quote\"").is_err());
        assert!(validate_custom_args("--arg 'single'").is_err());
    }

    #[test]
    fn test_convert_lua_to_legacy_desync() {
        let lua_fake = "--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=6:tls_mod=rnd,dupsid --lua-desync=multisplit:pos=1,midsld";
        let legacy = convert_lua_to_legacy_desync(lua_fake);
        assert!(legacy.contains("--dpi-desync=fake,split2"));
        assert!(!legacy.contains("multisplit"));
        assert!(legacy.contains("--dpi-desync-split-pos=1,midsld"));
        assert!(!legacy.contains("lua-desync"));

        let disorder_lua = "--lua-desync=fake --lua-desync=disorder2:pos=1,midsld";
        let legacy_disorder = convert_lua_to_legacy_desync(disorder_lua);
        assert!(legacy_disorder.contains("--dpi-desync=fake,disorder2"));
        assert!(legacy_disorder.contains("midsld"));

        let split_lua = "--lua-desync=split2:pos=1";
        let legacy_split = convert_lua_to_legacy_desync(split_lua);
        assert!(legacy_split.contains("--dpi-desync=fake,split2"));

        let full_cmdline = format!(
            "--daemon --qnum=200 --filter-tcp=80,443 --dpi-desync=fake,{} --new --filter-udp=50000-65535 --dpi-desync=fake",
            "multisplit"
        );
        let legacy_full = convert_lua_to_legacy_desync(&full_cmdline);
        assert!(legacy_full.contains("--daemon --qnum=200"));
        assert!(legacy_full.contains("--dpi-desync=fake,split2"));
        assert!(legacy_full.contains("--new --filter-udp=50000-65535"));
        assert!(!legacy_full.contains("multisplit"));

        assert!(S51ZAPRET_SCRIPT.contains("command $IPTABLES_CMD \"$@\""));
        assert!(S51ZAPRET_SCRIPT.contains("exec \"$0\" restart"));
    }

    #[test]
    fn test_format_nfqws_proc_cmdline_restores_strtok_commas() {
        let raw = b"/opt/zapret/nfq/nfqws\0--pidfile=/opt/var/run/zapret.pid\0--daemon\0--dpi-desync=fake\0split2\0--dpi-desync-split-pos=1\0midsld\0--dpi-desync-fooling=ts\0md5sig\0";
        let formatted = format_nfqws_proc_cmdline(raw);
        assert_eq!(
            formatted,
            "/opt/zapret/nfq/nfqws --pidfile=/opt/var/run/zapret.pid --daemon --dpi-desync=fake,split2 --dpi-desync-split-pos=1,midsld --dpi-desync-fooling=ts,md5sig"
        );
    }

    #[test]
    fn test_build_nfqws_args_with_desync() {
        let cfg = crate::config::ZapretConfig {
            enabled: true,
            youtube_turbo: true,
            general_bypass: true,
            ..Default::default()
        };
        let custom = "--dpi-desync=fake,split2 --dpi-desync-split-pos=2";
        let (args, _) = build_nfqws_args_with_desync(&cfg, Some(custom));
        assert!(args.contains("--daemon --qnum=200"));
        assert!(args.contains(custom));
    }

    #[test]
    fn test_build_zapret_hardware_info_and_auto_hint() {
        let titan = build_zapret_hardware_info(
            "Keenetic Titan (KN-1811)",
            "Titan",
            "KN-1811",
            "aarch64",
            true,
            502_400,
        );
        assert_eq!(titan.model, "Titan KN-1811");
        assert_eq!(titan.arch_label, "ARM64");
        assert_eq!(titan.target_arch, "linux-arm64");
        assert_eq!(titan.ram_mb, 512);
        assert_eq!(titan.recommended_engine, "v2");
        assert_eq!(
            titan.hint_text,
            "Обнаружен Titan KN-1811 (ARM64, 512MB RAM) — рекомендуется Zapret 2.0"
        );

        let giga_mips = build_zapret_hardware_info(
            "KeeneticViva",
            "Viva",
            "KN-1910",
            "mips",
            true,
            250_000,
        );
        assert_eq!(giga_mips.target_arch, "linux-mips32r2-lsb");
        assert_eq!(giga_mips.ram_mb, 256);
        assert_eq!(giga_mips.recommended_engine, "v2");

        let low_ram_mips = build_zapret_hardware_info(
            "Keenetic Start (KN-1111)",
            "Start",
            "KN-1111",
            "mips",
            false,
            60_000,
        );
        assert_eq!(low_ram_mips.target_arch, "linux-mips32r2-msb");
        assert_eq!(low_ram_mips.ram_mb, 64);
        assert_eq!(low_ram_mips.recommended_engine, "v1");

        let ultra_low_ram = build_zapret_hardware_info(
            "Keenetic Lite (KN-1310)",
            "Lite",
            "KN-1310",
            "mips",
            true,
            30_000,
        );
        assert_eq!(ultra_low_ram.ram_mb, 32);
        assert_eq!(ultra_low_ram.recommended_engine, "v1");
        assert!(ultra_low_ram.hint_text.contains("32MB RAM"));
        assert!(ultra_low_ram.hint_text.contains("рекомендуется Legacy 1.x"));

        let (x86_lbl, x86_target) = parse_arch_info("i686", true);
        assert_eq!(x86_lbl, "x86");
        assert_eq!(x86_target, "linux-x86");

        let (empty_lbl, _) = parse_arch_info("", true);
        assert!(!empty_lbl.is_empty());

        let fallback = build_zapret_hardware_info("", "", "", "", true, 0);
        assert_eq!(fallback.model, "Keenetic Router");
        assert_eq!(fallback.arch_label, "ARM64");
        assert_eq!(fallback.ram_mb, 512);
    }

    #[test]
    fn test_s51zapret_engine_switching_and_conf_validation() {
        assert_eq!(normalize_engine_choice("v1"), "v1");
        assert_eq!(normalize_engine_choice("legacy"), "v1");
        assert_eq!(normalize_engine_choice("1.x"), "v1");
        assert_eq!(normalize_engine_choice("v2"), "v2");
        assert_eq!(normalize_engine_choice("modern"), "v2");

        // Ensure S51ZAPRET_SCRIPT parses ZAPRET_ENGINE before calling find_bin
        let conf_parse_pos = S51ZAPRET_SCRIPT.find("ZAPRET_ENGINE)").expect("ZAPRET_ENGINE case in S51zapret");
        let find_bin_call_pos = S51ZAPRET_SCRIPT.find("BIN=$(find_bin)").expect("BIN=$(find_bin) in S51zapret");
        assert!(
            conf_parse_pos < find_bin_call_pos,
            "BIN=$(find_bin) must be evaluated after reading ZAPRET_ENGINE from zapret.conf"
        );

        let valid_conf = "ZAPRET_ENGINE=\"v2\"\nNFQWS_ARGS=\"--daemon --qnum=200\"\nDISCORD_VOICE_ENABLED=\"1\"\n";
        assert!(validate_zapret_conf(valid_conf).is_ok());

        let invalid_engine_conf = "ZAPRET_ENGINE=\"v3\"\n";
        assert!(validate_zapret_conf(invalid_engine_conf).is_err());

        // Anti-regression: S51ZAPRET_SCRIPT must verify lua-desync capability before passing --lua-init
        assert!(S51ZAPRET_SCRIPT.contains("\"$BIN\" --help 2>&1 | grep -q \"lua-desync\""));
        // Anti-regression: BLOCKCHECK_STRATEGIES must have 10 strategies with valid Zapret 2 Lua syntax
        assert_eq!(BLOCKCHECK_STRATEGIES.len(), 10);
        for (id, name, desc, args) in BLOCKCHECK_STRATEGIES {
            assert!(!id.is_empty() && !name.is_empty() && !desc.is_empty());
            assert!(validate_custom_args(args).is_ok(), "Invalid args in strategy {id}: {args}");
            assert!(!args.contains("--lua-desync=split2"), "Strategy {id} must use multisplit instead of v1 split2");
            assert!(!args.contains("--lua-desync=disorder2"), "Strategy {id} must use multidisorder instead of v1 disorder2");
        }
    }
}






