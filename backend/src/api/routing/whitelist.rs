use axum::extract::{Json, State};
use axum::response::Response;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::devices::device_providers_for;
use super::gaming::{resolve_group_leaf, sync_full_tunnel_modes};
use crate::api::{
    api_err, api_err_bad_request, api_ok, resolve_active_dns_port, sync_whitelist_mode_tunnel,
};
use crate::{config, failover, log_i, log_w, mihomo, rci, routing, transaction::ConfigTx, AppState};
#[derive(Serialize)]
pub struct WhitelistStatusResponse {
    pub enabled: bool,
    pub auto_by_failover: bool,
    pub strategy: config::WhitelistStrategy,
    pub target_server: Option<String>,
    pub follow_failover: bool,
    pub excluded_devices: Vec<String>,
    pub devices: Vec<config::WhitelistDevice>,
    pub gaming_suspended: bool,
    pub custom_domains: Vec<String>,
    pub domestic_catalog_count: usize,
    pub active_server: String,
    pub is_bs_server: bool,
    pub whitelist_servers: Vec<String>,
    pub normal_servers: Vec<String>,
    pub whitelist_providers: Vec<String>,
}

/// GET /api/whitelist — статус и конфигурация режима белых списков
pub async fn get_whitelist(State(state): State<AppState>) -> Response {
    let cfg = (**state.config.read().await).clone();
    let domestic_catalog_count = routing::DOMESTIC_WHITELIST_DOMAINS.len();

    let proxies_map = mihomo::get_proxies(&state.http, &cfg).await.ok();
    let group_now = proxies_map.as_ref().and_then(|map| {
        map.get(routing::WHITELIST_GROUP_NAME)
            .or_else(|| map.get("PROXY"))
            .and_then(|g| g.get("now"))
            .and_then(|n| n.as_str())
    }).unwrap_or("Fastest");

    let fallback_active = proxies_map.as_ref()
        .map(|map| resolve_group_leaf(map, group_now))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| group_now.to_string());

    let (active_server, is_bs_server) = if let Ok(servers) = mihomo::get_servers(&state.http, &cfg, &cfg.failover.priority_chain).await {
        if let Some(s) = servers.iter().find(|srv| srv.is_active && !srv.is_pool) {
            let is_bs = failover::is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg);
            (s.name.clone(), is_bs)
        } else {
            let is_bs = failover::is_whitelist_server_full(&fallback_active, &fallback_active, None, &cfg);
            (fallback_active, is_bs)
        }
    } else {
        let is_bs = failover::is_whitelist_server_full(&fallback_active, &fallback_active, None, &cfg);
        (fallback_active, is_bs)
    };

    api_ok(json!(WhitelistStatusResponse {
        enabled: cfg.whitelist.enabled,
        auto_by_failover: cfg.whitelist.auto_by_failover,
        strategy: cfg.whitelist.strategy,
        target_server: cfg.whitelist.target_server,
        follow_failover: cfg.whitelist.follow_failover,
        excluded_devices: cfg.whitelist.excluded_devices,
        devices: cfg.whitelist.devices,
        gaming_suspended: cfg.whitelist.gaming_suspended,
        custom_domains: cfg.whitelist.custom_domains,
        domestic_catalog_count,
        active_server,
        is_bs_server,
        whitelist_servers: cfg.whitelist_servers,
        normal_servers: cfg.normal_servers,
        whitelist_providers: cfg.whitelist_providers,
    }))
}

#[derive(Deserialize)]
pub struct WhitelistStrategyReq {
    pub strategy: config::WhitelistStrategy,
}

