use std::time::Duration;
use serde_json::Value;
use crate::mihomo::{self, Server};
use super::*;

/// Нагрузочный стресс-тест в 3 параллельных сокета через проверяемый узел.
/// Симулирует реальный трафик клиентов (загрузка 100 КБ полезной нагрузки + 2 параллельных L7-пробника).
/// Если сервер глушится DPI при открытии нескольких сокетов или таймаутит — возвращает <= 0 (-1).
pub async fn run_canary_probe(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    server_id: &str,
    timeout_ms: u64,
) -> i64 {
    let probe_timeout = timeout_ms.max(5000);
    let canary_url = canary_payload_url(cfg);
    let (d1, d2, d3) = tokio::join!(
        mihomo::probe_server_l7(http, cfg, server_id, probe_timeout, &canary_url, 200),
        mihomo::probe_server_l7(http, cfg, server_id, probe_timeout, mihomo::SECONDARY_L7_CHECK_URL, 204),
        mihomo::probe_server_l7(http, cfg, server_id, probe_timeout, mihomo::PRIMARY_L7_CHECK_URL, 204),
    );
    if d1 > 0 && d2 > 0 && d3 > 0 {
        std::cmp::max(d1, std::cmp::max(d2, d3))
    } else {
        -1
    }
}

/// F-05: WAN-проба по трём независимым эндпоинтам — падение одного сайта, DPI-блок
/// или временный сбой не должны замораживать failover при живом канале
/// (раньше единственный URL ya.ru через mihomo был точкой отказа самой защиты).
pub const WAN_CHECK_URLS: &[&str] = &[
    "http://ya.ru",
    "http://cp.cloudflare.com/generate_204",
    "http://connectivitycheck.gstatic.com/generate_204",
];

#[allow(dead_code)]
pub const WAN_CHECK_URL: &str = WAN_CHECK_URLS[0];

/// Проверка физической доступности провайдера (WAN) напрямую через DIRECT.
/// Если шлюз провайдера или кабель упал, возвращает false.
pub async fn is_wan_online(http: &reqwest::Client, cfg: &crate::config::AppConfig) -> bool {
    for url in WAN_CHECK_URLS {
        let ms = mihomo::ping_server_url_expected_range(
            http,
            cfg,
            "DIRECT",
            1500,
            Some(url),
            Some(wan_check_expected_status(url)),
        ).await;
        if ms > 0 {
            return true;
        }
    }
    false
}

/// Ожидаемый HTTP-статус для WAN-пробы (баг 102): generate_204 отвечает 204, а http://ya.ru —
/// 200 или редиректом на https. Раньше всем трём слали expected=200, и 204-пробы падали всегда.
pub fn wan_check_expected_status(url: &str) -> &'static str {
    if url.contains("generate_204") {
        "204"
    } else {
        "200-399"
    }
}

/// F-01: строгое сопоставление соединения с сервером (без fuzzy contains).
/// Раньше `sl.contains(&active_lower)` зачислял трафик сервера «DE-10» узлу «DE-1»:
/// критерий здоровья «есть живой трафик» получал чужие байты, упавший сервер
/// считался живым и failover не переключал. Служебные группы (proxy/global/direct/reject)
/// никогда не считаются трафиком конкретного узла.
pub fn conn_chain_matches_server(chains: &[Value], active_lower: &str) -> bool {
    chains.iter().any(|item| {
        item.as_str()
            .map(|s| {
                let sl = s.to_lowercase();
                if sl == "proxy" || sl == "global" || sl == "direct" || sl == "reject" {
                    false
                } else {
                    sl == active_lower
                }
            })
            .unwrap_or(false)
    })
}

/// Снимок сокетов для вычисления прироста трафика (дельты): { id -> (cumulative_upload, cumulative_download) }
pub(crate) static CONN_SNAPSHOT: std::sync::LazyLock<tokio::sync::Mutex<std::collections::HashMap<String, (u64, u64)>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

#[allow(dead_code)]
pub async fn clear_conn_snapshot() {
    CONN_SNAPSHOT.lock().await.clear();
}

/// Снимок сокетов для per-device failover: { id -> (cumulative_upload, cumulative_download) }
pub(crate) static DEV_CONN_SNAPSHOT: std::sync::LazyLock<tokio::sync::Mutex<std::collections::HashMap<String, (u64, u64)>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

#[allow(dead_code)]
pub async fn clear_dev_conn_snapshot() {
    DEV_CONN_SNAPSHOT.lock().await.clear();
}

/// Снятие снимка сокетов через единый запрос /connections к Mihomo API
#[allow(dead_code)]
pub async fn fetch_connections_snapshot(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
) -> Result<std::collections::HashMap<String, (u64, u64)>, String> {
    let val = mihomo::m_get(http, cfg, "/connections").await?;
    let mut map = std::collections::HashMap::new();
    if let Some(conns) = val.get("connections").and_then(|c| c.as_array()) {
        for c in conns {
            if let Some(id) = c.get("id").and_then(|i| i.as_str()) {
                let up = c.get("upload").and_then(|u| u.as_u64()).unwrap_or(0);
                let dl = c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);
                map.insert(id.to_string(), (up, dl));
            }
        }
    }
    Ok(map)
}

