use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::json;

use super::{api_err, api_ok};
use crate::{mihomo, AppState};

// ==================== СКВОЗНОЙ РЕЛЕЙ CLASH API (REVERSE PROXY) =============

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

// ==================== CONNECTIONS VIEWER =============

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

// ==================== ТРАФИК В РЕАЛЬНОМ ВРЕМЕНИ =============

/// GET /api/traffic/poll — текущая скорость прямого и проксированного трафика
pub async fn get_traffic_poll(_state: State<AppState>) -> Response {
    api_ok(crate::traffic::get_snapshot_json())
}

#[derive(Deserialize, Default)]
pub struct TrafficResetQuery {
    pub target: Option<String>,
}

/// POST /api/traffic/reset — сброс одного или всех счётчиков накопленного трафика
pub async fn post_traffic_reset(
    State(_state): State<AppState>,
    Query(query): Query<TrafficResetQuery>,
) -> Response {
    let target = query.target.as_deref().unwrap_or("all");
    let snap = crate::traffic::reset_traffic(target);
    crate::log_i!("Сброс статистики сетевого трафика: target='{}'", target);
    api_ok(json!({
        "status": "ok",
        "target": target,
        "direct_bytes": snap.direct_bytes,
        "proxy_bytes": snap.proxy_bytes,
        "whitelist_bytes": snap.whitelist_bytes,
    }))
}

// ==================== RULES & MATCH TESTER («КУДА ПОЙДЁТ?») =============

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

                let matched = match r_type.to_ascii_uppercase().as_str() {
                    "DOMAIN" => domain == r_payload.to_lowercase(),
                    "DOMAIN-SUFFIX" | "DOMAINSUFFIX" => {
                        let p = r_payload.to_lowercase();
                        domain == p || domain.ends_with(&format!(".{}", p))
                    }
                    "DOMAIN-KEYWORD" | "DOMAINKEYWORD" => domain.contains(&r_payload.to_lowercase()),
                    "SRC-IP-CIDR" | "SRCIPCIDR" => !src_ip.is_empty() && crate::config::ip_matches_cidr(src_ip, r_payload),
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