/// POST /api/whitelist/strategy — переключить стратегию работы сервера белых списков
pub async fn set_whitelist_strategy(
    State(state): State<AppState>,
    Json(body): Json<WhitelistStrategyReq>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    tx.config_mut().whitelist.strategy = body.strategy;

    let conflicts = config::device_conflicts(tx.config());
    if !conflicts.is_empty() {
        return api_err_bad_request(config::format_device_conflicts(&conflicts));
    }

    let raw_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка генерации правил маршрутизации: {e}")),
    };

    if let Err(e) = tx.set_yaml(&new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.apply_and_reload().await {
        return api_err(e);
    }

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    // BS-FIX: смена стратегии включает/выключает полный перехват XKEEN_BS немедленно
    {
        let yaml = tokio::fs::read_to_string(&tx.config().mihomo.config_path).await.unwrap_or_default();
        let ports = routing::redir_ports(&yaml);
        let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
        let dns_port = resolve_active_dns_port(tx.config());
        let _ = sync_full_tunnel_modes(tx.config(), redir_port, tproxy_port, dns_port).await;
    }

    log_i!("Стратегия белых списков переключена на: {:?}", body.strategy);
    api_ok(json!({ "strategy": body.strategy }))
}

#[derive(Deserialize)]
pub struct WhitelistExcludeReq {
    pub id: String, // IP or MAC
    #[serde(default)]
    pub ip: Option<String>,
    pub excluded: bool,
}

/// POST /api/whitelist/exclude — добавить или удалить устройство из списка исключений
pub async fn toggle_whitelist_exclude(
    State(state): State<AppState>,
    Json(body): Json<WhitelistExcludeReq>,
) -> Response {
    let id_clean = body.id.trim().to_string();
    if id_clean.is_empty() {
        return api_err("Не указан идентификатор устройства (IP или MAC)");
    }
    let ip_clean = body.ip.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty() && *s != id_clean);

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    if body.excluded {
        if !tx.config().whitelist.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(&id_clean)) {
            tx.config_mut().whitelist.excluded_devices.push(id_clean.clone());
        }
        if let Some(ip) = ip_clean {
            if !tx.config().whitelist.excluded_devices.iter().any(|d| d.eq_ignore_ascii_case(ip)) {
                tx.config_mut().whitelist.excluded_devices.push(ip.to_string());
            }
            if !crate::config::is_valid_ip_or_cidr(&id_clean) && crate::config::is_valid_ip_or_cidr(ip) {
                tx.config_mut().whitelist.excluded_last_ips.insert(id_clean.to_lowercase(), ip.to_string());
            }
        }
        if let Ok(arp) = tokio::fs::read_to_string("/proc/net/arp").await {
            crate::routing::refresh_excluded_whitelist_last_ips(tx.config_mut(), &arp);
        }
    } else {
        tx.config_mut().whitelist.excluded_devices.retain(|d| !d.eq_ignore_ascii_case(&id_clean));
        tx.config_mut().whitelist.excluded_last_ips.retain(|k, _| !k.eq_ignore_ascii_case(&id_clean));
        if let Some(ip) = ip_clean {
            tx.config_mut().whitelist.excluded_devices.retain(|d| !d.eq_ignore_ascii_case(ip));
        }
    }

    let conflicts = config::device_conflicts(tx.config());
    if !conflicts.is_empty() {
        return api_err_bad_request(config::format_device_conflicts(&conflicts));
    }

    let raw_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка генерации правил маршрутизации: {e}")),
    };

    if let Err(e) = tx.set_yaml(&new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.apply_and_reload().await {
        return api_err(e);
    }

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    // BS-FIX: список исключений изменил состав перехвата — синхронизируем iptables сразу
    {
        let yaml = tokio::fs::read_to_string(&tx.config().mihomo.config_path).await.unwrap_or_default();
        let ports = routing::redir_ports(&yaml);
        let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
        let dns_port = resolve_active_dns_port(tx.config());
        let _ = sync_full_tunnel_modes(tx.config(), redir_port, tproxy_port, dns_port).await;
    }

    log_i!("Статус исключения устройства {} из белых списков: {}", id_clean, body.excluded);
    api_ok(json!({ "excluded_devices": tx.config().whitelist.excluded_devices }))
}

#[derive(Deserialize, Clone, Debug)]
pub struct WhitelistDevicePatch {
    pub mac: String,
    #[serde(default)]
    pub ip: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub server: Option<String>,
}

