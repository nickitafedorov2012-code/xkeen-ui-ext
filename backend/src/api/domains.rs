use axum::extract::State;
use axum::response::{Json, Response};
use serde::Deserialize;
use serde_json::json;

use super::{api_err, api_ok};
use crate::{config, log_i, mihomo, routing, transaction::ConfigTx, AppState};

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

    // Мгновенное добавление сопутствующих CDN из офлайн-базы связок (без блокирующего сетевого опроса)
    let auto_cdns = crate::cdn_discovery::expand_bundles(&tx.config().force_domains);
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
    let mut new_yaml = match routing::apply_domain_rules_for_config(&yaml, tx.config(), &all_force) {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };
    if let Ok(with_zapret) = routing::apply_zapret_hybrid_rules_with_force(&new_yaml, &tx.config().zapret, &all_force) {
        new_yaml = with_zapret;
    }
    if tx.config().whitelist.enabled
        && tx.config().whitelist.strategy == config::WhitelistStrategy::StrictXKeen
    {
        new_yaml = match routing::apply_domain_rules_for_config(&new_yaml, tx.config(), &all_force) {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };
    }
    if let Err(e) = tx.set_yaml(new_yaml) {
        return api_err(e);
    }

    // Атомарное сохранение и перезагрузка Mihomo с двухфазным откатом при сбое
    if let Err(e) = tx.commit_and_reload().await {
        return api_err(e);
    }

    let (n_direct, n_force) = (tx.config().direct_domains.len(), tx.config().force_domains.len());
    let auto_cdns_list: Vec<String> = auto_cdns.into_iter().collect();

    // Автоматическая синхронизация IP-адресов принудительно проксируемых доменов и их CDN с geo_override
    let overridden = match crate::override_sync::sync_from_config(tx.config(), &auto_cdns_list).await {
        Ok(count) => count,
        Err(e) => {
            crate::log_w!("[OVERRIDE] Ошибка синхронизации geo_override: {e}");
            0
        }
    };
    let new_rev = calc_domains_revision(&tx.config().direct_domains, &tx.config().force_domains);

    api_ok(json!({
        "direct": n_direct,
        "force": n_force,
        "auto_cdns": auto_cdns_list,
        "overridden_ips": overridden,
        "revision": new_rev
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

    let auto_cdns_list: Vec<String> = auto_cdns.into_iter().collect();
    let overridden = match crate::override_sync::sync_from_config(&cfg, &auto_cdns_list).await {
        Ok(count) => count,
        Err(e) => {
            crate::log_w!("[MANUAL] Ошибка синхронизации geo_override: {e}");
            0
        }
    };

    crate::log_i!("[MANUAL] ✓ Ручное сканирование CDN завершено. Найдено CDN: {}, IP: {}", auto_cdns_list.len(), overridden);
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

    let sync_override = if let Some(ref ip) = client_ip {
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
        false
    } else {
        // Глобальное принудительное проксирование: исключаем конфликт с direct_domains
        tx.config_mut().direct_domains.retain(|d| !d.eq_ignore_ascii_case(&clean_domain));
        if !tx.config().force_domains.iter().any(|d| d.eq_ignore_ascii_case(&clean_domain)) {
            tx.config_mut().force_domains.push(clean_domain.clone());
            tx.config_mut().force_domains.sort();
            tx.config_mut().force_domains.dedup();
        }

        // Мгновенное добавление сопутствующих CDN из связок
        let auto_cdns = crate::cdn_discovery::expand_bundles(&tx.config().force_domains);
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

        let mut new_yaml = match routing::apply_domain_rules_for_config(&yaml, tx.config(), &force_list) {
            Ok(y) => y,
            Err(e) => return api_err(e),
        };
        if let Ok(with_zapret) = routing::apply_zapret_hybrid_rules_with_force(&new_yaml, &tx.config().zapret, &force_list) {
            new_yaml = with_zapret;
        }
        if tx.config().whitelist.enabled
            && tx.config().whitelist.strategy == config::WhitelistStrategy::StrictXKeen
        {
            new_yaml = match routing::apply_domain_rules_for_config(&new_yaml, tx.config(), &force_list) {
                Ok(y) => y,
                Err(e) => return api_err(e),
            };
        }

        if let Err(e) = tx.set_yaml(new_yaml) {
            return api_err(e);
        }
        log_i!("Домен {} жестко направлен в прокси глобально", clean_domain);
        let _ = force_list;
        true
    };

    if let Err(e) = tx.commit_and_reload().await {
        return api_err(e);
    }

    if sync_override {
        let _ = crate::override_sync::sync_from_config(tx.config(), &[]).await;
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

// ==================== PER-DEVICE DOMAIN RULES =============

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

// ==================== GEOIP & GEOSITE БАЗЫ =============

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

#[derive(Deserialize)]
pub struct CheckDomainQuery {
    pub domain: String,
}

/// GET /api/diagnostics/check-domain — Быстрая диагностика («Почему не открывается?»)
pub async fn check_domain(
    State(state): State<AppState>,
    axum::extract::Query(query): axum::extract::Query<CheckDomainQuery>,
) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::check_domain_adguard(&state.http, &cfg, &query.domain).await {
        Ok(res) => api_ok(serde_json::to_value(res).unwrap_or(serde_json::Value::Null)),
        Err(e) => api_err(&e),
    }
}

#[derive(Deserialize)]
pub struct UnblockDomainReq {
    pub domain: String,
}

/// POST /api/diagnostics/unblock-domain — Разблокировать в 1 клик через AdGuard Home
pub async fn unblock_domain(
    State(state): State<AppState>,
    axum::extract::Json(body): axum::extract::Json<UnblockDomainReq>,
) -> Response {
    let cfg = state.config.read().await.clone();
    match crate::adguard::unblock_domain_adguard(&state.http, &cfg.adguard, &body.domain).await {
        Ok(rule) => api_ok(json!({
            "message": format!("Правило исключения '{}' успешно добавлено в AdGuard Home", rule),
            "rule": rule,
        })),
        Err(e) => api_err(&e),
    }
}