/// Вычисление дельт трафика (download_delta, upload_delta) между предыдущим и текущим снимками
pub fn calc_traffic_deltas(prev: (u64, u64), current: (u64, u64)) -> (u64, u64) {
    let delta_up = current.0.saturating_sub(prev.0);
    let delta_dl = current.1.saturating_sub(prev.1);
    (delta_dl, delta_up)
}

/// Чистая обработка массива соединений с расчётом дельт трафика и детекцией чёрной дыры.
/// Возвращает (has_live_traffic, total_delta_download, total_delta_upload, matched_sockets, is_blackhole).
pub fn process_connections_delta(
    conns: &[Value],
    active_server_id: &str,
    snapshot: &mut std::collections::HashMap<String, (u64, u64)>,
) -> (bool, u64, u64, usize, bool) {
    if active_server_id.is_empty() || active_server_id.eq_ignore_ascii_case("direct") {
        return (false, 0, 0, 0, false);
    }
    let active_lower = active_server_id.to_lowercase();
    let mut matched_sockets = 0usize;
    let mut total_delta_upload = 0u64;
    let mut total_delta_download = 0u64;

    let mut current_ids = std::collections::HashSet::new();

    for c in conns {
        let Some(id) = c.get("id").and_then(|i| i.as_str()) else { continue; };
        let cur_up = c.get("upload").and_then(|u| u.as_u64()).unwrap_or(0);
        let cur_dl = c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);
        current_ids.insert(id.to_string());

        let in_chains = c.get("chains")
            .and_then(|ch| ch.as_array())
            .map(|arr| conn_chain_matches_server(arr, &active_lower))
            .unwrap_or(false);

        let (prev_up, prev_dl) = snapshot.get(id).copied().unwrap_or((0, 0));
        let (delta_dl, delta_up) = calc_traffic_deltas((prev_up, prev_dl), (cur_up, cur_dl));

        snapshot.insert(id.to_string(), (cur_up, cur_dl));

        if in_chains {
            matched_sockets += 1;
            total_delta_upload += delta_up;
            total_delta_download += delta_dl;
        }
    }

    // Удаляем закрытые соединения, чтобы предотвратить утечку памяти
    snapshot.retain(|k, _| current_ids.contains(k));

    let has_live_traffic = total_delta_download > 1024;
    let is_blackhole = matched_sockets >= 3 && total_delta_upload > 5120 && total_delta_download == 0;

    (has_live_traffic, total_delta_download, total_delta_upload, matched_sockets, is_blackhole)
}

/// Обработка соединений для отдельного устройства с расчётом дельт трафика и детекцией чёрной дыры.
/// Сравнивает текущие соединения со снимком prev_snapshot.
/// Возвращает (has_live_traffic, total_delta_download, total_delta_upload, matched_sockets, is_blackhole).
pub fn process_device_connections_delta(
    conns: &[Value],
    device_ip: &str,
    device_group: &str,
    active_server_id: &str,
    prev_snapshot: &std::collections::HashMap<String, (u64, u64)>,
) -> (bool, u64, u64, usize, bool) {
    if active_server_id.is_empty() || active_server_id.eq_ignore_ascii_case("direct") {
        return (false, 0, 0, 0, false);
    }
    let active_lower = active_server_id.to_lowercase();
    let group_lower = device_group.to_lowercase();
    let mut matched_sockets = 0usize;
    let mut total_delta_upload = 0u64;
    let mut total_delta_download = 0u64;

    for c in conns {
        let Some(id) = c.get("id").and_then(|i| i.as_str()) else { continue; };
        let cur_up = c.get("upload").and_then(|u| u.as_u64()).unwrap_or(0);
        let cur_dl = c.get("download").and_then(|d| d.as_u64()).unwrap_or(0);

        let src_ip = c.get("metadata")
            .and_then(|m| m.get("sourceIP"))
            .and_then(|s| s.as_str())
            .unwrap_or("");

        let chains = c.get("chains").and_then(|ch| ch.as_array());

        let matches_device = src_ip == device_ip
            || (!group_lower.is_empty() && chains.map_or(false, |arr| {
                arr.iter().any(|item| {
                    item.as_str().map_or(false, |s| s.to_lowercase() == group_lower)
                })
            }));

        let matches_server = chains.map_or(false, |arr| {
            conn_chain_matches_server(arr, &active_lower)
        });

        if matches_device && matches_server {
            let (prev_up, prev_dl) = prev_snapshot.get(id).copied().unwrap_or((0, 0));
            let (delta_dl, delta_up) = calc_traffic_deltas((prev_up, prev_dl), (cur_up, cur_dl));

            matched_sockets += 1;
            total_delta_upload += delta_up;
            total_delta_download += delta_dl;
        }
    }

    let has_live_traffic = total_delta_download > 1024;
    let is_blackhole = matched_sockets >= 3 && total_delta_upload > 5120 && total_delta_download == 0;

    (has_live_traffic, total_delta_download, total_delta_upload, matched_sockets, is_blackhole)
}