/// Точечное обновление одного устройства БС по MAC без затирания IP остальных устройств (баг 67).
pub fn patch_whitelist_device(
    devices: &mut Vec<config::WhitelistDevice>,
    patch: &WhitelistDevicePatch,
) -> Result<(), String> {
    let mac = patch.mac.trim();
    if mac.is_empty() || !config::is_valid_mac(mac) {
        return Err(format!("Некорректный MAC-адрес устройства: {mac}"));
    }
    if let Some(ref ip) = patch.ip {
        let ip_t = ip.trim();
        if !ip_t.is_empty() && !config::is_valid_ip_or_cidr(ip_t) {
            return Err(format!("Некорректный IP-адрес устройства: {ip_t}"));
        }
    }

    if let Some(existing) = devices.iter_mut().find(|d| d.mac.eq_ignore_ascii_case(mac)) {
        if let Some(enabled) = patch.enabled {
            existing.enabled = enabled;
        }
        if let Some(ref name) = patch.name {
            existing.name = Some(name.clone());
        }
        if let Some(ref srv) = patch.server {
            let s = srv.trim();
            existing.server = if s.is_empty() { None } else { Some(s.to_string()) };
        }
        // Обновляем IP только если у существующего устройства IP пуст (иначе сохраняем актуальный DHCP IP)
        if existing.ip.trim().is_empty() {
            if let Some(ref ip) = patch.ip {
                existing.ip = ip.trim().to_string();
            }
        }
    } else {
        let srv_opt = patch
            .server
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
        let enabled = patch.enabled.unwrap_or(true);
        if enabled || srv_opt.is_some() {
            devices.push(config::WhitelistDevice {
                mac: mac.to_string(),
                ip: patch.ip.as_deref().map(str::trim).unwrap_or("").to_string(),
                enabled,
                name: patch.name.clone(),
                server: srv_opt,
            });
        }
    }
    Ok(())
}

#[derive(Deserialize)]
pub struct WhitelistUpdateReq {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub auto_by_failover: Option<bool>,
    #[serde(default)]
    pub strategy: Option<config::WhitelistStrategy>,
    #[serde(default)]
    pub target_server: Option<String>,
    #[serde(default)]
    pub follow_failover: Option<bool>,
    #[serde(default)]
    pub excluded_devices: Option<Vec<String>>,
    #[serde(default)]
    pub devices: Option<Vec<config::WhitelistDevice>>,
    #[serde(default)]
    pub device_patch: Option<WhitelistDevicePatch>,
    #[serde(default)]
    pub custom_domains: Option<Vec<String>>,
}

/// POST /api/whitelist — полное сохранение настроек режима белых списков
/// Применить к `WhitelistConfig` только явно переданные поля патча (Bug 66).
/// Сетевая проверка имён узлов Mihomo выполняется отдельно в `save_whitelist`.
pub fn merge_whitelist_patch(
    wl: &mut config::WhitelistConfig,
    body: &WhitelistUpdateReq,
) -> Result<(), String> {
    if let Some(enabled) = body.enabled {
        wl.enabled = enabled;
        if enabled {
            wl.enabled_by = config::WhitelistEnabledBy::Manual;
        }
    }
    if let Some(auto_by_failover) = body.auto_by_failover {
        wl.auto_by_failover = auto_by_failover;
    }
    if let Some(strategy) = body.strategy {
        wl.strategy = strategy;
    }
    if let Some(ref target) = body.target_server {
        let t = target.trim();
        if t.is_empty() || t == "follow_failover" {
            wl.target_server = None;
            wl.follow_failover = true;
        } else {
            wl.target_server = Some(t.to_string());
        }
    }
    if let Some(follow) = body.follow_failover {
        wl.follow_failover = follow;
    }
    if let Some(ref excluded) = body.excluded_devices {
        wl.excluded_devices = excluded.clone();
    }
    if let Some(ref custom_domains) = body.custom_domains {
        let mut clean_domains = Vec::new();
        for d in custom_domains {
            let clean = config::normalize_domain(d);
            if clean.is_empty() {
                continue;
            }
            if !config::is_valid_domain(&clean) {
                return Err(format!("Некорректный домен в белом списке: {d}"));
            }
            if !clean_domains.contains(&clean) {
                clean_domains.push(clean);
            }
        }
        wl.custom_domains = clean_domains;
    }
    Ok(())
}

