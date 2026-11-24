//! Клиент Mihomo (Clash) REST API: серверы, переключение, пинг, reload конфига.

use futures_util::{stream, StreamExt};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::config::AppConfig;
use crate::routing::ip_key_from_group;
use crate::AppState;

/// 'Big PC 192_168_2_118' → '192.168.2.118' (формат AUTO-DEVICE групп).
/// Единая реализация с routing::ip_key_from_group.
pub fn ip_from_group_name(gname: &str) -> Option<String> {
    ip_key_from_group(gname)
}

/// PING-AUTO: время жизни записи кэша пингов (3 цикла фонового обновления по 30 с).
pub const PING_CACHE_TTL_SECS: u64 = 90;

type PingCache = std::sync::RwLock<std::collections::HashMap<String, (i64, std::time::Instant)>>;
static PING_CACHE: std::sync::OnceLock<PingCache> = std::sync::OnceLock::new();

fn ping_cache() -> &'static PingCache {
    PING_CACHE.get_or_init(|| std::sync::RwLock::new(std::collections::HashMap::new()))
}

/// Сохраняет свежие замеры пингов (delay > 0) в общий кэш панели.
/// mihomo не записывает delay-тесты нод провайдеров в историю (она обновляется
/// только его собственным healthcheck раз в ~5 минут), поэтому живые значения
/// храним сами: их пишут фоновое автообновление, ручной замер и замер после
/// переключения сервера, а GET /api/servers накладывает кэш поверх истории.
pub fn store_ping_cache(pings: &BTreeMap<String, i64>) {
    let mut cache = ping_cache().write().unwrap_or_else(|e| e.into_inner());
    let now = std::time::Instant::now();
    for (id, delay) in pings {
        if *delay > 0 {
            cache.insert(id.clone(), (*delay, now));
        } else if *delay < 0 {
            // При подтверждённом -1 (недоступен/таймаут) удаляем устаревший положительный пинг из кэша.
            cache.remove(id);
        }
    }
    // Не даём кэшу расти бесконечно: выкидываем протухшие записи
    if cache.len() > 512 {
        cache.retain(|_, (_, at)| now.duration_since(*at).as_secs() < PING_CACHE_TTL_SECS);
    }
}