/// Единая функция оценки активного трафика и аномалии "Чёрная дыра" за один проход.
/// Принимает опционально предзагруженный срез соединений (для исключения повторных запросов).
pub async fn check_active_traffic_and_blackhole(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    active_server_id: &str,
    cached_conns: Option<&[Value]>,
) -> (bool, u64, u64, usize, bool) {
    if let Some(conns) = cached_conns {
        let mut snap = CONN_SNAPSHOT.lock().await;
        let res = process_connections_delta(conns, active_server_id, &mut snap);
        if res.4 {
            crate::log_w!(
                "Обнаружена аномалия 'Чёрная дыра' на сервере '{}': {} сокетов, delta upload: {} байт, delta download: 0 байт",
                active_server_id, res.3, res.2
            );
        }
        return res;
    }

    let Ok(val) = mihomo::m_get(http, cfg, "/connections").await else {
        return (false, 0, 0, 0, false);
    };
    let Some(conns) = val.get("connections").and_then(|c| c.as_array()) else {
        return (false, 0, 0, 0, false);
    };

    let mut snap = CONN_SNAPSHOT.lock().await;
    let res = process_connections_delta(conns, active_server_id, &mut snap);
    if res.4 {
        crate::log_w!(
            "Обнаружена аномалия 'Чёрная дыра' на сервере '{}': {} сокетов, delta upload: {} байт, delta download: 0 байт",
            active_server_id, res.3, res.2
        );
    }
    res
}

/// Проверка реального входящего клиентского трафика через активный узел в Mihomo по дельте.
/// Возвращает (has_live_traffic, delta_download_bytes).
/// Живой трафик — только при дельте download > 1 КБ (1024 байта) за интервал проверки.
#[cfg(test)]
pub async fn check_active_download_traffic(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    active_server_id: &str,
) -> (bool, u64) {
    let (has_live, dl_delta, _, _, _) = check_active_traffic_and_blackhole(http, cfg, active_server_id, None).await;
    (has_live, dl_delta)
}

/// Определение исправности активного сервера:
/// 1) Если обнаружена "чёрная дыра" — ВСЕГДА неисправен.
/// 2) Если пинг в норме (ping > 0 && ping <= threshold) — исправен.
/// 3) Если пинг не получен (ping <= 0), но клиенты реально скачивают трафик (has_live_traffic) — исправен.
///    Если же пинг > threshold, активный узел считается медленным/неисправным даже при наличии трафика.
pub fn is_active_server_healthy(
    ping: i64,
    threshold: i64,
    has_live_traffic: bool,
    is_blackhole: bool,
) -> bool {
    !is_blackhole && ((ping > 0 && ping <= threshold) || (ping <= 0 && has_live_traffic))
}

/// Проверка условий переключения Best-Ping с учётом гистерезиса и порога:
/// 1) Если обнаружена "чёрная дыра" — немедленное переключение.
/// 2) Если сервер мёртв / не отвечает (act_ping <= 0) — немедленное переключение.
/// 3) Если активный узел превышает порог (act_ping > threshold), а кандидат укладывается в порог
///    (best_ms > 0 && best_ms <= threshold) — гистерезис не блокирует переключение.
/// 4) В остальных случаях лучший кандидат должен превосходить активный минимум на hysteresis.
pub fn should_best_ping_switch(
    act_ping: i64,
    threshold: i64,
    is_blackhole: bool,
    best_ms: i64,
    hysteresis: i64,
) -> bool {
    if is_blackhole || act_ping <= 0 {
        return true;
    }
    if threshold > 0 && act_ping > threshold && best_ms > 0 && best_ms <= threshold {
        return true;
    }
    best_ms + hysteresis < act_ping
}

/// Лимит одновременных L7-проверок для мгновенного фейловера.
pub const PING_CONCURRENCY_LIMIT: usize = 12;