pub async fn save_whitelist(
    State(state): State<AppState>,
    Json(body): Json<WhitelistUpdateReq>,
) -> Response {
    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    if let Err(e) = merge_whitelist_patch(&mut tx.config_mut().whitelist, &body) {
        return api_err(e);
    }
    if let Some(devices) = body.devices {
        for dev in &devices {
            let ip = dev.ip.trim();
            if !ip.is_empty() && !config::is_valid_ip_or_cidr(ip) {
                return api_err(format!("Некорректный IP-адрес устройства: {ip}"));
            }
            let mac = dev.mac.trim();
            if !mac.is_empty() && !config::is_valid_mac(mac) {
                return api_err(format!("Некорректный MAC-адрес устройства: {mac}"));
            }
        }
        // BS-PIN: закреплённая нода недоступна правилам mihomo напрямую (нода живёт
        // в proxy-providers), поэтому пин направляет устройство в его per-device
        // select-группу. Зеркалим закрепление в device_routing — apply_routing
        // создаст группу {name} {ip_underscored}, а ниже выбираем в ней ноду runtime.
        // Валидация: имя ноды должно существовать среди прокси/провайдеров.
        let known: std::collections::HashSet<String> = match mihomo::get_servers(&state.http, tx.config(), &tx.config().failover.priority_chain).await {
            Ok(servers) => servers.iter().map(|s| s.id.clone()).collect(),
            Err(_) => std::collections::HashSet::new(),
        };
        let mut known_providers: std::collections::HashSet<String> = std::collections::HashSet::new();
        if let Ok(provs) = mihomo::get_provider_proxies(&state.http, tx.config()).await {
            for (name, _) in provs.iter() {
                known_providers.insert(name.clone());
            }
        }
        for dev in &devices {
            let srv = dev.server.as_deref().map(str::trim).unwrap_or("");
            if srv.is_empty() {
                continue;
            }
            if !known.contains(srv) && !known_providers.contains(srv) {
                return api_err(format!("Неизвестный узел для закрепления устройства: {srv}"));
            }
        }
        let old_devices = tx.config().whitelist.devices.clone();
        let _ = old_devices;
        tx.config_mut().whitelist.devices = devices;
    }
    if let Some(patch) = body.device_patch {
        let srv = patch.server.as_deref().map(str::trim).unwrap_or("");
        if !srv.is_empty() {
            let known: std::collections::HashSet<String> = match mihomo::get_servers(&state.http, tx.config(), &tx.config().failover.priority_chain).await {
                Ok(servers) => servers.iter().map(|s| s.id.clone()).collect(),
                Err(_) => std::collections::HashSet::new(),
            };
            let mut known_providers: std::collections::HashSet<String> = std::collections::HashSet::new();
            if let Ok(provs) = mihomo::get_provider_proxies(&state.http, tx.config()).await {
                for (name, _) in provs.iter() {
                    known_providers.insert(name.clone());
                }
            }
            if !known.contains(srv) && !known_providers.contains(srv) {
                return api_err(format!("Неизвестный узел для закрепления устройства: {srv}"));
            }
        }
        if let Err(e) = patch_whitelist_device(&mut tx.config_mut().whitelist.devices, &patch) {
            return api_err(e);
        }
    }

    let mut raw_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    // BS-PIN: per-device select-группы для закреплённых устройств создаются ДО
    // генерации правил БС — правило SRC-IP-CIDR ссылается на группу по имени
    // (group_name_for), а провайдерные ноды правилам напрямую недоступны.
    // server="Fastest" только гарантирует существование группы; выбор ноды в ней —
    // runtime (switch_group ниже), Failover этот выбор не трогает.
    {
        let pin_assignments: Vec<routing::Assignment> = tx
            .config()
            .whitelist
            .devices
            .iter()
            .filter(|d| {
                d.enabled
                    && !d.ip.trim().is_empty()
                    && !d.server.as_deref().map(str::trim).unwrap_or("").is_empty()
            })
            .map(|d| routing::Assignment {
                ip: d.ip.trim().to_string(),
                name: d.name.clone().unwrap_or_default(),
                server: Some("Fastest".to_string()),
            })
            .collect();
        if !pin_assignments.is_empty() {
            let providers = device_providers_for(tx.config(), &raw_yaml);
            let pinned = routing::pinned_whitelist_ips(tx.config());
            match routing::apply_assignments(&raw_yaml, &pin_assignments, &providers, &pinned) {
                Ok(y) => raw_yaml = y,
                Err(e) => return api_err(format!("Ошибка создания группы закрепления устройства: {e}")),
            }
        }
    }

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка генерации правил маршрутизации: {e}")),
    };

    if let Err(e) = tx.set_yaml(&new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.apply_and_reload().await {
        return api_err(e);
    }

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    sync_whitelist_runtime_after_commit(&state, tx.config()).await;

    // BS-PIN: выбираем закреплённую ноду в per-device группе (runtime, вне Failover)
    // Баг 40: используем switch_group_scoped, чтобы не рвать соединения всей сети
    for dev in &tx.config().whitelist.devices {
        let srv = dev.server.as_deref().map(str::trim).unwrap_or("");
        let ip = dev.ip.trim();
        if !srv.is_empty() && !ip.is_empty() {
            let gname = routing::group_name_for(ip, dev.name.as_deref().unwrap_or(""));
            if let Err(e) = mihomo::switch_group_scoped(&state.http, tx.config(), &gname, srv).await {
                log_w!("[BS-PIN] Не удалось выбрать ноду '{}' в группе '{}': {}", srv, gname, e);
            } else {
                log_i!("[BS-PIN] Устройство {} ({}) закреплено за нодой '{}' (группа '{}')", dev.name.as_deref().unwrap_or("?"), ip, srv, gname);
            }
        }
    }

    log_i!("Настройки режима белых списков успешно сохранены и применены");
    api_ok(json!({ "saved": true, "whitelist": tx.config().whitelist }))
}

