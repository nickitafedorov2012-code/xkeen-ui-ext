use axum::extract::{Query, State};
use axum::response::{Json, Response};
use serde::Deserialize;
use serde_json::json;

use super::{api_err, api_ok, resync_full_tunnel_from_disk, ServiceReq};
use crate::{config, log_e, log_i, log_w, routing, transaction::ConfigTx, AppState};
// ==================== УПРАВЛЕНИЕ РЕЖИМАМИ DNS РОУТЕРА =============

#[derive(Deserialize)]
pub struct SetDnsModeRequest {
    pub enhanced_mode: String, // "fake-ip" | "redir-host"
}

/// Извлечение режима enhanced-mode из YAML-конфигурации Mihomo (fake-ip | redir-host | unknown)
pub fn parse_dns_enhanced_mode(yaml: &str) -> &'static str {
    static RE_REDIR: std::sync::LazyLock<regex_lite::Regex> =
        std::sync::LazyLock::new(|| regex_lite::Regex::new(r#"(?m)^\s*enhanced-mode:\s*['"]?redir-host['"]?"#).unwrap());
    static RE_FAKE_IP: std::sync::LazyLock<regex_lite::Regex> =
        std::sync::LazyLock::new(|| regex_lite::Regex::new(r#"(?m)^\s*enhanced-mode:\s*['"]?fake-ip['"]?"#).unwrap());

    if RE_REDIR.is_match(yaml) {
        "redir-host"
    } else if RE_FAKE_IP.is_match(yaml) {
        "fake-ip"
    } else {
        "unknown"
    }
}

/// Строгая замена режима enhanced-mode в YAML с валидацией допустимых значений
pub fn update_dns_enhanced_mode(yaml: &str, target_mode: &str) -> Result<String, String> {
    if target_mode != "fake-ip" && target_mode != "redir-host" {
        return Err("Недопустимый режим DNS: разрешены только 'fake-ip' или 'redir-host'".to_string());
    }

    static RE_ENHANCED_MODE: std::sync::LazyLock<regex_lite::Regex> =
        std::sync::LazyLock::new(|| regex_lite::Regex::new(r"(?m)^(\s*enhanced-mode:\s*)[^\r\n]+").unwrap());

    if RE_ENHANCED_MODE.is_match(yaml) {
        Ok(RE_ENHANCED_MODE.replace(yaml, format!("${{1}}{target_mode}")).into_owned())
    } else {
        Err("Параметр enhanced-mode не найден в конфигурационном файле Mihomo".to_string())
    }
}

/// GET /api/dns/mode
pub async fn get_dns_mode(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let config_yaml = match tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        Ok(s) => s,
        Err(e) => {
            log_w!("[API] Не удалось прочитать конфигурацию Mihomo {}: {e}", cfg.mihomo.config_path);
            return api_err(format!("Ошибка чтения конфигурации Mihomo: {e}"));
        }
    };

    let enhanced_mode = parse_dns_enhanced_mode(&config_yaml);

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
    let target_mode = body.enhanced_mode.trim();
    if target_mode != "fake-ip" && target_mode != "redir-host" {
        return api_err("Недопустимый режим DNS: разрешены только 'fake-ip' или 'redir-host'");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    let config_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let new_yaml = match update_dns_enhanced_mode(&config_yaml, target_mode) {
        Ok(y) => y,
        Err(e) => return api_err(e),
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
        Ok(_) => api_ok(json!({ "success": true, "message": "Чистые DNS-серверы (77.88.8.8, 77.88.8.1) успешно настроены в KeeneticOS" })),
        Err(e) => api_err(format!("Ошибка настройки DNS через RCI: {e}")),
    }
}

// ==================== ADBLOCK (БЛОКИРОВКА РЕКЛАМЫ) =============

/// GET /api/adblock — текущее состояние AdBlock
pub async fn get_adblock(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await;
    api_ok(json!({ "enabled": cfg.adblock_enabled }))
}

#[derive(Deserialize)]
pub struct AdBlockToggleReq {
    pub enabled: Option<bool>,
}

/// Общее ядро переключения AdBlock для API и Telegram-бота (баг 60):
/// 1) CommitConfig (транзакция + reload Mihomo),
/// 2) SetProtection в AdGuard (ошибка не глотается),
/// 3) SyncIptables (редирект DNS),
/// 4) ResyncTunnels (`resync_full_tunnel_from_disk`).
pub async fn adblock_toggle_core(state: &AppState, target: Option<bool>) -> Result<bool, String> {
    let mut tx = ConfigTx::begin(state).await?;
    let target_enabled = target.unwrap_or(!tx.config().adblock_enabled);
    let steps = crate::adguard::plan_adblock_toggle(tx.config(), target_enabled);

    crate::adguard::apply_adblock_toggle(tx.config_mut(), target_enabled);
    let raw_yaml = tx.read_yaml().await?;
    let (new_yaml, _) = routing::apply_routing(&raw_yaml, tx.config())
        .map_err(|e| format!("Ошибка роутинга: {}", e))?;
    tx.set_yaml(new_yaml)?;
    let agh_cfg = tx.config().adguard.clone();

    for step in steps {
        match step {
            crate::adguard::AdblockStep::CommitConfig => {
                tx.commit_and_reload().await?;
                break;
            }
            _ => {}
        }
    }

    for step in crate::adguard::plan_adblock_toggle(&state.config.read().await.clone(), target_enabled) {
        match step {
            crate::adguard::AdblockStep::CommitConfig => {}
            crate::adguard::AdblockStep::SetProtection(en) => {
                crate::adguard::set_protection(&state.http, &agh_cfg, en)
                    .await
                    .map_err(|e| format!("Ошибка переключения защиты AdGuard: {e}"))?;
            }
            crate::adguard::AdblockStep::SyncIptables { enable, port } => {
                if enable {
                    let active_port = crate::adguard::detect_adguard_dns_port().unwrap_or(port);
                    crate::adguard::apply_adguard_iptables(active_port)
                        .map_err(|e| format!("Ошибка настройки DNS-редиректа iptables: {e}"))?;
                } else {
                    let latest_cfg = state.config.read().await;
                    crate::adguard::remove_adguard_iptables_for_mihomo_config(&latest_cfg.mihomo.config_path)
                        .map_err(|e| format!("Ошибка снятия DNS-редиректа iptables: {e}"))?;
                }
            }
            crate::adguard::AdblockStep::ResyncTunnels => {
                let latest_cfg = state.config.read().await.clone();
                resync_full_tunnel_from_disk(&latest_cfg).await;
            }
        }
    }

    Ok(target_enabled)
}

/// POST /api/adblock/toggle — переключение блокировки рекламы с перезагрузкой Mihomo
pub async fn toggle_adblock(
    State(state): State<AppState>,
    body: Option<axum::extract::Json<AdBlockToggleReq>>,
) -> Response {
    let target = body.and_then(|axum::extract::Json(b)| b.enabled);
    // Вся оркестрация (включая resync_full_tunnel_from_disk(&latest_cfg)) выполняется в adblock_toggle_core
    match adblock_toggle_core(&state, target).await {
        Ok(enabled) => api_ok(json!({ "enabled": enabled, "saved": true, "applied": true })),
        Err(e) => api_err(e),
    }
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


/// Определение активного DNS-порта для перехвата (AdGuard 5353 или Mihomo 1053) с учётом доступности AdGuard (баг 1)
pub fn resolve_active_dns_port(app_cfg: &config::AppConfig) -> u16 {
    let adguard_alive = !crate::watchdog::is_adguard_down() && crate::adguard::is_adguard_process_alive();
    resolve_active_dns_port_with_health(app_cfg, adguard_alive)
}

pub fn resolve_active_dns_port_with_health(app_cfg: &config::AppConfig, adguard_is_alive: bool) -> u16 {
    if (app_cfg.adguard.enabled || app_cfg.adblock_enabled) && adguard_is_alive {
        crate::adguard::detect_adguard_dns_port().unwrap_or(if app_cfg.adguard.dns_port > 0 { app_cfg.adguard.dns_port } else { crate::adguard::DEFAULT_ADGUARD_DNS_FALLBACK_PORT })
    } else {
        // Баг 45: порт DNS Mihomo — из dns.listen его config.yaml, а не зашитый 1053
        crate::routing::mihomo_dns_port(app_cfg)
    }
}


/// POST /api/adguard/sync-firewall — переприменение DNS-редиректа AdGuard (53 -> порт AdGuard)
/// и полного перехвата по фактическому состоянию службы. Используется кнопкой
/// «Восстановить DNS» в самодиагностике (раньше маршрута не существовало — кнопка всегда падала).
pub async fn adguard_sync_firewall(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let enabled = cfg.adguard.enabled || cfg.adblock_enabled;
    let port = crate::adguard::detect_adguard_dns_port().unwrap_or(cfg.adguard.dns_port);
    let message = if !enabled {
        let mihomo_restore_port = crate::adguard::resolve_adguard_restore_port(None, &cfg.mihomo.config_path);
        let _ = crate::adguard::remove_adguard_iptables_with_restore_port(Some(mihomo_restore_port));
        "AdGuard выключен — системный DNS-редирект снят".to_string()
    } else if crate::adguard::test_agh_dns_listener(&cfg.adguard.host, port).await {
        if let Err(e) = crate::adguard::apply_adguard_iptables(port) {
            return api_err(format!("Не удалось применить DNS-редирект AdGuard: {e}"));
        }
        format!("DNS-редирект 53 -> {port} переприменён")
    } else {
        // AdGuard не отвечает: держим DNS LAN живым, редирект не ставим
        let _ = crate::adguard::remove_adguard_iptables_for_mihomo_config(&cfg.mihomo.config_path);
        return api_err(format!("AdGuard Home не отвечает на порту {port} — редирект снят, чтобы сеть не осталась без DNS"));
    };
    resync_full_tunnel_from_disk(&cfg).await;
    api_ok(json!({ "message": message, "dns_port": port }))
}


// ==================== AdGuard Home API Handlers =============

#[derive(Deserialize)]
pub struct AdGuardQueryParams {
    pub limit: Option<usize>,
    pub older_than: Option<String>,
    pub search: Option<String>,
    pub response_status: Option<String>,
}

/// GET /api/adguard/status
pub async fn adguard_status(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let status = crate::adguard::get_status(&state.http, &cfg.adguard).await;
    api_ok(json!(status))
}

/// GET /api/adguard/capabilities
pub async fn adguard_capabilities(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let cap = crate::adguard::get_capabilities(&cfg.adguard);
    api_ok(json!(cap))
}

/// GET /api/adguard/health
pub async fn adguard_health(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let router_ip = cfg.rci.host.clone();
    let health = crate::adguard::get_health(&state.http, &cfg.adguard, Some(&router_ip)).await;
    api_ok(json!(health))
}

/// GET /api/adguard/overview
pub async fn adguard_overview(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::get_overview(&state.http, &cfg.adguard).await {
        Ok(ov) => api_ok(json!(ov)),
        Err(e) => api_err(e),
    }
}

/// GET /api/adguard/query-log
pub async fn adguard_query_log(
    State(state): State<AppState>,
    Query(params): Query<AdGuardQueryParams>,
) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::get_query_log(
        &state.http,
        &cfg.adguard,
        params.limit.unwrap_or(50),
        params.older_than.as_deref(),
        params.search.as_deref(),
        params.response_status.as_deref(),
    )
    .await
    {
        Ok(log) => api_ok(json!(log)),
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct ProtectionReq {
    pub enabled: bool,
}

/// POST /api/adguard/protection
pub async fn adguard_set_protection(
    State(state): State<AppState>,
    Json(body): Json<ProtectionReq>,
) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::set_protection(&state.http, &cfg.adguard, body.enabled).await {
        Ok(now_enabled) => {
            if now_enabled {
                let active_port = crate::adguard::detect_adguard_dns_port().unwrap_or(cfg.adguard.dns_port);
                if let Err(e) = crate::adguard::apply_adguard_iptables(active_port) {
                    return api_err(format!("Ошибка настройки DNS-редиректа iptables для AdGuard: {e}"));
                }
                let _ = crate::adguard::heal_adguard_live_config(&state.http, &cfg.adguard, Some(&cfg.rci.host)).await;
            } else {
                if let Err(e) = crate::adguard::remove_adguard_iptables_for_mihomo_config(&cfg.mihomo.config_path) {
                    log_w!("Ошибка снятия DNS-редиректа iptables для AdGuard: {e}");
                }
            }
            if let Ok(mut tx) = ConfigTx::begin(&state).await {
                crate::adguard::apply_integration_toggle(tx.config_mut(), now_enabled);
                let active_port = crate::adguard::detect_adguard_dns_port().unwrap_or(cfg.adguard.dns_port);
                if now_enabled && active_port > 0 {
                    tx.config_mut().adguard.dns_port = active_port;
                }
                if let Ok(raw_yaml) = tx.read_yaml().await {
                    if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, tx.config()) {
                        let _ = tx.set_yaml(new_yaml);
                    }
                }
                if let Err(e) = tx.commit_and_reload().await {
                    log_e!("Ошибка сохранения транзакции защиты AdGuard: {e}");
                }
            }
            let latest_cfg = state.config.read().await.clone();
            resync_full_tunnel_from_disk(&latest_cfg).await;
            api_ok(json!({ "enabled": now_enabled }))
        }
        Err(e) => api_err(e),
    }
}

/// GET /api/adguard/filtering
pub async fn adguard_get_filtering(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::get_filtering(&state.http, &cfg.adguard).await {
        Ok(flt) => api_ok(json!(flt)),
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct RulesReq {
    pub rules: Option<Vec<String>>,
    pub rules_text: Option<String>,
}

/// POST /api/adguard/filtering/rules
pub async fn adguard_set_rules(
    State(state): State<AppState>,
    Json(body): Json<RulesReq>,
) -> Response {
    let rules = if let Some(r) = body.rules {
        r
    } else if let Some(txt) = body.rules_text {
        txt.lines().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    } else {
        return api_err("Требуется список правил (rules или rules_text)");
    };

    let cfg = state.config.read().await.clone();
    match crate::adguard::set_user_rules(&state.http, &cfg.adguard, &rules).await {
        Ok(_) => api_ok(json!({ "saved": true, "count": rules.len() })),
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct ToggleFilterReq {
    pub url: String,
    pub enabled: bool,
    pub whitelist: Option<bool>,
}

/// POST /api/adguard/filtering/toggle-filter
pub async fn adguard_toggle_filter(
    State(state): State<AppState>,
    Json(body): Json<ToggleFilterReq>,
) -> Response {
    let cfg = state.config.read().await.clone();
    let whitelist = body.whitelist.unwrap_or(false);
    match crate::adguard::toggle_filter(&state.http, &cfg.adguard, &body.url, body.enabled, whitelist).await {
        Ok(_) => api_ok(json!({ "success": true })),
        Err(e) => api_err(e),
    }
}

/// GET /api/adguard/rewrites
pub async fn adguard_get_rewrites(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::get_rewrites(&state.http, &cfg.adguard).await {
        Ok(r) => api_ok(json!(r)),
        Err(e) => api_err(e),
    }
}

#[derive(Deserialize)]
pub struct RewriteReq {
    pub domain: String,
    pub answer: String,
}

/// POST /api/adguard/rewrites/add
pub async fn adguard_add_rewrite(
    State(state): State<AppState>,
    Json(body): Json<RewriteReq>,
) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::add_rewrite(&state.http, &cfg.adguard, &body.domain, &body.answer).await {
        Ok(_) => api_ok(json!({ "success": true })),
        Err(e) => api_err(e),
    }
}

/// POST /api/adguard/rewrites/delete
pub async fn adguard_delete_rewrite(
    State(state): State<AppState>,
    Json(body): Json<RewriteReq>,
) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::delete_rewrite(&state.http, &cfg.adguard, &body.domain, &body.answer).await {
        Ok(_) => api_ok(json!({ "success": true })),
        Err(e) => api_err(e),
    }
}

/// GET /api/adguard/clients
pub async fn adguard_get_clients(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::get_clients(&state.http, &cfg.adguard).await {
        Ok(c) => api_ok(json!(c)),
        Err(e) => api_err(e),
    }
}

/// GET /api/adguard/diagnostics
pub async fn adguard_diagnostics(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let router_ip = cfg.rci.host.clone();
    let diag = crate::adguard::get_diagnostics(&state.http, &cfg.adguard, Some(&router_ip)).await;
    api_ok(json!(diag))
}

/// POST /api/adguard/service
pub async fn adguard_service(Json(body): Json<ServiceReq>) -> Response {
    match crate::adguard::service_action(&body.action).await {
        Ok(out) => api_ok(json!({ "output": out })),
        Err(e) => api_err(e),
    }
}

/// POST /api/adguard/install
pub async fn adguard_install() -> Response {
    match crate::adguard::install_adguard_package().await {
        Ok(out) => api_ok(json!({ "output": out })),
        Err(e) => api_err(e),
    }
}

/// GET /api/adguard/config
pub async fn adguard_get_config(State(state): State<AppState>) -> Response {
    let cfg = state.config.read().await.clone();
    let mut adguard = cfg.adguard.clone();
    if let Some(ref pw) = adguard.password {
        if !pw.is_empty() {
            adguard.password = Some("******".to_string());
        }
    }
    api_ok(json!(adguard))
}

/// POST /api/adguard/config
/// Частичное обновление настроек интеграции AdGuard (баг 133): применяются только присланные поля.
/// `enabled` и `integration_choice_locked` этой формой не меняются (для них есть переключатели),
/// маска пароля `******` сохраняет текущий пароль.
pub fn merge_adguard_config_patch(
    current: &crate::config::AdGuardConfig,
    patch: &serde_json::Value,
) -> Result<crate::config::AdGuardConfig, String> {
    let patch_obj = patch.as_object().ok_or("Ожидается JSON-объект настроек AdGuard")?;
    let mut base = serde_json::to_value(current).map_err(|e| e.to_string())?;
    let base_obj = base.as_object_mut().ok_or("internal")?;
    for (k, v) in patch_obj {
        if k == "enabled" || k == "integration_choice_locked" {
            continue;
        }
        if k == "password" && v.as_str() == Some("******") {
            continue;
        }
        base_obj.insert(k.clone(), v.clone());
    }
    serde_json::from_value(base).map_err(|e| format!("Некорректные настройки AdGuard: {e}"))
}

pub async fn adguard_set_config(
    State(state): State<AppState>,
    Json(patch): Json<serde_json::Value>,
) -> Response {
    let tx_res = crate::transaction::ConfigTx::begin(&state).await;
    let mut tx = match tx_res {
        Ok(t) => t,
        Err(e) => return api_err(format!("Ошибка начала транзакции: {e}")),
    };

    let mut current = tx.config().clone();
    let new_adguard = match merge_adguard_config_patch(&current.adguard, &patch) {
        Ok(a) => a,
        Err(e) => return api_err(e),
    };
    let adguard_enabled_changed = current.adguard.enabled != new_adguard.enabled;
    let port_changed = current.adguard.dns_port != new_adguard.dns_port;
    let new_enabled = new_adguard.enabled;
    let new_port = new_adguard.dns_port;
    current.adguard = new_adguard;
    tx.stage_json(current);

    if adguard_enabled_changed || port_changed {
        if let Ok(raw_yaml) = tx.read_yaml().await {
            if let Ok((new_yaml, _)) = routing::apply_routing(&raw_yaml, tx.config()) {
                let _ = tx.set_yaml(new_yaml);
            }
        }
    }

    let commit_res = if adguard_enabled_changed || port_changed {
        tx.commit_and_reload().await
    } else {
        tx.commit_without_reload().await
    };

    match commit_res {
        Ok(_) => {
            log_i!("Конфигурация AdGuard Home успешно обновлена и сохранена");
            if adguard_enabled_changed || port_changed {
                if new_enabled {
                    let active_port = crate::adguard::detect_adguard_dns_port().unwrap_or(new_port);
                    let _ = crate::adguard::apply_adguard_iptables(active_port);
                } else {
                    let _ = crate::adguard::remove_adguard_iptables();
                }
            }
            api_ok(json!({ "saved": true }))
        }
        Err(e) => api_err(format!("Ошибка сохранения конфигурации AdGuard Home: {e}")),
    }
}