/// Построение компактного шортлиста серверов для быстрого параллельного L7 пинга (~1.5-2с).
/// Отбирает до 8-10 лучших из Tier 1 (обычные VPS) и до 4 из Tier 2 (белые списки),
/// суммарно не более 12 (PING_CONCURRENCY_LIMIT), чтобы пинг выполнился за один параллельный залп.
pub fn build_fast_failover_shortlist(
    servers: &[Server],
    active_id: &str,
    cfg: &crate::config::AppConfig,
) -> Vec<String> {
    let eligible: Vec<&Server> = servers
        .iter()
        .filter(|s| {
            s.id != active_id
                && !s.is_pool
                && s.protocol != "URL-TEST"
                && s.protocol != "FALLBACK"
                && s.id != "DIRECT"
                && s.id != "REJECT"
                && s.id != "GLOBAL"
                && !s.id.starts_with("AUTO-")
        })
        .collect();

    let is_bs = |s: &Server| is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), cfg);

    let mut tier1: Vec<&Server> = eligible.iter().filter(|s| !is_bs(s)).copied().collect();
    let mut tier2: Vec<&Server> = eligible.iter().filter(|s| is_bs(s)).copied().collect();

    // Сортировка кандидатов по известному историческому/кэшированному пингу
    let sort_fn = |list: &mut Vec<&Server>| {
        list.sort_by_key(|s| {
            let p = crate::mihomo::cached_ping(&s.id).unwrap_or(s.ping_ms);
            if p > 0 { p } else { i64::MAX }
        });
    };
    sort_fn(&mut tier1);
    sort_fn(&mut tier2);

    let mut shortlist = Vec::with_capacity(PING_CONCURRENCY_LIMIT);

    // До 8-10 из Tier 1
    let t1_target = if tier2.is_empty() {
        PING_CONCURRENCY_LIMIT
    } else {
        8.min(PING_CONCURRENCY_LIMIT)
    };
    for s in tier1.iter().take(t1_target) {
        shortlist.push(s.id.clone());
    }

    // До 4 из Tier 2
    let remaining = PING_CONCURRENCY_LIMIT.saturating_sub(shortlist.len());
    for s in tier2.iter().take(remaining.min(4)) {
        shortlist.push(s.id.clone());
    }

    // Если остались слоты, добираем из Tier 1
    if shortlist.len() < PING_CONCURRENCY_LIMIT {
        let current_count = shortlist.len();
        for s in tier1.iter().skip(t1_target).take(PING_CONCURRENCY_LIMIT - current_count) {
            if !shortlist.contains(&s.id) {
                shortlist.push(s.id.clone());
            }
        }
    }

    // Если все еще остались слоты, добираем из Tier 2
    if shortlist.len() < PING_CONCURRENCY_LIMIT {
        let current_count = shortlist.len();
        for s in tier2.iter().skip(4).take(PING_CONCURRENCY_LIMIT - current_count) {
            if !shortlist.contains(&s.id) {
                shortlist.push(s.id.clone());
            }
        }
    }

    shortlist
}