#[derive(Deserialize)]
pub struct WhitelistDeviceToggleReq {
    pub ip: Option<String>,
    pub mac: Option<String>,
    pub enabled: bool,
    #[serde(default)]
    pub name: Option<String>,
}

/// POST /api/whitelist/device/toggle — быстрое переключение устройства в режиме белых списков
/// Синхронизация рантайма БС после коммита конфигурации (баг 38): туннель БС для активного сервера
/// и полный перехват iptables XKEEN_BS. Общая для save_whitelist и toggle_whitelist_device.
pub async fn sync_whitelist_runtime_after_commit(state: &AppState, cfg: &config::AppConfig) {
    if cfg.whitelist.enabled {
        let active_id = mihomo::get_active_server(&state.http, cfg).await.ok().flatten().map(|s| s.id).unwrap_or_default();
        if !active_id.is_empty() {
            let _ = sync_whitelist_mode_tunnel(state, &active_id).await;
        }
    }
    // BS-FIX: полный перехват всех портов (iptables XKEEN_BS) применяется/снимается сразу,
    // а не только после перезапуска панели
    let yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let ports = routing::redir_ports(&yaml);
    let (redir_port, tproxy_port) = (ports.redir, ports.tproxy);
    let dns_port = resolve_active_dns_port(cfg);
    sync_full_tunnel_modes(cfg, redir_port, tproxy_port, dns_port).await;
}

/// Действие над глобальным статусом БС после переключения устройства (баг 38).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WhitelistModeChange {
    /// Переключить в режим «только указанные устройства»
    PerDeviceList,
    /// Полностью выключить БС (так как выключено последнее устройство, активировавшее режим)
    Disable,
    /// Не менять текущий статус (уже включен вручную, или остались другие устройства)
    Keep,
}