/// Свежее (в пределах TTL) значение пинга из кэша панели, если есть.
pub fn cached_ping(id: &str) -> Option<i64> {
    let cache = ping_cache().read().unwrap_or_else(|e| e.into_inner());
    let (delay, at) = cache.get(id)?;
    if at.elapsed().as_secs() < PING_CACHE_TTL_SECS {
        Some(*delay)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub is_active: bool,
    pub is_priority: bool,
    pub ping_ms: i64,
    pub provider: Option<String>,
    pub provider_name: Option<String>,
    pub is_pool: bool,
    pub resolved_ip: Option<String>,
}

/// Быстрое асинхронное разрешение IP-адреса хоста (с таймаутом 250 мс для защиты от зависаний)
pub async fn resolve_host_ip(host: &str) -> Option<String> {
    let clean = host.trim();
    if clean.is_empty() || clean.starts_with("Авто:") || clean.starts_with("Выбран:") {
        return None;
    }
    if clean.parse::<std::net::IpAddr>().is_ok() {
        return Some(clean.to_string());
    }
    match tokio::time::timeout(
        std::time::Duration::from_millis(250),
        tokio::net::lookup_host(format!("{clean}:443")),
    )
    .await
    {
        Ok(Ok(mut addrs)) => addrs.next().map(|a| a.ip().to_string()),
        _ => None,
    }
}

impl AsRef<str> for Server {
    fn as_ref(&self) -> &str {
        &self.id
    }
}

fn auth_header(secret: &str) -> Option<(&'static str, String)> {
    if secret.is_empty() {
        None
    } else {
        Some(("Authorization", format!("Bearer {secret}")))
    }
}

pub async fn m_get(http: &reqwest::Client, cfg: &AppConfig, path: &str) -> Result<Value, String> {
    let url = format!("{}{}", cfg.mihomo_url(), path);
    let mut req = http.get(&url).timeout(std::time::Duration::from_secs(5));
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| format!("Mihomo {path}: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| format!("Mihomo {path}: {e}"))?;
    if !status.is_success() {
        return Err(format!("Mihomo {path}: статус {status}"));
    }
    Ok(body)
}

pub async fn m_put(http: &reqwest::Client, cfg: &AppConfig, path: &str, body: Value, timeout: u64) -> Result<(), String> {
    let url = format!("{}{}", cfg.mihomo_url(), path);
    let mut req = http
        .put(&url)
        .timeout(std::time::Duration::from_secs(timeout))
        .json(&body);
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| format!("Mihomo {path}: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err_body = resp.text().await.unwrap_or_default();
        let err_msg = if !err_body.trim().is_empty() {
            format!("Mihomo {path}: статус {} ({})", status, err_body.trim())
        } else {
            format!("Mihomo {path}: статус {}", status)
        };
        return Err(err_msg);
    }
    Ok(())
}

pub async fn m_delete(http: &reqwest::Client, cfg: &AppConfig, path: &str, timeout: u64) -> Result<(), String> {
    let url = format!("{}{}", cfg.mihomo_url(), path);
    let mut req = http
        .delete(&url)
        .timeout(std::time::Duration::from_secs(timeout));
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| format!("Mihomo {path}: {e}"))?;
    if !resp.status().is_success() {
        let status = resp.status();
        let err_body = resp.text().await.unwrap_or_default();
        let err_msg = if !err_body.trim().is_empty() {
            format!("Mihomo {path}: статус {} ({})", status, err_body.trim())
        } else {
            format!("Mihomo {path}: статус {}", status)
        };
        return Err(err_msg);
    }
    Ok(())
}

/// Сброс всех активных соединений ядра Mihomo, чтобы клиенты не держали сокеты в старый сервер.
pub async fn close_all_connections(http: &reqwest::Client, cfg: &AppConfig) -> Result<(), String> {
    m_delete(http, cfg, "/connections", 3).await.map_err(|e| {
        crate::log_w!("Не удалось закрыть активные соединения Mihomo: {e}");
        e
    })
}

/// Возвращает количество активных соединений в ядре Mihomo.
#[allow(dead_code)]
pub async fn get_connections_count(http: &reqwest::Client, cfg: &AppConfig) -> Result<usize, String> {
    let val = m_get(http, cfg, "/connections").await?;
    Ok(val
        .get("connections")
        .and_then(|c| c.as_array())
        .map(|a| a.len())
        .unwrap_or(0))
}

/// Возвращает количество активных соединений Mihomo, проходящих через конкретный узел.
pub async fn get_node_connections_count(
    http: &reqwest::Client,
    cfg: &AppConfig,
    node_id: &str,
) -> Result<usize, String> {
    let val = m_get(http, cfg, "/connections").await?;
    let conns = val
        .get("connections")
        .and_then(|c| c.as_array())
        .map(|a| a.as_slice())
        .unwrap_or(&[]);
    let target_lower = node_id.to_lowercase();
    let count = conns
        .iter()
        .filter(|c| {
            c.get("chains")
                .and_then(|ch| ch.as_array())
                .map(|arr| {
                    arr.iter().any(|s| {
                        s.as_str()
                            .map(|st| st.to_lowercase() == target_lower)
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false)
        })
        .count();
    Ok(count)
}

/// Селективный сброс сокетов: закрывает только соединения, проходящие через указанный сервер.
/// Соединения DIRECT (банки, Госуслуги, ТВ, SIP) остаются абсолютно нетронутыми!
pub async fn close_server_connections(
    http: &reqwest::Client,
    cfg: &AppConfig,
    server_id: &str,
) -> usize {
    if server_id.is_empty() || server_id.eq_ignore_ascii_case("direct") {
        return 0;
    }
    let Ok(val) = m_get(http, cfg, "/connections").await else {
        return 0;
    };
    let Some(conns) = val.get("connections").and_then(|c| c.as_array()) else {
        return 0;
    };

    let srv_lower = server_id.to_lowercase();
    let mut ids_to_close = Vec::new();

    for c in conns {
        let Some(id) = c.get("id").and_then(|i| i.as_str()) else { continue; };
        let in_chains = c.get("chains")
            .and_then(|ch| ch.as_array())
            .map(|arr| {
                arr.iter().any(|item| {
                    item.as_str().map(|s| s.to_lowercase() == srv_lower).unwrap_or(false)
                })
            })
            .unwrap_or(false);

        if in_chains {
            ids_to_close.push(id.to_string());
        }
    }
    drop(val);

    let count = ids_to_close.len();
    if count == 0 {
        return 0;
    }

    crate::log_i!(
        "Селективный сброс сокетов: закрытие {count} соединений для упавшего узла '{server_id}' (DIRECT не затронут)"
    );

    stream::iter(ids_to_close)
        .for_each_concurrent(10, |cid| async move {
            let _ = m_delete(http, cfg, &format!("/connections/{cid}"), 2).await;
        })
        .await;

    count
}

/// Баг 8: id соединений, открытых указанными LAN-адресами (metadata.sourceIP).
pub fn connection_ids_from_sources(val: &Value, ips: &[String]) -> Vec<String> {
    let Some(conns) = val.get("connections").and_then(|c| c.as_array()) else {
        return Vec::new();
    };
    conns
        .iter()
        .filter(|c| {
            let src = c.get("metadata").and_then(|m| m.get("sourceIP")).and_then(|s| s.as_str()).unwrap_or("");
            !src.is_empty() && ips.iter().any(|ip| ip == src)
        })
        .filter_map(|c| c.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect()
}

/// IP игровых устройств для точечного сброса сокетов: включённые, иначе все известные.
pub fn gaming_device_ips(gaming: &crate::config::GamingConfig) -> Vec<String> {
    let with_ip = |enabled_only: bool| -> Vec<String> {
        gaming
            .devices
            .iter()
            .filter(|d| !enabled_only || d.enabled)
            .map(|d| d.ip.trim().to_string())
            .filter(|ip| !ip.is_empty())
            .collect()
    };
    let enabled = with_ip(true);
    if enabled.is_empty() { with_ip(false) } else { enabled }
}

/// Переключение группы 🎮 Gaming (баг 8): сбрасываются только соединения игровых устройств, чтобы они
/// пошли по новым правилам; остальная LAN (звонки, загрузки, DIRECT) не затрагивается.

/// Баг 54: id соединений, направленных к доменам Google Flow/AI (по metadata.host или rulePayload).
pub fn connection_ids_matching_flow(val: &Value) -> Vec<String> {
    let Some(conns) = val.get("connections").and_then(|c| c.as_array()) else {
        return Vec::new();
    };
    conns
        .iter()
        .filter(|c| {
            let host = c.get("metadata").and_then(|m| m.get("host")).and_then(|s| s.as_str()).unwrap_or("").trim();
            let rule_payload = c.get("rulePayload").and_then(|s| s.as_str()).unwrap_or("").trim();
            let is_flow_domain = |d: &str| {
                if d.is_empty() {
                    return false;
                }
                let dl = d.to_lowercase();
                crate::routing::FLOW_DOMAINS.iter().any(|g| {
                    let gl = g.to_lowercase();
                    dl == gl || dl.ends_with(&format!(".{gl}"))
                })
            };
            is_flow_domain(host) || is_flow_domain(rule_payload)
        })
        .filter_map(|c| c.get("id").and_then(|i| i.as_str()).map(String::from))
        .collect()
}

/// Баг 54: точечный сброс соединений только для доменов Google Flow.
/// В отличие от close_all_connections, не рвёт соединения всей LAN (банки, Госуслуги, ТВ, SIP, загрузки).
pub async fn close_flow_connections(http: &reqwest::Client, cfg: &AppConfig) -> usize {
    let Ok(val) = m_get(http, cfg, "/connections").await else {
        return 0;
    };
    let ids_to_close = connection_ids_matching_flow(&val);
    drop(val);

    let count = ids_to_close.len();
    if count == 0 {
        return 0;
    }

    crate::log_i!("Селективный сброс сокетов Flow: закрытие {count} соединений для доменов Google AI");

    stream::iter(ids_to_close)
        .for_each_concurrent(10, |cid| async move {
            let _ = m_delete(http, cfg, &format!("/connections/{cid}"), 2).await;
        })
        .await;

    count
}

pub async fn switch_gaming_group(http: &reqwest::Client, cfg: &AppConfig, server: &str) -> Result<(), String> {
    switch_selector(http, cfg, crate::routing::GAMING_GROUP_NAME, server).await?;
    let ips = gaming_device_ips(&cfg.gaming);
    if ips.is_empty() {
        let _ = close_server_connections(http, cfg, crate::routing::GAMING_GROUP_NAME).await;
        return Ok(());
    }
    let Ok(val) = m_get(http, cfg, "/connections").await else {
        return Ok(());
    };
    for cid in connection_ids_from_sources(&val, &ips) {
        let _ = m_delete(http, cfg, &format!("/connections/{cid}"), 2).await;
    }
    Ok(())
}

// =========================================================================
// FO-MHO-01: 3-Second TTL Cache & Resilient Fallback for Mihomo Proxies
// =========================================================================

pub const PROXIES_CACHE_TTL_SECS: u64 = 3;
pub const PROXIES_CACHE_MAX_STALE_SECS: u64 = 30;

#[derive(Clone, Debug)]
pub struct ProxiesCacheEntry {
    pub cached_at: std::time::Instant,
    pub endpoint: Option<(String, u16)>,
    pub data: BTreeMap<String, Value>,
}

static PROXIES_CACHE: std::sync::RwLock<Option<ProxiesCacheEntry>> = std::sync::RwLock::new(None);
static PROVIDER_PROXIES_CACHE: std::sync::RwLock<Option<ProxiesCacheEntry>> = std::sync::RwLock::new(None);
static PROVIDER_PROXIES_FETCH_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static LAST_SELECTED_TUNNEL_SERVER: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

/// Возвращает последний конкретный узел туннеля, выбранный в рантайме (вручную, failover или детектором БС).
pub fn last_selected_tunnel_server() -> Option<String> {
    LAST_SELECTED_TUNNEL_SERVER
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Фиксирует последний выбранный конкретный узел туннеля в рантайме.
pub fn record_selected_tunnel_server(server: &str) {
    let trimmed = server.trim();
    if is_concrete_server_candidate(trimmed) {
        let mut guard = LAST_SELECTED_TUNNEL_SERVER.write().unwrap_or_else(|e| e.into_inner());
        *guard = Some(trimmed.to_string());
    }
}

/// Очищает последний выбранный узел туннеля (например, при ручном отключении в DIRECT).
pub fn clear_selected_tunnel_server() {
    let mut guard = LAST_SELECTED_TUNNEL_SERVER.write().unwrap_or_else(|e| e.into_inner());
    *guard = None;
}

/// Инвалидация кэша прокси и провайдеров: вызывается при переключении узлов, групп или reload конфига.
pub fn invalidate_proxies_cache() {
    let mut guard = PROXIES_CACHE.write().unwrap_or_else(|e| e.into_inner());
    *guard = None;
    let mut p_guard = PROVIDER_PROXIES_CACHE.write().unwrap_or_else(|e| e.into_inner());
    *p_guard = None;
}

/// Проверка наличия свежего кэша в пределах TTL (3 секунды).
pub fn get_cached_proxies_if_fresh() -> Option<BTreeMap<String, Value>> {
    let guard = PROXIES_CACHE.read().unwrap_or_else(|e| e.into_inner());
    if let Some(ref entry) = *guard {
        if entry.cached_at.elapsed() < std::time::Duration::from_secs(PROXIES_CACHE_TTL_SECS) {
            return Some(entry.data.clone());
        }
    }
    None
}

fn get_cached_proxies_if_fresh_for(host: &str, port: u16) -> Option<BTreeMap<String, Value>> {
    let guard = PROXIES_CACHE.read().unwrap_or_else(|e| e.into_inner());
    if let Some(ref entry) = *guard {
        let ep_ok = entry
            .endpoint
            .as_ref()
            .map_or(true, |(h, p)| h == host && *p == port);
        if ep_ok && entry.cached_at.elapsed() < std::time::Duration::from_secs(PROXIES_CACHE_TTL_SECS) {
            return Some(entry.data.clone());
        }
    }
    None
}

fn get_cached_provider_proxies_if_fresh_for(host: &str, port: u16) -> Option<BTreeMap<String, Value>> {
    let guard = PROVIDER_PROXIES_CACHE.read().unwrap_or_else(|e| e.into_inner());
    if let Some(ref entry) = *guard {
        let ep_ok = entry
            .endpoint
            .as_ref()
            .map_or(true, |(h, p)| h == host && *p == port);
        if ep_ok && entry.cached_at.elapsed() < std::time::Duration::from_secs(PROXIES_CACHE_TTL_SECS) {
            return Some(entry.data.clone());
        }
    }
    None
}

/// Получение устаревшего кэша в пределах допустимого окна (до 30 секунд) при сбое сети.
pub fn get_cached_proxies_if_recent() -> Option<(BTreeMap<String, Value>, std::time::Duration)> {
    let guard = PROXIES_CACHE.read().unwrap_or_else(|e| e.into_inner());
    if let Some(ref entry) = *guard {
        let age = entry.cached_at.elapsed();
        if age < std::time::Duration::from_secs(PROXIES_CACHE_MAX_STALE_SECS) {
            return Some((entry.data.clone(), age));
        }
    }
    None
}

fn get_cached_proxies_if_recent_for(host: &str, port: u16) -> Option<(BTreeMap<String, Value>, std::time::Duration)> {
    let guard = PROXIES_CACHE.read().unwrap_or_else(|e| e.into_inner());
    if let Some(ref entry) = *guard {
        let ep_ok = entry
            .endpoint
            .as_ref()
            .map_or(true, |(h, p)| h == host && *p == port);
        let age = entry.cached_at.elapsed();
        if ep_ok && age < std::time::Duration::from_secs(PROXIES_CACHE_MAX_STALE_SECS) {
            return Some((entry.data.clone(), age));
        }
    }
    None
}

pub fn get_cached_provider_proxies_if_recent() -> Option<(BTreeMap<String, Value>, std::time::Duration)> {
    let guard = PROVIDER_PROXIES_CACHE.read().unwrap_or_else(|e| e.into_inner());
    if let Some(ref entry) = *guard {
        let age = entry.cached_at.elapsed();
        if age < std::time::Duration::from_secs(PROXIES_CACHE_MAX_STALE_SECS) {
            return Some((entry.data.clone(), age));
        }
    }
    None
}

fn get_cached_provider_proxies_if_recent_for(host: &str, port: u16) -> Option<(BTreeMap<String, Value>, std::time::Duration)> {
    let guard = PROVIDER_PROXIES_CACHE.read().unwrap_or_else(|e| e.into_inner());
    if let Some(ref entry) = *guard {
        let ep_ok = entry
            .endpoint
            .as_ref()
            .map_or(true, |(h, p)| h == host && *p == port);
        let age = entry.cached_at.elapsed();
        if ep_ok && age < std::time::Duration::from_secs(PROXIES_CACHE_MAX_STALE_SECS) {
            return Some((entry.data.clone(), age));
        }
    }
    None
}

/// Mihomo отвечает на /proxies объектом {"proxies": {...}} — достаём карту.
/// Оптимизировано 3-секундным TTL-кэшем с защитой от сбоев (stale-if-error).
pub async fn get_proxies(http: &reqwest::Client, cfg: &AppConfig) -> Result<BTreeMap<String, Value>, String> {
    // 1. Быстрый возврат из кэша (zero network I/O, zero JSON parsing)
    if let Some(cached) = get_cached_proxies_if_fresh_for(&cfg.mihomo.host, cfg.mihomo.port) {
        return Ok(cached);
    }

    // 2. Сетевой запрос к Mihomo API
    let v = match m_get(http, cfg, "/proxies").await {
        Ok(val) => val,
        Err(err) => {
            // Resilient Fallback: если ядро временно перезагружается или занято,
            // возвращаем недавний кэш (до 30 с), предотвращая ложный сбой failover
            if let Some((stale_data, age)) = get_cached_proxies_if_recent_for(&cfg.mihomo.host, cfg.mihomo.port) {
                crate::log_w!(
                    "Mihomo /proxies временно недоступен ({}), отдаём кэш возрастом {:.1} с",
                    err,
                    age.as_secs_f64()
                );
                return Ok(stale_data);
            }
            return Err(err);
        }
    };

    // 3. Десериализация ответа
    let proxies_val = v
        .get("proxies")
        .ok_or_else(|| "Ответ Mihomo не содержит объект 'proxies' (ядро перезагружается)".to_string())?;
    let map = proxies_val
        .as_object()
        .ok_or_else(|| "Поле 'proxies' ответа Mihomo не является объектом".to_string())?;

    let mut out = BTreeMap::new();
    for (k, val) in map {
        out.insert(k.clone(), val.clone());
    }

    // 4. Сохранение в кэш
    {
        let mut guard = PROXIES_CACHE.write().unwrap_or_else(|e| e.into_inner());
        *guard = Some(ProxiesCacheEntry {
            cached_at: std::time::Instant::now(),
            endpoint: Some((cfg.mihomo.host.clone(), cfg.mihomo.port)),
            data: out.clone(),
        });
    }

    Ok(out)
}
/// Mihomo отвечает на /providers/proxies объектом {"providers": {...}} — разворачиваем в карту
/// «имя прокси → данные». Провайдеры с vehicleType "Compatible" (служебные DIRECT/REJECT) пропускаем.
/// Оптимизировано 3-секундным TTL-кэшем с fallback-защитой при сетевых сбоях ядра.
pub async fn get_provider_proxies(http: &reqwest::Client, cfg: &AppConfig) -> Result<BTreeMap<String, Value>, String> {
    if let Some(cached) = get_cached_provider_proxies_if_fresh_for(&cfg.mihomo.host, cfg.mihomo.port) {
        return Ok(cached);
    }

    let _fetch_guard = PROVIDER_PROXIES_FETCH_LOCK.lock().await;
    if let Some(cached) = get_cached_provider_proxies_if_fresh_for(&cfg.mihomo.host, cfg.mihomo.port) {
        return Ok(cached);
    }

    let v = match m_get(http, cfg, "/providers/proxies").await {
        Ok(val) => val,
        Err(err) => {
            if let Some((stale_data, age)) = get_cached_provider_proxies_if_recent_for(&cfg.mihomo.host, cfg.mihomo.port) {
                crate::log_w!(
                    "Mihomo /providers/proxies временно недоступен ({}), отдаём кэш возрастом {:.1} с",
                    err,
                    age.as_secs_f64()
                );
                return Ok(stale_data);
            }
            return Err(err);
        }
    };
    let mut out = BTreeMap::new();
    if let Some(map) = v.get("providers").and_then(|p| p.as_object()) {
        for (pname, prov) in map {
            if prov.get("vehicleType").and_then(|t| t.as_str()) == Some("Compatible") {
                continue;
            }
            if crate::routing::is_service_provider(pname) {
                continue;
            }
            if cfg.disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case(pname)) {
                continue;
            }
            if let Some(list) = prov.get("proxies").and_then(|p| p.as_array()) {
                for p in list {
                    if let Some(name) = p.get("name").and_then(|n| n.as_str()) {
                        let mut providers_for_name: Vec<Value> = out
                            .get(name)
                            .and_then(|prev: &Value| prev.get("_providers"))
                            .and_then(|arr: &Value| arr.as_array().cloned())
                            .unwrap_or_default();
                        if !providers_for_name.iter().any(|v| v.as_str() == Some(pname.as_str())) {
                            providers_for_name.push(Value::String(pname.clone()));
                        }
                        let mut p_val = p.clone();
                        if let Some(obj) = p_val.as_object_mut() {
                            obj.insert("_provider".to_string(), Value::String(pname.clone()));
                            obj.insert("_providers".to_string(), Value::Array(providers_for_name));
                        }
                        out.insert(name.to_string(), p_val);
                    }
                }
            }
        }
    }
    {
        let mut guard = PROVIDER_PROXIES_CACHE.write().unwrap_or_else(|e| e.into_inner());
        *guard = Some(ProxiesCacheEntry {
            cached_at: std::time::Instant::now(),
            endpoint: Some((cfg.mihomo.host.clone(), cfg.mihomo.port)),
            data: out.clone(),
        });
    }
    Ok(out)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SubscriptionUserinfo {
    pub upload: u64,
    pub download: u64,
    pub total: u64,
    pub expire: i64,
}

pub fn parse_subscription_userinfo_str(val: &str) -> Option<SubscriptionUserinfo> {
    let mut info = SubscriptionUserinfo::default();
    let mut found = false;
    for part in val.split(';') {
        let p = part.trim();
        if let Some((k, v)) = p.split_once('=') {
            let key = k.trim().to_ascii_lowercase();
            let v_str = v.trim();
            match key.as_str() {
                "upload" => {
                    if let Ok(num) = v_str.parse::<u64>() {
                        info.upload = num;
                        found = true;
                    }
                }
                "download" => {
                    if let Ok(num) = v_str.parse::<u64>() {
                        info.download = num;
                        found = true;
                    }
                }
                "total" => {
                    if let Ok(num) = v_str.parse::<u64>() {
                        info.total = num;
                        found = true;
                    }
                }
                "expire" => {
                    if let Ok(num) = v_str.parse::<i64>() {
                        info.expire = num;
                        found = true;
                    }
                }
                _ => {}
            }
        }
    }
    if found {
        Some(info)
    } else {
        None
    }
}

pub fn extract_subscription_userinfo_from_body(body: &str) -> Option<SubscriptionUserinfo> {
    for line in body.lines().take(50) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("#") {
            let rest_trimmed = rest.trim();
            if let Some(val) = rest_trimmed.strip_prefix("subscription-userinfo:") {
                if let Some(info) = parse_subscription_userinfo_str(val) {
                    return Some(info);
                }
            } else if let Some(val) = rest_trimmed.strip_prefix("Subscription-Userinfo:") {
                if let Some(info) = parse_subscription_userinfo_str(val) {
                    return Some(info);
                }
            }
        }
    }
    None
}

pub fn format_subscription_userinfo_line(info: &SubscriptionUserinfo) -> String {
    format!(
        "# subscription-userinfo: upload={}; download={}; total={}; expire={}\n",
        info.upload, info.download, info.total, info.expire
    )
}

pub fn inject_or_replace_userinfo_in_yaml(yaml: &str, info: &SubscriptionUserinfo) -> String {
    let userinfo_line = format_subscription_userinfo_line(info);
    let mut lines: Vec<&str> = yaml.lines().collect();
    if !lines.is_empty() && lines[0].trim_start().to_ascii_lowercase().starts_with("# subscription-userinfo:") {
        lines.remove(0);
    }
    let rest = lines.join("\n");
    let mut out = userinfo_line;
    out.push_str(&rest);
    if yaml.ends_with('\n') && !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    pub count: usize,
    pub vehicle_type: String,
    pub updated_at: Option<String>,
    pub url: Option<String>,
    pub hwid: Option<String>,
    pub enabled: bool,
    pub upload_bytes: Option<u64>,
    pub download_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
    pub expire_timestamp: Option<i64>,
}

/// Список всех подписок (proxy-providers) с метаданными и пользовательскими псевдонимами.
pub async fn get_providers_info(http: &reqwest::Client, cfg: &AppConfig) -> Result<Vec<ProviderInfo>, String> {
    let v = m_get(http, cfg, "/providers/proxies").await?;
    let (urls, hwids) = if let Ok(yaml) = tokio::fs::read_to_string(&cfg.mihomo.config_path).await {
        (
            crate::routing::parse_provider_urls(&yaml),
            crate::routing::parse_provider_hwids(&yaml),
        )
    } else {
        (std::collections::BTreeMap::new(), std::collections::BTreeMap::new())
    };
    let mut out = Vec::new();
    if let Some(map) = v.get("providers").and_then(|p| p.as_object()) {
        for (pname, prov) in map {
            if pname == crate::vkturn::PROVIDER_ID
                || crate::routing::is_service_provider(pname)
                || prov.get("vehicleType").and_then(|t| t.as_str()) == Some("Compatible")
            {
                continue;
            }
            let count = prov.get("proxies").and_then(|p| p.as_array()).map(|a| a.len()).unwrap_or(0);
            let vtype = prov.get("vehicleType").and_then(|t| t.as_str()).unwrap_or("HTTP").to_string();
            let updated = prov.get("updatedAt").and_then(|u| u.as_str()).map(|s| s.to_string());
            let display_name = cfg.provider_aliases.get(pname).cloned().unwrap_or_else(|| pname.clone());
            let is_enabled = !cfg.disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case(pname));

            let sub_info = prov.get("subscriptionInfo").and_then(|s| s.as_object());
            let mut upload_bytes = sub_info
                .and_then(|s| s.get("Upload").or_else(|| s.get("upload")))
                .and_then(|v| v.as_u64());
            let mut download_bytes = sub_info
                .and_then(|s| s.get("Download").or_else(|| s.get("download")))
                .and_then(|v| v.as_u64());
            let mut total_bytes = sub_info
                .and_then(|s| s.get("Total").or_else(|| s.get("total")))
                .and_then(|v| v.as_u64());
            let mut expire_timestamp = sub_info
                .and_then(|s| s.get("Expire").or_else(|| s.get("expire")))
                .and_then(|v| v.as_i64());

            // Для локальных файлов подписок (vehicle_type == "File") Mihomo может отдавать
            // устаревший frozen subscriptionInfo из cache.db, либо не отдавать вовсе.
            // Поэтому ВСЕГДА проверяем актуальную заголовочную строку # subscription-userinfo
            // из файла ./providers/{id}.yaml и отдаем ей безусловный приоритет.
            let is_file_vehicle = vtype.eq_ignore_ascii_case("File");
            if total_bytes.is_none() || is_file_vehicle {
                let prov_dir = std::path::Path::new(&cfg.mihomo.config_path)
                    .parent()
                    .map(|p| p.join("providers"))
                    .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));
                let prov_path = prov_dir.join(format!("{pname}.yaml"));
                if let Ok(content) = tokio::fs::read_to_string(&prov_path).await {
                    if let Some(userinfo) = extract_subscription_userinfo_from_body(&content) {
                        upload_bytes = Some(userinfo.upload);
                        download_bytes = Some(userinfo.download);
                        total_bytes = Some(userinfo.total);
                        expire_timestamp = Some(userinfo.expire);
                    }
                }
            }

            out.push(ProviderInfo {
                id: pname.clone(),
                name: display_name,
                count,
                vehicle_type: vtype,
                updated_at: updated,
                url: urls.get(pname).cloned(),
                hwid: hwids.get(pname).cloned(),
                enabled: is_enabled,
                upload_bytes,
                download_bytes,
                total_bytes,
                expire_timestamp,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Имена провайдеров, которых нужно принудительно обновлять (vehicleType HTTP/File).
pub async fn updatable_provider_names(http: &reqwest::Client, cfg: &AppConfig) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(v) = m_get(http, cfg, "/providers/proxies").await {
        if let Some(map) = v.get("providers").and_then(|p| p.as_object()) {
            for (pname, prov) in map {
                if pname == crate::vkturn::PROVIDER_ID || crate::routing::is_service_provider(pname) {
                    continue;
                }
                if let Some(t) = prov.get("vehicleType").and_then(|t| t.as_str()) {
                    if t == "HTTP" || t == "File" {
                        out.push(pname.clone());
                    }
                }
            }
        }
    }
    out
}

/// Принудительное обновление провайдера: exclude-filter применяется при загрузке,
/// поэтому после правки config.yaml провайдера нужно перечитать (PUT, пустой ответ = ок).
pub async fn force_update_provider(http: &reqwest::Client, cfg: &AppConfig, name: &str) -> Result<(), String> {
    let enc = urlencoding_lite(name);
    let url = format!("{}{}", cfg.mihomo_url(), format_args!("/providers/proxies/{enc}"));
    let mut req = http
        .put(&url)
        .timeout(std::time::Duration::from_secs(60))
        .header("Content-Length", "0");
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| format!("Mihomo update {name}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Mihomo update {name}: статус {}", resp.status()));
    }
    invalidate_proxies_cache();
    Ok(())
}

/// Принудительное обновление всех провайдеров (после правки exclude-filter).
pub async fn force_update_all_providers(http: &reqwest::Client, cfg: &AppConfig) -> usize {
    let mut ok = 0usize;
    for name in updatable_provider_names(http, cfg).await {
        if force_update_provider(http, cfg, &name).await.is_ok() {
            ok += 1;
        }
    }
    ok
}

pub const FREE_SUB_L7_PRIMARY_URL: &str = "https://www.gstatic.com/generate_204";
pub const FREE_SUB_L7_FALLBACK_URL: &str = "https://cp.cloudflare.com/generate_204";
pub const FREE_SUB_L7_VERIFY_URL: &str = "https://www.cloudflare.com/cdn-cgi/trace";
pub const FREE_SUB_L7_THROUGHPUT_URL: &str = "https://speed.cloudflare.com/__down?bytes=50000";
pub const FREE_SUB_L7_BLOCKED_SITE_URL: &str = "https://www.youtube.com/generate_204";
pub const FREE_SUB_L7_BLOCKED_FALLBACK_URL: &str = "https://discord.com/cdn-cgi/trace";
pub const FREE_SUB_L7_TELEGRAM_URL: &str = "https://telegram.org/dl";

/// Строгий разбор ответа точечного L7 healthcheck Mihomo (`/providers/proxies/{provider}/{node}/healthcheck`).
/// Отклоняет любые HTTP-ошибки, JSON с `error`/`message` и `delay <= 0` без опоры на старую историю.
pub fn parse_l7_healthcheck_response(status: reqwest::StatusCode, body: &str) -> Result<i64, String> {
    if !status.is_success() {
        return Err(format!("L7 healthcheck HTTP {status}"));
    }
    let val: Value = serde_json::from_str(body.trim())
        .map_err(|e| format!("Некорректный JSON ответа healthcheck: {e}"))?;
    if let Some(err_msg) = val
        .get("error")
        .or_else(|| val.get("message"))
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return Err(format!("L7 healthcheck ошибка: {err_msg}"));
    }
    let delay = val.get("delay").and_then(|d| d.as_i64()).unwrap_or(0);
    if delay <= 0 {
        return Err("L7 healthcheck вернул delay <= 0".into());
    }
    Ok(delay)
}

async fn run_single_provider_node_healthcheck(
    http: &reqwest::Client,
    cfg: &AppConfig,
    provider_id: &str,
    node_name: &str,
    target_url: &str,
    expected_status: u16,
    timeout_ms: u64,
) -> Result<i64, String> {
    let prov_enc = urlencoding_lite(provider_id);
    let node_enc = urlencoding_lite(node_name);
    let check_url = urlencoding_lite(target_url);
    let url = format!(
        "{}/providers/proxies/{prov_enc}/{node_enc}/healthcheck?url={check_url}&timeout={timeout_ms}&expected={expected_status}",
        cfg.mihomo_url()
    );
    let req_timeout = std::time::Duration::from_millis(std::cmp::max(timeout_ms + 2000, 5000));
    let mut req = http.get(&url).timeout(req_timeout);
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| format!("L7 healthcheck запрос: {e}"))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    parse_l7_healthcheck_response(status, &body)
}

fn should_fallback_l7_primary_error(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("deadline")
        || lower.contains("connection refused")
        || lower.contains("reset")
        || lower.contains("eof")
        || lower.contains("error trying to connect")
    {
        return false;
    }
    lower.contains("http ")
        || ["400", "401", "403", "404", "429", "500", "502", "503", "504"]
            .iter()
            .any(|code| lower.contains(code))
}

/// Точечная L7-валидация узла через отдельный служебный провайдер Mihomo (без fallback на PROXY/историю).
/// При сбое основной HTTPS-цели проверяет резервную независимую HTTPS-цель (только при HTTP-ошибке, не при таймауте).
pub async fn probe_service_provider_node_l7(
    http: &reqwest::Client,
    cfg: &AppConfig,
    provider_id: &str,
    node_name: &str,
    timeout_ms: u64,
    primary_url: Option<&str>,
    fallback_url: Option<&str>,
) -> Result<i64, String> {
    let primary = primary_url.unwrap_or(FREE_SUB_L7_PRIMARY_URL);
    match run_single_provider_node_healthcheck(http, cfg, provider_id, node_name, primary, 204, timeout_ms).await {
        Ok(delay) => Ok(delay),
        Err(first_err) => {
            if !should_fallback_l7_primary_error(&first_err) {
                return Err(first_err);
            }
            let fallback = fallback_url.unwrap_or(FREE_SUB_L7_FALLBACK_URL);
            if fallback == primary {
                return Err(first_err);
            }
            run_single_provider_node_healthcheck(http, cfg, provider_id, node_name, fallback, 204, timeout_ms)
                .await
                .map_err(|second_err| format!("{first_err}; fallback: {second_err}"))
        }
    }
}

fn is_throughput_endpoint_http_rejection(err: &str) -> bool {
    let lower = err.to_ascii_lowercase();
    if lower.contains("timeout") || lower.contains("timed out") || lower.contains("deadline") {
        return false;
    }
    ["403", "500", "502", "503", "504"]
        .iter()
        .any(|code| lower.contains(code))
}

/// Расчёт итогового скора L7-пробы с учётом реальной пропускной способности под нагрузкой (50 КБ).
/// Медленные перегруженные узлы с большим временем скачивания payload получают штраф `2x` за время передачи,
/// чтобы `should_upgrade_active_node` автоматически переключал туннель на более быстрый сервер.
pub fn compute_l7_stable_score(
    first_delay: i64,
    throughput_delay: i64,
    blocked_delay: i64,
    used_throughput_probe: bool,
) -> i64 {
    if !used_throughput_probe {
        return (first_delay + throughput_delay + blocked_delay) / 3;
    }
    let base_rtt = (first_delay + blocked_delay) / 2;
    let payload_transfer_ms = (throughput_delay - first_delay).max(0);
    base_rtt + payload_transfer_ms * 2
}

/// Stability Gate: тройная последовательная L7-проверка узла.
/// 1) `generate_204` (status 204)
/// 2) Throughput Gate: загрузка 50 КБ (`speed.cloudflare.com/__down?bytes=50000`, status 200, таймаут `min(timeout_ms, 3000)`) —
///    отсеивает зашейпленные ТСПУ узлы (2–10 КБ/с), с fallback на `cdn-cgi/trace` только при HTTP 403/5xx самого эндпоинта.
/// 3) Проверка доступа к заблокированному в РФ ресурсу (`youtube.com/generate_204`, status 204) — отсеивает российские реле под ТСПУ.
pub async fn probe_service_provider_node_l7_stable(
    http: &reqwest::Client,
    cfg: &AppConfig,
    provider_id: &str,
    node_name: &str,
    timeout_ms: u64,
) -> Result<i64, String> {
    let first_delay = probe_service_provider_node_l7(
        http,
        cfg,
        provider_id,
        node_name,
        timeout_ms,
        None,
        None,
    )
    .await?;
    let tp_timeout_ms = timeout_ms.min(3000);
    let mut used_throughput_probe = true;
    let second_delay = match run_single_provider_node_healthcheck(
        http,
        cfg,
        provider_id,
        node_name,
        FREE_SUB_L7_THROUGHPUT_URL,
        200,
        tp_timeout_ms,
    )
    .await
    {
        Ok(d) => d,
        Err(tp_err) if is_throughput_endpoint_http_rejection(&tp_err) => {
            used_throughput_probe = false;
            run_single_provider_node_healthcheck(
                http,
                cfg,
                provider_id,
                node_name,
                FREE_SUB_L7_VERIFY_URL,
                200,
                timeout_ms,
            )
            .await
            .map_err(|e| format!("Stability gate (throughput fallback cdn-cgi/trace) провален: {e}"))?
        }
        Err(tp_err) => {
            return Err(format!("Stability gate (speed.cloudflare.com 50KB) провален: {tp_err}"));
        }
    };
    let blocked_delay = run_single_provider_node_healthcheck(
        http,
        cfg,
        provider_id,
        node_name,
        FREE_SUB_L7_BLOCKED_SITE_URL,
        204,
        timeout_ms,
    )
    .await
    .map_err(|e| format!("Stability gate (youtube.com/generate_204) провален: {e}"))?;
    Ok(compute_l7_stable_score(
        first_delay,
        second_delay,
        blocked_delay,
        used_throughput_probe,
    ))
}

/// Добавляет оценку доступности Telegram к базовому L7-скору.
/// Если Telegram отвечает быстро (<= 2000 мс) — добавляет 1/3 задержки;
/// если Telegram тормозит (> 2000 мс) — добавляет прогрессивный штраф `tg / 2 + (tg - 2000)`;
/// если Telegram не отвечает/таймаутит — добавляет жёсткий штраф +4000 мс.
pub fn compute_l7_scored_with_telegram(base_score: i64, telegram_delay: Option<i64>) -> i64 {
    match telegram_delay {
        Some(tg) if tg > 2000 => base_score + tg / 2 + (tg - 2000),
        Some(tg) if tg > 0 => base_score + tg / 3,
        _ => base_score + 4000,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct L7ProbeMetrics {
    pub rtt_ms: i64,
    pub quality_score: i64,
}

/// Устойчивый L7-скоринг для автоподбора бесплатных узлов:
/// 1) Обязательный базовый `generate_204` (`gstatic` / `cp.cloudflare`).
/// 2) Обязательная проверка доступа к заблокированному ресурсу (`youtube.com/generate_204`, с fallback на `discord.com/cdn-cgi/trace` при HTTP-отказе) — отсеивает RU-релеи под ТСПУ.
/// 3) Параллельно: замер 50 КБ с `speed.cloudflare.com` и проверка `telegram.org/dl` (status 302).
///    Возвращает отдельно чистый `rtt_ms` (для UI/PING_CACHE) и `quality_score` (со штрафами за throughput/Telegram/fallback).
pub async fn probe_service_provider_node_l7_metrics(
    http: &reqwest::Client,
    cfg: &AppConfig,
    provider_id: &str,
    node_name: &str,
    timeout_ms: u64,
) -> Result<L7ProbeMetrics, String> {
    let first_delay = probe_service_provider_node_l7(
        http,
        cfg,
        provider_id,
        node_name,
        timeout_ms,
        None,
        None,
    )
    .await?;
    let (blocked_delay, blocked_penalty) = match run_single_provider_node_healthcheck(
        http,
        cfg,
        provider_id,
        node_name,
        FREE_SUB_L7_BLOCKED_SITE_URL,
        204,
        timeout_ms,
    )
    .await
    {
        Ok(d) => (d, 0i64),
        Err(yt_err) if should_fallback_l7_primary_error(&yt_err) => {
            let fb_delay = run_single_provider_node_healthcheck(
                http,
                cfg,
                provider_id,
                node_name,
                FREE_SUB_L7_BLOCKED_FALLBACK_URL,
                200,
                timeout_ms.min(2500),
            )
            .await
            .map_err(|fb_err| {
                format!("Stability gate (youtube.com + discord fallback) провален: {yt_err}; fallback: {fb_err}")
            })?;
            (fb_delay, 400i64)
        }
        Err(yt_err) => {
            return Err(format!("Stability gate (youtube.com/generate_204) провален: {yt_err}"));
        }
    };
    let rtt_ms = ((first_delay + blocked_delay) / 2).max(1);

    let tp_timeout_ms = timeout_ms.min(3000);
    let tg_timeout_ms = timeout_ms.min(3500);

    let tp_fut = async {
        match run_single_provider_node_healthcheck(
            http,
            cfg,
            provider_id,
            node_name,
            FREE_SUB_L7_THROUGHPUT_URL,
            200,
            tp_timeout_ms,
        )
        .await
        {
            Ok(d) => compute_l7_stable_score(first_delay, d, blocked_delay, true),
            Err(tp_err) if is_throughput_endpoint_http_rejection(&tp_err) => {
                match run_single_provider_node_healthcheck(
                    http,
                    cfg,
                    provider_id,
                    node_name,
                    FREE_SUB_L7_VERIFY_URL,
                    200,
                    timeout_ms,
                )
                .await
                {
                    Ok(d) => compute_l7_stable_score(first_delay, d, blocked_delay, false),
                    Err(_) => compute_l7_stable_score(
                        first_delay,
                        first_delay + 1250,
                        blocked_delay,
                        true,
                    ),
                }
            }
            Err(_) => compute_l7_stable_score(
                first_delay,
                first_delay + 1250,
                blocked_delay,
                true,
            ),
        }
    };

    let tg_fut = run_single_provider_node_healthcheck(
        http,
        cfg,
        provider_id,
        node_name,
        FREE_SUB_L7_TELEGRAM_URL,
        302,
        tg_timeout_ms,
    );

    let (base_score, tg_res) = tokio::join!(tp_fut, tg_fut);
    let quality_score = compute_l7_scored_with_telegram(base_score + blocked_penalty, tg_res.ok());
    Ok(L7ProbeMetrics {
        rtt_ms,
        quality_score,
    })
}

pub async fn probe_service_provider_node_l7_scored(
    http: &reqwest::Client,
    cfg: &AppConfig,
    provider_id: &str,
    node_name: &str,
    timeout_ms: u64,
) -> Result<i64, String> {
    probe_service_provider_node_l7_metrics(http, cfg, provider_id, node_name, timeout_ms)
        .await
        .map(|m| m.quality_score)
}

/// Рекурсивное разрешение активного «листа» из цепочек selector/fallback/urltest.
pub fn resolve_active_leaf(proxies: &BTreeMap<String, Value>) -> String {
    let mut initial = ["PROXY", "GLOBAL", "Proxy", "auto", "Fallback"]
        .iter()
        .find_map(|g| {
            proxies.get(*g).and_then(|p| p.get("now")).and_then(|n| n.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
    // AUDIT-003: если PROXY в DIRECT или пуст, а 🛡️ Whitelist активен — резолвим лист из 🛡️ Whitelist
    if initial.is_empty() || initial.eq_ignore_ascii_case("DIRECT") {
        if let Some(wl_now) = proxies
            .get(crate::routing::WHITELIST_GROUP_NAME)
            .and_then(|p| p.get("now"))
            .and_then(|n| n.as_str())
            .filter(|s| !s.is_empty() && !s.eq_ignore_ascii_case("DIRECT"))
        {
            initial = wl_now.to_string();
        }
    }
    // Универсальный fallback: нет привычных имён — берём первый selector.
    if initial.is_empty() {
        for p in proxies.values() {
            let typ = p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
            if typ == "selector" {
                if let Some(now) = p.get("now").and_then(|n| n.as_str()) {
                    if !now.is_empty() {
                        initial = now.to_string();
                        break;
                    }
                }
            }
        }
    }
    if initial.is_empty() {
        return String::new();
    }

    let mut cur = initial;
    let mut visited = std::collections::HashSet::new();
    while let Some(sub) = proxies.get(&cur) {
        if visited.len() >= 32 || visited.contains(&cur) {
            break;
        }
        visited.insert(cur.clone());
        let typ = sub.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        if typ != "fallback" && typ != "urltest" && typ != "selector" {
            break;
        }
        match sub.get("now").and_then(|n| n.as_str()) {
            Some(next) if !next.is_empty() && next != cur => cur = next.to_string(),
            _ => break,
        }
    }
    cur
}

// --- Отображение имён: mojibake-ремонт и чистка эмодзи ---

/// Попытка обратной перекодировки: строка была UTF-8, но байты были прочитаны как
/// cp1251/cp1252/cp866 и сохранены как есть. Возвращаем байты исходного UTF-8.
fn decode_as(source: &str, table: fn(char) -> Option<u8>) -> Option<String> {
    let mut bytes = Vec::with_capacity(source.len());
    for c in source.chars() {
        if c.is_ascii() {
            bytes.push(c as u8);
        } else {
            bytes.push(table(c)?);
        }
    }
    String::from_utf8(bytes).ok()
}

fn cp1251_byte(c: char) -> Option<u8> {
    let u = c as u32;
    match u {
        0x0410..=0x044F => Some((u - 0x0410 + 0xC0) as u8),
        0x0401 => Some(0xA8),
        0x0451 => Some(0xB8),
        // Символы cp1251 0x80-0xBF (в mojibake встречаются байты из этого диапазона)
        0x0402 => Some(0x80), 0x0403 => Some(0x81), 0x0404 => Some(0xAA), 0x0405 => Some(0xBD),
        0x0407 => Some(0xAF), 0x0408 => Some(0xA3), 0x0409 => Some(0x8A), 0x040A => Some(0x8C),
        0x040B => Some(0x8E), 0x040C => Some(0x8D), 0x040E => Some(0xA1), 0x040F => Some(0x8F),
        0x0452 => Some(0x90), 0x0453 => Some(0x83), 0x0454 => Some(0xBA), 0x0455 => Some(0xBE),
        0x0456 => Some(0xB3), 0x0457 => Some(0xBF), 0x0458 => Some(0xBC), 0x0459 => Some(0x9A),
        0x045A => Some(0x9C), 0x045B => Some(0x9E), 0x045C => Some(0x9D), 0x045E => Some(0xA2),
        0x045F => Some(0x9F),
        0x00A0 => Some(0xA0), 0x00A4 => Some(0xA4), 0x0490 => Some(0xA5), 0x0491 => Some(0xB4), 0x00A6 => Some(0xA6), 0x00A7 => Some(0xA7),
        0x00A9 => Some(0xA9), 0x00AA => Some(0xAA), 0x00AB => Some(0xAB), 0x00AC => Some(0xAC),
        0x00AD => Some(0xAD), 0x00AE => Some(0xAE), 0x00AF => Some(0xAF),
        0x00B0..=0x00BE => Some(u as u8),
        0x2116 => Some(0xB9),
        // C1-управляющие и латин-1 (mojibake-рендеринг байтов 0x80-0x9F/0xC0-0xFF как latin1)
        0x0080..=0x009F => Some(u as u8),
        0x00A8 | 0x00C0..=0x00FF => Some(u as u8),
        _ => None,
    }
}

fn cp1252_byte(c: char) -> Option<u8> {
    let u = c as u32;
    match u {
        0x20AC => Some(0x80), 0x201A => Some(0x82), 0x0192 => Some(0x83),
        0x201E => Some(0x84), 0x2026 => Some(0x85), 0x2020 => Some(0x86),
        0x2021 => Some(0x87), 0x02C6 => Some(0x88), 0x2030 => Some(0x89),
        0x0160 => Some(0x8A), 0x2039 => Some(0x8B), 0x0152 => Some(0x8C),
        0x017D => Some(0x8E), 0x2018 => Some(0x91), 0x2019 => Some(0x92),
        0x201C => Some(0x93), 0x201D => Some(0x94), 0x2022 => Some(0x95),
        0x2013 => Some(0x96), 0x2014 => Some(0x97), 0x02DC => Some(0x98),
        0x2122 => Some(0x99), 0x0161 => Some(0x9A), 0x203A => Some(0x9B),
        0x0153 => Some(0x9C), 0x017E => Some(0x9E), 0x0178 => Some(0x9F),
        // C1-управляющие (байты 0x80-0x9F, не имеющие представления в cp1252) — passthrough
        0x0080..=0x009F => Some(u as u8),
        0x00A0..=0x00FF => Some(u as u8),
        _ => None,
    }
}

fn cp866_byte(c: char) -> Option<u8> {
    let u = c as u32;
    match u {
        0x0410..=0x043F => Some((u - 0x0410 + 0x80) as u8),
        0x0440..=0x044F => Some((u - 0x0440 + 0xE0) as u8),
        0x0401 => Some(0xF0),
        0x0451 => Some(0xF1),
        // Псевдографика/символы (mojibake от байтов 0xB0-0xDF): «╨д╨╕╨╜…» — это
        // UTF-8 «Фин…», прочитанный как cp866. Без этих веток ремонт не срабатывает.
        0x2591 => Some(0xB0), 0x2592 => Some(0xB1), 0x2593 => Some(0xB2), 0x2502 => Some(0xB3),
        0x2524 => Some(0xB4), 0x2561 => Some(0xB5), 0x2562 => Some(0xB6), 0x2556 => Some(0xB7),
        0x2555 => Some(0xB8), 0x2563 => Some(0xB9), 0x2551 => Some(0xBA), 0x2557 => Some(0xBB),
        0x255D => Some(0xBC), 0x255C => Some(0xBD), 0x255B => Some(0xBE), 0x2510 => Some(0xBF),
        0x2514 => Some(0xC0), 0x2534 => Some(0xC1), 0x252C => Some(0xC2), 0x251C => Some(0xC3),
        0x2500 => Some(0xC4), 0x253C => Some(0xC5), 0x255E => Some(0xC6), 0x255F => Some(0xC7),
        0x255A => Some(0xC8), 0x2554 => Some(0xC9), 0x2569 => Some(0xCA), 0x2566 => Some(0xCB),
        0x2560 => Some(0xCC), 0x2550 => Some(0xCD), 0x256C => Some(0xCE), 0x2567 => Some(0xCF),
        0x2568 => Some(0xD0), 0x2564 => Some(0xD1), 0x2565 => Some(0xD2), 0x2559 => Some(0xD3),
        0x2558 => Some(0xD4), 0x2552 => Some(0xD5), 0x2553 => Some(0xD6), 0x256B => Some(0xD7),
        0x256A => Some(0xD8), 0x2518 => Some(0xD9), 0x250C => Some(0xDA), 0x2588 => Some(0xDB),
        0x2584 => Some(0xDC), 0x258C => Some(0xDD), 0x2590 => Some(0xDE), 0x2580 => Some(0xDF),
        0x240A => Some(0x0A), 0x240D => Some(0x0D), 0x2409 => Some(0x09), 0x2400 => Some(0x00),
        _ => None,
    }
}

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x0400..=0x04FF))
}

/// Качество кандидата ремонта: доля «хороших» символов (буквы/цифры/пробел/
/// пунктуация). Псевдографика (╨╤┌) и прочий мусор снижают оценку.
fn repair_score(s: &str) -> f32 {
    let mut good = 0usize;
    let mut total = 0usize;
    for c in s.chars() {
        total += 1;
        let u = c as u32;
        if c.is_alphanumeric()
            || c.is_whitespace()
            || matches!(u, 0x20..=0x2F | 0x3A..=0x40 | 0x5B..=0x60 | 0x7B..=0x7E | 0x00AB | 0x00BB | 0x2013 | 0x2014)
        {
            good += 1;
        }
    }
    if total == 0 {
        return 0.0;
    }
    good as f32 / total as f32
}

/// Ремонт mojibake: строка — результат чтения UTF-8 как cp1251/cp1252/cp866
/// (однократно или дважды). Каждая таблица применяется СТРОГО: все не-ASCII
/// символы должны маппиться в одну кодовую страницу. Раньше таблицы смешивались
/// посимвольно (cp1251 → cp1252 → cp866), из-за чего ремонт порождал мусор
/// вроде «тЯЗл ЬЯыЗо ⚡⚡⚡» вместо «🇫🇮 Финляндия [⚡ Стабильный]».
pub fn fix_mojibake(s: &str) -> Option<String> {
    let tables: [fn(char) -> Option<u8>; 3] = [cp1251_byte, cp866_byte, cp1252_byte];
    let mut best: Option<(f32, String)> = None;
    for table in tables {
        let Some(fixed) = decode_as(s, table) else {
            continue;
        };
        if !has_cyrillic(&fixed) || fixed == s {
            continue;
        }
        // Рекурсия на случай двойного перекодирования.
        let candidate = fix_mojibake(&fixed).unwrap_or(fixed);
        let score = repair_score(&candidate);
        if best.as_ref().is_none_or(|(bs, _)| score > *bs) {
            best = Some((score, candidate));
        }
    }
    best.map(|(_, s)| s)
}

/// Чистое имя для отображения: без ведущих эмодзи/флагов/fe0f, обрезка пробелов.
/// id/переключение всегда по оригинальному имени (mihomo требует точное имя).
pub fn clean_display_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .skip_while(|c| {
            let u = *c as u32;
            c.is_whitespace()
                || u == 0xFE0F
                || u == 0x200D
                || u == 0x20E3
                || (0x1F000..=0x1FAFF).contains(&u)
                || (0x2600..=0x27BF).contains(&u)
                || (0x2B00..=0x2BFF).contains(&u)
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        name.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Имя для показа в панели: ремонт mojibake с сохранением полного оригинального имени (флаги, эмодзи, теги).
pub fn display_name(name: &str) -> String {
    let repaired = fix_mojibake_smart(name);
    let trimmed = repaired.trim();
    if trimmed.is_empty() {
        name.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Умный ремонт mojibake. Порядок важен:
/// 1) вся строка целиком (байт 0xA0 в mojibake — это NBSP, поэтому split_whitespace
///    рвёт слова — делить нельзя, пока строка не декодируется целиком);
/// 2) срезаем ведущие эмодзи-флаги (они ломают декодирование) и пробуем снова;
/// 3) запасной вариант — по словам (только по обычному пробелу).
pub fn fix_mojibake_smart(s: &str) -> String {
    if let Some(fixed) = fix_mojibake(s) {
        return fixed;
    }
    let stripped = clean_display_name(s);
    if let Some(fixed) = fix_mojibake(&stripped) {
        let prefix: String = s.chars().take_while(|c| !c.is_alphanumeric() && *c != '[' && *c != '(').collect();
        if !prefix.is_empty() {
            return format!("{} {}", prefix.trim(), fixed.trim());
        }
        return fixed;
    }
    let words: Vec<String> = s
        .split(' ')
        .map(|w| fix_mojibake(w).unwrap_or_else(|| w.to_string()))
        .collect();
    let joined = words.join(" ");
    if joined == s {
        s.to_string()
    } else {
        joined
    }
}

pub fn last_delay(p: &Value) -> i64 {
    if let Some(alive) = p.get("alive").and_then(|a| a.as_bool()) {
        if !alive {
            return -1;
        }
    }
    p.get("history")
        .and_then(|h| h.as_array())
        .and_then(|a| a.last())
        .and_then(|e| e.get("delay"))
        .and_then(|d| d.as_i64())
        .filter(|&d| d > 0)
        .unwrap_or(-1)
}

/// Список серверов с синтетическими Fastest/Fallback и активным листом.
/// Определение основной селекторной группы прокси (PROXY, GLOBAL, Proxy или первый selector).
pub fn find_primary_group(proxies: &BTreeMap<String, Value>) -> Option<String> {
    ["PROXY", "GLOBAL", "Proxy"]
        .iter()
        .find(|g| proxies.contains_key(**g))
        .map(|g| g.to_string())
        .or_else(|| {
            proxies
                .iter()
                .find(|(name, p)| {
                    p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase() == "selector"
                        && ip_from_group_name(name).is_none()
                        && *name != "Fastest"
                        && *name != "Fallback"
                })
                .map(|(name, _)| name.clone())
        })
}

pub async fn get_servers(
    http: &reqwest::Client,
    cfg: &AppConfig,
    priority_chain: &[String],
) -> Result<Vec<Server>, String> {
    let proxies = get_proxies(http, cfg).await?;
    let active = resolve_active_leaf(&proxies);
    // Активная группа: привычные имена, иначе первый selector (универсальность).
    let proxy_group = find_primary_group(&proxies);
    let proxy_now = proxy_group
        .as_deref()
        .and_then(|g| proxies.get(g))
        .and_then(|p| p.get("now"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();

    let mut servers: Vec<Server> = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (id, label, icon) in [("Fastest", "URL-TEST", "⚡"), ("Fallback", "FALLBACK", "🛡️")] {
        if let Some(p) = proxies.get(id) {
            seen_names.insert(id.to_string());
            let now_node = p.get("now").and_then(|n| n.as_str()).unwrap_or("");
            let group_ping = last_delay(p);
            let effective_ping = if group_ping > 0 {
                group_ping
            } else if !now_node.is_empty() {
                proxies.get(now_node).map(last_delay).unwrap_or(0)
            } else {
                0
            };
            servers.push(Server {
                id: id.into(),
                name: format!("{icon} {id} (Авто)"),
                protocol: label.into(),
                host: format!("Авто: {}", if now_node.is_empty() { "—" } else { now_node }),
                port: 0,
                is_active: proxy_now == id,
                is_priority: priority_chain.iter().any(|p| p == id),
                ping_ms: effective_ping,
                provider: Some("__pool__".into()),
                provider_name: Some("Группа / Пул".into()),
                is_pool: true,
                resolved_ip: None,
            });
        }
    }

    for (name, p) in &proxies {
        let typ = p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        let is_group = ["selector", "urltest", "fallback", "load"].contains(&typ.as_str());

        if is_group {
            if seen_names.contains(name) || ip_from_group_name(name).is_some() {
                continue;
            }
            seen_names.insert(name.clone());
            let now_target = p.get("now").and_then(|n| n.as_str()).unwrap_or("—");
            let icon = match name.as_str() {
                "PROXY" => "🌐",
                "Google AI" => "🤖",
                "YouTube" => "📺",
                "Discord" => "💬",
                "Telegram" => "✈️",
                "Steam" => "🎮",
                "Twitch" => "🟣",
                "Torrent" => "🧲",
                _ => "🔀",
            };
            let group_delay = last_delay(p);
            let effective_delay = if group_delay > 0 {
                group_delay
            } else if !now_target.is_empty() && now_target != "—" {
                proxies.get(now_target).map(last_delay).unwrap_or(0)
            } else {
                0
            };
            servers.push(Server {
                id: name.clone(),
                name: format!("{icon} {name}"),
                protocol: format!("POOL ({})", typ.to_uppercase()),
                host: format!("Выбран: {now_target}"),
                port: 0,
                is_active: *name == active || proxy_now == *name,
                is_priority: priority_chain.iter().any(|p| p == name),
                ping_ms: effective_delay,
                provider: Some("__pool__".into()),
                provider_name: Some("Группа / Пул".into()),
                is_pool: true,
                resolved_ip: None,
            });
            continue;
        }

        if SKIP_TYPES.contains(&typ.as_str()) {
            continue;
        }
        if seen_names.contains(name) {
            continue;
        }
        seen_names.insert(name.clone());
        let host_str = p.get("server").and_then(|s| s.as_str()).unwrap_or(name).to_string();
        let initial_ip = if host_str.parse::<std::net::IpAddr>().is_ok() { Some(host_str.clone()) } else { None };
        servers.push(Server {
            id: name.clone(),
            name: display_name(name),
            protocol: typ.to_uppercase(),
            host: host_str,
            port: p.get("port").and_then(|s| s.as_u64()).unwrap_or(0) as u16,
            is_active: *name == active,
            is_priority: priority_chain.iter().any(|p| p == name),
            ping_ms: last_delay(p),
            provider: None,
            provider_name: None,
            is_pool: false,
            resolved_ip: initial_ip,
        });
    }

    // Провайдерные серверы: GET /proxies их НЕ содержит — мержим /providers/proxies.
    let provider_proxies = get_provider_proxies(http, cfg).await;
    if let Ok(providers) = &provider_proxies {
        for (name, p) in providers {
            let typ = p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
            if SKIP_TYPES.contains(&typ.as_str()) {
                continue;
            }
            let provider_id = p
                .get("_providers")
                .and_then(|arr| arr.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str())
                        .find(|cand| crate::failover::provider_matches_whitelist(cand, cfg))
                        .map(|s| s.to_string())
                })
                .or_else(|| p.get("_provider").and_then(|pr| pr.as_str()).map(|pr| pr.to_string()));
            let provider_name = provider_id.as_ref().map(|pid| {
                cfg.provider_aliases.get(pid).cloned().unwrap_or_else(|| pid.clone())
            });
            let real_host = p.get("server").and_then(|s| s.as_str()).unwrap_or("").to_string();
            let real_port = p.get("port").and_then(|s| s.as_u64()).unwrap_or(0) as u16;

            if let Some(existing) = servers.iter_mut().find(|s| &s.id == name) {
                if !real_host.is_empty() {
                    existing.host = real_host.clone();
                    if real_host.parse::<std::net::IpAddr>().is_ok() {
                        existing.resolved_ip = Some(real_host);
                    }
                }
                if real_port > 0 {
                    existing.port = real_port;
                }
                if existing.provider.is_none()
                    || provider_id
                        .as_deref()
                        .map_or(false, |pid| crate::failover::provider_matches_whitelist(pid, cfg))
                {
                    existing.provider = provider_id;
                    existing.provider_name = provider_name;
                }
                continue;
            }

            if seen_names.contains(name) {
                continue;
            }
            seen_names.insert(name.clone());
            let final_host = if !real_host.is_empty() { real_host } else { name.clone() };
            let initial_ip = if final_host.parse::<std::net::IpAddr>().is_ok() { Some(final_host.clone()) } else { None };
            servers.push(Server {
                id: name.clone(),
                name: display_name(name),
                protocol: typ.to_uppercase(),
                host: final_host,
                port: real_port,
                is_active: *name == active,
                is_priority: priority_chain.iter().any(|p| p == name),
                ping_ms: last_delay(p),
                provider: provider_id,
                provider_name,
                is_pool: false,
                resolved_ip: initial_ip,
            });
        }
    }

    // VK-звонок создаётся как Mihomo `direct`-proxy на интерфейсе csq0,
    // поэтому обычно отфильтровывается как служебный тип. Для пользователя это
    // полноценный узел: ищем его в обоих ответах Mihomo, включая file-провайдер.
    // Если ядро ещё не зарегистрировало провайдер, но режим включен — узел всё равно показываем.
    if cfg.vkturn.enabled {
        let vk_proxy = proxies.get(crate::vkturn::PROXY_NAME).or_else(|| {
            provider_proxies.as_ref().ok()?.get(crate::vkturn::PROXY_NAME)
        });
        let is_active = active == crate::vkturn::PROXY_NAME || proxy_now == crate::vkturn::PROXY_NAME;
        let (peer_host, peer_port) = cfg
            .vkturn
            .peer
            .trim()
            .rsplit_once(':')
            .and_then(|(h, p)| {
                let host = h.trim();
                let port = p.trim().parse::<u16>().ok()?;
                if host.is_empty() || port == 0 {
                    None
                } else {
                    Some((host.to_string(), port))
                }
            })
            .unwrap_or_else(|| ("Интерфейс csq0 (Звонки ВК)".to_string(), 0));
        let unified_name = display_name(crate::vkturn::PROXY_NAME);
        if let Some(existing) = servers.iter_mut().find(|server| server.id == crate::vkturn::PROXY_NAME) {
            existing.name = unified_name;
            existing.protocol = "VK-CALL".into();
            existing.host = peer_host;
            existing.port = peer_port;
            existing.is_active = is_active;
            existing.provider = Some(crate::vkturn::PROVIDER_ID.into());
            existing.provider_name = Some("Звонки ВКонтакте".into());
        } else {
            let ping = vk_proxy.map(last_delay).unwrap_or(0);
            servers.push(Server {
                id: crate::vkturn::PROXY_NAME.into(),
                name: unified_name,
                protocol: "VK-CALL".into(),
                host: peer_host,
                port: peer_port,
                is_active,
                is_priority: priority_chain.iter().any(|id| id == crate::vkturn::PROXY_NAME),
                ping_ms: ping,
                provider: Some(crate::vkturn::PROVIDER_ID.into()),
                provider_name: Some("Звонки ВКонтакте".into()),
                is_pool: false,
                resolved_ip: None,
            });
        }
    } else {
        servers.retain(|server| server.id != crate::vkturn::PROXY_NAME);
    }

    // Виртуальные карточки бесплатных мульти-подписок (`free_vpn` и `free_wl`):
    // скрываем сырые узлы пула из общего списка и показываем единые карточки с реальными метриками активного узла.
    let registered_yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let registered_providers = crate::routing::parse_provider_names(&registered_yaml);
    for (pid, default_title, proto, default_host) in [
        (crate::free_sub::FREE_VPN_PROVIDER_ID, "🆓 Бесплатный VPN (Авто)", "FREE-VPN", "Пул бесплатного VPN"),
        (crate::free_sub::FREE_WL_PROVIDER_ID, "🛡️ Бесплатный обход БС (Авто)", "FREE-WL", "Пул обхода БС"),
    ] {
        let pool_nodes: Vec<Server> = servers
            .iter()
            .filter(|s| s.provider.as_deref() == Some(pid))
            .cloned()
            .collect();
        let is_registered = registered_providers.iter().any(|p| p.eq_ignore_ascii_case(pid));
        servers.retain(|s| s.provider.as_deref() != Some(pid));

        if is_registered || !pool_nodes.is_empty() {
            let active_leaf = pool_nodes.iter().find(|s| s.is_active);
            let best_leaf = active_leaf.or_else(|| pool_nodes.first());
            let card_host = best_leaf
                .map(|s| s.host.clone())
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| default_host.to_string());
            let card_port = best_leaf.map(|s| s.port).unwrap_or(0);
            let card_ping = best_leaf.map(|s| s.ping_ms).unwrap_or(-1);
            let card_ip = best_leaf.and_then(|s| s.resolved_ip.clone());
            let is_active = active_leaf.is_some();

            servers.push(Server {
                id: pid.to_string(),
                name: default_title.to_string(),
                protocol: proto.to_string(),
                host: card_host,
                port: card_port,
                is_active,
                is_priority: priority_chain.iter().any(|id| id == pid),
                ping_ms: card_ping,
                provider: Some(pid.to_string()),
                provider_name: Some(default_title.to_string()),
                is_pool: true,
                resolved_ip: card_ip,
            });
        }
    }

    // Разрешаем DNS для активного узла, если он задан доменным именем
    for s in &mut servers {
        if s.is_active && s.resolved_ip.is_none() && !s.is_pool && s.protocol != "VK-CALL" {
            s.resolved_ip = resolve_host_ip(&s.host).await;
        }
    }

    servers.sort_by(|a, b| {
        b.is_active.cmp(&a.is_active).then_with(|| {
            // -1/0 = пинг не измерен/недоступен → в конец списка
            match (a.ping_ms > 0, b.ping_ms > 0) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.ping_ms.cmp(&b.ping_ms),
            }
        })
    });
    Ok(servers)
}

/// Карта IP → группа из SRC-IP правил Mihomo.
pub fn ip_groups_from_rules(rules: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(list) = rules.get("rules").and_then(|r| r.as_array()) {
        for r in list {
            let ptype = r.get("type").and_then(|t| t.as_str()).unwrap_or("").to_uppercase();
            let payload = r.get("payload").and_then(|p| p.as_str()).unwrap_or("").to_string();
            let target = r.get("proxy").and_then(|p| p.as_str()).unwrap_or("").to_string();
            let ptype_norm = ptype.replace('-', "").replace('_', "");
            if ptype_norm.contains("SRCIP") && !target.is_empty() {
                let ip = payload.split('/').next().unwrap_or("").trim().to_string();
                if !ip.is_empty() {
                    out.insert(ip, target);
                }
            }
        }
    }
    out
}

/// Получить текущий активный сервер Mihomo
pub async fn get_active_server(
    http: &reqwest::Client,
    cfg: &AppConfig,
) -> Result<Option<Server>, String> {
    let servers = get_servers(http, cfg, &cfg.failover.priority_chain).await?;
    let act = servers
        .iter()
        .find(|s| s.is_active && !s.is_pool)
        .or_else(|| servers.iter().find(|s| s.is_active))
        .cloned();
    Ok(act)
}

/// Типы прокси, не являющиеся реальными серверами (служебные/групповые).
const SKIP_TYPES: &[&str] = &[
    "direct", "reject", "reject-drop", "rejectdrop", "pass", "passrule",
    "compatible", "selector", "urltest", "fallback", "relay", "load",
];

/// Live-состояние: какой сервер реально используется каждым устройством.
pub async fn live_device_servers(http: &reqwest::Client, cfg: &AppConfig) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let proxies = match get_proxies(http, cfg).await {
        Ok(p) => p,
        Err(_) => return out,
    };
    let rules = match m_get(http, cfg, "/rules").await {
        Ok(r) => r,
        Err(e) => {
            crate::log_w!("Ошибка запроса правил Mihomo (/rules): {}", e);
            Value::Null
        }
    };

    // 1. Из SRC-IP правил: ip → группа → now
    for (ip, group) in ip_groups_from_rules(&rules) {
        if let Some(now) = proxies.get(&group).and_then(|g| g.get("now")).and_then(|n| n.as_str()) {
            out.insert(ip, now.to_string());
        }
    }

    // 2. Из групп с IP в имени ('Big PC 192_168_2_118')
    for (gname, pdata) in &proxies {
        let Some(now) = pdata.get("now").and_then(|n| n.as_str()) else { continue };
        if let Some(ip) = ip_from_group_name(gname) {
            out.entry(ip).or_insert_with(|| now.to_string());
        }
    }
    out
}

pub async fn switch_server(http: &reqwest::Client, cfg: &AppConfig, server_id: &str) -> Result<String, String> {
    switch_server_inner(http, cfg, server_id, false).await
}

/// Мягкое переключение сервера (Graceful): переключает селекторные группы,
/// но не сбрасывает активные соединения старого узла.
pub async fn switch_server_graceful(http: &reqwest::Client, cfg: &AppConfig, server_id: &str) -> Result<String, String> {
    switch_server_inner(http, cfg, server_id, true).await
}

/// Какие selector-группы переключать при смене основного сервера (баг 122).
/// Всегда пропускаются авто-группы, AI/Flow, гейминг, группы устройств, Canary и 🛡️ Whitelist
/// (её переключает failover отдельно). Если есть основная группа (PROXY/GLOBAL/Proxy), остальные группы
/// переключаются только когда «следуют» за ней — стоят на том же узле (prev_active) или на том же выборе.
pub fn select_groups_to_switch(proxies: &BTreeMap<String, Value>, prev_active: &str) -> Vec<String> {
    let mut groups: Vec<String> = Vec::new();
    for (name, p) in proxies {
        let typ = p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        if typ != "selector" {
            continue;
        }
        let lower_g = name.to_lowercase();
        if name == "Fastest"
            || name == "Fallback"
            || crate::routing::is_flow_group_name(name)
            || lower_g.contains("gaming")
            || lower_g.contains("гейминг")
            || name.contains("🎮")
            || lower_g.contains("canary probe")
            || name.contains("🛡️ Canary Probe")
            || name == crate::routing::WHITELIST_GROUP_NAME
            || lower_g.contains("whitelist")
            || lower_g.contains("белые списки")
            || ip_from_group_name(name).is_some()
        {
            continue;
        }
        groups.push(name.clone());
    }
    // Известные общие имена — в начало (если существуют).
    for pref in ["Proxy", "GLOBAL", "PROXY"] {
        if let Some(pos) = groups.iter().position(|g| g == pref) {
            let g = groups.remove(pos);
            groups.insert(0, g);
        }
    }
    let primary_now: Option<String> = ["PROXY", "GLOBAL", "Proxy"]
        .iter()
        .find_map(|g| proxies.get(*g).and_then(|p| p.get("now")).and_then(|n| n.as_str()).map(str::to_string));
    if let Some(primary_now) = primary_now {
        groups.retain(|g| {
            if matches!(g.as_str(), "PROXY" | "GLOBAL" | "Proxy") {
                return true;
            }
            let now = proxies.get(g).and_then(|p| p.get("now")).and_then(|n| n.as_str()).unwrap_or("");
            now == primary_now || (!prev_active.is_empty() && now == prev_active)
        });
    }
    groups
}

async fn switch_server_inner(http: &reqwest::Client, cfg: &AppConfig, server_id: &str, graceful: bool) -> Result<String, String> {
    let raw_target = match server_id.to_lowercase().as_str() {
        "fastest" => "Fastest".to_string(),
        "fallback" => "Fallback".to_string(),
        _ => server_id.to_string(),
    };
    let target = resolve_virtual_pool_target(http, cfg, &raw_target).await?;
    if !cfg.disabled_providers.is_empty() {
        if let Ok(v) = m_get(http, cfg, "/providers/proxies").await {
            if let Some(map) = v.get("providers").and_then(|p| p.as_object()) {
                for (pname, prov) in map {
                    if cfg.disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case(pname)) {
                        if let Some(list) = prov.get("proxies").and_then(|p| p.as_array()) {
                            if list.iter().any(|p| p.get("name").and_then(|n| n.as_str()) == Some(&target)) {
                                return Err(format!("Сервер '{target}' принадлежит отключенной подписке '{pname}'"));
                            }
                        }
                    }
                }
            }
        }
    }
    let proxies = get_proxies(http, cfg).await?;
    let prev_active = resolve_active_leaf(&proxies);
    let prev_wl_now = proxies
        .get(crate::routing::WHITELIST_GROUP_NAME)
        .and_then(|v| v.get("now"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let wl_group_has_target = proxies
        .get(crate::routing::WHITELIST_GROUP_NAME)
        .and_then(|v| v.get("all"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().any(|x| x.as_str() == Some(&target)))
        .unwrap_or(false);
    let wl_has_target = cfg.whitelist.enabled && wl_group_has_target;
    let groups = select_groups_to_switch(&proxies, &prev_active);
    let target_in_any_group = groups.iter().any(|g| {
        proxies
            .get(g)
            .and_then(|p| p.get("all"))
            .and_then(|a| a.as_array())
            .map(|members| members.iter().any(|m| m.as_str() == Some(target.as_str())))
            .unwrap_or(false)
    });
    if !target_in_any_group && !wl_group_has_target {
        if groups.is_empty() {
            return Err("Не найдено ни одной select-группы для переключения".into());
        }
    }
    let mut switched = 0usize;
    let mut skipped = 0usize;
    let mut last_err = String::new();
    for g in &groups {
        // Специализированные группы (YouTube и т.п.) могут не содержать целевой
        // сервер — Mihomo ответит 400. Пропускаем такие группы без ошибки.
        let member = proxies
            .get(g)
            .and_then(|p| p.get("all"))
            .and_then(|a| a.as_array())
            .map(|members| members.iter().any(|m| m.as_str() == Some(target.as_str())));
        if member == Some(false) {
            skipped += 1;
            continue;
        }
        match m_put(http, cfg, &format!("/proxies/{}", urlencoding_lite(g)), json!({ "name": target }), 5).await {
            Ok(_) => switched += 1,
            Err(e) => last_err = e,
        }
    }
    let mut wl_switched = false;
    // Если ни одна общая группа (PROXY и т.п.) не содержит целевой узел, но он есть в 🛡️ Whitelist,
    // разрешаем прямое переключение 🛡️ Whitelist (явный выбор БС-узла), не затрагивая обычные VPS-свитчи.
    if switched == 0 && wl_group_has_target {
        if prev_wl_now == target {
            switched = 1;
        } else {
            match switch_selector(http, cfg, crate::routing::WHITELIST_GROUP_NAME, &target).await {
                Ok(_) => {
                    wl_switched = true;
                    switched = 1;
                }
                Err(e) => last_err = e,
            }
        }
    }
    if switched > 0 {
        invalidate_proxies_cache();
        if target.eq_ignore_ascii_case("DIRECT") {
            clear_selected_tunnel_server();
        } else {
            record_selected_tunnel_server(&target);
        }
        if wl_has_target && !wl_switched && prev_wl_now != target {
            match switch_selector(http, cfg, crate::routing::WHITELIST_GROUP_NAME, &target).await {
                Ok(_) => wl_switched = true,
                Err(e) => crate::log_w!(
                    "[MIHOMO] Не удалось синхронизировать '{}' -> '{}': {}",
                    crate::routing::WHITELIST_GROUP_NAME,
                    target,
                    e
                ),
            }
        }
        if !graceful {
            if !prev_active.is_empty() && prev_active != target && !prev_active.eq_ignore_ascii_case("direct") {
                let closed = close_server_connections(http, cfg, &prev_active).await;
                if closed == 0 {
                    crate::log_i!(
                        "Селективный сброс: 0 соединений для узла '{}' — общий сброс соединений не выполняется (DIRECT не затрагивается)",
                        prev_active
                    );
                }
            } else if !prev_active.is_empty() && prev_active.eq_ignore_ascii_case("direct") {
                // Переход с DIRECT на прокси: сбрасываем сокеты, чтобы клиентские сессии
                // переустановились уже через туннель.
                let _ = close_all_connections(http, cfg).await;
            }
        } else {
            crate::log_i!("Graceful Failback: группы переключены на '{target}', активные сокеты старого узла не сбрасываются");
        }
        if !graceful && wl_switched && !prev_wl_now.is_empty() && prev_wl_now != target {
            let wl_closed = close_server_connections(http, cfg, crate::routing::WHITELIST_GROUP_NAME).await;
            let prev_wl_closed = if !prev_wl_now.eq_ignore_ascii_case("direct") && prev_wl_now != prev_active {
                close_server_connections(http, cfg, &prev_wl_now).await
            } else {
                0
            };
            if wl_closed > 0 || prev_wl_closed > 0 {
                crate::log_i!(
                    "[MIHOMO] Сброшены устаревшие сокеты '{}' (закрыто {} через группу, {} через прежний узел '{}')",
                    crate::routing::WHITELIST_GROUP_NAME,
                    wl_closed,
                    prev_wl_closed,
                    prev_wl_now
                );
            }
        }
        invalidate_proxies_cache();
        let skip_note = if skipped > 0 { format!(", пропущено: {skipped}") } else { String::new() };
        Ok(format!("Активный сервер переключен на '{target}' (групп: {switched}{skip_note})"))
    } else if skipped > 0 && last_err.is_empty() {
        Err(format!("Сервер '{target}' не входит ни в одну переключаемую группу"))
    } else {
        Err(format!("Не удалось изменить активный сервер в Mihomo: {last_err}"))
    }
}

/// Получить текущий сервер, назначенный для Google Flow & AI
pub async fn get_flow_status(http: &reqwest::Client, cfg: &AppConfig) -> Result<serde_json::Value, String> {
    let proxies = get_proxies(http, cfg).await.unwrap_or_default();
    let mut active_server = String::new();
    let mut group_found: Option<String> = None;

    for (gname, p) in &proxies {
        if crate::routing::is_flow_group_name(gname) {
            group_found = Some(gname.to_string());
            if let Some(now) = p.get("now").and_then(|n| n.as_str()) {
                if !now.is_empty() {
                    active_server = now.to_string();
                    break;
                }
            }
        }
    }

    if active_server.is_empty() {
        if let Some(ref s) = cfg.flow_server {
            active_server = s.clone();
        }
    }

    if active_server.is_empty() {
        active_server = resolve_active_leaf(&proxies);
    }

    Ok(json!({
        "flow_server": active_server,
        "group_name": group_found,
        "flow_auto_select": cfg.flow_auto_select,
    }))
}

/// Переключить выделенный сервер для Google Flow & AI с использованием ConfigTx и локов (BUG-15)
pub async fn switch_flow_server(state: &AppState, server_id: &str) -> Result<String, String> {
    let target = server_id.trim();
    if target.is_empty() {
        return Err("Имя сервера не может быть пустым".into());
    }
    if ["PROXY", "GLOBAL", "Fastest", "Fallback", "DIRECT", "REJECT"].contains(&target) {
        return Err(format!("'{target}' является системной группой/селектором, а не конкретным выходным сервером для Google Flow"));
    }
    let cfg = state.config.read().await.clone();
    let proxies = get_proxies(&state.http, &cfg).await.unwrap_or_default();

    // Проверяем наличие группы Google Flow ДО любой записи в конфиг (баг A-10).
    // Если группы Flow нет, нельзя писать сырое имя ноды в правила Mihomo.
    let group_names: Vec<String> = proxies.keys().cloned().collect();
    let flow_plan = crate::routing::plan_switch_flow_server(&group_names, target)?;
    let flow_group = flow_plan.group_name;

    let member = proxies
        .get(&flow_group)
        .and_then(|p| p.get("all"))
        .and_then(|a| a.as_array())
        .map(|members| members.iter().any(|m| m.as_str() == Some(target)));

    if member == Some(true) {
        m_put(&state.http, &cfg, &format!("/proxies/{}", urlencoding_lite(&flow_group)), json!({ "name": target }), 3).await?;
        invalidate_proxies_cache();
        let _ = close_flow_connections(&state.http, &cfg).await;
        return Ok(format!("Выделенный маршрут Google Flow & AI переключен на '{target}'"));
    }

    // Если сервер не входит в селекторную группу, прописываем правила через ConfigTx
    let mut tx = crate::transaction::ConfigTx::begin(state).await?;
    let yaml = tx.read_yaml().await?;
    let new_yaml = crate::routing::apply_flow_rules(&yaml, target, Some(&flow_group))?;
    if new_yaml != yaml {
        tx.set_yaml(new_yaml)?;
        tx.commit_and_reload().await?;
    }

    let _ = m_put(&state.http, &cfg, &format!("/proxies/{}", urlencoding_lite(&flow_group)), json!({ "name": target }), 3).await;
    invalidate_proxies_cache();

    let _ = close_flow_connections(&state.http, &cfg).await;
    Ok(format!("Выделенный маршрут Google Flow & AI переключен на '{target}'"))
}

/// Пинг всех участников селекторной группы через /group/{group}/delay (мс по каждому серверу в группе).
/// Возвращает карту: server_id -> delay_ms.
#[allow(dead_code)]
pub async fn ping_group(
    http: &reqwest::Client,
    cfg: &AppConfig,
    group: &str,
    timeout_ms: u64,
) -> BTreeMap<String, i64> {
    ping_group_url(http, cfg, group, timeout_ms, None).await
}

/// Пинг участников группы с произвольным URL проверки.
pub async fn ping_group_url(
    http: &reqwest::Client,
    cfg: &AppConfig,
    group: &str,
    timeout_ms: u64,
    test_url: Option<&str>,
) -> BTreeMap<String, i64> {
    let enc = urlencoding_lite(group);
    let check_url = match test_url {
        Some(u) => urlencoding_lite(u),
        None => cfg.health_check_url_encoded(),
    };
    let url = format!(
        "{}/group/{enc}/delay?timeout={timeout_ms}&url={check_url}",
        cfg.mihomo_url()
    );
    let req_timeout = std::cmp::max(timeout_ms + 15000, 25000);
    let mut req = http.get(&url).timeout(std::time::Duration::from_millis(req_timeout));
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    let mut out = BTreeMap::new();
    if let Ok(resp) = req.send().await {
        if resp.status().is_success() {
            if let Ok(map) = resp.json::<BTreeMap<String, Value>>().await {
                for (k, v) in map {
                    if let Some(d) = v.as_i64() {
                        if d > 0 {
                            out.insert(k, d);
                        }
                    }
                }
            }
        }
    }
    out
}

pub const PRIMARY_L7_CHECK_URL: &str = "http://cp.cloudflare.com/generate_204";
pub const SECONDARY_L7_CHECK_URL: &str = "https://www.gstatic.com/generate_204";



/// Пинг сервера с произвольным тестовым URL (автоматическое определение ожидаемого HTTP-статуса).
pub async fn ping_server_url(
    http: &reqwest::Client,
    cfg: &AppConfig,
    server_id: &str,
    timeout_ms: u64,
    test_url: Option<&str>,
) -> i64 {
    let expected = match test_url {
        Some(u) if u.contains("generate_204") => Some(204),
        Some(u) if u.contains("__down") => Some(200),
        Some(_) => None,
        None => Some(204),
    };
    ping_server_url_expected(http, cfg, server_id, timeout_ms, test_url, expected).await
}

/// Пинг сервера с произвольным тестовым URL и валидацией статус-кода L7 (expected_status).
/// Защищает от ложных срабатываний при Reality fallthrough и captive portal заглушках (200/302).
pub async fn ping_server_url_expected(
    http: &reqwest::Client,
    cfg: &AppConfig,
    server_id: &str,
    timeout_ms: u64,
    test_url: Option<&str>,
    expected_status: Option<u16>,
) -> i64 {
    let exp = expected_status.map(|c| c.to_string());
    ping_server_url_expected_range(http, cfg, server_id, timeout_ms, test_url, exp.as_deref()).await
}

/// То же, что `ping_server_url_expected`, но `expected` в формате Mihomo: "204", "200-399", "200/204".
pub async fn ping_server_url_expected_range(
    http: &reqwest::Client,
    cfg: &AppConfig,
    server_id: &str,
    timeout_ms: u64,
    test_url: Option<&str>,
    expected_status: Option<&str>,
) -> i64 {
    let enc = urlencoding_lite(server_id);
    let check_url = match test_url {
        Some(u) => urlencoding_lite(u),
        None => cfg.health_check_url_encoded(),
    };
    let exp_param = match expected_status {
        Some(code) => format!("&expected={}", urlencoding_lite(code)),
        None => String::new(),
    };
    let req_timeout = std::time::Duration::from_millis(std::cmp::max(timeout_ms + 2000, 5000));

    // 0. Виртуальные карточки бесплатных мульти-подписок (free_vpn / free_wl):
    // резолвим текущий активный листовой узел и пингуем его через /providers/proxies/{provider}/{leaf}/healthcheck.
    if is_virtual_free_sub_pool(server_id) {
        if let Some((leaf, _)) = resolve_virtual_pool_active_leaf(http, cfg, server_id).await {
            let prov_enc = urlencoding_lite(server_id);
            let leaf_enc = urlencoding_lite(&leaf);
            let node_hc_url = format!(
                "{}/providers/proxies/{prov_enc}/{leaf_enc}/healthcheck?timeout={timeout_ms}&url={check_url}{exp_param}",
                cfg.mihomo_url()
            );
            let mut hc_req = http.get(&node_hc_url).timeout(req_timeout);
            if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
                hc_req = hc_req.header(k, v);
            }
            if let Ok(resp) = hc_req.send().await {
                if resp.status().is_success() {
                    if let Ok(v) = resp.json::<Value>().await {
                        if let Some(d) = v.get("delay").and_then(|d| d.as_i64()) {
                            if d > 0 {
                                return d;
                            }
                        }
                    }
                }
            }
        }
        return -1;
    }

    // 1. Если узел принадлежит proxy-провайдеру (подписке / file-провайдеру),
    // сразу используем штатный точечный эндпоинт Mihomo:
    // GET /providers/proxies/{provider}/{node}/healthcheck
    // Это исключает лишний 404 на /proxies/{id}/delay и шторм /group/PROXY/delay.
    let provider_name = get_provider_proxies(http, cfg)
        .await
        .ok()
        .and_then(|providers| {
            providers.get(server_id).and_then(|p| {
                p.get("_provider")
                    .and_then(|v| v.as_str())
                    .or_else(|| {
                        p.get("_providers")
                            .and_then(|arr| arr.as_array())
                            .and_then(|arr| arr.first())
                            .and_then(|v| v.as_str())
                    })
                    .map(str::to_string)
            })
        });

    if let Some(ref prov_name) = provider_name {
        let prov_enc = urlencoding_lite(prov_name);
        let node_hc_url = format!(
            "{}/providers/proxies/{prov_enc}/{enc}/healthcheck?timeout={timeout_ms}&url={check_url}{exp_param}",
            cfg.mihomo_url()
        );
        let mut hc_req = http.get(&node_hc_url).timeout(req_timeout);
        if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
            hc_req = hc_req.header(k, v);
        }
        if let Ok(resp) = hc_req.send().await {
            if resp.status().is_success() {
                if let Ok(v) = resp.json::<Value>().await {
                    if let Some(d) = v.get("delay").and_then(|d| d.as_i64()) {
                        if d > 0 {
                            return d;
                        }
                    }
                }
            } else if resp.status() != reqwest::StatusCode::NOT_FOUND {
                // Эндпоинт есть, но узел вернул 504/503/ошибку — считаем мёртвым без группового теста.
                return -1;
            }
        }
    } else {
        let url = format!(
            "{}/proxies/{enc}/delay?timeout={timeout_ms}&url={check_url}{exp_param}",
            cfg.mihomo_url()
        );
        let mut req = http.get(&url).timeout(req_timeout);
        if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
            req = req.header(k, v);
        }
        if let Ok(resp) = req.send().await {
            if resp.status().is_success() {
                if let Ok(v) = resp.json::<Value>().await {
                    if let Some(d) = v.get("delay").and_then(|d| d.as_i64()) {
                        if d > 0 {
                            return d;
                        }
                    }
                }
            }
        }
    }

    // Для узла VK-туннеля (file-провайдер) fallback на точечный healthcheck провайдера vkturn,
    // если поузловой эндпоинт недоступен (например, 404 или пустой кэш провайдеров).
    if server_id == crate::vkturn::PROXY_NAME {
        let prov_enc = urlencoding_lite(crate::vkturn::PROVIDER_ID);
        let hc_url = format!(
            "{}/providers/proxies/{prov_enc}/healthcheck?timeout={timeout_ms}&url={check_url}{exp_param}",
            cfg.mihomo_url()
        );
        let mut hc_req = http.get(&hc_url).timeout(req_timeout);
        if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
            hc_req = hc_req.header(k, v);
        }
        if let Ok(hc_resp) = hc_req.send().await {
            if hc_resp.status().is_success() {
                let info_url = format!("{}/providers/proxies/{prov_enc}", cfg.mihomo_url());
                let mut info_req = http.get(&info_url).timeout(std::time::Duration::from_millis(3000));
                if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
                    info_req = info_req.header(k, v);
                }
                if let Ok(info_resp) = info_req.send().await {
                    if info_resp.status().is_success() {
                        if let Ok(prov_json) = info_resp.json::<Value>().await {
                            if let Some(arr) = prov_json.get("proxies").and_then(|p| p.as_array()) {
                                if let Some(node) = arr.iter().find(|p| p.get("name").and_then(|n| n.as_str()) == Some(server_id)) {
                                    let d = last_delay(node);
                                    if d > 0 {
                                        return d;
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Для групп/пулов (Fallback, Fastest, Selector) прямой /proxies/{id}/delay в ядре Mihomo отдаёт 404/ошибку.
    // Группу (включая PROXY) тестируем ТОЛЬКО если server_id в ней сейчас активен (group.now == server_id),
    // чтобы не запускать массовый /group/PROXY/delay по всем узлам при проверке неактивного узла.
    if let Ok(proxies) = get_proxies(http, cfg).await {
        let selector_groups = proxies
            .iter()
            .filter_map(|(name, group)| {
                let typ = group.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
                let active = group.get("now").and_then(|n| n.as_str()) == Some(server_id);
                let contains = group.get("all").and_then(|all| all.as_array()).is_some_and(|all| {
                    all.iter().any(|node| node.as_str() == Some(server_id))
                });
                (typ == "selector" && active && contains).then(|| name.clone())
            })
            .collect::<Vec<_>>();
        for group_name in selector_groups {
            let group_enc = urlencoding_lite(&group_name);
            let group_url = format!(
                "{}/group/{group_enc}/delay?timeout={timeout_ms}&url={check_url}{exp_param}",
                cfg.mihomo_url()
            );
            let mut group_req = http.get(&group_url).timeout(std::time::Duration::from_millis(std::cmp::max(timeout_ms + 2000, 5000)));
            if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
                group_req = group_req.header(k, v);
            }
            if let Ok(resp) = group_req.send().await {
                if resp.status().is_success() {
                    if let Ok(delays) = resp.json::<BTreeMap<String, Value>>().await {
                        if let Some(delay) = delays.get(server_id).and_then(|d| d.as_i64()) {
                            if delay > 0 {
                                return delay;
                            }
                        }
                    }
                }
            }
        }
        if let Some(group_val) = proxies.get(server_id) {
            let typ = group_val.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
            if typ == "fallback" || typ == "urltest" || typ == "selector" {
                if let Some(now_leaf) = group_val.get("now").and_then(|n| n.as_str()) {
                    if !now_leaf.is_empty() && now_leaf != server_id {
                        if let Some(leaf_val) = proxies.get(now_leaf) {
                            let ld = last_delay(leaf_val);
                            if ld > 0 {
                                return ld;
                            }
                        }
                        let leaf_enc = urlencoding_lite(now_leaf);
                        let leaf_url = format!(
                            "{}/proxies/{leaf_enc}/delay?timeout={timeout_ms}&url={check_url}{exp_param}",
                            cfg.mihomo_url()
                        );
                        let mut leaf_req = http.get(&leaf_url).timeout(std::time::Duration::from_millis(std::cmp::max(timeout_ms + 2000, 5000)));
                        if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
                            leaf_req = leaf_req.header(k, v);
                        }
                        if let Ok(resp) = leaf_req.send().await {
                            if resp.status().is_success() {
                                if let Ok(v) = resp.json::<Value>().await {
                                    if let Some(d) = v.get("delay").and_then(|d| d.as_i64()) {
                                        if d > 0 {
                                            return d;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Для неактивных серверов из proxy-providers:
    // Считываем проверенный L7-статус из /providers/proxies (память ядра Mihomo):
    if let Ok(providers) = get_provider_proxies(http, cfg).await {
        if let Some(p) = providers.get(server_id) {
            let alive = p.get("alive").and_then(|a| a.as_bool()).unwrap_or(true);
            if !alive {
                return -1;
            }
            let d = last_delay(p);
            if d > 0 {
                return d;
            }
        }
    }

    -1
}

pub const CANARY_PROBE_GROUP_NAME: &str = "🛡️ Canary Probe";

/// Служебные/системные группы и встроенные политики, которые не должны участвовать в массовых и авто-пингах.
pub fn is_service_group_or_builtin(id: &str) -> bool {
    let t = id.trim();
    t.eq_ignore_ascii_case("DIRECT")
        || t.eq_ignore_ascii_case("REJECT")
        || t.eq_ignore_ascii_case("GLOBAL")
        || t == CANARY_PROBE_GROUP_NAME
        || t.eq_ignore_ascii_case("Canary Probe")
}

/// Параллельная L7-проверка сервера реальным трафиком без переключения селектора клиентов (PROXY).
/// 1. Если сервер активен в основной группе (PROXY) — проверяем через неё.
/// 2. Если в Mihomo доступен изолированный селектор '🛡️ Canary Probe' или 'Canary Probe' —
///    переключаем его на проверяемый узел (без close_all_connections!) и делаем реальный HTTP-запрос.
/// 3. Иначе проверяем прямой пинг /proxies/{id}/delay.
/// 4. Для серверов из подписок (proxy-providers), пока селектор не создан —
///    валидируем через подтвержденный L7 health-check ядра Mihomo (/providers/proxies).
pub async fn probe_server_l7(
    http: &reqwest::Client,
    cfg: &AppConfig,
    server_id: &str,
    timeout_ms: u64,
    test_url: &str,
    expected_status: u16,
) -> i64 {
    let proxies = get_proxies(http, cfg).await.unwrap_or_default();

    // 1. Если узел уже активен в primary_group (PROXY)
    if let Some(group) = find_primary_group(&proxies) {
        let is_now = proxies
            .get(&group)
            .and_then(|p| p.get("now"))
            .and_then(|n| n.as_str())
            .map(|now| now == server_id)
            .unwrap_or(false);

        if is_now {
            let enc = urlencoding_lite(&group);
            let check_url = urlencoding_lite(test_url);
            let url = format!(
                "{}/proxies/{enc}/delay?timeout={timeout_ms}&url={check_url}&expected={expected_status}",
                cfg.mihomo_url()
            );
            let mut req = http.get(&url).timeout(std::time::Duration::from_millis(std::cmp::max(timeout_ms + 2000, 5000)));
            if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
                req = req.header(k, v);
            }
            if let Ok(resp) = req.send().await {
                if resp.status().is_success() {
                    if let Ok(v) = resp.json::<Value>().await {
                        if let Some(d) = v.get("delay").and_then(|d| d.as_i64()) {
                            if d > 0 {
                                return d;
                            }
                        }
                    }
                }
            }
        }
    }

    // 2. Изолированная probe-группа
    let probe_group_opt = if proxies.contains_key(CANARY_PROBE_GROUP_NAME) {
        Some(CANARY_PROBE_GROUP_NAME)
    } else if proxies.contains_key("Canary Probe") {
        Some("Canary Probe")
    } else {
        None
    };

    if let Some(pg) = probe_group_opt {
        let enc_pg = urlencoding_lite(pg);
        // Переключаем только тестовую группу БЕЗ сброса клиентских соединений
        if m_put(http, cfg, &format!("/proxies/{enc_pg}"), serde_json::json!({ "name": server_id }), 3).await.is_ok() {
            let check_url = urlencoding_lite(test_url);
            let url = format!(
                "{}/proxies/{enc_pg}/delay?timeout={timeout_ms}&url={check_url}&expected={expected_status}",
                cfg.mihomo_url()
            );
            let mut req = http.get(&url).timeout(std::time::Duration::from_millis(std::cmp::max(timeout_ms + 2000, 5000)));
            if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
                req = req.header(k, v);
            }
            if let Ok(resp) = req.send().await {
                if resp.status().is_success() {
                    if let Ok(v) = resp.json::<Value>().await {
                        if let Some(d) = v.get("delay").and_then(|d| d.as_i64()) {
                            if d > 0 {
                                return d;
                            }
                        }
                    }
                }
            }
        }
    }

    // 3. Прямой пинг одиночного прокси /proxies/{id}/delay
    let enc_id = urlencoding_lite(server_id);
    let check_url = urlencoding_lite(test_url);
    let direct_url = format!(
        "{}/proxies/{enc_id}/delay?timeout={timeout_ms}&url={check_url}&expected={expected_status}",
        cfg.mihomo_url()
    );
    let mut req = http.get(&direct_url).timeout(std::time::Duration::from_millis(std::cmp::max(timeout_ms + 2000, 5000)));
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    if let Ok(resp) = req.send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                if let Some(d) = v.get("delay").and_then(|d| d.as_i64()) {
                    if d > 0 {
                        return d;
                    }
                }
            }
        }
    }

    // 4. Проверка по подтвержденному статусу L7 из proxy-provider
    if let Ok(providers) = get_provider_proxies(http, cfg).await {
        if let Some(p) = providers.get(server_id) {
            let alive = p.get("alive").and_then(|a| a.as_bool()).unwrap_or(true);
            if !alive {
                return -1;
            }
            let d = last_delay(p);
            if d > 0 {
                return d;
            }
        }
    }

    -1
}

/// L7 Split-Target Probing:
/// Параллельно опрашивает Cloudflare и Google эндпоинты через tokio::select!.
/// Первый успешный ответ (expected=204) мгновенно выигрывает и возвращает задержку,
/// исключая задержки ожидания таймаута сбойного CDN.
pub async fn ping_server_l7_split(
    http: &reqwest::Client,
    cfg: &AppConfig,
    server_id: &str,
    timeout_ms: u64,
) -> i64 {
    let f1 = ping_server_url_expected(http, cfg, server_id, timeout_ms, Some(PRIMARY_L7_CHECK_URL), Some(204));
    let f2 = ping_server_url_expected(http, cfg, server_id, timeout_ms, Some(SECONDARY_L7_CHECK_URL), Some(204));
    tokio::pin!(f1);
    tokio::pin!(f2);

    tokio::select! {
        res1 = &mut f1 => {
            if res1 > 0 {
                res1
            } else {
                f2.await
            }
        }
        res2 = &mut f2 => {
            if res2 > 0 {
                res2
            } else {
                f1.await
            }
        }
    }
}

/// Параллельный пинг списка серверов
pub async fn ping_all(http: &reqwest::Client, cfg: &AppConfig, ids: &[String], timeout_ms: u64) -> BTreeMap<String, i64> {
    ping_all_url(http, cfg, ids, timeout_ms, None).await
}

pub fn is_virtual_free_sub_pool(id: &str) -> bool {
    id == crate::free_sub::FREE_VPN_PROVIDER_ID || id == crate::free_sub::FREE_WL_PROVIDER_ID
}

fn provider_node_belongs_to(val: &Value, provider_id: &str) -> bool {
    if val.get("_provider").and_then(|v| v.as_str()) == Some(provider_id) {
        return true;
    }
    val.get("_providers")
        .and_then(|arr| arr.as_array())
        .is_some_and(|arr| arr.iter().any(|v| v.as_str() == Some(provider_id)))
}

/// Резолвит текущий активный листовой узел для виртуальной карточки пула (`free_vpn` / `free_wl`)
/// и возвращает `(имя_листа, alive_в_провайдере)`.
pub fn resolve_virtual_pool_leaf_from_maps(
    provider_id: &str,
    proxies: Option<&BTreeMap<String, Value>>,
    provider_proxies: Option<&BTreeMap<String, Value>>,
) -> Option<(String, bool)> {
    let provs = provider_proxies?;
    if let Some(pmap) = proxies {
        let mut candidate_groups: Vec<String> = Vec::new();
        if provider_id == crate::free_sub::FREE_WL_PROVIDER_ID
            && pmap.contains_key(crate::routing::WHITELIST_GROUP_NAME)
        {
            candidate_groups.push(crate::routing::WHITELIST_GROUP_NAME.to_string());
        }
        if let Some(primary) = find_primary_group(pmap) {
            if !candidate_groups.iter().any(|g| g == &primary) {
                candidate_groups.push(primary);
            }
        }
        for grp in candidate_groups {
            if let Some(leaf) = resolve_group_active_leaf(pmap, &grp) {
                if let Some(pval) = provs.get(&leaf) {
                    if provider_node_belongs_to(pval, provider_id) {
                        let alive = pval.get("alive").and_then(|a| a.as_bool()).unwrap_or(true);
                        return Some((leaf, alive));
                    }
                }
            }
        }
    }
    let mut first_any: Option<(String, bool)> = None;
    for (name, pval) in provs {
        if !provider_node_belongs_to(pval, provider_id) {
            continue;
        }
        let alive = pval.get("alive").and_then(|a| a.as_bool()).unwrap_or(true);
        if alive {
            return Some((name.clone(), true));
        }
        if first_any.is_none() {
            first_any = Some((name.clone(), false));
        }
    }
    first_any
}

pub async fn resolve_virtual_pool_active_leaf(
    http: &reqwest::Client,
    cfg: &AppConfig,
    provider_id: &str,
) -> Option<(String, bool)> {
    let (proxies_res, provs_res) = tokio::join!(get_proxies(http, cfg), get_provider_proxies(http, cfg));
    resolve_virtual_pool_leaf_from_maps(provider_id, proxies_res.ok().as_ref(), provs_res.ok().as_ref())
}

/// Если `id` — виртуальный идентификатор пула бесплатной подписки (`free_wl` / `free_vpn`),
/// разрешает его в имя реального рабочего листового узла этого провайдера.
pub async fn resolve_virtual_pool_target(
    http: &reqwest::Client,
    cfg: &AppConfig,
    id: &str,
) -> Result<String, String> {
    if !is_virtual_free_sub_pool(id) {
        return Ok(id.to_string());
    }
    resolve_virtual_pool_active_leaf(http, cfg, id)
        .await
        .map(|(leaf, _)| leaf)
        .filter(|leaf| !leaf.is_empty() && !is_virtual_free_sub_pool(leaf))
        .ok_or_else(|| format!("В пуле '{id}' нет доступных листовых узлов для переключения"))
}

fn resolve_group_active_leaf(proxies: &BTreeMap<String, Value>, start: &str) -> Option<String> {
    let mut cur = start.to_string();
    let mut visited = std::collections::HashSet::new();
    for _ in 0..8 {
        if !visited.insert(cur.clone()) {
            return None;
        }
        let Some(val) = proxies.get(&cur) else {
            return Some(cur);
        };
        let typ = val.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        if !matches!(typ.as_str(), "selector" | "select" | "urltest" | "fallback" | "load-balance" | "loadbalance") {
            return Some(cur);
        }
        let next = val.get("now").and_then(|n| n.as_str()).unwrap_or("").trim();
        if next.is_empty() {
            return None;
        }
        cur = next.to_string();
    }
    None
}

/// Параллельный пинг списка серверов с произвольным тестовым URL.
/// Служебные группы (DIRECT/REJECT/GLOBAL/Canary Probe) пропускаются;
/// пулы/селекторные группы не запускают отдельный каскад сетевых запросов,
/// а получают задержку своего активного листового узла.
pub async fn ping_all_url(
    http: &reqwest::Client,
    cfg: &AppConfig,
    ids: &[String],
    timeout_ms: u64,
    test_url: Option<&str>,
) -> BTreeMap<String, i64> {
    let mut out = BTreeMap::new();
    if ids.is_empty() {
        return out;
    }

    let (proxies_res, provs_res) = tokio::join!(get_proxies(http, cfg), get_provider_proxies(http, cfg));
    let proxies_map = proxies_res.ok();
    let provider_proxies = provs_res.ok();
    let mut leaf_ids: Vec<String> = Vec::new();
    let mut pool_ids: Vec<String> = Vec::new();
    let mut virtual_pool_ids: Vec<String> = Vec::new();

    for id in ids {
        if is_service_group_or_builtin(id) {
            continue;
        }
        if is_virtual_free_sub_pool(id) {
            virtual_pool_ids.push(id.clone());
            continue;
        }
        let is_group = proxies_map.as_ref().and_then(|m| m.get(id)).is_some_and(|val| {
            let typ = val.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
            matches!(typ.as_str(), "selector" | "select" | "urltest" | "fallback" | "load-balance" | "loadbalance")
        });
        if is_group {
            pool_ids.push(id.clone());
        } else {
            leaf_ids.push(id.clone());
        }
    }

    // Не более 12 активных запросов. Остальные узлы остаются в исходном списке,
    // без выделения отдельной задачи и копии настроек на каждый узел подписки.
    let effective_timeout = std::cmp::min(timeout_ms, 2500);
    let mut pings = stream::iter(leaf_ids.iter().cloned())
        .map(|id| async move {
            let ms = ping_server_url(http, cfg, &id, effective_timeout, test_url).await;
            (id, ms)
        })
        .buffer_unordered(12);
    while let Some((id, ms)) = pings.next().await {
        out.insert(id, ms);
    }

    // Виртуальные карточки пулов free_vpn / free_wl: пингуем их активный листовой узел
    // и сохраняем задержку и для самой карточки, и для селекторов (PROXY / 🛡️ Whitelist).
    let mut resolved_leaf_delays: BTreeMap<String, i64> = BTreeMap::new();
    for vpid in virtual_pool_ids {
        let mut delay = -1;
        if let Some((leaf, _)) =
            resolve_virtual_pool_leaf_from_maps(&vpid, proxies_map.as_ref(), provider_proxies.as_ref())
        {
            if let Some(&d) = out.get(&leaf).or_else(|| resolved_leaf_delays.get(&leaf)) {
                delay = d;
            } else {
                delay = ping_server_url_expected_range(http, cfg, &vpid, effective_timeout, test_url, None).await;
                resolved_leaf_delays.insert(leaf, delay);
            }
        }
        out.insert(vpid, delay);
    }

    if let Some(ref proxies) = proxies_map {
        for pool_id in pool_ids {
            let mut resolved_delay = -1;
            if let Some(leaf) = resolve_group_active_leaf(proxies, &pool_id) {
                if !is_service_group_or_builtin(&leaf) {
                    if let Some(&d) = out.get(&leaf).or_else(|| resolved_leaf_delays.get(&leaf)) {
                        resolved_delay = d;
                    } else if let Some(leaf_val) = proxies.get(&leaf) {
                        let ld = last_delay(leaf_val);
                        if ld > 0 {
                            resolved_delay = ld;
                        } else if leaf_ids.is_empty() {
                            // Если пинговали одиночный пул без листовых узлов — пингуем только его активный лист.
                            resolved_delay = ping_server_url(http, cfg, &leaf, effective_timeout, test_url).await;
                        }
                    } else if leaf_ids.is_empty() {
                        resolved_delay = ping_server_url(http, cfg, &leaf, effective_timeout, test_url).await;
                    }
                }
            }
            out.insert(pool_id, resolved_delay);
        }
    } else {
        for pool_id in pool_ids {
            out.insert(pool_id, -1);
        }
    }

    // PING-AUTO: все замеры (фоновые, ручные, после переключения) попадают в кэш панели
    store_ping_cache(&out);
    if !resolved_leaf_delays.is_empty() {
        store_ping_cache(&resolved_leaf_delays);
    }

    out
}

/// Ограничение набора id для фонового автообновления пингов (анти-регрессия PING-AUTO):
/// не даём фоновой задаче раздуться до всех нод провайдеров.
pub const AUTO_PING_MAX_IDS: usize = 24;
pub const AUTO_PING_COOLDOWN_SECS: u64 = 20;

static MANUAL_PING_IN_FLIGHT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static LAST_MANUAL_PING_AT: std::sync::RwLock<Option<std::time::Instant>> = std::sync::RwLock::new(None);

pub struct ManualPingGuard;

impl Drop for ManualPingGuard {
    fn drop(&mut self) {
        MANUAL_PING_IN_FLIGHT.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        let mut guard = LAST_MANUAL_PING_AT.write().unwrap_or_else(|e| e.into_inner());
        *guard = Some(std::time::Instant::now());
    }
}

/// Отмечает начало ручного пинга из UI/API; при выходе из области видимости фиксирует время завершения.
pub fn begin_manual_ping() -> ManualPingGuard {
    MANUAL_PING_IN_FLIGHT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    let mut guard = LAST_MANUAL_PING_AT.write().unwrap_or_else(|e| e.into_inner());
    *guard = Some(std::time::Instant::now());
    ManualPingGuard
}

/// Возвращает true, если фоновый тик PING-AUTO нужно пропустить (идёт ручной пинг или прошёл < 20 с).
pub fn should_skip_auto_ping() -> bool {
    if MANUAL_PING_IN_FLIGHT.load(std::sync::atomic::Ordering::SeqCst) > 0 {
        return true;
    }
    let guard = LAST_MANUAL_PING_AT.read().unwrap_or_else(|e| e.into_inner());
    if let Some(at) = *guard {
        if at.elapsed() < std::time::Duration::from_secs(AUTO_PING_COOLDOWN_SECS) {
            return true;
        }
    }
    false
}

/// Выполняет один тик фонового PING-AUTO, если не активен ручной пинг.
/// Возвращает `true`, если тик был выполнен, и `false`, если пропущен.
pub async fn run_auto_ping_tick(http: &reqwest::Client, cfg: &AppConfig) -> bool {
    if should_skip_auto_ping() {
        return false;
    }
    let ids = collect_auto_ping_ids(cfg);
    if !ids.is_empty() {
        let _ = ping_all_url(http, cfg, &ids, 2500, None).await;
    }
    true
}


/// Набор id для фонового автообновления пингов (чистая функция, тестируемая):
/// приоритетная цепочка failover + основной узел + авто-группы Fastest/Fallback/PROXY,
/// дедупликация с сохранением порядка, исключение ignore_servers и служебных групп, ограничение AUTO_PING_MAX_IDS.
/// Замер этих id держит историю mihomo свежей — GET /api/servers и опрос UI
/// отдают живые пинги без каких-либо действий пользователя в панели.
pub fn collect_auto_ping_ids(cfg: &AppConfig) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let push = |ids: &mut Vec<String>, seen: &mut std::collections::HashSet<String>, v: &str| {
        let v = v.trim();
        let is_ignored = cfg.ignore_servers.iter().any(|ig| ig.trim().eq_ignore_ascii_case(v));
        if !v.is_empty() && !is_service_group_or_builtin(v) && !is_ignored && seen.insert(v.to_string()) {
            ids.push(v.to_string());
        }
    };
    for id in &cfg.failover.priority_chain {
        push(&mut ids, &mut seen, id);
    }
    push(&mut ids, &mut seen, &cfg.failover.priority_server);
    for group in ["Fastest", "Fallback", "PROXY"] {
        push(&mut ids, &mut seen, group);
    }
    ids.truncate(AUTO_PING_MAX_IDS);
    ids
}

/// Выбор групп-кандидатов для пинга Google Flow (Баг 55):
/// при активном БС (`bs_active=true`) используются только БС-группа и Flow-группы, без `PROXY`/`Fastest`.
pub fn flow_ping_candidates(_cfg: &AppConfig, group_names: &[String], bs_active: bool) -> Vec<String> {
    let mut out = Vec::new();
    if bs_active {
        if let Some(wl) = group_names.iter().find(|g| g.as_str() == crate::routing::WHITELIST_GROUP_NAME) {
            out.push(wl.clone());
        }
        for g in group_names {
            if crate::routing::is_flow_group_name(g) && !out.contains(g) {
                out.push(g.clone());
            }
        }
        return out;
    }

    for g in group_names {
        if crate::routing::is_flow_group_name(g) && !out.contains(g) {
            out.push(g.clone());
        }
    }
    for cand in &["PROXY", "Fastest", "Fallback", "GLOBAL"] {
        if let Some(found) = group_names.iter().find(|g| g.as_str() == *cand) {
            if !out.contains(found) {
                out.push(found.clone());
            }
        }
    }
    out
}

/// Пинг участников Google AI группы и всех серверов специально по Google Flow URL (https://flow.google.com).
pub async fn ping_flow_servers(http: &reqwest::Client, cfg: &AppConfig, timeout_ms: u64) -> BTreeMap<String, i64> {
    let test_url = "https://flow.google.com";
    let proxies = get_proxies(http, cfg).await.unwrap_or_default();
    let bs_active = cfg.whitelist.enabled;
    let group_names: Vec<String> = proxies.keys().cloned().collect();

    for gname in flow_ping_candidates(cfg, &group_names, bs_active) {
        let mut pings = ping_group_url(http, cfg, &gname, timeout_ms, Some(test_url)).await;
        if bs_active && gname != crate::routing::WHITELIST_GROUP_NAME {
            let servers = get_servers(http, cfg, &cfg.failover.priority_chain).await.unwrap_or_default();
            pings.retain(|node_id, _| {
                if let Some(srv) = servers.iter().find(|s| s.id == *node_id) {
                    crate::failover::is_whitelist_server_full(&srv.name, &srv.id, srv.provider.as_deref(), cfg)
                } else {
                    crate::failover::is_whitelist_server_full(node_id, node_id, None, cfg)
                }
            });
        }
        if !pings.is_empty() {
            return pings;
        }
    }

    BTreeMap::new()
}

/// Проверяет и вычисляет исправление для селектора с учётом типа группы (БС / Gaming / обычная, Баг 123).
pub fn evaluate_selector_auto_heal_for_group<'a>(
    group_name: &str,
    now: &'a str,
    all: &'a [&'a str],
    proxies_has_key: impl Fn(&str) -> bool,
    cfg: &AppConfig,
) -> Option<&'a str> {
    if all.is_empty() {
        return None;
    }
    // Сервер считается валидным, если он не пуст И (содержится в all-списке группы ИЛИ присутствует среди корневых прокси).
    let now_valid = !now.is_empty() && (all.contains(&now) || proxies_has_key(now));
    if now_valid {
        return None;
    }

    let is_bs_group = group_name == crate::routing::WHITELIST_GROUP_NAME
        || (group_name.starts_with("🛡️ ") && group_name != "🛡️ Canary Probe")
        || group_name.starts_with("📱 ")
        || group_name.contains("Whitelist");
    if is_bs_group {
        // Для БС-селекторов НИКОГДА не выбираем Fallback/Fastest/VPS — только узел БС, иначе оставляем без изменений
        return all
            .iter()
            .find(|&&x| {
                x != now
                    && !matches!(x, "Fallback" | "Fastest" | "PROXY" | "DIRECT" | "REJECT")
                    && crate::whitelist_tags::is_whitelist_server_full(x, x, None, cfg)
            })
            .copied();
    }

    let is_gaming_group =
        group_name == crate::routing::GAMING_GROUP_NAME || group_name.starts_with("🎮 ");
    if is_gaming_group {
        return None;
    }

    let target = if all.contains(&"Fallback") {
        "Fallback"
    } else if all.contains(&"Fastest") {
        "Fastest"
    } else {
        all.iter()
            .find(|&&x| (all.contains(&x) || proxies_has_key(x)) && x != now)
            .copied()
            .unwrap_or(all[0])
    };
    Some(target)
}

/// Проверяет и вычисляет исправление для селектора, если его текущий узел отсутствует или невалиден.
/// Возвращает Some(target) если селектор нужно переключить, или None если он валиден.
pub fn evaluate_selector_auto_heal<'a>(
    now: &'a str,
    all: &'a [&'a str],
    proxies_has_key: impl Fn(&str) -> bool,
) -> Option<&'a str> {
    evaluate_selector_auto_heal_for_group("", now, all, proxies_has_key, &AppConfig::default())
}

/// Автоматическое восстановление зависших селекторов групп (если текущий сервер удален из подписки или переименован)
pub async fn auto_heal_proxy_selectors(http: &reqwest::Client, cfg: &AppConfig) {
    let proxies = match get_proxies(http, cfg).await {
        Ok(p) => p,
        Err(_) => return,
    };

    for (name, val) in &proxies {
        let p_type = val.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if p_type != "Selector" {
            continue;
        }

        let now = val.get("now").and_then(|n| n.as_str()).unwrap_or("");
        let all: Vec<&str> = val
            .get("all")
            .and_then(|a| a.as_array())
            .map(|arr| arr.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();

        if let Some(target) =
            evaluate_selector_auto_heal_for_group(name, now, &all, |k| proxies.contains_key(k), cfg)
        {
            crate::log_w!(
                "[AUTO-HEAL] Селектор '{}' ссылается на отсутствующий узел '{}'. Автоматическое переключение на '{}'",
                name,
                now,
                target
            );
            let _ = m_put(
                http,
                cfg,
                &format!("/proxies/{}", urlencoding_lite(name)),
                json!({ "name": target }),
                3,
            )
            .await;
        }
    }
}

fn is_concrete_server_candidate(name: &str) -> bool {
    let t = name.trim();
    !t.is_empty()
        && !t.eq_ignore_ascii_case("DIRECT")
        && !t.eq_ignore_ascii_case("REJECT")
        && !t.eq_ignore_ascii_case("Fallback")
        && !t.eq_ignore_ascii_case("Fastest")
        && !t.eq_ignore_ascii_case("GLOBAL")
        && !t.eq_ignore_ascii_case("follow_failover")
}

/// Вычисляет целевой сервер туннеля для восстановления селекторов после `reload_config` или старта панели.
pub fn desired_tunnel_server_for_cfg(
    cfg: &AppConfig,
    pre_reload_proxy_now: Option<&str>,
) -> Option<String> {
    if let Some(live) = pre_reload_proxy_now
        .map(str::trim)
        .filter(|s| is_concrete_server_candidate(s))
    {
        return Some(live.to_string());
    }
    if cfg.whitelist.enabled {
        if let Some(target) = cfg
            .whitelist
            .target_server
            .as_deref()
            .map(str::trim)
            .filter(|s| is_concrete_server_candidate(s))
        {
            return Some(target.to_string());
        }
    }
    if is_concrete_server_candidate(&cfg.failover.priority_server) {
        return Some(cfg.failover.priority_server.trim().to_string());
    }
    if let Some(first) = cfg
        .failover
        .priority_chain
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .find(|s| is_concrete_server_candidate(s))
    {
        return Some(first.to_string());
    }
    if let Some(first_bs) = cfg
        .failover
        .bs_priority_chain
        .iter()
        .map(String::as_str)
        .map(str::trim)
        .find(|s| is_concrete_server_candidate(s))
    {
        return Some(first_bs.to_string());
    }
    None
}

/// Восстанавливает активные селекторы `PROXY` и `🛡️ Whitelist` после `reload_config` или на старте,
/// чтобы перезагрузка YAML не сбрасывала выбранный сервер к первому элементу списка.
pub async fn restore_active_selectors_after_reload(
    http: &reqwest::Client,
    cfg: &AppConfig,
    pre_reload_proxy_now: Option<&str>,
) {
    invalidate_proxies_cache();
    let proxies = match get_proxies(http, cfg).await {
        Ok(p) => p,
        Err(_) => return,
    };

    let primary_group = find_primary_group(&proxies).unwrap_or_else(|| "PROXY".to_string());
    let primary_all: Vec<String> = proxies
        .get(&primary_group)
        .and_then(|v| v.get("all"))
        .and_then(|v| v.as_array())
        .map(|arr| arr.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    let primary_now = proxies
        .get(&primary_group)
        .and_then(|v| v.get("now"))
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if pre_reload_proxy_now.is_some_and(|p| p.trim().eq_ignore_ascii_case("DIRECT")) {
        let direct_in_primary = primary_all.is_empty()
            || primary_all.iter().any(|x| x.eq_ignore_ascii_case("DIRECT"));
        if direct_in_primary && !primary_now.eq_ignore_ascii_case("DIRECT") {
            if let Err(e) = switch_selector(http, cfg, &primary_group, "DIRECT").await {
                crate::log_w!(
                    "[MIHOMO] Не удалось сохранить DIRECT для '{}' после reload: {}",
                    primary_group,
                    e
                );
            }
        }
        return;
    }

    let candidate = desired_tunnel_server_for_cfg(cfg, pre_reload_proxy_now);
    let desired_for_primary = match candidate {
        Some(ref c) if primary_all.is_empty() || primary_all.iter().any(|x| x == c) => Some(c.clone()),
        _ => desired_tunnel_server_for_cfg(cfg, None)
            .filter(|c| primary_all.is_empty() || primary_all.iter().any(|x| x == c)),
    };

    if let Some(ref target) = desired_for_primary {
        if primary_now != target {
            if let Err(e) = switch_selector(http, cfg, &primary_group, target).await {
                crate::log_w!(
                    "[MIHOMO] Не удалось восстановить селектор '{}' -> '{}' после reload: {}",
                    primary_group,
                    target,
                    e
                );
            } else {
                crate::log_i!(
                    "[MIHOMO] Селектор '{}' восстановлен на '{}' после reload (было '{}')",
                    primary_group,
                    target,
                    primary_now
                );
            }
        }
    }

    if cfg.whitelist.enabled {
        if let Some(wl_group) = proxies.get(crate::routing::WHITELIST_GROUP_NAME) {
            let wl_all: Vec<String> = wl_group
                .get("all")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|x| x.as_str().map(str::to_string)).collect())
                .unwrap_or_default();
            let wl_now = wl_group.get("now").and_then(|v| v.as_str()).unwrap_or("");

            let configured_wl = cfg
                .whitelist
                .target_server
                .as_deref()
                .map(str::trim)
                .filter(|s| is_concrete_server_candidate(s))
                .map(str::to_string);
            let wl_target = desired_for_primary
                .as_ref()
                .filter(|c| wl_all.is_empty() || wl_all.iter().any(|x| x == *c))
                .cloned()
                .or_else(|| configured_wl.filter(|c| wl_all.is_empty() || wl_all.iter().any(|x| x == c)));

            if let Some(ref target) = wl_target {
                if wl_now != target {
                    if let Err(e) = switch_selector(http, cfg, crate::routing::WHITELIST_GROUP_NAME, target).await {
                        crate::log_w!(
                            "[MIHOMO] Не удалось восстановить селектор '{}' -> '{}' после reload: {}",
                            crate::routing::WHITELIST_GROUP_NAME,
                            target,
                            e
                        );
                    } else {
                        crate::log_i!(
                            "[MIHOMO] Селектор '{}' восстановлен на '{}' после reload (было '{}')",
                            crate::routing::WHITELIST_GROUP_NAME,
                            target,
                            wl_now
                        );
                    }
                }
            }
        }
    }
}

/// Reload конфига Mihomo (после правки config.yaml). Путь — из настроек.
pub async fn reload_config(http: &reqwest::Client, cfg: &AppConfig) -> Result<(), String> {
    invalidate_proxies_cache();
    let pre_reload_proxy_now = get_proxies(http, cfg).await.ok().and_then(|proxies| {
        let pg = find_primary_group(&proxies)?;
        proxies
            .get(&pg)
            .and_then(|v| v.get("now"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    });
    let res = m_put(http, cfg, "/configs?force=true", json!({ "path": cfg.mihomo.config_path }), 15).await;
    invalidate_proxies_cache();
    auto_heal_proxy_selectors(http, cfg).await;
    if res.is_ok() {
        restore_active_selectors_after_reload(http, cfg, pre_reload_proxy_now.as_deref()).await;
    }
    res
}



/// Переключение группы с точечным сбросом сокетов ТОЛЬКО этой группы
/// (per-device failover, расписания устройств). В отличие от switch_group(),
/// не рвёт соединения остальных устройств LAN и DIRECT-сессии (контракт F-02):
/// раньше failover одного устройства обрывал игры/звонки всем остальным.
pub async fn switch_group_scoped(http: &reqwest::Client, cfg: &AppConfig, group: &str, server: &str) -> Result<(), String> {
    switch_selector(http, cfg, group, server).await?;
    let _ = close_server_connections(http, cfg, group).await;
    Ok(())
}

/// Переключение селектор-группы БЕЗ сброса клиентских соединений (для Canary Probe и фоновых тестов).
/// В отличие от switch_group(), данный метод не разрывает активные сокеты клиентов.
pub async fn switch_selector(
    http: &reqwest::Client,
    cfg: &AppConfig,
    group: &str,
    server: &str,
) -> Result<(), String> {
    let enc = urlencoding_lite(group);
    m_put(
        http,
        cfg,
        &format!("/proxies/{enc}"),
        json!({ "name": server }),
        5,
    )
    .await?;

    invalidate_proxies_cache();

    Ok(())
}


/// Версия ядра Mihomo из GET /version.
pub async fn get_version(http: &reqwest::Client, cfg: &AppConfig) -> Option<String> {
    let url = format!("{}/version", cfg.mihomo_url());
    let mut req = http.get(&url).timeout(std::time::Duration::from_millis(1500));
    if !cfg.mihomo.secret.is_empty() {
        req = req.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
    }
    let resp = req.send().await.ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    v.get("version").and_then(|v| v.as_str()).map(|s| s.to_string())
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct GoogleGeoStatus {
    pub is_clean: bool,
    pub server_name: String,
    pub google_country: String,
    pub google_domain: String,
    pub client_ip: String,
    pub google_lang: String,
    pub active_server: String,
    pub message: String,
}

/// Определение региона сервера по названию (Баг 55: строгий разбор `BY` как заглавного токена, не ` by `).
pub fn detect_country(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    let is_ru = lower.contains("росси")
        || lower.contains("russia")
        || lower.contains(" ru ")
        || lower.contains("[ru]")
        || lower.contains("(ru)")
        || lower.contains(" ru-")
        || lower.contains(" ru_")
        || lower.starts_with("ru ")
        || lower.starts_with("ru-")
        || lower.starts_with("ru_")
        || lower.ends_with(" ru")
        || lower.ends_with("-ru")
        || lower.ends_with("_ru")
        || lower.contains("🇷🇺")
        || lower.contains("москва")
        || lower.contains("moscow")
        || lower.contains("санкт-петербург")
        || lower.contains("питер")
        || lower.contains("спб");

    let has_uppercase_by_token = name
        .split(|c: char| !c.is_alphanumeric())
        .any(|tok| tok == "BY");
    let is_by = lower.contains("беларус")
        || lower.contains("belarus")
        || has_uppercase_by_token
        || lower.contains("🇧🇾")
        || lower.contains("минск")
        || lower.contains("minsk");

    let is_fi = lower.contains("финлянд")
        || lower.contains("finland")
        || lower.contains(" fi ")
        || lower.contains("[fi]")
        || lower.contains("(fi)")
        || lower.contains("🇫🇮")
        || lower.contains("хельсинки")
        || lower.contains("helsinki");

    let is_kz = lower.contains("казахстан")
        || lower.contains("kazakhstan")
        || lower.contains(" kz ")
        || lower.contains("[kz]")
        || lower.contains("🇰🇿")
        || lower.contains("алматы")
        || lower.contains("астана");

    let is_ca = lower.contains("канад")
        || lower.contains("canada")
        || lower.contains(" ca ")
        || lower.contains("[ca]")
        || lower.contains("(ca)")
        || lower.contains(" ca-")
        || lower.contains(" ca_")
        || lower.starts_with("ca ")
        || lower.starts_with("ca-")
        || lower.starts_with("ca_")
        || lower.ends_with(" ca")
        || lower.ends_with("-ca")
        || lower.ends_with("_ca")
        || lower.contains("🇨🇦");

    let is_us = lower.contains("сша")
        || lower.contains("usa")
        || lower.contains("united states")
        || lower.contains(" us ")
        || lower.contains("[us]")
        || lower.contains("(us)")
        || lower.contains(" us-")
        || lower.contains(" us_")
        || lower.starts_with("us ")
        || lower.starts_with("us-")
        || lower.starts_with("us_")
        || lower.ends_with(" us")
        || lower.ends_with("-us")
        || lower.ends_with("_us")
        || lower.contains("🇺🇸")
        || lower.contains("вашингтон")
        || lower.contains("washington")
        || lower.contains("chicago")
        || lower.contains("чикаго")
        || lower.contains("miami")
        || lower.contains("майами")
        || lower.contains("seattle")
        || lower.contains("сиэтл")
        || lower.contains("лос-анджелес")
        || lower.contains("los angeles")
        || lower.contains("атланта")
        || lower.contains("atlanta")
        || lower.contains("феникс")
        || lower.contains("phoenix");

    let is_de = lower.contains("герман")
        || lower.contains("germany")
        || lower.contains(" de ")
        || lower.contains("[de]")
        || lower.contains("(de)")
        || lower.contains(" de-")
        || lower.contains(" de_")
        || lower.contains("🇩🇪")
        || lower.contains("frankfurt")
        || lower.contains("франкфурт")
        || lower.contains("berlin")
        || lower.contains("берлин")
        || lower.starts_with("de ")
        || lower.starts_with("de-")
        || lower.starts_with("de_")
        || lower.ends_with(" de")
        || lower.ends_with("-de")
        || lower.ends_with("_de");

    let is_nl = lower.contains("нидерланд")
        || lower.contains("netherlands")
        || lower.contains("holland")
        || lower.contains("амстердам")
        || lower.contains("amsterdam")
        || lower.contains(" nl ")
        || lower.contains("[nl]")
        || lower.contains("(nl)")
        || lower.contains(" nl-")
        || lower.contains(" nl_")
        || lower.contains("🇳🇱")
        || lower.starts_with("nl ")
        || lower.starts_with("nl-")
        || lower.starts_with("nl_")
        || lower.ends_with(" nl")
        || lower.ends_with("-nl")
        || lower.ends_with("_nl");

    if is_us {
        Some("US")
    } else if is_ca {
        Some("CA")
    } else if is_ru {
        Some("RU")
    } else if is_by {
        Some("BY")
    } else if is_de {
        Some("DE")
    } else if is_nl {
        Some("NL")
    } else if is_fi {
        Some("FI")
    } else if is_kz {
        Some("KZ")
    } else {
        None
    }
}

/// Определение региона сервера по названию (со строковым фоллбеком `"GLOBAL"`).
pub fn detect_server_country(name: &str) -> String {
    detect_country(name).unwrap_or("GLOBAL").to_string()
}

/// Быстрая оценка статуса Flow для сервера ("ok" | "blocked" | "unknown")
pub fn eval_flow_status(raw_id: &str, display_name: &str) -> &'static str {
    let country_raw = detect_server_country(raw_id);
    let country_disp = detect_server_country(display_name);

    if country_raw == "RU" || country_disp == "RU" || country_raw == "BY" || country_disp == "BY" {
        "blocked"
    } else {
        "ok"
    }
}

/// Проверка доступности Google Flow & AI через выделенный узел Flow
pub async fn check_google_geo(http: &reqwest::Client, cfg: &AppConfig, active_server: &str) -> Result<GoogleGeoStatus, String> {
    let detected_country = detect_server_country(active_server);
    let is_blocked = detected_country == "RU";
    let is_us = detected_country == "US";
    let is_ca = detected_country == "CA";

    // 2. Тестирование задержки до flow.google.com через группу Google AI в Mihomo
    let mut delay_ms: Option<u64> = None;
    let proxies = get_proxies(http, cfg).await.unwrap_or_default();
    let mut flow_candidates: Vec<String> = ["Google AI", "Google-AI", "GoogleFlow", "Flow"]
        .iter()
        .filter(|g| proxies.contains_key(**g) && crate::routing::is_flow_group_name(g))
        .map(|g| (*g).to_string())
        .collect();
    for gname in proxies.keys() {
        if crate::routing::is_flow_group_name(gname) && !flow_candidates.iter().any(|c| c == gname) {
            flow_candidates.push(gname.clone());
        }
    }
    if flow_candidates.is_empty() {
        flow_candidates.extend(["Google AI", "Google-AI", "GoogleFlow", "Flow"].iter().map(|s| (*s).to_string()));
    }
    for gname in &flow_candidates {
        let enc = urlencoding_lite(gname);
        if let Ok(v) = m_get(http, cfg, &format!("/proxies/{enc}/delay?url=https://flow.google.com&timeout=4000")).await {
            if let Some(d) = v.get("delay").and_then(|d| d.as_u64()) {
                if d > 0 {
                    delay_ms = Some(d);
                    break;
                }
            }
        }
    }

    // 3. HTTP GET запрос к flow.google.com через mixed_port (маршрутизирует flow.google.com на Google AI группу)
    let mixed_port = m_get(http, cfg, "/configs")
        .await
        .ok()
        .and_then(|v| v.get("mixed-port").and_then(|p| p.as_u64()))
        .unwrap_or(cfg.mihomo.mixed_port as u64);

    let proxy_url = format!("http://{}:{mixed_port}", cfg.mihomo.host);
    let client = match reqwest::Proxy::all(&proxy_url) {
        Ok(p) => reqwest::Client::builder()
            .proxy(p)
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_else(|_| http.clone()),
        Err(_) => http.clone(),
    };

    let http_ok = match client
        .get("https://flow.google.com")
        .header("User-Agent", "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36")
        .send()
        .await
    {
        Ok(resp) => resp.status().is_success() || resp.status().is_redirection(),
        Err(_) => false,
    };

    // 4. Определение чистоты (is_clean): только при отсутствии блокировки и при подтверждении связи (HTTP или замер задержки)
    let is_clean = !is_blocked && (http_ok || delay_ms.is_some());

    let google_lang = if is_us {
        "en-US".to_string()
    } else if is_ca {
        "en-CA".to_string()
    } else if is_blocked {
        "ru".to_string()
    } else {
        "en".to_string()
    };

    let message = if is_clean {
        let ping_str = delay_ms.map(|d| format!(" ({d} мс)")).unwrap_or_default();
        format!("Google Flow доступен через выделенный узел '{active_server}'{ping_str}. Регион: {detected_country}. Трафик AI изолирован от основного прокси.")
    } else if is_blocked {
        format!("Узел '{active_server}' связан с регионом {detected_country} — Google Flow и Gemini Labs будут заблокированы! Рекомендуется переключиться на чистый узел США или Канады.")
    } else {
        format!("Узел '{active_server}' не смог подтвердить доступ к Google Flow (регион: {detected_country}). Проверьте соединение или выберите чистый узел США.")
    };

    Ok(GoogleGeoStatus {
        is_clean,
        server_name: active_server.to_string(),
        google_country: detected_country,
        google_domain: "flow.google.com".to_string(),
        client_ip: String::new(),
        google_lang,
        active_server: active_server.to_string(),
        message,
    })
}

/// Минимальный percent-encoding для имён прокси в URL.
pub fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}


#[cfg(test)]
#[path = "mihomo_tests.rs"]
mod tests;
#[cfg(test)]
pub use tests::{
    clear_proxies_cache_for_test, set_provider_proxies_cache_for_test, set_proxies_cache_for_test,
    PROXIES_CACHE_TEST_LOCK,
};
