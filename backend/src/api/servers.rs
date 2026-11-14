use axum::extract::State;
use axum::response::{IntoResponse, Json, Response};
use serde::Deserialize;
use serde_json::json;

use super::{api_err, api_ok, reapply_canary_routing};
use crate::{config, failover, log_e, log_i, mihomo, rci, transaction::ConfigTx, AppState};
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
                    let custom_alias = cfg.server_aliases.get(&s.id).cloned();
                    let is_whitelist = crate::failover::is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);
                    // PING-AUTO: живой замер из кэша панели важнее протухшей истории
                    // healthcheck-а mihomo (обновляется лишь раз в ~5 минут)
                    let ping_ms = mihomo::cached_ping(&s.id).unwrap_or(s.ping_ms);
                    json!({
                        "id": s.id, "name": s.name, "protocol": s.protocol,
                        "host": s.host, "port": s.port,
                        "resolved_ip": s.resolved_ip,
                        "is_active": s.is_active, "is_priority": s.is_priority,
                        "is_google_ai": is_google_ai,
                        "flow_status": flow_status,
                        "ping_ms": ping_ms,
                        "provider": s.provider,
                        "provider_name": s.provider_name,
                        "is_pool": s.is_pool,
                        "custom_alias": custom_alias,
                        "is_whitelist": is_whitelist,
                    })
                })
                .collect();
            api_ok(json!({
                "servers": list,
                "server_aliases": cfg.server_aliases,
                "whitelist_servers": cfg.whitelist_servers,
                "normal_servers": cfg.normal_servers,
                "whitelist_providers": cfg.whitelist_providers,
            }))
        }
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct ServerAliasReq {
    pub server_id: String,
    #[serde(default)]
    pub alias: String,
}

/// POST /api/servers/alias — задать или сбросить пользовательское имя (alias) сервера
pub async fn set_server_alias(
    State(state): State<AppState>,
    Json(req): Json<ServerAliasReq>,
) -> Response {
    let _cfg_guard = state.config_lock.lock().await;
    let mut cfg = (**state.config.read().await).clone();
    let alias_trimmed = req.alias.trim();
    if alias_trimmed.is_empty() {
        cfg.server_aliases.remove(&req.server_id);
    } else {
        cfg.server_aliases.insert(req.server_id.clone(), alias_trimmed.to_string());
    }
    if let Err(e) = config::save(&state.config_path, &cfg).await {
        return api_err(format!("Ошибка сохранения алиаса сервера: {e}"));
    }
    let aliases = cfg.server_aliases.clone();
    *state.config.write().await = std::sync::Arc::new(cfg);
    log_i!("Алиас сервера '{}' установлен в '{}'", req.server_id, alias_trimmed);
    api_ok(json!({ "server_aliases": aliases }))
}

#[derive(Deserialize)]
pub struct ServerWhitelistReq {
    #[serde(default)]
    pub server_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub servers: Option<Vec<String>>,
    /// Добавить серверы к существующему списку без перезаписи (баг 98)
    #[serde(default)]
    pub add_servers: Option<Vec<String>>,
    /// Явно отметить сервер обычным (баг 99)
    #[serde(default)]
    pub normal: Option<bool>,
}

