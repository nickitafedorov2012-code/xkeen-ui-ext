pub mod subscriptions;
pub use subscriptions::*;

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

/// Стабилизация пинга после переключения сервера:
/// если первый замер после переключения провалился в 0 или дал холодный пик > 600 мс,
/// учитывает повторный замер и не перезаписывает прежний живой пинг ложным таймаутом.
pub fn stabilize_post_switch_ping(prev_ms: Option<i64>, attempt1_ms: i64, attempt2_ms: Option<i64>) -> Option<i64> {
    if attempt1_ms > 0 && attempt1_ms <= 600 {
        return Some(attempt1_ms);
    }
    if let Some(a2) = attempt2_ms {
        if a2 > 0 && attempt1_ms > 0 {
            return Some(attempt1_ms.min(a2));
        }
        if a2 > 0 {
            return Some(a2);
        }
        if attempt1_ms > 0 {
            return Some(attempt1_ms);
        }
    } else if attempt1_ms > 0 {
        return Some(attempt1_ms);
    }
    prev_ms.filter(|&p| p > 0)
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
                    // Баг #16: при ручном выборе БС-узла (в т.ч. VK-туннеля) синхронизируем цель БС,
                    // чтобы группа 🛡️ Whitelist не оставалась на старом Fallback.
                    if mut_cfg.whitelist.enabled {
                        mut_cfg.whitelist.target_server = Some(id.clone());
                    }
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
            // Баг #16: всегда синхронизируем группу 🛡️ Whitelist в рантайме с выбранным узлом,
            // чтобы трафик БС не уходил в Fallback на обычный сервер подписки.
            let _ = sync_whitelist_mode_tunnel(state, server_id).await;

            // Баг #13: стабилизированный фоновый замер пинга после переключения —
            // ждём 800 мс после смены маршрута и не перезаписываем хороший пинг холодным пиком/таймаутом.
            let cfg_now = state.config.read().await.clone();
            let http = state.http.clone();
            let selected = server_id.to_string();
            let prev_ping = mihomo::cached_ping(&selected);
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(800)).await;
                let a1 = mihomo::ping_server_url(&http, &cfg_now, &selected, 3500, None).await;
                let a2 = if a1 == 0 || a1 > 600 {
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                    Some(mihomo::ping_server_url(&http, &cfg_now, &selected, 3500, None).await)
                } else {
                    None
                };
                if let Some(stable) = stabilize_post_switch_ping(prev_ping, a1, a2) {
                    let mut map = std::collections::BTreeMap::new();
                    map.insert(selected, stable);
                    mihomo::store_ping_cache(&map);
                }
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

pub(crate) fn is_excluded_for_flow(
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

    #[test]
    fn regression_bug_13_stabilize_post_switch_ping_prevents_false_timeout() {
        // Нормальный быстрый пинг принимается сразу
        assert_eq!(stabilize_post_switch_ping(Some(180), 175, None), Some(175));
        // Холодный скачок 2900 мс, второй замер 185 мс -> берём 185 мс
        assert_eq!(stabilize_post_switch_ping(Some(180), 2900, Some(185)), Some(185));
        // Первый таймаут (0), второй успешный (190) -> 190
        assert_eq!(stabilize_post_switch_ping(Some(180), 0, Some(190)), Some(190));
        // Оба таймаута сразу после переключения при наличии живого прежнего пинга -> сохраняем прежний пинг
        assert_eq!(stabilize_post_switch_ping(Some(180), 0, Some(0)), Some(180));
        // Если прежнего пинга не было и оба таймаут -> None
        assert_eq!(stabilize_post_switch_ping(None, 0, Some(0)), None);
    }
}