/// Чистая функция расчета изменения режима БС при переключении устройства (баг 38).
pub fn whitelist_mode_after_device_toggle(
    cfg: &config::AppConfig,
    target_dev: &config::WhitelistDevice,
    next_enabled: bool,
) -> WhitelistModeChange {
    if next_enabled {
        if !cfg.whitelist.enabled {
            WhitelistModeChange::PerDeviceList
        } else {
            WhitelistModeChange::Keep
        }
    } else {
        // Выключение устройства
        if !cfg.whitelist.enabled {
            return WhitelistModeChange::Keep;
        }
        if cfg.whitelist.enabled_by == config::WhitelistEnabledBy::Devices {
            // Проверяем, останутся ли ещё активные устройства помимо target_dev
            let has_other_enabled = cfg.whitelist.devices.iter().any(|d| {
                let same_mac = !d.mac.is_empty() && !target_dev.mac.is_empty() && d.mac.eq_ignore_ascii_case(&target_dev.mac);
                let same_ip = !d.ip.is_empty() && !target_dev.ip.is_empty() && d.ip == target_dev.ip;
                let is_target = same_mac || same_ip;
                !is_target && d.enabled
            });
            if !has_other_enabled {
                WhitelistModeChange::Disable
            } else {
                WhitelistModeChange::Keep
            }
        } else {
            // Если включен Manual или Failover — выключение устройства не трогает глобальный статус БС
            WhitelistModeChange::Keep
        }
    }
}

/// Баг 43: устройство идентифицируется строго по MAC-адресу, если он передан.
/// Если MAC передан, ищем ТОЛЬКО по MAC, обновляя IP. Если MAC не передан, ищем по IP,
/// но никогда не перезаписываем чужой MAC при DHCP-смене адресов.
pub(crate) fn apply_whitelist_device_toggle(
    devices: &mut Vec<config::WhitelistDevice>,
    mac_str: &str,
    ip_str: &str,
    name_opt: Option<&str>,
    enabled: bool,
) {
    let mut matched = false;
    for dev in devices.iter_mut() {
        let is_target = if !mac_str.is_empty() {
            dev.mac.eq_ignore_ascii_case(mac_str)
        } else if !ip_str.is_empty() {
            dev.ip == ip_str
        } else {
            false
        };

        if is_target {
            dev.enabled = enabled;
            if !ip_str.is_empty() {
                dev.ip = ip_str.to_string();
            }
            if dev.mac.is_empty() && !mac_str.is_empty() {
                dev.mac = mac_str.to_string();
            }
            if let Some(n) = name_opt {
                if !n.trim().is_empty() {
                    dev.name = Some(n.to_string());
                }
            }
            matched = true;
            break;
        }
    }

    if !matched && enabled {
        devices.push(config::WhitelistDevice {
            mac: mac_str.to_string(),
            ip: ip_str.to_string(),
            enabled: true,
            name: name_opt.map(|s| s.to_string()),
            server: None,
        });
    }
}