/// Объединение списка серверов БС без дубликатов и потери существующих пометок (баг 98).
pub fn merge_whitelist_servers(existing: &[String], add: &[String]) -> Vec<String> {
    let mut out = existing.to_vec();
    for item in add {
        let trimmed = item.trim();
        if !trimmed.is_empty() && !out.iter().any(|s| s == trimmed) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// Объединение списка подписок БС без дубликатов (регистронезависимо) и потери существующих (баг 98).
pub fn merge_whitelist_providers(existing: &[String], add: &[String]) -> Vec<String> {
    let mut out = existing.to_vec();
    for item in add {
        let trimmed = item.trim();
        if trimmed.is_empty() {
            continue;
        }
        let lower = trimmed.to_lowercase();
        if !out.iter().any(|p| p.trim().to_lowercase() == lower) {
            out.push(trimmed.to_string());
        }
    }
    out
}

/// Синхронизация режима белых списков.
/// Обновляет статус активности телеметрии (traffic::set_whitelist_active).
/// Если включен режим следования за failover (follow_failover) и Whitelist включен,
/// переключает селекторную группу '🛡️ Whitelist' в ядре Mihomo на актуальный узел без перезапуска даэмона.
/// ПОЛНОСТЬЮ ОТВЯЗАНО ОТ GAMING MODE: не мутирует cfg.gaming и не затирает игровые устройства!
/// X-01: Единый критерий активности режима белых списков.
/// Раньше GET /api/status и sync_whitelist_mode_tunnel использовали РАЗНЫЕ критерии,
/// из-за чего флаг WHITELIST_ACTIVE флаппировал и канал на графике прыгал БС↔VPS.
pub fn compute_whitelist_mode_active(
    cfg: &crate::config::AppConfig,
    active_id: &str,
    active_name: &str,
    active_provider: Option<&str>,
) -> bool {
    if cfg.whitelist.enabled {
        return true;
    }
    crate::failover::is_whitelist_server_full(active_name, active_id, active_provider, cfg)
}

pub async fn sync_whitelist_mode_tunnel(state: &AppState, target_server_id: &str) -> Result<(), String> {
    let cfg = state.config.read().await.clone();
    let mut target_name = target_server_id.to_string();
    let mut target_provider: Option<String> = None;
    if !target_server_id.is_empty() {
        if let Ok(servers) = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await {
            if let Some(s) = servers.iter().find(|srv| srv.id == target_server_id) {
                target_name = s.name.clone();
                target_provider = s.provider.clone();
            }
        }
    }
    let prov_ref = target_provider.as_deref().filter(|p| !p.is_empty());
    let is_whitelist = compute_whitelist_mode_active(&cfg, target_server_id, &target_name, prov_ref);
    crate::traffic::set_whitelist_active(is_whitelist);

    if cfg.whitelist.enabled {
        let should_follow = cfg.whitelist.follow_failover
            || cfg.whitelist.target_server.as_deref() == Some("follow_failover")
            || cfg.whitelist.target_server.as_deref().map_or(true, |s| s.trim().is_empty());
        if should_follow {
            crate::failover::sync_whitelist_on_switch(&state.http, &cfg, target_server_id, &target_name).await;
        } else if let Some(ref target) = cfg.whitelist.target_server {
            if !target.trim().is_empty() {
                let _ = mihomo::switch_group_scoped(&state.http, &cfg, crate::routing::WHITELIST_GROUP_NAME, target.trim()).await;
            }
        }
    }

    Ok(())
}


/// POST /api/servers/whitelist — добавить/удалить сервер из белого списка (обход белых списков)
/// Если передано `normal: Some(true)` (или `enabled: Some(false)` при наличии тега/подписки),
/// добавляет сервер в `normal_servers` (и убирает из `whitelist_servers`).
pub async fn set_server_whitelist(
    State(state): State<AppState>,
    Json(req): Json<ServerWhitelistReq>,
) -> Response {
    let (cfg, wl, nl, wl_p) = {
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        if let Some(servers) = req.servers {
            cfg.whitelist_servers = servers;
        } else if let Some(add_servers) = req.add_servers {
            cfg.whitelist_servers = merge_whitelist_servers(&cfg.whitelist_servers, &add_servers);
            for s in &add_servers {
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    cfg.normal_servers.retain(|n| n != trimmed);
                }
            }
        } else if let Some(server_id) = req.server_id {
            if let Some(is_normal) = req.normal {
                if is_normal {
                    cfg.whitelist_servers.retain(|s| s != &server_id);
                    if !cfg.normal_servers.contains(&server_id) {
                        cfg.normal_servers.push(server_id);
                    }
                } else {
                    cfg.normal_servers.retain(|s| s != &server_id);
                }
            } else {
                let enable = req.enabled.unwrap_or(true);
                if enable {
                    cfg.normal_servers.retain(|s| s != &server_id);
                    if !cfg.whitelist_servers.contains(&server_id) {
                        cfg.whitelist_servers.push(server_id);
                    }
                } else {
                    cfg.whitelist_servers.retain(|s| s != &server_id);
                    if !cfg.normal_servers.contains(&server_id) {
                        cfg.normal_servers.push(server_id);
                    }
                }
            }
        }
        if let Err(e) = config::save(&state.config_path, &cfg).await {
            return api_err(format!("Ошибка сохранения белого списка серверов: {e}"));
        }
        let wl = cfg.whitelist_servers.clone();
        let nl = cfg.normal_servers.clone();
        let wl_p = cfg.whitelist_providers.clone();
        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        log_i!("Обновлён список серверов белых списков (всего БС: {}, обычных: {})", wl.len(), nl.len());
        (cfg, wl, nl, wl_p)
    };

    if let Ok(Some(active_srv)) = mihomo::get_active_server(&state.http, &cfg).await {
        failover::handle_whitelist_transition(
            &state,
            &active_srv.id,
            &active_srv.name,
            active_srv.provider.as_deref(),
        )
        .await;
        let _ = sync_whitelist_mode_tunnel(&state, &active_srv.id).await;
    }

    api_ok(json!({ "whitelist_servers": wl, "normal_servers": nl, "whitelist_providers": wl_p }))
}

#[derive(Deserialize)]
pub struct ProviderWhitelistReq {
    #[serde(default)]
    pub provider_id: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub providers: Option<Vec<String>>,
    /// Добавить подписки к существующему списку без перезаписи (баг 98)
    #[serde(default)]
    pub add_providers: Option<Vec<String>>,
}

/// POST /api/whitelist/providers — добавить/удалить подписку из белого списка (обход белых списков)
pub async fn set_provider_whitelist(
    State(state): State<AppState>,
    Json(req): Json<ProviderWhitelistReq>,
) -> Response {
    let (cfg, wl_p, wl_s) = {
        let _cfg_guard = state.config_lock.lock().await;
        let mut cfg = (**state.config.read().await).clone();
        if let Some(providers) = req.providers {
            cfg.whitelist_providers = providers;
        } else if let Some(add_providers) = req.add_providers {
            cfg.whitelist_providers = merge_whitelist_providers(&cfg.whitelist_providers, &add_providers);
        } else if let Some(provider_id) = req.provider_id.or(req.provider) {
            let enable = req.enabled.unwrap_or(true);
            let pid_lower = provider_id.trim().to_lowercase();
            if enable {
                if !cfg.whitelist_providers.iter().any(|p| p.trim().to_lowercase() == pid_lower) {
                    cfg.whitelist_providers.push(provider_id);
                }
            } else {
                cfg.whitelist_providers.retain(|p| p.trim().to_lowercase() != pid_lower);
            }
        }
        if let Err(e) = config::save(&state.config_path, &cfg).await {
            return api_err(format!("Ошибка сохранения белого списка подписок: {e}"));
        }
        let wl_p = cfg.whitelist_providers.clone();
        let wl_s = cfg.whitelist_servers.clone();
        *state.config.write().await = std::sync::Arc::new(cfg.clone());
        log_i!("Обновлён список подписок белых списков (всего: {})", wl_p.len());
        (cfg, wl_p, wl_s)
    };

    if let Ok(Some(active_srv)) = mihomo::get_active_server(&state.http, &cfg).await {
        failover::handle_whitelist_transition(
            &state,
            &active_srv.id,
            &active_srv.name,
            active_srv.provider.as_deref(),
        )
        .await;
        let _ = sync_whitelist_mode_tunnel(&state, &active_srv.id).await;
    }

    api_ok(json!({ "whitelist_providers": wl_p, "whitelist_servers": wl_s }))
}

#[derive(Deserialize)]
pub struct SwitchReq {
    #[serde(alias = "id")]
    pub server_id: String,
}

/// Единый обработчик переключения сервера (из API веб-панели и Telegram-бота).
/// Выполняет полный рабочий процесс:
/// 1. Отмена активного замера скорости
/// 2. Переключение сервера в ядре Mihomo (с инвалидацией кэша)
/// 3. Регистрация ручного переключения для failover-иммунитета
/// 4. Обработка перехода белых списков (handle_whitelist_transition)
/// 5. Обновление и сохранение приоритета в config.json (чтобы фоновый failover не откатывал узел)
/// 6. Запуск быстрого фонового замера пингов нового сервера
pub async fn execute_server_switch_workflow(state: &AppState, server_id: &str) -> Result<String, String> {
    let to_vkturn = server_id == crate::vkturn::PROXY_NAME;
    if to_vkturn {
        let rt = state.vkturn.rt.read().await;
        if !rt.tunnel_ready() || rt.connected_streams == 0 {
            return Err("Туннель ещё не подключился к серверу csqtt — дождитесь подключения перед выбором сервера".into());
        }
    }
    // Ручной выбор пользователя имеет абсолютный приоритет:
    // Немедленно прерываем запущенный Speedtest и отменяем возврат старого узла
    crate::speedtest::cancel_active_speedtest();
    let cfg = state.config.read().await.clone();
    let switched = if to_vkturn {
        // Существующие сессии через рабочий VPS могут завершиться штатно. Новые
        // идут через VK сразу после смены селектора, без массового переподключения.
        mihomo::switch_server_graceful(&state.http, &cfg, server_id).await
    } else {
        mihomo::switch_server(&state.http, &cfg, server_id).await
    };
    match switched {
        Ok(msg) => {
            log_i!("Смена сервера: {} — {}", server_id, msg);
            failover::record_manual_switch(server_id).await;
            let (srv_name, srv_prov) = if to_vkturn {
                (crate::vkturn::PROXY_NAME.to_string(), Some(crate::vkturn::PROVIDER_ID.to_string()))
            } else if let Ok(servers) = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await {
                servers.iter().find(|s| s.id == server_id).map(|s| (s.name.clone(), s.provider.clone())).unwrap_or_else(|| (server_id.to_string(), None))
            } else {
                (server_id.to_string(), None)
            };
            failover::handle_whitelist_transition(state, server_id, &srv_name, srv_prov.as_deref()).await;
            // Если переключен конкретный сервер, обновляем его как основной в соответствующей цепочке:
            // - Ручной выбор обычного VPS: обновляет priority_chain и сохраняет last_good_regular_vps
            // - Ручной выбор сервера [БС]: обновляет только bs_priority_chain, никогда не затирая priority_chain обычных VPS
            if !server_id.is_empty() && server_id != "Fastest" && server_id != "Fallback" {
                let _cfg_guard = state.config_lock.lock().await;
                let mut mut_cfg = (**state.config.read().await).clone();
                let id = server_id.to_string();
                let is_bs = failover::is_whitelist_server_full(&srv_name, server_id, srv_prov.as_deref(), &mut_cfg);

                if is_bs {
                    if let Some(pos) = mut_cfg.failover.bs_priority_chain.iter().position(|s| s == &id) {
                        mut_cfg.failover.bs_priority_chain.remove(pos);
                    }
                    mut_cfg.failover.bs_priority_chain.insert(0, id.clone());
                } else {
                    if let Some(pos) = mut_cfg.failover.priority_chain.iter().position(|s| s == &id) {
                        mut_cfg.failover.priority_chain.remove(pos);
                    }
                    mut_cfg.failover.priority_chain.insert(0, id.clone());
                    mut_cfg.failover.priority_server = id.clone();
                    mut_cfg.failover.last_good_regular_vps = Some(id.clone());
                }

                if let Err(e) = config::save(&state.config_path, &mut_cfg).await {
                    log_e!("Ошибка сохранения приоритета в config.json: {e}");
                } else {
                    *state.config.write().await = std::sync::Arc::new(mut_cfg);
                }
            }
            // PING-AUTO: мгновенный фоновый замер — свежий пинг нового сервера
            // виден в панели уже следующим опросом, без действий пользователя
            let cfg_now = state.config.read().await.clone();
            let http = state.http.clone();
            let selected = server_id.to_string();
            tokio::spawn(async move {
                let _ = mihomo::ping_all_url(&http, &cfg_now, &[selected], 2500, None).await;
            });
            Ok(msg)
        }
        Err(e) => {
            log_e!("Ошибка смены сервера на {}: {}", server_id, e);
            Err(e)
        }
    }
}

/// POST /api/servers/switch
pub async fn switch_server(State(state): State<AppState>, Json(req): Json<SwitchReq>) -> Response {
    match execute_server_switch_workflow(&state, &req.server_id).await {
        Ok(msg) => api_ok(json!({ "message": msg })),
        Err(e) => api_err(e),
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

/// Мгновенный фоновый замер пингов после изменения маршрутизации (fire-and-forget):
/// замер через mihomo delay-тест обновляет историю нод, поэтому уже следующий
/// опрос GET /api/servers (и 3-секундный опрос UI) отдаёт свежие значения
/// без каких-либо действий пользователя в панели (анти-регрессия PING-AUTO).
pub fn spawn_ping_refresh(http: reqwest::Client, cfg: config::AppConfig) {
    let ids = mihomo::collect_auto_ping_ids(&cfg);
    if ids.is_empty() {
        return;
    }
    tokio::spawn(async move {
        let _ = mihomo::ping_all_url(&http, &cfg, &ids, 2500, None).await;
    });
}

/// POST /api/flow/ping — специализированный пинг серверов по Google Flow (https://flow.google.com).
pub async fn ping_flow_servers(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let pings = mihomo::ping_flow_servers(&state.http, &cfg, 3500).await;

    let mut best_server: Option<String> = None;
    let mut min_ping = i64::MAX;

    for (name, &ping) in &pings {
        if ping > 0 && ping < min_ping {
            let country = crate::mihomo::detect_server_country(name);
            let is_blocked = country == "RU" || country == "BY";
            if !is_blocked {
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
    let srv_id = body.server_id.trim();
    if srv_id.is_empty() {
        return api_err("Не указан server_id для Google Flow");
    }
    if ["PROXY", "GLOBAL", "Fastest", "Fallback", "DIRECT", "REJECT"].contains(&srv_id) {
        return api_err(format!("'{srv_id}' является системной группой/селектором, а не конкретным выходным сервером для Google Flow"));
    }

    match mihomo::switch_flow_server(&state, srv_id).await {
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

fn is_excluded_for_flow(
    id: &str,
    name: &str,
    provider: Option<&str>,
    protocol: Option<&str>,
    cfg: &config::AppConfig,
) -> bool {
    if id.is_empty()
        || id == crate::vkturn::PROXY_NAME
        || provider == Some(crate::vkturn::PROVIDER_ID)
        || protocol == Some("VK-CALL")
        || id.starts_with("AUTO-")
        || name.starts_with("AUTO-")
        || [
            "PROXY",
            "GLOBAL",
            "Fastest",
            "Fallback",
            "DIRECT",
            "REJECT",
            "Google AI",
            "YouTube",
            "Discord",
            "Telegram",
            "Steam",
            "Twitch",
            "Torrent",
        ]
        .contains(&id)
    {
        return true;
    }
    crate::failover::is_whitelist_server_full(name, id, provider, cfg)
}

/// POST /api/flow/repair — комплексная починка Google Flow:
/// 1) Находит лучший узел США
/// 2) Переключает маршрут Google AI на этот узел
/// 3) Сбрасывает сокеты и соединения ядра Mihomo
pub async fn repair_flow(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let known_servers = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain)
        .await
        .unwrap_or_default();

    // 1. Поиск лучшего сервера США
    let pings = mihomo::ping_flow_servers(&state.http, &cfg, 3500).await;
    let mut best_server: Option<String> = None;
    let mut min_ping = i64::MAX;

    for (name, &ping) in &pings {
        let srv_meta = known_servers.iter().find(|s| &s.id == name);
        let excluded = srv_meta.map_or_else(
            || is_excluded_for_flow(name, name, None, None, &cfg),
            |s| is_excluded_for_flow(&s.id, &s.name, s.provider.as_deref(), Some(s.protocol.as_str()), &cfg),
        );
        if excluded {
            continue;
        }
        if ping > 0 && ping < min_ping {
            let country = crate::mihomo::detect_server_country(name);
            let is_blocked = country == "RU" || country == "BY";
            if !is_blocked {
                min_ping = ping;
                best_server = Some(name.clone());
            }
        }
    }

    // DIAG-04: Не выбирать отсутствующий узел, проверять реальное наличие в конфигурации
    let target_server = match best_server.or_else(|| {
        cfg.flow_server.as_ref().and_then(|s| {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                return None;
            }
            let srv_meta = known_servers.iter().find(|srv| srv.id == trimmed);
            let excluded = srv_meta.map_or_else(
                || is_excluded_for_flow(trimmed, trimmed, None, None, &cfg),
                |srv| is_excluded_for_flow(&srv.id, &srv.name, srv.provider.as_deref(), Some(srv.protocol.as_str()), &cfg),
            );
            if excluded { None } else { Some(trimmed.to_string()) }
        })
    }) {
        Some(s) => s,
        _ => {
            // Ищем подходящий сервер не из РФ/РБ/БС из mihomo::get_servers, либо первый доступный не-БС узел
            if !known_servers.is_empty() {
                let cand = known_servers.iter().find(|s| {
                    if s.is_pool
                        || is_excluded_for_flow(&s.id, &s.name, s.provider.as_deref(), Some(s.protocol.as_str()), &cfg)
                    {
                        return false;
                    }
                    let country = crate::mihomo::detect_server_country(&s.id);
                    country != "RU" && country != "BY"
                });
                if let Some(srv) = cand {
                    srv.id.clone()
                } else if let Some(first_valid) = known_servers.into_iter().find(|s| {
                    !s.is_pool
                        && !is_excluded_for_flow(&s.id, &s.name, s.provider.as_deref(), Some(s.protocol.as_str()), &cfg)
                }) {
                    first_valid.id
                } else {
                    return api_err("В конфигурации ядра Mihomo не найдено ни одного действительного прокси-сервера для маршрута Google Flow");
                }
            } else {
                return api_err("Не удалось определить доступный прокси-сервер для маршрута Google Flow (Mihomo API недоступен)");
            }
        }
    };

    // 2. Валидация существования группы Flow ДО любой записи на диск (баг 52)
    let proxies_before = match mihomo::get_proxies(&state.http, &cfg).await {
        Ok(p) => p,
        Err(e) => return api_err(format!("Не удалось проверить группы маршрута в ядре Mihomo: {}", e)),
    };
    let group_names: Vec<String> = proxies_before.keys().cloned().collect();
    let plan = match crate::routing::plan_repair_flow(&group_names, &target_server) {
        Ok(p) => p,
        Err(e) => return api_err(e),
    };

    // 3. Обновляем YAML (правила Flow) и flow_server в единой транзакции ConfigTx (баг 52)
    {
        let mut tx = match ConfigTx::begin(&state).await {
            Ok(tx) => tx,
            Err(e) => return api_err(format!("Не удалось начать транзакцию сохранения маршрута Google Flow: {}", e)),
        };
        let yaml = match tx.read_yaml().await {
            Ok(y) => y,
            Err(e) => return api_err(format!("Не удалось прочитать config.yaml для маршрута Google Flow: {}", e)),
        };
        let new_yaml = match crate::routing::apply_flow_rules(&yaml, &plan.target_server, Some(&plan.group_name)) {
            Ok(y) => y,
            Err(e) => return api_err(format!("Не удалось подготовить правила Google Flow: {}", e)),
        };
        let yaml_changed = new_yaml != yaml;
        if yaml_changed {
            if let Err(e) = tx.set_yaml(new_yaml) {
                return api_err(format!("Некорректный YAML после применения правил Google Flow: {}", e));
            }
        }
        tx.config_mut().flow_server = Some(plan.target_server.clone());
        let commit_res = if yaml_changed {
            tx.commit_and_reload().await
        } else {
            tx.commit_without_reload().await
        };
        if let Err(e) = commit_res {
            return api_err(format!("Не удалось сохранить маршрут Google AI: {}", e));
        }
    }

    // 4. Переключение селектора группы Flow и подтверждение в runtime (DIAG-04)
    if let Err(e) = mihomo::switch_flow_server(&state, &plan.target_server).await {
        return api_err(format!("Не удалось переключить маршрут Google AI на сервер '{}': {}", plan.target_server, e));
    }

    let proxies = match mihomo::get_proxies(&state.http, &cfg).await {
        Ok(p) => p,
        Err(e) => return api_err(format!("Ошибка подтверждения маршрута в ядре Mihomo: {}", e)),
    };
    let is_confirmed = proxies.iter().any(|(name, obj)| {
        crate::routing::is_flow_group_name(name)
            && obj.get("now").and_then(|n| n.as_str()) == Some(plan.target_server.as_str())
    });
    if !is_confirmed {
        return api_err(format!("Маршрут Google AI в ядре Mihomo не подтвердил выбор узла '{}' в runtime", plan.target_server));
    }

    // 5. Селективный сброс соединений только для доменов Google Flow (баг 54)
    let _ = mihomo::close_flow_connections(&state.http, &cfg).await;

    api_ok(json!({
        "success": true,
        "flow_server": plan.target_server,
        "verified": true,
        "min_ping": if min_ping == i64::MAX { None } else { Some(min_ping) },
        "message": format!("Маршрут Google Flow подтвержден и переключен на '{}', сокеты сброшены", plan.target_server),
    }))
}

#[derive(Deserialize)]
pub struct FlowAutoSelectReq {
    pub enabled: Option<bool>,
}

/// POST /api/flow/auto-select — включение/выключение автоподбора сервера Google AI и автовыбор лучшего узла
pub async fn auto_select_flow_server(
    State(state): State<AppState>,
    Json(req): Json<FlowAutoSelectReq>,
) -> Response {
    let current_cfg = state.config.read().await.clone();
    let target_enabled = req.enabled.unwrap_or(!current_cfg.flow_auto_select);

    let mut message = if target_enabled {
        "Автоматический подбор сервера Google AI включен".to_string()
    } else {
        "Автоматический подбор сервера Google AI выключен".to_string()
    };
    let mut best_server_id: Option<String> = None;
    let mut chosen_ping: Option<i64> = None;

    if target_enabled {
        let known_servers = mihomo::get_servers(&state.http, &current_cfg, &current_cfg.failover.priority_chain)
            .await
            .unwrap_or_default();
        let pings = mihomo::ping_flow_servers(&state.http, &current_cfg, 3500).await;
        let mut min_ping = i64::MAX;
        let is_excluded_node = |id: &str| -> bool {
            let meta = known_servers.iter().find(|s| s.id == id);
            meta.map_or_else(
                || is_excluded_for_flow(id, id, None, None, &current_cfg),
                |s| is_excluded_for_flow(&s.id, &s.name, s.provider.as_deref(), Some(s.protocol.as_str()), &current_cfg),
            )
        };

        for (name, &ping) in &pings {
            if is_excluded_node(name) {
                continue;
            }
            if ping > 0 && ping < min_ping {
                let country = crate::mihomo::detect_server_country(name);
                let is_blocked = country == "RU" || country == "BY";
                if !is_blocked {
                    min_ping = ping;
                    best_server_id = Some(name.clone());
                }
            }
        }

        if best_server_id.is_none() {
            for (name, &ping) in &pings {
                if is_excluded_node(name) {
                    continue;
                }
                if ping > 0 && ping < min_ping {
                    min_ping = ping;
                    best_server_id = Some(name.clone());
                }
            }
        }

        if let Some(ref target) = best_server_id {
            chosen_ping = Some(min_ping);
            if let Ok(_msg) = mihomo::switch_flow_server(&state, target).await {
                message = format!("Автоматически выбран лучший сервер для Google AI: '{target}' ({min_ping} мс)");
            }
        }
    }

    {
        let _cfg_guard = state.config_lock.lock().await;
        let mut mut_cfg = (**state.config.read().await).clone();
        mut_cfg.flow_auto_select = target_enabled;
        if let Some(ref target) = best_server_id {
            mut_cfg.flow_server = Some(target.clone());
        }
        if let Err(e) = config::save(&state.config_path, &mut_cfg).await {
            return api_err(format!("Ошибка сохранения: {e}"));
        }
        *state.config.write().await = std::sync::Arc::new(mut_cfg);
    }

    api_ok(json!({
        "saved": true,
        "enabled": target_enabled,
        "best_server": best_server_id,
        "ping_ms": chosen_ping,
        "message": message,
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

/// POST /api/failover/whitelist-detector/check — ручной запуск проверки детектора белых списков.
pub async fn whitelist_detector_check(State(state): State<AppState>) -> Response {
    match failover::run_whitelist_detector_check(&state, "Ручной запуск из API").await {
        Ok(msg) => api_ok(json!({ "message": msg })),
        Err(e) => api_err(e),
    }
}

#[derive(Default, Deserialize)]
pub struct DetectServersReq {
    #[serde(default)]
    pub apply: Option<bool>,
}

/// POST /api/failover/whitelist-detector/detect-servers — поиск серверов и провайдеров БС с опциональным применением
pub async fn whitelist_detector_detect_servers(
    State(state): State<AppState>,
    Json(req): Json<DetectServersReq>,
) -> Response {
    let detect_res = match failover::detect_whitelist_servers_and_providers(&state).await {
        Ok(r) => r,
        Err(e) => return api_err(format!("Ошибка анализа серверов белых списков: {e}")),
    };

    if req.apply.unwrap_or(false) {
        let cfg = {
            let _cfg_guard = state.config_lock.lock().await;
            let mut cfg = (**state.config.read().await).clone();

            // 1. Добавляем обнаруженные BS провайдеры в whitelist_providers (дедупликация)
            for wp in &detect_res.detected_bs_providers {
                let wp_lower = wp.trim().to_lowercase();
                if !cfg.whitelist_providers.iter().any(|p| p.trim().to_lowercase() == wp_lower) {
                    cfg.whitelist_providers.push(wp.clone());
                }
            }

            // 2. Добавляем обнаруженные узлы БС в whitelist_servers (дедупликация)
            for ws in &detect_res.detected_bs_servers {
                if !cfg.whitelist_servers.iter().any(|s| s == ws) {
                    cfg.whitelist_servers.push(ws.clone());
                }
            }

            // 3. Если monitored_provider пуст, заполняем suggested_monitored_provider
            if cfg.failover.whitelist_detector.monitored_provider.trim().is_empty() {
                if let Some(ref sug) = detect_res.suggested_monitored_provider {
                    cfg.failover.whitelist_detector.monitored_provider = sug.clone();
                }
            }

            // 4. Если target_bs_provider пуст, заполняем suggested_target_bs_provider
            if cfg.failover.whitelist_detector.target_bs_provider.trim().is_empty() {
                if let Some(ref sug) = detect_res.suggested_target_bs_provider {
                    cfg.failover.whitelist_detector.target_bs_provider = sug.clone();
                }
            }

            if let Err(e) = config::save(&state.config_path, &cfg).await {
                return api_err(format!("Ошибка сохранения конфигурации при автонастройке БС: {e}"));
            }

            *state.config.write().await = std::sync::Arc::new(cfg.clone());
            log_i!("[DETECTOR] Автоматически применены настройки обнаруженных БС узлов и провайдеров");
            cfg
        };

        if let Ok(Some(active_srv)) = mihomo::get_active_server(&state.http, &cfg).await {
            failover::handle_whitelist_transition(
                &state,
                &active_srv.id,
                &active_srv.name,
                active_srv.provider.as_deref(),
            )
            .await;
            let _ = sync_whitelist_mode_tunnel(&state, &active_srv.id).await;
        }
    }

    api_ok(json!(detect_res))
}

/// GET /api/failover/whitelist-detector/status — статус детектора белых списков.
pub async fn whitelist_detector_status() -> Response {
    let status = failover::get_whitelist_detector_status().await;
    api_ok(json!(status))
}

/// GET /api/failover/events — лента событий.
pub async fn failover_events(State(state): State<AppState>) -> Response {
    let events = state.failover_log.snapshot().await;
    api_ok(json!({ "events": events }))
}

#[derive(Default, Deserialize)]
pub struct QuarantineClearReq {
    pub server_id: Option<String>,
}

/// GET /api/failover/quarantine — список серверов в карантине (анти-флеппинг).
pub async fn failover_quarantine_list() -> Response {
    let list = failover::quarantine_status().await;
    api_ok(json!({ "quarantine": list }))
}

/// POST /api/failover/quarantine/clear — снятие карантина для одного сервера или всех.
pub async fn failover_quarantine_clear(Json(req): Json<QuarantineClearReq>) -> Response {
    if let Some(id) = req.server_id {
        let clean = id.trim();
        if !clean.is_empty() {
            failover::clear_quarantine(clean).await;
            log_i!("Карантин сервера '{}' снят вручную", clean);
            return api_ok(json!({ "message": format!("Карантин сервера '{clean}' снят") }));
        }
    }
    failover::clear_all_quarantine().await;
    log_i!("Карантин всех серверов сброшен вручную");
    api_ok(json!({ "message": "Карантин всех серверов сброшен" }))
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
    drop(_cfg_guard);
    reapply_canary_routing(&state).await;
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
    *state.config.write().await = std::sync::Arc::new(cfg.clone());
    failover::clear_all_quarantine().await;
    // PING-AUTO: после правки цепочки приоритета пинги обновляются сразу,
    // а не при следующем ручном действии в панели
    spawn_ping_refresh(state.http.clone(), cfg);
    api_ok(json!({ "saved": true, "message": message }))
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
    // Актуализируем активного провайдера для фонового учета дельты
    if let Ok(Some(act_srv)) = mihomo::get_active_server(&state.http, &cfg).await {
        crate::traffic::set_active_provider_id(act_srv.provider);
    }
    match mihomo::get_providers_info(&state.http, &cfg).await {
        Ok(providers) => {
            let list: Vec<serde_json::Value> = providers
                .into_iter()
                .map(|p| {
                    let is_wl = crate::failover::is_whitelist_server_full(&p.name, &p.id, Some(&p.id), &cfg);
                    let live_delta = crate::traffic::get_provider_live_delta(&p.id);

                    // Если есть квота (total_bytes), вычисляем actual used (baseline + live_delta) и accurate remaining
                    // Строго официальный отчёт провайдера (Geodema) без двойного начисления локальной дельты
                    let official_used = p.upload_bytes.unwrap_or(0).saturating_add(p.download_bytes.unwrap_or(0));
                    let remaining_bytes = p.total_bytes.map(|tot| tot.saturating_sub(official_used));

                    let mut val = serde_json::to_value(&p).unwrap_or_default();
                    if let Some(obj) = val.as_object_mut() {
                        obj.insert("is_whitelist".to_string(), serde_json::Value::Bool(is_wl));
                        obj.insert("live_delta_bytes".to_string(), json!(live_delta));
                        obj.insert("actual_used_bytes".to_string(), json!(official_used));
                        obj.insert("remaining_bytes".to_string(), json!(remaining_bytes));
                    }
                    val
                })
                .collect();
            api_ok(json!({
                "providers": list,
                "whitelist_providers": cfg.whitelist_providers,
            }))
        }
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
pub struct ToggleProviderReq {
    #[serde(alias = "id")]
    pub provider_id: String,
    pub enabled: Option<bool>,
}

/// POST /api/providers/toggle — быстрое включение/отключение подписки
pub async fn toggle_provider(State(state): State<AppState>, Json(req): Json<ToggleProviderReq>) -> Response {
    let pid = req.provider_id.trim().to_string();
    if pid.is_empty() {
        return api_err("ID подписки не может быть пустым");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка захвата транзакции: {e}")),
    };

    let is_currently_disabled = tx.config().disabled_providers.iter().any(|p| p.eq_ignore_ascii_case(&pid));
    let target_enabled = req.enabled.unwrap_or(is_currently_disabled);

    if target_enabled {
        tx.config_mut().disabled_providers.retain(|p| !p.eq_ignore_ascii_case(&pid));
    } else if !is_currently_disabled {
        tx.config_mut().disabled_providers.push(pid.clone());
    }

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let synced_yaml = match crate::routing::sync_disabled_providers_in_yaml(&yaml, &tx.config().disabled_providers) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (routed_yaml, _) = match crate::routing::apply_routing(&synced_yaml, tx.config()) {
        Ok(r) => r,
        Err(e) => return api_err(e),
    };

    if let Err(e) = tx.set_yaml(routed_yaml) {
        return api_err(format!("Ошибка валидации YAML: {e}"));
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка сохранения и перезагрузки: {e}"));
    }

    let disabled_list = state.config.read().await.disabled_providers.clone();
    let label = if target_enabled {
        format!("Подписка '{pid}' включена")
    } else {
        format!("Подписка '{pid}' отключена")
    };
    api_ok(json!({
        "saved": true,
        "message": label,
        "provider_id": pid,
        "enabled": target_enabled,
        "disabled_providers": disabled_list,
    }))
}

#[derive(Deserialize)]
pub struct UpdateProviderReq {
    #[serde(alias = "id")]
    pub provider_id: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderKind {
    Http,
    MobileFile,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProviderUpdateAction {
    KeepHttp,
    SaveConvertedFile,
    Reject(String),
}

/// Решение о действии при ручном обновлении подписки (баг A-13):
/// обычный HTTP-провайдер никогда не переводится неявно в `type: file`.
pub fn provider_update_action(
    provider_kind: ProviderKind,
    body_kind: SubscriptionBodyKind,
) -> ProviderUpdateAction {
    match (provider_kind, body_kind) {
        (ProviderKind::MobileFile, SubscriptionBodyKind::Base64OrUri) => {
            ProviderUpdateAction::SaveConvertedFile
        }
        (ProviderKind::MobileFile, SubscriptionBodyKind::NativeYaml) => {
            ProviderUpdateAction::Reject(
                "Мобильная подписка вернула YAML-файл вместо списка серверов".to_string(),
            )
        }
        (ProviderKind::Http, SubscriptionBodyKind::NativeYaml)
        | (ProviderKind::Http, SubscriptionBodyKind::Base64OrUri) => {
            ProviderUpdateAction::KeepHttp
        }
        (_, SubscriptionBodyKind::Unknown) => ProviderUpdateAction::Reject(
            "Подписка вернула пустой или непонятный ответ — обновление отклонено".to_string(),
        ),
    }
}

/// POST /api/providers/update — принудительное обновление подписки/подписок
pub async fn update_provider(State(state): State<AppState>, Json(req): Json<UpdateProviderReq>) -> Response {
    let cfg = state.config.read().await.clone();

    let yaml_content = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let provider_hwids = crate::routing::parse_provider_hwids(&yaml_content);

    if let Some(pid) = req.provider_id.filter(|p| !p.trim().is_empty()) {
        let hwid = provider_hwids.get(&pid).map(|s| s.as_str());
        // 1. Мобильные подписки (зарегистрированные пользователем в mobile_provider_urls)
        if let Some(mobile_url) = cfg.mobile_provider_urls.get(&pid) {
            match provider_update_action(ProviderKind::MobileFile, SubscriptionBodyKind::Base64OrUri) {
                ProviderUpdateAction::SaveConvertedFile => {
                    match refresh_mobile_sub(&state, &pid, mobile_url, hwid).await {
                        Ok(count) => {
                            let _ = mihomo::force_update_provider(&state.http, &cfg, &pid).await;
                            return api_ok(json!({ "updated": 1, "message": format!("Мобильная подписка '{pid}' обновлена ({count} серверов)") }));
                        }
                        Err(e) => return api_err(format!("Ошибка обновления мобильной подписки '{pid}': {e}")),
                    }
                }
                ProviderUpdateAction::Reject(reason) => return api_err(reason),
                ProviderUpdateAction::KeepHttp => {}
            }
        }

        // 2. Обычный HTTP-провайдер остаётся type: http и обновляется штатно через Mihomo (баг A-13)
        match mihomo::force_update_provider(&state.http, &cfg, &pid).await {
            Ok(_) => api_ok(json!({ "updated": 1, "message": format!("Подписка '{pid}' обновлена") })),
            Err(e) => api_err(format!("Ошибка обновления подписки '{pid}': {e}")),
        }
    } else {
        let mobile_count = sync_all_mobile_providers(&state).await;
        let count = mihomo::force_update_all_providers(&state.http, &cfg).await;
        api_ok(json!({ "updated": count + mobile_count, "message": format!("Обновлено подписок: {}", count + mobile_count) }))
    }
}

/// Разделение вывода curl -i на HTTP-заголовки (включая мультиблочные заголовки CONNECT-прокси) и тело
pub fn split_curl_response(raw: &str) -> (Option<crate::mihomo::SubscriptionUserinfo>, String) {
    let mut curr = raw;
    let mut userinfo = None;

    // Цикл пропускает все блоки HTTP-заголовков (включая HTTP/1.1 200 Connection established от прокси и редиректы)
    while !curr.is_empty() {
        let trimmed_leading = curr.trim_start_matches(|c| c == '\r' || c == '\n');
        let lower = trimmed_leading.to_ascii_lowercase();
        if !lower.starts_with("http/") {
            break;
        }

        // Ищем конец текущего блока заголовков
        let sep_info = ["\r\n\r\n", "\n\n", "\r\r"]
            .iter()
            .filter_map(|sep| trimmed_leading.find(sep).map(|pos| (pos, sep.len())))
            .min_by_key(|&(pos, _)| pos);

        if let Some((pos, sep_len)) = sep_info {
            let header_block = &trimmed_leading[..pos];
            if userinfo.is_none() {
                userinfo = header_block
                    .lines()
                    .find(|l| l.to_ascii_lowercase().starts_with("subscription-userinfo:"))
                    .and_then(|l| l.split_once(':').map(|(_, v)| v.trim()))
                    .and_then(crate::mihomo::parse_subscription_userinfo_str);
            }
            curr = &trimmed_leading[pos + sep_len..];
        } else {
            break;
        }
    }

    // Если всё ещё остался префикс до структуры тела (например, в случае нестандартных прокси)
    let body_str = if let Some(bracket_pos) = curr.find("\n[") {
        &curr[bracket_pos + 1..]
    } else if let Some(brace_pos) = curr.find("\n{") {
        &curr[brace_pos + 1..]
    } else if let Some(proxies_pos) = curr.find("\nproxies:") {
        &curr[proxies_pos + 1..]
    } else {
        curr
    };

    if userinfo.is_none() {
        userinfo = raw.lines()
            .find(|l| l.to_ascii_lowercase().starts_with("subscription-userinfo:"))
            .and_then(|l| l.split_once(':').map(|(_, v)| v.trim()))
            .and_then(crate::mihomo::parse_subscription_userinfo_str);
    }

    (userinfo, body_str.trim().to_string())
}

fn curl_escape_value(v: &str) -> String {
    v.replace(['\r', '\n'], "")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Аргументы запуска curl для загрузки мобильной подписки без утечки URL/HWID в argv (`ps`).
pub fn build_curl_args() -> Vec<String> {
    vec![
        "-s".to_string(),
        "-i".to_string(),
        "-f".to_string(),
        "-L".to_string(),
        "--max-redirs".to_string(),
        "5".to_string(),
        "--max-time".to_string(),
        "15".to_string(),
        "-K".to_string(),
        "-".to_string(),
    ]
}

/// Конфиг для `curl -K -` (передаётся через stdin, скрывая URL, User-Agent и x-hwid от `ps`).
pub fn build_curl_stdin_config(url: &str, proxy_url: &str, ua: &str, hwid: Option<&str>) -> String {
    let mut lines = vec![
        format!("url = \"{}\"", curl_escape_value(url)),
        format!("proxy = \"{}\"", curl_escape_value(proxy_url)),
        format!("header = \"User-Agent: {}\"", curl_escape_value(ua)),
    ];
    if let Some(h) = hwid.filter(|s| !s.is_empty()) {
        lines.push(format!("header = \"x-hwid: {}\"", curl_escape_value(h)));
    }
    lines.join("\n")
}

/// Атомарная запись сконвертированного файла мобильного провайдера (tmp + rename).
pub async fn write_mobile_provider_yaml_atomic(
    prov_path: &std::path::Path,
    conv_yaml: &str,
) -> Result<(), String> {
    crate::api::atomic_write_file(prov_path, conv_yaml)
        .await
        .map_err(|e| format!("Запись {}: {e}", prov_path.display()))
}

/// Загрузка мобильной подписки (V2Ray/JSON/Base64) через прокси Mihomo и сохранение в ./providers/{id}.yaml
pub async fn refresh_mobile_sub(state: &AppState, id: &str, url: &str, hwid: Option<&str>) -> Result<usize, String> {
    let ua = "v2rayNG/1.8.12";
    let (proxy_url, resolved_hwid) = {
        let cfg = state.config.read().await;
        let h = hwid
            .filter(|h| !h.is_empty())
            .map(|s| s.to_string())
            .or_else(|| cfg.mobile_provider_hwids.get(id).cloned());
        (cfg.mihomo_proxy_url(), h)
    };
    let proxied = crate::updater::proxied_client(&proxy_url);
    let client_ref = proxied.as_ref().unwrap_or(&state.http);
    let mut req_builder = client_ref.get(url).header("User-Agent", ua).timeout(std::time::Duration::from_secs(12));
    if let Some(ref h) = resolved_hwid {
        req_builder = req_builder.header("x-hwid", h.as_str());
    }
    // 1. Сначала пробуем получить подписку через системный curl роутера (/opt/bin/curl или curl)
    // (curl на роутере идеально согласует TLS/ALPN с прокси Mihomo и сервером QRATOR)
    let curl_bin = if std::path::Path::new("/opt/bin/curl").exists() {
        "/opt/bin/curl"
    } else {
        "curl"
    };
    let mut cmd = tokio::process::Command::new(curl_bin);
    cmd.args(build_curl_args())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let stdin_cfg = build_curl_stdin_config(url, &proxy_url, ua, resolved_hwid.as_deref());
    let curl_output = match cmd.spawn() {
        Ok(mut child) => {
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                let _ = stdin.write_all(stdin_cfg.as_bytes()).await;
                let _ = stdin.shutdown().await;
            }
            child.wait_with_output().await
        }
        Err(e) => Err(e),
    };

    let (mut header_userinfo, text) = if let Ok(out) = curl_output {
        if out.status.success() && !out.stdout.is_empty() {
            let full_str = String::from_utf8_lossy(&out.stdout).to_string();
            split_curl_response(&full_str)
        } else {
            (None, String::new())
        }
    } else {
        (None, String::new())
    };

    let text = if text.is_empty() {
        let resp = match req_builder.send().await {
            Ok(r) if r.status().is_success() => r,
            _ => {
                let mut direct_builder = state.http.get(url).header("User-Agent", ua).timeout(std::time::Duration::from_secs(12));
                if let Some(ref h) = resolved_hwid {
                    direct_builder = direct_builder.header("x-hwid", h.as_str());
                }
                direct_builder.send().await.map_err(|e| format!("HTTP {e}"))?
            }
        };
        if !resp.status().is_success() {
            return Err(format!("HTTP статус {}", resp.status()));
        }
        let headers = resp.headers();
        if header_userinfo.is_none() {
            header_userinfo = headers
                .iter()
                .find(|(k, _)| k.as_str().eq_ignore_ascii_case("subscription-userinfo"))
                .and_then(|(_, v)| v.to_str().ok())
                .and_then(crate::mihomo::parse_subscription_userinfo_str);
        }
        let bytes = match resp.bytes().await {
            Ok(b) => b,
            Err(e) => return Err(format!("Read bytes: {e}")),
        };
        String::from_utf8_lossy(&bytes).to_string()
    } else {
        text
    };

    // Если основной запрос (например, с UA v2rayNG) не вернул заголовок Subscription-Userinfo,
    // делаем лёгкий запрос (HEAD или GET с Range: bytes=0-0) с ClashMeta UA для гарантированного получения квоты
    if header_userinfo.is_none() {
        let clash_ua = "ClashMeta/1.19.24";
        let mut meta_builder = client_ref.get(url).header("User-Agent", clash_ua).header("Range", "bytes=0-0").timeout(std::time::Duration::from_secs(6));
        if let Some(ref h) = resolved_hwid {
            meta_builder = meta_builder.header("x-hwid", h.as_str());
        }
        if let Ok(meta_resp) = meta_builder.send().await {
            let u = meta_resp.headers()
                .iter()
                .find(|(k, _)| k.as_str().eq_ignore_ascii_case("subscription-userinfo"))
                .and_then(|(_, v)| v.to_str().ok())
                .and_then(crate::mihomo::parse_subscription_userinfo_str);
            if u.is_some() {
                header_userinfo = u;
            }
        }
    }
    let (mut conv_yaml, count) = crate::subscription_parser::convert_subscription_to_mihomo_yaml(&text)?;
    let prov_dir = std::path::Path::new(&state.config.read().await.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));
    let prov_path = prov_dir.join(format!("{id}.yaml"));
    if let Some(parent) = prov_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    // Preservation and injection of Subscription-Userinfo
    let final_userinfo = if let Some(u) = header_userinfo {
        crate::traffic::reset_provider_live_delta(id);
        Some(u)
    } else if let Some(u) = crate::mihomo::extract_subscription_userinfo_from_body(&text) {
        crate::traffic::reset_provider_live_delta(id);
        Some(u)
    } else if let Ok(existing_yaml) = tokio::fs::read_to_string(&prov_path).await {
        crate::mihomo::extract_subscription_userinfo_from_body(&existing_yaml)
    } else {
        None
    };

    if let Some(ref u) = final_userinfo {
        conv_yaml = crate::mihomo::inject_or_replace_userinfo_in_yaml(&conv_yaml, u);
    }

    write_mobile_provider_yaml_atomic(&prov_path, &conv_yaml).await?;
    Ok(count)
}

/// Фоновое автообновление и автомиграция всех мобильных подписок:
/// Проверяет зарегистрированные mobile_provider_urls, а также автоматически мигрирует
/// любые HTTP-провайдеры, отдающие V2Ray/JSON подписки, в локальный type: file.
/// Фоновое обновление мобильных подписок (баг 80): только те, что пользователь сам перевёл в мобильный
/// формат (mobile_provider_urls). Обычные http-провайдеры Mihomo обновляет сам, их тип не меняется.
pub async fn sync_all_mobile_providers(state: &AppState) -> usize {
    let cfg = state.config.read().await.clone();
    let yaml_content = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let provider_hwids = crate::routing::parse_provider_hwids(&yaml_content);

    let mut mobile_count = 0;
    for (pid, mobile_url) in &cfg.mobile_provider_urls {
        let hwid = provider_hwids
            .get(pid)
            .map(|s| s.as_str())
            .or_else(|| cfg.mobile_provider_hwids.get(pid).map(|s| s.as_str()));
        if let Ok(count) = refresh_mobile_sub(state, pid, mobile_url, hwid).await {
            let _ = mihomo::force_update_provider(&state.http, &cfg, pid).await;
            log_i!("[MOBILE-SUB] ✓ Мобильная подписка '{pid}' обновлена ({count} серверов)");
            mobile_count += 1;
        }
    }
    mobile_count
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionBodyKind {
    NativeYaml,
    Base64OrUri,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProviderPath {
    NativeHttp,
    MobileFile,
}

/// Нейтральный User-Agent по умолчанию для обычных подписок (`clash.meta`/`mihomo`),
/// `v2rayNG` используется только при явном выборе пользователя (баг 81).
pub fn default_provider_user_agent(explicit_ua: Option<&str>) -> &str {
    explicit_ua
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("clash.meta/mihomo")
}

/// Определение формата тела подписки (нативный Clash/Mihomo YAML vs Base64/URI-список).
pub fn detect_subscription_body_kind(body: &str) -> SubscriptionBodyKind {
    let trimmed = body.trim();
    if trimmed.is_empty() {
        return SubscriptionBodyKind::Unknown;
    }
    if crate::subscription_parser::is_v2ray_or_base64_subscription(trimmed) {
        return SubscriptionBodyKind::Base64OrUri;
    }
    if trimmed.lines().any(|l| {
        let lt = l.trim_start();
        lt.starts_with("proxies:") || lt.starts_with("proxy-groups:")
    }) {
        return SubscriptionBodyKind::NativeYaml;
    }
    SubscriptionBodyKind::Unknown
}

/// Выбор пути добавления провайдера по явному мобильному UA и типу тела подписки (баг 81).
pub fn choose_provider_path(explicit_mobile_ua: bool, body_kind: SubscriptionBodyKind) -> ProviderPath {
    match body_kind {
        SubscriptionBodyKind::NativeYaml => ProviderPath::NativeHttp,
        SubscriptionBodyKind::Base64OrUri => ProviderPath::MobileFile,
        SubscriptionBodyKind::Unknown => {
            if explicit_mobile_ua {
                ProviderPath::MobileFile
            } else {
                ProviderPath::NativeHttp
            }
        }
    }
}

/// POST /api/providers/add — добавление новой подписки в config.yaml
pub async fn add_provider(State(state): State<AppState>, Json(req): Json<AddProviderReq>) -> Response {
    let id = req.id.trim();
    let raw_url = req.url.trim();
    if id.is_empty() || raw_url.is_empty() {
        return api_err("ID и URL подписки не могут быть пустыми");
    }
    if !crate::config::is_valid_provider_id(id) {
        return api_err("Некорректный ID подписки: разрешены только символы [a-zA-Z0-9_-], длина до 64 символов");
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

    // Проверяем, является ли подписка мобильной V2Ray/Base64: только по явному мобильному UA или по факту формата тела (баг 81)
    let mut mobile_yaml_opt: Option<(String, usize)> = None;
    let is_explicit_mobile_ua = req.user_agent.as_deref().map_or(false, |u| {
        let l = u.to_lowercase();
        l.contains("v2ray") || l.contains("happ") || l.contains("singbox") || l.contains("sing-box")
    });

    if final_url.starts_with("http://") || final_url.starts_with("https://") {
        let ua = default_provider_user_agent(req.user_agent.as_deref());
        let proxy_url = {
            let cfg = state.config.read().await;
            cfg.mihomo_proxy_url()
        };
        let proxied = crate::updater::proxied_client(&proxy_url);
        let client_ref = proxied.as_ref().unwrap_or(&state.http);
        let mut req_builder = client_ref.get(&final_url).header("User-Agent", ua);
        if let Some(h) = hwid_trimmed {
            req_builder = req_builder.header("x-hwid", h);
        }
        let resp = match req_builder.send().await {
            Ok(r) if r.status().is_success() => Ok(r),
            _ => {
                let mut direct_builder = state.http.get(&final_url).header("User-Agent", ua);
                if let Some(h) = hwid_trimmed {
                    direct_builder = direct_builder.header("x-hwid", h);
                }
                direct_builder.send().await
            }
        };
        if let Ok(resp) = resp {
            if resp.status().is_success() {
                let userinfo_hdr = resp
                    .headers()
                    .iter()
                    .find(|(k, _)| k.as_str().eq_ignore_ascii_case("subscription-userinfo"))
                    .and_then(|(_, v)| v.to_str().ok())
                    .and_then(crate::mihomo::parse_subscription_userinfo_str);
                if let Ok(text) = resp.text().await {
                    let body_kind = detect_subscription_body_kind(&text);
                    if choose_provider_path(is_explicit_mobile_ua, body_kind) == ProviderPath::MobileFile
                        && body_kind == SubscriptionBodyKind::Base64OrUri
                    {
                        if let Ok((mut conv_yaml, count)) = crate::subscription_parser::convert_subscription_to_mihomo_yaml(&text) {
                            let parsed_ui = userinfo_hdr.or_else(|| crate::mihomo::extract_subscription_userinfo_from_body(&text));
                            if let Some(ref u) = parsed_ui {
                                conv_yaml = crate::mihomo::inject_or_replace_userinfo_in_yaml(&conv_yaml, u);
                            }
                            mobile_yaml_opt = Some((conv_yaml, count));
                        }
                    }
                }
            }
        }
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка захвата транзакции: {e}")),
    };

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (new_yaml, parsed_mobile_count) = if let Some((conv_yaml, count)) = mobile_yaml_opt {
        let providers_dir = std::path::Path::new(&tx.config().mihomo.config_path)
            .parent()
            .map(|p| p.join("providers"))
            .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));
        let prov_file = providers_dir.join(format!("{id}.yaml"));
        if let Err(e) = tx.set_extra_file(&prov_file, conv_yaml, true).await {
            return api_err(format!("Ошибка подготовки файла провайдера: {e}"));
        }
        let ny = match crate::routing::add_file_provider_to_yaml(
            &yaml,
            id,
            &format!("./providers/{id}.yaml"),
            Some(tx.config().health_check_url()),
            Some(tx.config().mihomo.health_check_interval),
        ) {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };
        tx.config_mut().mobile_provider_urls.insert(id.to_string(), final_url.clone());
        if let Some(h) = hwid_trimmed {
            tx.config_mut().mobile_provider_hwids.insert(id.to_string(), h.to_string());
        }
        (ny, Some(count))
    } else {
        let ny = match crate::routing::add_provider_to_yaml_full(
            &yaml,
            id,
            &final_url,
            Some(tx.config().health_check_url()),
            Some(tx.config().mihomo.health_check_interval),
            hwid_trimmed,
            req.user_agent.as_deref(),
        ) {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };
        (ny, None)
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(format!("Ошибка валидации YAML: {e}"));
    }

    // Если указано пользовательское имя — сохраняем псевдоним
    if let Some(alias) = req.name.filter(|n| !n.trim().is_empty()) {
        tx.config_mut().provider_aliases.insert(id.to_string(), alias.trim().to_string());
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка сохранения и применения подписки: {e}"));
    }

    let cfg = state.config.read().await.clone();
    let _ = mihomo::force_update_provider(&state.http, &cfg, id).await;

    let msg = if let Some(count) = parsed_mobile_count {
        format!("Мобильная подписка '{id}' (V2Ray / Base64) успешно добавлена ({count} серверов с именами как на телефоне)")
    } else {
        format!("Подписка '{id}' успешно добавлена")
    };

    api_ok(json!({ "added": true, "message": msg }))
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
    if !crate::config::is_valid_provider_id(id) {
        return api_err("Некорректный ID подписки: разрешены только символы [a-zA-Z0-9_-], длина до 64 символов");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка захвата транзакции: {e}")),
    };

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let new_yaml = match crate::routing::delete_provider_from_yaml(&yaml, id) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(format!("Ошибка валидации YAML: {e}"));
    }

    // Удаляем псевдоним и фильтры из AppConfig
    tx.config_mut().provider_aliases.remove(id);
    tx.config_mut().provider_filters.remove(id);
    tx.config_mut().mobile_provider_urls.remove(id);
    tx.config_mut().mobile_provider_hwids.remove(id);

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка сохранения config.yaml: {e}"));
    }

    // Удаляем кэш-файлы провайдера с диска (если создавались)
    let prov_cache_yaml = format!("/opt/etc/mihomo/providers/{id}.yaml");
    let prov_cache_json = format!("/opt/etc/mihomo/providers/{id}.json");
    let _ = tokio::fs::remove_file(&prov_cache_yaml).await;
    let _ = tokio::fs::remove_file(&prov_cache_json).await;

    api_ok(json!({ "deleted": true, "message": format!("Подписка '{id}' успешно удалена") }))
}

#[derive(Deserialize)]
pub struct UpdateProviderHwidReq {
    pub id: String,
    pub hwid: Option<String>,
}

/// POST /api/providers/hwid — обновление или удаление HWID подписки в config.yaml
pub async fn update_provider_hwid(
    State(state): State<AppState>,
    Json(req): Json<UpdateProviderHwidReq>,
) -> Response {
    let id = req.id.trim();
    if id.is_empty() {
        return api_err("ID подписки не может быть пустым");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка захвата транзакции: {e}")),
    };

    let yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let new_yaml = match crate::routing::update_provider_hwid_in_yaml(&yaml, id, req.hwid.as_deref()) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(format!("Ошибка валидации YAML: {e}"));
    }

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(format!("Ошибка сохранения и применения HWID: {e}"));
    }

    let cfg = state.config.read().await.clone();
    let _ = mihomo::force_update_provider(&state.http, &cfg, id).await;

    let msg = match req.hwid.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
        Some(h) => format!("HWID для подписки '{id}' успешно обновлён на '{h}'"),
        None => format!("HWID для подписки '{id}' успешно удалён"),
    };

    api_ok(json!({ "updated": true, "message": msg, "id": id, "hwid": req.hwid }))
}


// ==================== ЗАМЕР СКОРОСТИ (SPEEDTEST) =============

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


// ==================== ИМПОРТ НОД (OUTBOUND GENERATOR) =============

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
        let prov_name = prov_name.trim();
        if !crate::config::is_valid_provider_id(prov_name) {
            return api_err("Некорректное имя провайдера: разрешены только символы [a-zA-Z0-9_-], длина до 64 символов");
        }
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
        if current_content.contains("proxies: []") {
            current_content = current_content.replace("proxies: []", "proxies:");
        } else if !current_content.contains("proxies:") {
            current_content = format!("proxies:\n{}", current_content);
        }
        current_content.push('\n');
        current_content.push_str(yaml);
        current_content.push('\n');

        if let Err(e) = tx.set_extra_file(&path, current_content, true).await {
            return api_err(e);
        }

        let config_yaml = match tx.read_yaml().await {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };
        let existing_provs = crate::routing::parse_provider_names(&config_yaml);
        if !existing_provs.iter().any(|p| p.eq_ignore_ascii_case(prov_name)) {
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
    let proxies_idx = if let Some(idx) = lines.iter().position(|l| l.trim_end() == "proxies:") {
        idx
    } else if let Some(idx) = lines.iter().position(|l| l.trim() == "proxies: []") {
        lines[idx] = "proxies:".to_string();
        idx
    } else {
        lines.insert(0, "proxies:".to_string());
        0
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn regression_bug_82b_refresh_mobile_sub_curl_args_and_atomic_write() {
        let args = build_curl_args();
        assert!(args.iter().any(|a| a == "-f"), "curl must fail on HTTP errors (-f)");
        assert!(args.iter().any(|a| a == "-L"), "curl must follow redirects (-L)");
        assert!(args.iter().any(|a| a == "--max-redirs"), "curl must cap redirects");
        assert!(args.windows(2).any(|w| w == ["-K", "-"]), "curl must read config from stdin (-K -)");
        let joined = args.join(" ");
        assert!(!joined.contains("x-hwid"), "x-hwid must not appear in argv");
        assert!(!joined.contains("https://"), "URL must not appear in argv");

        let cfg = build_curl_stdin_config(
            "https://sub.example.com/get?token=abc",
            "http://127.0.0.1:7890",
            "v2rayNG/1.8.12",
            Some("SECRET-HWID-123"),
        );
        assert!(cfg.contains("url = \"https://sub.example.com/get?token=abc\""));
        assert!(cfg.contains("proxy = \"http://127.0.0.1:7890\""));
        assert!(cfg.contains("header = \"User-Agent: v2rayNG/1.8.12\""));
        assert!(cfg.contains("header = \"x-hwid: SECRET-HWID-123\""));

        // Атомарная запись провайдера не оставляет .tmp и пишет итоговый YAML
        let dir = std::env::temp_dir().join(format!("xkeen_bug82_{}", std::process::id()));
        let _ = tokio::fs::create_dir_all(&dir).await;
        let target = dir.join("sub1.yaml");
        write_mobile_provider_yaml_atomic(&target, "proxies: []\n").await.unwrap();
        let read_back = tokio::fs::read_to_string(&target).await.unwrap();
        assert_eq!(read_back, "proxies: []\n");
        let _ = tokio::fs::remove_dir_all(&dir).await;
    }

    #[test]
    fn regression_bug_98b_add_servers_merges_not_replaces() {
        let existing = vec!["srv-a".to_string()];
        let add = vec!["srv-b".to_string(), "srv-a".to_string(), "  ".to_string()];
        let merged = merge_whitelist_servers(&existing, &add);
        assert_eq!(merged, vec!["srv-a".to_string(), "srv-b".to_string()]);

        let prov_existing = vec!["Sub1".to_string()];
        let prov_add = vec!["sub1".to_string(), "Sub2".to_string()];
        let prov_merged = merge_whitelist_providers(&prov_existing, &prov_add);
        assert_eq!(prov_merged, vec!["Sub1".to_string(), "Sub2".to_string()]);

        let s_req: ServerWhitelistReq =
            serde_json::from_str(r#"{"add_servers":["s1","s2"]}"#).unwrap();
        assert_eq!(s_req.add_servers, Some(vec!["s1".to_string(), "s2".to_string()]));

        let p_req: ProviderWhitelistReq =
            serde_json::from_str(r#"{"add_providers":["p1"]}"#).unwrap();
        assert_eq!(p_req.add_providers, Some(vec!["p1".to_string()]));
    }

    #[test]
    fn regression_bug_81b_provider_path_by_body_kind() {
        // 81.2: UA по умолчанию нейтральный, v2rayNG только по явному выбору
        let default_ua = default_provider_user_agent(None);
        assert!(
            default_ua.to_lowercase().contains("mihomo") || default_ua.to_lowercase().contains("clash"),
            "default UA must be neutral clash/mihomo, got: {default_ua}"
        );
        assert!(!default_ua.to_lowercase().contains("v2ray"));
        assert_eq!(default_provider_user_agent(Some("v2rayNG/1.8.12")), "v2rayNG/1.8.12");

        // 81.1 & 81.3: выбор пути по явному признаку и типу тела
        assert_eq!(
            choose_provider_path(false, SubscriptionBodyKind::NativeYaml),
            ProviderPath::NativeHttp
        );
        assert_eq!(
            choose_provider_path(true, SubscriptionBodyKind::NativeYaml),
            ProviderPath::NativeHttp
        );
        assert_eq!(
            choose_provider_path(false, SubscriptionBodyKind::Base64OrUri),
            ProviderPath::MobileFile
        );
        assert_eq!(
            choose_provider_path(false, SubscriptionBodyKind::Unknown),
            ProviderPath::NativeHttp
        );
        assert_eq!(
            choose_provider_path(true, SubscriptionBodyKind::Unknown),
            ProviderPath::MobileFile
        );

        // Определение типа тела
        assert_eq!(
            detect_subscription_body_kind("proxies:\n  - name: Node1\n    type: vless\n"),
            SubscriptionBodyKind::NativeYaml
        );
        assert_eq!(
            detect_subscription_body_kind("vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443#N1"),
            SubscriptionBodyKind::Base64OrUri
        );
    }

    #[test]
    fn regression_bug_a13_manual_update_does_not_convert_http_provider() {
        // [http + base64] -> не SaveConvertedFile (тип http-провайдера не меняется неявно)
        assert_eq!(
            provider_update_action(ProviderKind::Http, SubscriptionBodyKind::Base64OrUri),
            ProviderUpdateAction::KeepHttp
        );
        // [http + native yaml] -> KeepHttp
        assert_eq!(
            provider_update_action(ProviderKind::Http, SubscriptionBodyKind::NativeYaml),
            ProviderUpdateAction::KeepHttp
        );
        // [мобильная + base64] -> SaveConvertedFile
        assert_eq!(
            provider_update_action(ProviderKind::MobileFile, SubscriptionBodyKind::Base64OrUri),
            ProviderUpdateAction::SaveConvertedFile
        );
        // [http + мусор] -> Reject с понятным текстом
        match provider_update_action(ProviderKind::Http, SubscriptionBodyKind::Unknown) {
            ProviderUpdateAction::Reject(msg) => assert!(!msg.is_empty()),
            other => panic!("expected Reject for unknown body, got {other:?}"),
        }
    }
}