/// Ступенчатый опрос списка серверов через семафор (по 4 потока) с интервалом между запусками,
/// чтобы минимизировать нагрузку на CPU роутера Keenetic.
pub async fn ping_servers_staggered(
    http: &reqwest::Client,
    cfg: &crate::config::AppConfig,
    ids: &[String],
    timeout_ms: u64,
    concurrency: usize,
    interval_ms: u64,
) -> std::collections::BTreeMap<String, i64> {
    let mut out = std::collections::BTreeMap::new();
    if ids.is_empty() {
        return out;
    }
    let max_concurrency = concurrency.clamp(1, 6);
    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(max_concurrency));
    let shared_cfg = std::sync::Arc::new(cfg.clone());
    let bounded_timeout = timeout_ms.clamp(1000, 2500);
    let mut handles = Vec::new();

    for id in ids {
        let http_c = http.clone();
        let cfg_c = shared_cfg.clone();
        let sem_c = sem.clone();
        let id_c = id.clone();
        handles.push(tokio::spawn(async move {
            let _permit = sem_c.acquire().await.ok();
            let ms = mihomo::ping_server_url(&http_c, &cfg_c, &id_c, bounded_timeout, None).await;
            (id_c, ms)
        }));
        if interval_ms > 0 {
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
    }

    for h in handles {
        if let Ok((id, ms)) = h.await {
            out.insert(id, ms);
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelectedTarget {
    pub id: String,
    pub ping_ms: i64,
    pub is_quarantined: bool,
    pub is_alive_slow: bool,
    pub tier: u8,
}

impl std::ops::Deref for SelectedTarget {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.id
    }
}

impl PartialEq<str> for SelectedTarget {
    fn eq(&self, other: &str) -> bool {
        self.id == other
    }
}

impl PartialEq<&str> for SelectedTarget {
    fn eq(&self, other: &&str) -> bool {
        self.id == *other
    }
}

impl PartialEq<String> for SelectedTarget {
    fn eq(&self, other: &String) -> bool {
        &self.id == other
    }
}

/// Чистая 4-уровневая иерархия отбора резервного сервера:
/// 1. В пределах порога пинга вне карантина (0 < ping <= threshold)
/// 2. Живой медленный вне карантина (ping > threshold)
pub trait PingMap {
    fn get_ping(&self, id: &str) -> Option<i64>;
}

impl PingMap for std::collections::HashMap<String, i64> {
    fn get_ping(&self, id: &str) -> Option<i64> {
        self.get(id).copied()
    }
}

impl PingMap for std::collections::BTreeMap<String, i64> {
    fn get_ping(&self, id: &str) -> Option<i64> {
        self.get(id).copied()
    }
}

/// Отбор кандидата только по 1-му уровню иерархии (Быстрый и здоровый: 0 < ping <= threshold && !quarantined).
pub fn select_fast_healthy_from_candidates(
    candidates: &[String],
    pings: &impl PingMap,
    is_quarantined_fn: impl Fn(&str) -> bool,
    threshold: i64,
    preserve_order: bool,
) -> Option<SelectedTarget> {
    let mut list = Vec::new();
    for id in candidates {
        if let Some(ms) = pings.get_ping(id.as_str()) {
            if ms <= 0 {
                continue;
            }
            if !is_quarantined_fn(id.as_str()) && ms <= threshold {
                list.push((id.clone(), ms));
            }
        }
    }
    if list.is_empty() {
        return None;
    }
    if !preserve_order {
        list.sort_by_key(|(_, ms)| *ms);
    }
    list.first().map(|(id, ms)| SelectedTarget {
        id: id.clone(),
        ping_ms: *ms,
        is_quarantined: false,
        is_alive_slow: false,
        tier: 1,
    })
}

/// Отбор аварийного медленного / карантинного кандидата (уровни 2, 3, 4: slow или quarantined).
pub fn select_fallback_emergency_from_candidates(
    candidates: &[String],
    pings: &impl PingMap,
    is_quarantined_fn: impl Fn(&str) -> bool,
    threshold: i64,
    preserve_order: bool,
) -> Option<SelectedTarget> {
    let mut tier2 = Vec::new();
    let mut tier3 = Vec::new();
    let mut tier4 = Vec::new();

    for id in candidates {
        if let Some(ms) = pings.get_ping(id.as_str()) {
            if ms <= 0 {
                continue;
            }
            let quarantined = is_quarantined_fn(id.as_str());
            if !quarantined && ms > threshold {
                tier2.push((id.clone(), ms));
            } else if quarantined && ms <= threshold {
                tier3.push((id.clone(), ms));
            } else if quarantined && ms > threshold {
                tier4.push((id.clone(), ms));
            }
        }
    }

    let pick = |mut list: Vec<(String, i64)>, tier: u8, is_q: bool, is_slow: bool| -> Option<SelectedTarget> {
        if list.is_empty() {
            return None;
        }
        if !preserve_order {
            list.sort_by_key(|(_, ms)| *ms);
        }
        list.first().map(|(id, ms)| SelectedTarget {
            id: id.clone(),
            ping_ms: *ms,
            is_quarantined: is_q,
            is_alive_slow: is_slow,
            tier,
        })
    };

    if let Some(target) = pick(tier2, 2, false, true) {
        return Some(target);
    }
    if let Some(target) = pick(tier3, 3, true, false) {
        return Some(target);
    }
    if let Some(target) = pick(tier4, 4, true, true) {
        return Some(target);
    }

    None
}

/// Чистая 4-уровневая иерархия отбора резервного сервера:
/// 1. В пределах порога пинга вне карантина (0 < ping <= threshold)
/// 2. Живой медленный вне карантина (ping > threshold)
/// 3. В пределах порога пинга в карантине (0 < ping <= threshold)
/// 4. Живой медленный в карантине (ping > threshold)
pub fn select_from_candidates(
    candidates: &[String],
    pings: &impl PingMap,
    is_quarantined_fn: impl Fn(&str) -> bool,
    threshold: i64,
    preserve_order: bool,
) -> Option<SelectedTarget> {
    let mut tier1 = Vec::new();
    let mut tier2 = Vec::new();
    let mut tier3 = Vec::new();
    let mut tier4 = Vec::new();

    for id in candidates {
        if let Some(ms) = pings.get_ping(id.as_str()) {
            if ms <= 0 {
                continue; // узел лежит или таймаут
            }
            let quarantined = is_quarantined_fn(id.as_str());
            if !quarantined && ms <= threshold {
                tier1.push((id.clone(), ms));
            } else if !quarantined && ms > threshold {
                tier2.push((id.clone(), ms));
            } else if quarantined && ms <= threshold {
                tier3.push((id.clone(), ms));
            } else if quarantined && ms > threshold {
                tier4.push((id.clone(), ms));
            }
        }
    }

    let pick = |mut list: Vec<(String, i64)>, tier: u8, is_q: bool, is_slow: bool| -> Option<SelectedTarget> {
        if list.is_empty() {
            return None;
        }
        if !preserve_order {
            list.sort_by_key(|(_, ms)| *ms);
        }
        list.first().map(|(id, ms)| SelectedTarget {
            id: id.clone(),
            ping_ms: *ms,
            is_quarantined: is_q,
            is_alive_slow: is_slow,
            tier,
        })
    };

    if let Some(target) = pick(tier1, 1, false, false) {
        return Some(target);
    }
    if let Some(target) = pick(tier2, 2, false, true) {
        return Some(target);
    }
    if let Some(target) = pick(tier3, 3, true, false) {
        return Some(target);
    }
    if let Some(target) = pick(tier4, 4, true, true) {
        return Some(target);
    }

    None
}

/// Нужно ли после Best-Ping (активный узел здоров) идти дальше к автовозврату «Сценария Б» (баг 62).
pub fn best_ping_should_continue_to_restore(
    auto_restore_priority: bool,
    chain_has_servers: bool,
    smart_failover_enabled: bool,
    active_is_bs: bool,
) -> bool {
    (auto_restore_priority && chain_has_servers) || (smart_failover_enabled && active_is_bs)
}

/// Кандидаты цепочки приоритетов в её порядке: только существующие сейчас серверы, кроме активного (баг 103).
pub fn priority_chain_candidates(chain_ids: &[String], servers: &[Server], active_id: &str) -> Vec<String> {
    chain_ids
        .iter()
        .filter(|id| id.as_str() != active_id && servers.iter().any(|s| &s.id == *id))
        .cloned()
        .collect()
}

/// Логика отбора кандидата при аварийном переключении (Strict Chain Boundary vs Best-of-Rest)
/// с поддержкой Умного фейловера (Tier 1: обычные VPS -> Tier 2: серверы белых списков),
/// приоритетом цепочки провайдеров (provider_chain) и отсечкой медленных якорей (Anchor Cutoff).
pub fn select_failover_target_tiered(
    has_chain: bool,
    chain_candidate_ids: &[String],
    all_servers: &[Server],
    active_id: &str,
    is_quarantined_fn: impl Fn(&str) -> bool,
    is_whitelist_fn: impl Fn(&Server) -> bool,
    smart_failover_enabled: bool,
    pings: &impl PingMap,
    threshold: i64,
) -> Result<SelectedTarget, String> {
    select_failover_target_tiered_with_provider_chain(
        has_chain,
        chain_candidate_ids,
        all_servers,
        active_id,
        is_quarantined_fn,
        is_whitelist_fn,
        smart_failover_enabled,
        &[],
        pings,
        threshold,
    )
}

/// Расширенная версия select_failover_target_tiered с поддержкой provider_chain.
pub fn select_failover_target_tiered_with_provider_chain(
    has_chain: bool,
    chain_candidate_ids: &[String],
    all_servers: &[Server],
    active_id: &str,
    is_quarantined_fn: impl Fn(&str) -> bool,
    is_whitelist_fn: impl Fn(&Server) -> bool,
    smart_failover_enabled: bool,
    provider_chain: &[String],
    pings: &impl PingMap,
    threshold: i64,
) -> Result<SelectedTarget, String> {
    if has_chain {
        if chain_candidate_ids.is_empty() {
            return Err("Все серверы цепочки приоритетов недоступны (в таймауте или карантине). Переключение заблокировано.".to_string());
        }
        if smart_failover_enabled {
            let tier1_chain: Vec<String> = chain_candidate_ids
                .iter()
                .filter(|id| {
                    all_servers.iter().find(|s| &s.id == *id).map_or(true, |s| !is_whitelist_fn(s))
                })
                .cloned()
                .collect();
            let tier2_chain: Vec<String> = chain_candidate_ids
                .iter()
                .filter(|id| {
                    all_servers.iter().find(|s| &s.id == *id).map_or(false, |s| is_whitelist_fn(s))
                })
                .cloned()
                .collect();

            // Bucket 1: Здоровые и быстрые (<= threshold) из Tier 1
            if !tier1_chain.is_empty() {
                if let Some(target) = select_fast_healthy_from_candidates(&tier1_chain, pings, &is_quarantined_fn, threshold, true) {
                    return Ok(target);
                }
            }
            // Bucket 2: Здоровые и быстрые (<= threshold) из Tier 2
            if !tier2_chain.is_empty() {
                if let Some(target) = select_fast_healthy_from_candidates(&tier2_chain, pings, &is_quarantined_fn, threshold, true) {
                    return Ok(target);
                }
            }
            // Bucket 3: Аварийный отбор (slow / quarantine: Tier 1 -> Tier 2)
            if !tier1_chain.is_empty() {
                if let Some(target) = select_fallback_emergency_from_candidates(&tier1_chain, pings, &is_quarantined_fn, threshold, true) {
                    return Ok(target);
                }
            }
            if !tier2_chain.is_empty() {
                if let Some(target) = select_fallback_emergency_from_candidates(&tier2_chain, pings, &is_quarantined_fn, threshold, true) {
                    return Ok(target);
                }
            }
        } else {
            if let Some(target) = select_from_candidates(chain_candidate_ids, pings, &is_quarantined_fn, threshold, true) {
                return Ok(target);
            }
        }
        return Err("Все серверы цепочки приоритетов недоступны (в таймауте или карантине). Переключение заблокировано.".to_string());
    }

    // Резервный поиск среди всех остальных серверов (best-of-rest) — ТОЛЬКО если цепочка НЕ настроена
    let eligible_servers: Vec<&Server> = all_servers
        .iter()
        .filter(|s| {
            s.id != active_id
                && !s.is_pool
                && s.protocol != "URL-TEST"
                && s.protocol != "FALLBACK"
                && s.id != "DIRECT"
                && s.id != "REJECT"
                && s.id != "GLOBAL"
                && !s.id.starts_with("AUTO-")
        })
        .collect();

    if eligible_servers.is_empty() {
        return Err("Нет резервных серверов для переключения".to_string());
    }

    if smart_failover_enabled {
        // Разделяем на Tier 1 (обычные VPS) и Tier 2 (серверы белых списков)
        let tier1_servers: Vec<&Server> = eligible_servers
            .iter()
            .filter(|s| !is_whitelist_fn(s))
            .copied()
            .collect();
        let tier2_servers: Vec<&Server> = eligible_servers
            .iter()
            .filter(|s| is_whitelist_fn(s))
            .copied()
            .collect();

        // Сортировка серверов по provider_chain, если она задана
        let sort_by_prov = |servers: &[&Server]| -> Vec<String> {
            if provider_chain.is_empty() {
                return servers.iter().map(|s| s.id.clone()).collect();
            }
            let mut sorted: Vec<&Server> = servers.to_vec();
            sorted.sort_by_key(|s| {
                let prov = s.provider.as_deref().unwrap_or("");
                provider_chain.iter().position(|p| p.eq_ignore_ascii_case(prov)).unwrap_or(usize::MAX)
            });
            sorted.iter().map(|s| s.id.clone()).collect()
        };

        let tier1_ids = sort_by_prov(&tier1_servers);
        let tier2_ids = sort_by_prov(&tier2_servers);
        let preserve_order = !provider_chain.is_empty();

        // Bucket 1: Здоровые и быстрые (<= threshold) из Tier 1 (с учетом приоритета провайдеров)
        if !tier1_ids.is_empty() {
            if !provider_chain.is_empty() {
                // Группируем по провайдерам по цепочке
                let mut chosen: Option<SelectedTarget> = None;
                for prov in provider_chain {
                    let group_ids: Vec<String> = tier1_servers
                        .iter()
                        .filter(|s| s.provider.as_deref().map_or(false, |p| p.eq_ignore_ascii_case(prov)))
                        .map(|s| s.id.clone())
                        .collect();
                    if !group_ids.is_empty() {
                        if let Some(target) = select_fast_healthy_from_candidates(&group_ids, pings, &is_quarantined_fn, threshold, false) {
                            chosen = Some(target);
                            break;
                        }
                    }
                }
                if let Some(target) = chosen {
                    return Ok(target);
                }
                // Оставшиеся Tier 1 провайдеры вне цепочки
                let rest_ids: Vec<String> = tier1_servers
                    .iter()
                    .filter(|s| {
                        let prov = s.provider.as_deref().unwrap_or("");
                        !provider_chain.iter().any(|p| p.eq_ignore_ascii_case(prov))
                    })
                    .map(|s| s.id.clone())
                    .collect();
                if !rest_ids.is_empty() {
                    if let Some(target) = select_fast_healthy_from_candidates(&rest_ids, pings, &is_quarantined_fn, threshold, false) {
                        return Ok(target);
                    }
                }
            } else if let Some(target) = select_fast_healthy_from_candidates(&tier1_ids, pings, &is_quarantined_fn, threshold, false) {
                return Ok(target);
            }
        }

        // Bucket 2: Здоровые и быстрые (<= threshold) из Tier 2 (whitelist_providers)
        if !tier2_ids.is_empty() {
            if !provider_chain.is_empty() {
                let mut chosen: Option<SelectedTarget> = None;
                for prov in provider_chain {
                    let group_ids: Vec<String> = tier2_servers
                        .iter()
                        .filter(|s| s.provider.as_deref().map_or(false, |p| p.eq_ignore_ascii_case(prov)))
                        .map(|s| s.id.clone())
                        .collect();
                    if !group_ids.is_empty() {
                        if let Some(target) = select_fast_healthy_from_candidates(&group_ids, pings, &is_quarantined_fn, threshold, false) {
                            chosen = Some(target);
                            break;
                        }
                    }
                }
                if let Some(target) = chosen {
                    return Ok(target);
                }
                let rest_ids: Vec<String> = tier2_servers
                    .iter()
                    .filter(|s| {
                        let prov = s.provider.as_deref().unwrap_or("");
                        !provider_chain.iter().any(|p| p.eq_ignore_ascii_case(prov))
                    })
                    .map(|s| s.id.clone())
                    .collect();
                if !rest_ids.is_empty() {
                    if let Some(target) = select_fast_healthy_from_candidates(&rest_ids, pings, &is_quarantined_fn, threshold, false) {
                        return Ok(target);
                    }
                }
            } else if let Some(target) = select_fast_healthy_from_candidates(&tier2_ids, pings, &is_quarantined_fn, threshold, false) {
                return Ok(target);
            }
        }

        // Bucket 3: Аварийный fallback (slow / quarantine: Tier 1 -> Tier 2)
        if !tier1_ids.is_empty() {
            if let Some(target) = select_fallback_emergency_from_candidates(&tier1_ids, pings, &is_quarantined_fn, threshold, preserve_order) {
                return Ok(target);
            }
        }
        if !tier2_ids.is_empty() {
            if let Some(target) = select_fallback_emergency_from_candidates(&tier2_ids, pings, &is_quarantined_fn, threshold, preserve_order) {
                return Ok(target);
            }
        }
    } else {
        let candidate_ids: Vec<String> = eligible_servers.iter().map(|s| s.id.clone()).collect();
        if let Some(target) = select_from_candidates(&candidate_ids, pings, &is_quarantined_fn, threshold, false) {
            return Ok(target);
        }
    }

    Err("Все резервные серверы недоступны".to_string())
}

#[cfg(test)]
pub fn select_failover_target(
    has_chain: bool,
    chain_candidate_ids: &[String],
    all_servers: &[Server],
    active_id: &str,
    is_quarantined_fn: impl Fn(&str) -> bool,
    pings: &impl PingMap,
    threshold: i64,
) -> Result<SelectedTarget, String> {
    select_failover_target_tiered(
        has_chain,
        chain_candidate_ids,
        all_servers,
        active_id,
        is_quarantined_fn,
        |_| false,
        false,
        pings,
        threshold,
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceFailoverDecision {
    KeepCurrent {
        reason: String,
        has_live_traffic: bool,
    },
    AutoRestore {
        target: String,
        ping_ms: i64,
        is_main: bool,
    },
    Switch {
        target: SelectedTarget,
        reason: String,
    },
    NoBackupAvailable {
        reason: String,
    },
}

/// Чистая функция принятия решения для per-device failover.
/// Учитывает наличие активного трафика загрузки (> 1 КБ), детекцию чёрной дыры,
/// 4-уровневую иерархию и субординацию уровней (Smart Failover: Tier 1 VPS -> Tier 2 БС).
pub fn evaluate_device_failover(
    _device_ip: &str,
    cur: &str,
    servers_chain: &[String],
    cur_ping: i64,
    threshold: i64,
    auto_restore: bool,
    has_live_traffic: bool,
    is_blackhole: bool,
    all_servers: &[Server],
    higher_pings: &impl PingMap,
    candidate_pings: &impl PingMap,
    is_quarantined_fn: impl Fn(&str) -> bool,
    is_whitelist_fn: impl Fn(&Server) -> bool,
    smart_failover_enabled: bool,
) -> DeviceFailoverDecision {
    let hyst = calc_hysteresis(threshold);
    let is_cur_healthy = is_active_server_healthy(cur_ping, threshold, has_live_traffic, is_blackhole);

    if is_cur_healthy {
        // При активной загрузке (> 1 КБ) сбои пинга толерируются
        if has_live_traffic && (cur_ping <= 0 || cur_ping > threshold) {
            return DeviceFailoverDecision::KeepCurrent {
                reason: format!("пинг сбоит ({cur_ping} мс), но идёт активная загрузка (> 1 КБ)"),
                has_live_traffic: true,
            };
        }

        // Автовозврат: только если на устройстве НЕТ активной загрузки (> 1 КБ)
        if auto_restore && !has_live_traffic {
            let higher = get_higher_priority_candidates(servers_chain, cur);
            for target_server in higher {
                // Не восстанавливаем на узел в карантине
                if is_quarantined_fn(target_server) {
                    continue;
                }
                if let Some(p) = higher_pings.get_ping(target_server) {
                    if p > 0 && p <= hyst {
                        let is_main = target_server == &servers_chain[0];
                        return DeviceFailoverDecision::AutoRestore {
                            target: target_server.to_string(),
                            ping_ms: p,
                            is_main,
                        };
                    }
                }
            }
        }

        return DeviceFailoverDecision::KeepCurrent {
            reason: format!("пинг {cur_ping} мс в норме"),
            has_live_traffic,
        };
    }

    // Текущий сервер сбоит или попал в чёрную дыру
    let reason = if is_blackhole {
        "DPI чёрная дыра (upload > 5KB, download == 0)".to_string()
    } else if cur_ping > 0 {
        format!("пинг {cur_ping} мс > порога {threshold} мс")
    } else {
        "сервер не отвечает".to_string()
    };

    let cands: Vec<String> = servers_chain.iter().filter(|s| *s != cur).cloned().collect();
    if cands.is_empty() {
        return DeviceFailoverDecision::NoBackupAvailable { reason };
    }

    let target_res = select_failover_target_tiered(
        true, // has_chain = true (соблюдает порядок цепочки dr.servers)
        &cands,
        all_servers,
        cur,
        &is_quarantined_fn,
        &is_whitelist_fn,
        smart_failover_enabled,
        candidate_pings,
        threshold,
    );

    match target_res {
        Ok(target) => DeviceFailoverDecision::Switch { target, reason },
        Err(_) => DeviceFailoverDecision::NoBackupAvailable { reason },
    }
}