pub async fn toggle_whitelist_device(
    State(state): State<AppState>,
    Json(body): Json<WhitelistDeviceToggleReq>,
) -> Response {
    let mut ip_str = body.ip.as_deref().unwrap_or("").trim().to_string();
    let mut mac_str = body.mac.as_deref().unwrap_or("").trim().to_string();
    let mut name_opt = body.name.clone();

    if ip_str.is_empty() && mac_str.is_empty() {
        return api_err("Не указан IP или MAC адрес устройства");
    }

    let cfg_snap = state.config.read().await.clone();
    if ip_str.is_empty() || mac_str.is_empty() {
        let policies = rci::get_policies(&state.http, &cfg_snap).await.unwrap_or_default();
        if let Ok(devs) = rci::get_devices(&state.http, &cfg_snap, &policies, "").await {
            if let Some(found) = devs.iter().find(|d| {
                (!mac_str.is_empty() && d.mac.eq_ignore_ascii_case(&mac_str))
                    || (!ip_str.is_empty() && d.ip == ip_str)
            }) {
                if ip_str.is_empty() { ip_str = found.ip.clone(); }
                if mac_str.is_empty() { mac_str = found.mac.clone(); }
                if name_opt.is_none() || name_opt.as_deref() == Some("") {
                    name_opt = Some(found.name.clone());
                }
            }
        }
    }

    if !ip_str.is_empty() && !config::is_valid_ip_or_cidr(&ip_str) {
        return api_err("Некорректный IP-адрес устройства");
    }
    if !mac_str.is_empty() && !config::is_valid_mac(&mac_str) {
        return api_err("Некорректный MAC-адрес устройства");
    }

    let mut tx = match ConfigTx::begin(&state).await {
        Ok(tx) => tx,
        Err(e) => return api_err(e),
    };

    apply_whitelist_device_toggle(
        &mut tx.config_mut().whitelist.devices,
        &mac_str,
        &ip_str,
        name_opt.as_deref(),
        body.enabled,
    );

    let target_dev_probe = config::WhitelistDevice {
        mac: mac_str.clone(),
        ip: ip_str.clone(),
        enabled: body.enabled,
        name: name_opt.clone(),
        server: None,
    };
    let mode_change = whitelist_mode_after_device_toggle(tx.config(), &target_dev_probe, body.enabled);
    match mode_change {
        WhitelistModeChange::PerDeviceList => {
            tx.config_mut().whitelist.enabled = true;
            tx.config_mut().whitelist.enabled_by = config::WhitelistEnabledBy::Devices;
        }
        WhitelistModeChange::Disable => {
            tx.config_mut().whitelist.enabled = false;
        }
        WhitelistModeChange::Keep => {}
    }

    let conflicts = config::device_conflicts(tx.config());
    if !conflicts.is_empty() {
        return api_err_bad_request(config::format_device_conflicts(&conflicts));
    }

    let raw_yaml = match tx.read_yaml().await {
        Ok(y) => y,
        Err(e) => return api_err(e),
    };

    let (new_yaml, _) = match routing::apply_routing(&raw_yaml, tx.config()) {
        Ok(res) => res,
        Err(e) => return api_err(format!("Ошибка генерации правил маршрутизации: {e}")),
    };

    if let Err(e) = tx.set_yaml(&new_yaml) {
        return api_err(e);
    }

    if let Err(e) = tx.apply_and_reload().await {
        return api_err(e);
    }

    if let Err(e) = tx.commit().await {
        return api_err(format!("Ошибка сохранения config.json: {e}"));
    }

    // Баг 38: та же синхронизация туннеля БС и iptables XKEEN_BS, что и при сохранении настроек БС
    sync_whitelist_runtime_after_commit(&state, tx.config()).await;

    log_i!(
        "Режим белых списков переключен для устройства ip='{}', mac='{}' -> enabled={}",
        ip_str, mac_str, body.enabled
    );
    api_ok(json!({
        "toggled": true,
        "ip": ip_str,
        "mac": mac_str,
        "enabled": body.enabled,
        "whitelist_enabled": tx.config().whitelist.enabled,
    }))
}

/// GET /api/whitelist/catalog — отечественный каталог из 376 базовых сервисов с категориями
pub async fn get_whitelist_catalog() -> Response {
    let mut categories_map: std::collections::BTreeMap<&'static str, Vec<&'static str>> = std::collections::BTreeMap::new();
    for entry in routing::DOMESTIC_CATALOG {
        categories_map.entry(entry.category).or_default().push(entry.domain);
    }

    api_ok(json!({
        "total": routing::DOMESTIC_WHITELIST_DOMAINS.len(),
        "domains": routing::DOMESTIC_WHITELIST_DOMAINS,
        "categories": categories_map,
        "items": routing::DOMESTIC_CATALOG,
    }))
}


#[cfg(test)]
mod tests {
    use super::super::devices::finish_rci_apply;

    #[test]
    fn regression_bug_121b_rci_save_error_is_reported() {
        let err = finish_rci_apply(1, vec![], Some(Err("flash read-only".to_string())))
            .expect_err("rci::save_config failure must return Err instead of silent Ok");
        assert!(err.contains("flash read-only"));

        let ok = finish_rci_apply(2, vec![], Some(Ok(()))).expect("Ok save must succeed");
        assert_eq!(ok["applied"], 2);
        assert_eq!(ok["saved"], true);

        let none_ok = finish_rci_apply(0, vec!["mac1: timeout".to_string()], None)
            .expect_err("zero applied must return Err");
        assert!(none_ok.contains("timeout"));
    }
}
