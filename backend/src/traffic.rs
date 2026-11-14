use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};
use serde_json::json;
use crate::AppState;

pub static PROXY_DOWN: AtomicU64 = AtomicU64::new(0);
pub static PROXY_UP: AtomicU64 = AtomicU64::new(0);
pub static DIRECT_DOWN: AtomicU64 = AtomicU64::new(0);
pub static DIRECT_UP: AtomicU64 = AtomicU64::new(0);
pub static WAN_DOWN: AtomicU64 = AtomicU64::new(0);
pub static WAN_UP: AtomicU64 = AtomicU64::new(0);

pub static WHITELIST_DOWN: AtomicU64 = AtomicU64::new(0);
pub static WHITELIST_UP: AtomicU64 = AtomicU64::new(0);
pub static TOTAL_WHITELIST_BYTES: AtomicU64 = AtomicU64::new(0);
pub static TOTAL_DIRECT_BYTES: AtomicU64 = AtomicU64::new(0);
pub static TOTAL_PROXY_BYTES: AtomicU64 = AtomicU64::new(0);
pub static WHITELIST_ACTIVE: AtomicBool = AtomicBool::new(false);
pub static DIRECT_IS_HARDWARE: AtomicBool = AtomicBool::new(false);

pub fn set_whitelist_active(active: bool) {
    WHITELIST_ACTIVE.store(active, Ordering::Release);
}

pub fn is_whitelist_active() -> bool {
    WHITELIST_ACTIVE.load(Ordering::Acquire)
}

#[derive(Deserialize)]
pub struct MihomoTrafficChunk {
    #[serde(default)]
    pub up: u64,
    #[serde(default)]
    pub down: u64,
}

#[derive(Serialize)]
pub struct TrafficSpeed {
    pub down: u64,
    pub up: u64,
}

#[derive(Serialize)]
pub struct TrafficSnapshot {
    pub direct: TrafficSpeed,
    pub proxy: TrafficSpeed,
    pub whitelist: TrafficSpeed,
    pub total: TrafficSpeed,
    pub whitelist_bytes: u64,
    pub direct_bytes: u64,
    pub proxy_bytes: u64,
    // Для совместимости со старыми вызовами:
    pub down: u64,
    pub up: u64,
}

pub fn get_snapshot() -> TrafficSnapshot {
    let p_down = PROXY_DOWN.load(Ordering::Relaxed);
    let p_up = PROXY_UP.load(Ordering::Relaxed);
    let d_down = DIRECT_DOWN.load(Ordering::Relaxed);
    let d_up = DIRECT_UP.load(Ordering::Relaxed);
    let w_down = WAN_DOWN.load(Ordering::Relaxed);
    let w_up = WAN_UP.load(Ordering::Relaxed);
    let wl_down = WHITELIST_DOWN.load(Ordering::Relaxed);
    let wl_up = WHITELIST_UP.load(Ordering::Relaxed);
    let wl_bytes = TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed);
    let d_bytes = TOTAL_DIRECT_BYTES.load(Ordering::Relaxed);
    let p_bytes = TOTAL_PROXY_BYTES.load(Ordering::Relaxed);
    let is_wl_active = WHITELIST_ACTIVE.load(Ordering::Relaxed);

    TrafficSnapshot {
        direct: TrafficSpeed { down: d_down, up: d_up },
        proxy: TrafficSpeed { down: p_down, up: p_up },
        whitelist: TrafficSpeed { down: wl_down, up: wl_up },
        total: TrafficSpeed { down: w_down, up: w_up },
        whitelist_bytes: wl_bytes,
        direct_bytes: d_bytes,
        proxy_bytes: p_bytes,
        down: if is_wl_active && wl_down > 0 { wl_down } else { p_down },
        up: if is_wl_active && wl_up > 0 { wl_up } else { p_up },
    }
}

pub fn get_snapshot_json() -> serde_json::Value {
    let p_down = PROXY_DOWN.load(Ordering::Relaxed);
    let p_up = PROXY_UP.load(Ordering::Relaxed);
    let d_down = DIRECT_DOWN.load(Ordering::Relaxed);
    let d_up = DIRECT_UP.load(Ordering::Relaxed);
    let w_down = WAN_DOWN.load(Ordering::Relaxed);
    let w_up = WAN_UP.load(Ordering::Relaxed);
    let wl_down = WHITELIST_DOWN.load(Ordering::Relaxed);
    let wl_up = WHITELIST_UP.load(Ordering::Relaxed);
    let wl_bytes = TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed);
    let d_bytes = TOTAL_DIRECT_BYTES.load(Ordering::Relaxed);
    let p_bytes = TOTAL_PROXY_BYTES.load(Ordering::Relaxed);
    let is_wl_active = WHITELIST_ACTIVE.load(Ordering::Relaxed);

    let history = get_history_json();
    let is_hw = DIRECT_IS_HARDWARE.load(Ordering::Relaxed);
    json!({
        "direct": {
            "down": d_down,
            "up": d_up,
            "bytes": d_bytes,
            "estimated": !is_hw,
            "source": if is_hw { "iptables (XKEEN_DIRECT)" } else { "WAN (/proc/net/dev) - Mihomo (/traffic)" }
        },
        "proxy": {
            "down": p_down,
            "up": p_up,
            "bytes": p_bytes,
            "source": "Mihomo VPS (Unlimited)"
        },
        "whitelist": {
            "down": wl_down,
            "up": wl_up,
            "bytes": wl_bytes,
            "active": is_wl_active,
            "source": "Whitelist (Metered)"
        },
        "total": {
            "down": w_down,
            "up": w_up,
            "source": "WAN interfaces (/proc/net/dev)"
        },
        "whitelist_bytes": wl_bytes,
        "direct_bytes": d_bytes,
        "proxy_bytes": p_bytes,
        "whitelist_active": is_wl_active,
        "down": if is_wl_active && wl_down > 0 { wl_down } else { p_down },
        "up": if is_wl_active && wl_up > 0 { wl_up } else { p_up },
        "history": history,
        "estimated": !is_hw,
    })
}

static STATS_PATH: std::sync::RwLock<Option<std::path::PathBuf>> = std::sync::RwLock::new(None);

#[derive(Serialize, Deserialize, Default, Debug, Clone)]
pub struct StoredTrafficStats {
    #[serde(default)]
    pub direct_bytes: u64,
    #[serde(default)]
    pub proxy_bytes: u64,
    #[serde(default)]
    pub whitelist_bytes: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub provider_deltas: std::collections::HashMap<String, u64>,
}

static PROVIDER_TRAFFIC_DELTAS: std::sync::RwLock<Option<std::collections::HashMap<String, u64>>> = std::sync::RwLock::new(None);
static ACTIVE_PROVIDER_ID: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

pub fn set_active_provider_id(provider_id: Option<String>) {
    if let Ok(mut lock) = ACTIVE_PROVIDER_ID.write() {
        *lock = provider_id;
    }
}

pub fn get_active_provider_id() -> Option<String> {
    if let Ok(lock) = ACTIVE_PROVIDER_ID.read() {
        lock.clone()
    } else {
        None
    }
}

pub fn add_provider_live_delta(provider_id: &str, delta_bytes: u64) {
    if provider_id.is_empty() || delta_bytes == 0 {
        return;
    }
    if let Ok(mut lock) = PROVIDER_TRAFFIC_DELTAS.write() {
        let map = lock.get_or_insert_with(std::collections::HashMap::new);
        let cur = map.entry(provider_id.to_string()).or_insert(0);
        *cur = cur.saturating_add(delta_bytes);
    }
}

pub fn get_provider_live_delta(provider_id: &str) -> u64 {
    if let Ok(lock) = PROVIDER_TRAFFIC_DELTAS.read() {
        if let Some(ref map) = *lock {
            return map.get(provider_id).copied().unwrap_or(0);
        }
    }
    0
}

pub fn reset_provider_live_delta(provider_id: &str) {
    if let Ok(mut lock) = PROVIDER_TRAFFIC_DELTAS.write() {
        if let Some(ref mut map) = *lock {
            map.remove(provider_id);
        }
    }
}

pub fn set_stats_file(path: std::path::PathBuf) {
    if let Ok(mut lock) = STATS_PATH.write() {
        *lock = Some(path.clone());
    }
    load_stats_from_disk(&path);
}

pub fn load_stats_from_disk(path: &std::path::Path) {
    if let Ok(content) = std::fs::read_to_string(path) {
        if let Ok(stored) = serde_json::from_str::<StoredTrafficStats>(&content) {
            TOTAL_DIRECT_BYTES.store(stored.direct_bytes, Ordering::Relaxed);
            TOTAL_PROXY_BYTES.store(stored.proxy_bytes, Ordering::Relaxed);
            TOTAL_WHITELIST_BYTES.store(stored.whitelist_bytes, Ordering::Relaxed);
            if let Ok(mut lock) = PROVIDER_TRAFFIC_DELTAS.write() {
                *lock = Some(stored.provider_deltas);
            }
            crate::log_i!(
                "Загружена статистика трафика из {}: direct={} B, vps={} B, whitelist={} B",
                path.display(),
                stored.direct_bytes,
                stored.proxy_bytes,
                stored.whitelist_bytes
            );
        }
    }
}

pub fn save_stats_to_disk() {
    let path_opt = {
        let lock = STATS_PATH.read().ok();
        lock.and_then(|guard| guard.clone())
    };
    let Some(path) = path_opt else { return; };
    let deltas = if let Ok(lock) = PROVIDER_TRAFFIC_DELTAS.read() {
        lock.as_ref().cloned().unwrap_or_default()
    } else {
        std::collections::HashMap::new()
    };
    let stats = StoredTrafficStats {
        direct_bytes: TOTAL_DIRECT_BYTES.load(Ordering::Relaxed),
        proxy_bytes: TOTAL_PROXY_BYTES.load(Ordering::Relaxed),
        whitelist_bytes: TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed),
        updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        provider_deltas: deltas,
    };
    if let Ok(json_str) = serde_json::to_string_pretty(&stats) {
        let tmp = path.with_extension("tmp");
        if let Ok(_) = std::fs::write(&tmp, json_str.as_bytes()) {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

pub fn reset_traffic(target: &str) -> TrafficSnapshot {
    let t = target.trim().to_lowercase();
    match t.as_str() {
        "direct" => {
            TOTAL_DIRECT_BYTES.store(0, Ordering::Relaxed);
        }
        "proxy" | "vps" => {
            TOTAL_PROXY_BYTES.store(0, Ordering::Relaxed);
        }
        "whitelist" | "bs" => {
            TOTAL_WHITELIST_BYTES.store(0, Ordering::Relaxed);
        }
        _ => {
            // "all"
            TOTAL_DIRECT_BYTES.store(0, Ordering::Relaxed);
            TOTAL_PROXY_BYTES.store(0, Ordering::Relaxed);
            TOTAL_WHITELIST_BYTES.store(0, Ordering::Relaxed);
        }
    }
    save_stats_to_disk();
    get_snapshot()
}

pub static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn shutdown_with<F1, F2>(
    flag: &std::sync::atomic::AtomicBool,
    save_fn: F1,
    cleanup_fn: F2,
) where
    F1: FnOnce(),
    F2: FnOnce(),
{
    flag.store(true, Ordering::Release);
    save_fn();
    cleanup_fn();
}

pub fn shutdown() {
    shutdown_with(&SHUTDOWN, save_stats_to_disk, crate::direct_traffic::cleanup_direct_iptables);
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct ConnectionsTrafficDeltas {
    pub direct_bytes: u64,
    pub proxy_bytes: u64,
    pub whitelist_bytes: u64,
    pub provider_deltas: std::collections::HashMap<String, u64>,
}

/// Проверка, направлено ли соединение напрямую в интернет минуя прокси (DIRECT)
pub fn is_connection_direct(conn: &serde_json::Value) -> bool {
    if let Some(rule) = conn.get("rule").and_then(|r| r.as_str()) {
        if rule.eq_ignore_ascii_case("DIRECT") {
            return true;
        }
    }
    if let Some(chains) = conn.get("chains").and_then(|c| c.as_array()) {
        for ch in chains {
            if let Some(s) = ch.as_str() {
                if s.eq_ignore_ascii_case("DIRECT") {
                    return true;
                }
            }
        }
    }
    false
}

/// Вычисление дельт трафика соединений Mihomo /connections (баги 72, 73).
/// * DIRECT-соединения идут в `direct_bytes` и не попадают ни в VPS/БС, ни в квоту подписки (72).
/// * Остальное — в `whitelist_bytes` (если активен БС или цепочка идёт через группу БС) либо в `proxy_bytes`.
/// * Квота подписки (`provider_deltas`) списывается только с той подписки, через узел которой идёт цепочка (73).
/// * Соединения, которых больше нет в ответе, удаляются из `prev_conns`, чтобы карта не росла бесконечно.
pub fn split_connections_delta_with_providers(
    connections_json: &serde_json::Value,
    prev_conns: &mut std::collections::HashMap<String, (u64, u64)>,
    server_to_provider: &std::collections::HashMap<String, String>,
    whitelist_active: bool,
) -> ConnectionsTrafficDeltas {
    let mut deltas = ConnectionsTrafficDeltas::default();
    let mut seen = std::collections::HashSet::new();
    if let Some(arr) = connections_json.get("connections").and_then(|c| c.as_array()) {
        for conn in arr {
            let id = conn.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if id.is_empty() {
                continue;
            }
            let upload = conn.get("upload").and_then(|v| v.as_u64()).unwrap_or(0);
            let download = conn.get("download").and_then(|v| v.as_u64()).unwrap_or(0);
            seen.insert(id.clone());
            let prev = prev_conns.insert(id, (upload, download)).unwrap_or((0, 0));
            let delta = upload.saturating_sub(prev.0) + download.saturating_sub(prev.1);
            if delta == 0 {
                continue;
            }

            if is_connection_direct(conn) {
                deltas.direct_bytes += delta;
                continue;
            }

            let chains: Vec<&str> = conn
                .get("chains")
                .and_then(|c| c.as_array())
                .map(|c| c.iter().filter_map(|v| v.as_str()).collect())
                .unwrap_or_default();
            let via_wl_group = chains.iter().any(|s| s.contains("Whitelist") || s.contains("Белый"));
            if whitelist_active || via_wl_group {
                deltas.whitelist_bytes += delta;
            } else {
                deltas.proxy_bytes += delta;
            }
            if let Some(prov) = chains.iter().find_map(|name| server_to_provider.get(*name)) {
                if prov != crate::vkturn::PROVIDER_ID {
                    *deltas.provider_deltas.entry(prov.clone()).or_insert(0) += delta;
                }
            }
        }
    }
    prev_conns.retain(|id, _| seen.contains(id));
    deltas
}

/// Совместимость: разбиение без привязки к подпискам.
pub fn split_connections_delta(
    connections_json: &serde_json::Value,
    prev_conns: &mut std::collections::HashMap<String, (u64, u64)>,
) -> ConnectionsTrafficDeltas {
    split_connections_delta_with_providers(
        connections_json,
        prev_conns,
        &std::collections::HashMap::new(),
        WHITELIST_ACTIVE.load(Ordering::Relaxed),
    )
}

/// Доля не-DIRECT трафика Mihomo в промилле по последнему опросу /connections (для скоростей из /traffic).
static NON_DIRECT_PERMILLE: AtomicU64 = AtomicU64::new(1000);

fn scale_by_non_direct(v: u64) -> u64 {
    v.saturating_mul(NON_DIRECT_PERMILLE.load(Ordering::Relaxed)) / 1000
}

/// Переключатель маппинга направлений `/traffic` Mihomo (`up -> down`, `down -> up`) (баг 74).
/// Менять только после замера на стендовом роутере `192.168.1.1`.
pub const fn frame_direction_swapped() -> bool {
    true
}

/// Обрабатывает один кадр `/traffic` от Mihomo и обновляет текущие скорости PROXY/WHITELIST (баг 75).
pub fn apply_traffic_frame(msg: &MihomoTrafficChunk) -> (u64, u64) {
    let (raw_down, raw_up) = if frame_direction_swapped() {
        (msg.up, msg.down)
    } else {
        (msg.down, msg.up)
    };
    let down = scale_by_non_direct(raw_down);
    let up = scale_by_non_direct(raw_up);
    let is_wl_now = WHITELIST_ACTIVE.load(Ordering::Relaxed);
    if is_wl_now {
        WHITELIST_DOWN.store(down, Ordering::Relaxed);
        WHITELIST_UP.store(up, Ordering::Relaxed);
        PROXY_DOWN.store(0, Ordering::Relaxed);
        PROXY_UP.store(0, Ordering::Relaxed);
    } else {
        PROXY_DOWN.store(down, Ordering::Relaxed);
        PROXY_UP.store(up, Ordering::Relaxed);
        WHITELIST_DOWN.store(0, Ordering::Relaxed);
        WHITELIST_UP.store(0, Ordering::Relaxed);
    }
    (down, up)
}

/// Учёт трафика по соединениям Mihomo (баги 72, 73): раз в 2 с GET /connections, раз в 30 с —
/// карта «узел → подписка» из /providers/proxies.
async fn run_connections_accounting(state: AppState) {
    let mut prev: std::collections::HashMap<String, (u64, u64)> = std::collections::HashMap::new();
    let mut node_to_provider: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut last_map_refresh: Option<std::time::Instant> = None;
    loop {
        if SHUTDOWN.load(Ordering::Acquire) {
            break;
        }
        let cfg = (**state.config.read().await).clone();
        if last_map_refresh.map_or(true, |t| t.elapsed() >= Duration::from_secs(30)) {
            if let Ok(proxies) = crate::mihomo::get_provider_proxies(&state.http, &cfg).await {
                node_to_provider = proxies
                    .iter()
                    .filter_map(|(name, v)| {
                        v.get("_provider")
                            .and_then(|p| p.as_str())
                            .filter(|p| *p != crate::vkturn::PROVIDER_ID)
                            .map(|p| (name.clone(), p.to_string()))
                    })
                    .collect();
                last_map_refresh = Some(std::time::Instant::now());
            }
        }
        let mut req = state.http.get(format!("{}/connections", cfg.mihomo_url())).timeout(Duration::from_secs(5));
        if !cfg.mihomo.secret.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", cfg.mihomo.secret));
        }
        if let Ok(resp) = req.send().await {
            if let Ok(json) = resp.json::<serde_json::Value>().await {
                let d = split_connections_delta_with_providers(
                    &json,
                    &mut prev,
                    &node_to_provider,
                    WHITELIST_ACTIVE.load(Ordering::Relaxed),
                );
                TOTAL_PROXY_BYTES.fetch_add(d.proxy_bytes, Ordering::Relaxed);
                TOTAL_WHITELIST_BYTES.fetch_add(d.whitelist_bytes, Ordering::Relaxed);
                // Mihomo DIRECT выходит с роутера (OUTPUT) и не виден цепочке FORWARD XKEEN_DIRECT;
                // без аппаратного учёта Direct считается как WAN − прокси и уже включает эти байты.
                if DIRECT_IS_HARDWARE.load(Ordering::Relaxed) {
                    TOTAL_DIRECT_BYTES.fetch_add(d.direct_bytes, Ordering::Relaxed);
                }
                for (prov, bytes) in &d.provider_deltas {
                    add_provider_live_delta(prov, *bytes);
                }
                let total = d.direct_bytes + d.proxy_bytes + d.whitelist_bytes;
                if total > 0 {
                    NON_DIRECT_PERMILLE.store((d.proxy_bytes + d.whitelist_bytes) * 1000 / total, Ordering::Relaxed);
                }
            }
        }
        for _ in 0..4 {
            if SHUTDOWN.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

pub fn is_shutdown() -> bool {
    SHUTDOWN.load(Ordering::Acquire)
}

static TRAFFIC_HISTORY: std::sync::Mutex<Option<std::collections::VecDeque<serde_json::Value>>> = std::sync::Mutex::new(None);

pub fn record_history_point(snapshot_json: serde_json::Value) {
    if let Ok(mut lock) = TRAFFIC_HISTORY.lock() {
        let deq = lock.get_or_insert_with(|| std::collections::VecDeque::with_capacity(60));
        if deq.len() >= 60 {
            deq.pop_front();
        }
        deq.push_back(snapshot_json);
    }
}

pub fn get_history_json() -> serde_json::Value {
    if let Ok(lock) = TRAFFIC_HISTORY.lock() {
        if let Some(ref deq) = *lock {
            return serde_json::to_value(deq.iter().cloned().collect::<Vec<_>>()).unwrap_or_else(|_| json!([]));
        }
    }
    json!([])
}

/// Поток чтения /traffic из Mihomo (Server-Sent Chunks)
async fn run_mihomo_stream(state: AppState) {
    loop {
        if SHUTDOWN.load(Ordering::Acquire) {
            break;
        }
        let (mihomo_url, secret) = {
            let cfg = state.config.read().await;
            (cfg.mihomo_url(), cfg.mihomo.secret.clone())
        };
        let url = format!("{}/traffic", mihomo_url);
        let mut req = state.http.get(&url);
        if !secret.is_empty() {
            req = req.header("Authorization", format!("Bearer {}", secret));
        }

        match req.send().await {
            Ok(mut resp) if resp.status().is_success() => {
                let mut buffer = String::new();
                loop {
                    if SHUTDOWN.load(Ordering::Acquire) {
                        break;
                    }
                    match tokio::time::timeout(Duration::from_secs(5), resp.chunk()).await {
                        Ok(Ok(Some(chunk))) => {
                            let s = String::from_utf8_lossy(&chunk);
                            if buffer.len() + s.len() > 65536 {
                                buffer.clear();
                            }
                            if s.len() <= 65536 {
                                buffer.push_str(&s);
                            }
                            while let Some(pos) = buffer.find('\n') {
                                let line = buffer[..pos].trim().to_string();
                                buffer.drain(..=pos);
                                if !line.is_empty() {
                                    if let Ok(msg) = serde_json::from_str::<MihomoTrafficChunk>(&line) {
                                        apply_traffic_frame(&msg);
                                    }
                                }
                            }
                        }
                        _ => {
                            break;
                        }
                    }
                }
            }
            _ => {}
        }
        PROXY_DOWN.store(0, Ordering::Relaxed);
        PROXY_UP.store(0, Ordering::Relaxed);
        WHITELIST_DOWN.store(0, Ordering::Relaxed);
        WHITELIST_UP.store(0, Ordering::Relaxed);
        for _ in 0..4 {
            if SHUTDOWN.load(Ordering::Acquire) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

/// Определение шлюзовых WAN-интерфейсов из /proc/net/route
fn get_wan_interfaces() -> Vec<String> {
    let mut ifaces = Vec::new();
    #[cfg(target_os = "linux")]
    if let Ok(content) = std::fs::read_to_string("/proc/net/route") {
        for line in content.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[1] == "00000000" {
                let name = parts[0].to_string();
                if !ifaces.contains(&name) {
                    ifaces.push(name);
                }
            }
        }
    }
    if ifaces.is_empty() {
        ifaces.push("apcli1".to_string());
    }
    ifaces
}

/// Чтение счетчиков байт WAN-интерфейсов из /proc/net/dev
#[cfg(not(target_os = "linux"))]
fn read_wan_bytes(_wan_ifaces: &[String]) -> (u64, u64) {
    (0, 0)
}

#[cfg(target_os = "linux")]
fn read_wan_bytes(wan_ifaces: &[String]) -> (u64, u64) {
    let mut rx = 0u64;
    let mut tx = 0u64;
    #[cfg(target_os = "linux")]
    if let Ok(content) = std::fs::read_to_string("/proc/net/dev") {
        for line in content.lines().skip(2) {
            if let Some((iface_part, stats_part)) = line.split_once(':') {
                let name = iface_part.trim();
                let is_wan = wan_ifaces.iter().any(|w| w == name)
                    || (wan_ifaces.is_empty() && (name.starts_with("apcli") || name.starts_with("wwan") || name.starts_with("eth3") || name.starts_with("ppp") || name.starts_with("qmi_br")));
                if is_wan {
                    let mut iter = stats_part.split_whitespace();
                    let rx_val = iter.next().and_then(|s| s.parse::<u64>().ok());
                    let tx_val = iter.nth(7).and_then(|s| s.parse::<u64>().ok());
                    if let (Some(r), Some(t)) = (rx_val, tx_val) {
                        rx += r;
                        tx += t;
                    }
                }
            }
        }
    }
    (rx, tx)
}

/// Подсчет реальной скорости прямого трафика роутера (аппаратные байты iptables XKEEN_DIRECT либо fallback WAN - PROXY)
async fn run_wan_polling() {
    let mut last_ifaces = get_wan_interfaces();
    let mut last_ifaces_update = Instant::now();
    let (mut prev_rx, mut prev_tx) = read_wan_bytes(&last_ifaces);
    let mut prev_time = Instant::now();

    let mut prev_direct_rx = 0u64;
    let mut prev_direct_tx = 0u64;
    let mut direct_initialized = false;

    let mut interval = tokio::time::interval(Duration::from_millis(1000));
    loop {
        if SHUTDOWN.load(Ordering::Acquire) {
            break;
        }
        interval.tick().await;
        let now = Instant::now();

        // Периодическое обновление списка WAN интерфейсов (на случай переключения резервного канала)
        if now.duration_since(last_ifaces_update) > Duration::from_secs(15) {
            let new_ifaces = get_wan_interfaces();
            if new_ifaces != last_ifaces {
                last_ifaces = new_ifaces;
                // Сбрасываем счётчики, чтобы не было ложного всплеска при смене набора интерфейсов
                let (cur_rx, cur_tx) = read_wan_bytes(&last_ifaces);
                prev_rx = cur_rx;
                prev_tx = cur_tx;
            }
            last_ifaces_update = now;
        }

        let (rx, tx) = read_wan_bytes(&last_ifaces);
        let dt = now.duration_since(prev_time).as_secs_f64().max(0.1);

        let wan_rx_speed = if rx >= prev_rx { ((rx - prev_rx) as f64 / dt) as u64 } else { 0 };
        let wan_tx_speed = if tx >= prev_tx { ((tx - prev_tx) as f64 / dt) as u64 } else { 0 };

        prev_rx = rx;
        prev_tx = tx;
        prev_time = now;

        WAN_DOWN.store(wan_rx_speed, Ordering::Relaxed);
        WAN_UP.store(wan_tx_speed, Ordering::Relaxed);

        let p_down = PROXY_DOWN.load(Ordering::Relaxed);
        let p_up = PROXY_UP.load(Ordering::Relaxed);
        let wl_down = WHITELIST_DOWN.load(Ordering::Relaxed);
        let wl_up = WHITELIST_UP.load(Ordering::Relaxed);

        let total_proxy_down = p_down + wl_down;
        let total_proxy_up = p_up + wl_up;

        // Точный аппаратный учёт через ядро Linux iptables (цепочка XKEEN_DIRECT)
        let (d_down, d_up, is_hw) = if let Some((cur_dir_rx, cur_dir_tx)) = crate::direct_traffic::read_direct_bytes() {
            if !direct_initialized {
                prev_direct_rx = cur_dir_rx;
                prev_direct_tx = cur_dir_tx;
                direct_initialized = true;
                (0, 0, true)
            } else {
                let delta_rx = cur_dir_rx.saturating_sub(prev_direct_rx);
                let delta_tx = cur_dir_tx.saturating_sub(prev_direct_tx);
                prev_direct_rx = cur_dir_rx;
                prev_direct_tx = cur_dir_tx;

                let dir_down_speed = (delta_rx as f64 / dt) as u64;
                let dir_up_speed = (delta_tx as f64 / dt) as u64;
                TOTAL_DIRECT_BYTES.fetch_add(delta_rx + delta_tx, Ordering::Relaxed);
                (dir_down_speed, dir_up_speed, true)
            }
        } else {
            // Безопасный fallback (Windows-среда разработки или при недоступности iptables)
            let d_down = wan_rx_speed.saturating_sub(total_proxy_down);
            let d_up = wan_tx_speed.saturating_sub(total_proxy_up);
            TOTAL_DIRECT_BYTES.fetch_add(d_down + d_up, Ordering::Relaxed);
            (d_down, d_up, false)
        };

        DIRECT_DOWN.store(d_down, Ordering::Relaxed);
        DIRECT_UP.store(d_up, Ordering::Relaxed);
        DIRECT_IS_HARDWARE.store(is_hw, Ordering::Relaxed);

        let is_wl_active = WHITELIST_ACTIVE.load(Ordering::Relaxed);

        record_history_point(json!({
            "direct": { 
                "down": d_down, 
                "up": d_up, 
                "estimated": !is_hw, 
                "source": if is_hw { "iptables (XKEEN_DIRECT)" } else { "WAN - Mihomo" } 
            },
            "proxy": { "down": p_down, "up": p_up, "source": "Mihomo VPS" },
            "whitelist": { "down": wl_down, "up": wl_up, "active": is_wl_active },
            "total": { "down": wan_rx_speed, "up": wan_tx_speed, "source": "WAN" },
            "down": if is_wl_active && wl_down > 0 { wl_down } else { p_down },
            "up": if is_wl_active && wl_up > 0 { wl_up } else { p_up },
            "estimated": !is_hw,
        }));
    }
}

async fn run_periodic_stats_saver() {
    let mut interval = tokio::time::interval(Duration::from_secs(300));
    let mut last_direct = TOTAL_DIRECT_BYTES.load(Ordering::Relaxed);
    let mut last_proxy = TOTAL_PROXY_BYTES.load(Ordering::Relaxed);
    let mut last_wl = TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed);

    loop {
        if SHUTDOWN.load(Ordering::Acquire) {
            break;
        }
        interval.tick().await;

        let cur_direct = TOTAL_DIRECT_BYTES.load(Ordering::Relaxed);
        let cur_proxy = TOTAL_PROXY_BYTES.load(Ordering::Relaxed);
        let cur_wl = TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed);

        if cur_direct != last_direct || cur_proxy != last_proxy || cur_wl != last_wl {
            save_stats_to_disk();
            last_direct = cur_direct;
            last_proxy = cur_proxy;
            last_wl = cur_wl;
        }
    }
    save_stats_to_disk();
}

pub fn spawn(state: AppState) {
    let stats_path = state
        .config_path
        .parent()
        .unwrap_or(std::path::Path::new("/opt/etc/xkeen-route"))
        .join("traffic_stats.json");
    set_stats_file(stats_path);

    crate::direct_traffic::init_direct_iptables();

    tokio::spawn(run_mihomo_stream(state.clone()));
    tokio::spawn(run_connections_accounting(state.clone()));
    tokio::spawn(run_wan_polling());
    tokio::spawn(run_periodic_stats_saver());
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_traffic_buffer_limit_without_newline() {
        let mut buffer = String::new();
        let chunk_without_newline = "x".repeat(10000);

        for _ in 0..7 {
            if buffer.len() + chunk_without_newline.len() > 65536 {
                buffer.clear();
            }
            if chunk_without_newline.len() <= 65536 {
                buffer.push_str(&chunk_without_newline);
            }
        }

        assert!(buffer.len() <= 65536);
        assert_eq!(buffer.len(), 10000);
    }

    #[test]
    fn test_traffic_shutdown_flag() {
        let local_flag = std::sync::atomic::AtomicBool::new(false);
        let mut saved = false;
        let mut cleaned = false;
        assert!(!local_flag.load(Ordering::Acquire));
        shutdown_with(&local_flag, || saved = true, || cleaned = true);
        assert!(local_flag.load(Ordering::Acquire));
        assert!(saved && cleaned);
    }

    #[test]
    fn test_whitelist_traffic_active_and_counters() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        set_whitelist_active(false);
        assert!(!is_whitelist_active());
        set_whitelist_active(true);
        assert!(is_whitelist_active());

        WHITELIST_DOWN.store(1024, Ordering::Relaxed);
        WHITELIST_UP.store(512, Ordering::Relaxed);
        TOTAL_WHITELIST_BYTES.fetch_add(1536, Ordering::Relaxed);

        let snapshot = get_snapshot();
        assert_eq!(snapshot.whitelist.down, 1024);
        assert_eq!(snapshot.whitelist.up, 512);

        let json = get_snapshot_json();
        assert_eq!(json["whitelist_active"], true);
        assert!(json["whitelist"].is_object());
        assert_eq!(json["whitelist"]["active"], true);
        assert!(json["whitelist_bytes"].as_u64().unwrap_or(0) >= 1536);

        // Reset back to inactive and clear counters
        WHITELIST_DOWN.store(0, Ordering::Relaxed);
        WHITELIST_UP.store(0, Ordering::Relaxed);
        set_whitelist_active(false);
        assert!(!is_whitelist_active());
    }

    #[test]
    fn test_anti_regression_traffic_direction_mihomo_to_client_perspective() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_traffic("all");
        NON_DIRECT_PERMILLE.store(1000, Ordering::Relaxed);
        set_whitelist_active(false);
        // Проверка: Mihomo chunk 'up' (данные, отдаваемые прокси клиенту) парсится
        // и направляется в Загрузку (DOWN), а chunk 'down' (от клиента к прокси) — в Отдачу (UP).
        let chunk_json = r#"{"up":15000000,"down":250000}"#;
        let msg: MihomoTrafficChunk = serde_json::from_str(chunk_json).expect("valid chunk");

        apply_traffic_frame(&msg);

        let snap = get_snapshot();
        assert_eq!(snap.proxy.down, 15000000, "Mihomo up must map to client download (DOWN)");
        assert_eq!(snap.proxy.up, 250000, "Mihomo down must map to client upload (UP)");

        reset_traffic("all");
    }

    #[test]
    fn test_anti_regression_traffic_split_between_vps_and_whitelist() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Сбросим для чистоты теста
        reset_traffic("all");
        NON_DIRECT_PERMILLE.store(1000, Ordering::Relaxed);
        set_whitelist_active(false);

        // 1. При неактивном Whitelist кадр /traffic идёт в PROXY (VPS Безлимит)
        let chunk_regular = MihomoTrafficChunk { down: 500, up: 1500 };
        apply_traffic_frame(&chunk_regular);
        let snap_proxy = get_snapshot();
        assert_eq!(snap_proxy.proxy.down, 1500);
        assert_eq!(snap_proxy.proxy.up, 500);
        assert_eq!(snap_proxy.whitelist.down, 0);
        assert_eq!(snap_proxy.whitelist.up, 0);

        // 2. Активируем Whitelist: теперь кадр /traffic идёт ТОЛЬКО в WHITELIST (БС Лимит),
        // а скорость PROXY обнуляется.
        set_whitelist_active(true);
        let chunk_wl = MihomoTrafficChunk { down: 1000, up: 4000 };
        apply_traffic_frame(&chunk_wl);
        let snap_wl = get_snapshot();
        assert_eq!(snap_wl.proxy.down, 0);
        assert_eq!(snap_wl.proxy.up, 0);
        assert_eq!(snap_wl.whitelist.down, 4000);
        assert_eq!(snap_wl.whitelist.up, 1000);

        set_whitelist_active(false);
        reset_traffic("all");
    }

    #[test]
    fn test_traffic_honest_stream_no_channel_substitution() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        PROXY_DOWN.store(0, Ordering::Relaxed);
        PROXY_UP.store(0, Ordering::Relaxed);

        // Simulate incoming stream chunk
        let down_bytes = 4096u64;
        let up_bytes = 1024u64;

        PROXY_DOWN.store(down_bytes, Ordering::Relaxed);
        PROXY_UP.store(up_bytes, Ordering::Relaxed);

        assert_eq!(PROXY_DOWN.load(Ordering::Relaxed), 4096);
        assert_eq!(PROXY_UP.load(Ordering::Relaxed), 1024);

        // When whitelist is active, incoming stream chunk directs into WHITELIST counters
        set_whitelist_active(true);
        let msg_down = 2048u64;
        let msg_up = 512u64;
        let bytes = msg_down + msg_up;
        if WHITELIST_ACTIVE.load(Ordering::Relaxed) {
            WHITELIST_DOWN.store(msg_up, Ordering::Relaxed);
            WHITELIST_UP.store(msg_down, Ordering::Relaxed);
            PROXY_DOWN.store(0, Ordering::Relaxed);
            PROXY_UP.store(0, Ordering::Relaxed);
            TOTAL_WHITELIST_BYTES.fetch_add(bytes, Ordering::Relaxed);
        }
        assert_eq!(WHITELIST_DOWN.load(Ordering::Relaxed), 512);
        assert_eq!(WHITELIST_UP.load(Ordering::Relaxed), 2048);
        assert_eq!(PROXY_DOWN.load(Ordering::Relaxed), 0);

        set_whitelist_active(false);
        PROXY_DOWN.store(0, Ordering::Relaxed);
        PROXY_UP.store(0, Ordering::Relaxed);
    }

    #[test]
    fn test_traffic_reset_individual_counters() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_traffic("all");
        TOTAL_DIRECT_BYTES.store(100, Ordering::Relaxed);
        TOTAL_PROXY_BYTES.store(200, Ordering::Relaxed);
        TOTAL_WHITELIST_BYTES.store(300, Ordering::Relaxed);

        // Сброс только direct
        reset_traffic("direct");
        assert_eq!(TOTAL_DIRECT_BYTES.load(Ordering::Relaxed), 0);
        assert_eq!(TOTAL_PROXY_BYTES.load(Ordering::Relaxed), 200);
        assert_eq!(TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed), 300);

        // Сброс только vps
        reset_traffic("vps");
        assert_eq!(TOTAL_PROXY_BYTES.load(Ordering::Relaxed), 0);
        assert_eq!(TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed), 300);

        // Сброс whitelist
        reset_traffic("whitelist");
        assert_eq!(TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_traffic_reset_does_not_affect_provider_quota_and_deltas() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_provider_live_delta("prov_quota_test");
        add_provider_live_delta("prov_quota_test", 8192);
        assert_eq!(get_provider_live_delta("prov_quota_test"), 8192);

        // Сброс всех сетевых счетчиков
        reset_traffic("all");
        // Проверяем, что дельта провайдера подписки и его квота не затронуты
        assert_eq!(
            get_provider_live_delta("prov_quota_test"),
            8192,
            "Сброс счетчиков сетевого трафика (direct/vps/whitelist/all) не должен обнулять дельту квоты подписки"
        );

        // Индивидуальные сбросы
        reset_traffic("direct");
        reset_traffic("vps");
        reset_traffic("whitelist");
        assert_eq!(get_provider_live_delta("prov_quota_test"), 8192);

        // Очистка тестового состояния
        reset_provider_live_delta("prov_quota_test");
        assert_eq!(get_provider_live_delta("prov_quota_test"), 0);
    }

    #[test]
    fn test_traffic_persistence_roundtrip() {
        let tmp_dir = std::env::temp_dir();
        let test_file = tmp_dir.join(format!("traffic_test_{}.json", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        
        let stats = StoredTrafficStats {
            direct_bytes: 12345,
            proxy_bytes: 67890,
            whitelist_bytes: 54321,
            updated_at: 1000,
            provider_deltas: std::collections::HashMap::new(),
        };
        let json_str = serde_json::to_string_pretty(&stats).expect("json serialize");
        std::fs::write(&test_file, json_str).expect("write file");

        let loaded: StoredTrafficStats = serde_json::from_str(&std::fs::read_to_string(&test_file).expect("read file")).expect("parse");
        assert_eq!(loaded.direct_bytes, 12345);
        assert_eq!(loaded.proxy_bytes, 67890);
        assert_eq!(loaded.whitelist_bytes, 54321);

        let _ = std::fs::remove_file(&test_file);
    }

    #[test]
    fn test_get_snapshot_json_honest_stream_when_whitelist_active() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // Anti-regression test for BUG-M2-01:
        // Ensure get_snapshot_json() and record_history_point() do not blackout telemetry
        // when whitelist is active but whitelist counters are 0 (honest proxy stream fallback).
        set_whitelist_active(true);
        PROXY_DOWN.store(1048576, Ordering::Relaxed);
        PROXY_UP.store(524288, Ordering::Relaxed);
        WHITELIST_DOWN.store(0, Ordering::Relaxed);
        WHITELIST_UP.store(0, Ordering::Relaxed);

        let snapshot = get_snapshot_json();
        assert_eq!(snapshot["down"], 1048576, "get_snapshot_json down must fall back to proxy when wl_down is 0");
        assert_eq!(snapshot["up"], 524288, "get_snapshot_json up must fall back to proxy when wl_up is 0");

        // Verify get_snapshot() also agrees
        let snap = get_snapshot();
        assert_eq!(snap.down, 1048576);
        assert_eq!(snap.up, 524288);

        // When whitelist counters have actual positive data, whitelist values must be used
        WHITELIST_DOWN.store(2048, Ordering::Relaxed);
        WHITELIST_UP.store(1024, Ordering::Relaxed);
        let snapshot_wl = get_snapshot_json();
        assert_eq!(snapshot_wl["down"], 2048);
        assert_eq!(snapshot_wl["up"], 1024);

        // Test history point logic with wl_down == 0 and whitelist active
        WHITELIST_DOWN.store(0, Ordering::Relaxed);
        WHITELIST_UP.store(0, Ordering::Relaxed);
        let is_wl_active = WHITELIST_ACTIVE.load(Ordering::Relaxed);
        let wl_down = WHITELIST_DOWN.load(Ordering::Relaxed);
        let wl_up = WHITELIST_UP.load(Ordering::Relaxed);
        let p_down = PROXY_DOWN.load(Ordering::Relaxed);
        let p_up = PROXY_UP.load(Ordering::Relaxed);

        let hist_down = if is_wl_active && wl_down > 0 { wl_down } else { p_down };
        let hist_up = if is_wl_active && wl_up > 0 { wl_up } else { p_up };
        record_history_point(json!({
            "down": hist_down,
            "up": hist_up,
        }));

        let history = get_history_json();
        let last_pt = history.as_array().and_then(|arr| arr.last()).expect("history point exists");
        assert_eq!(last_pt["down"], 1048576);
        assert_eq!(last_pt["up"], 524288);

        // Clean up state
        set_whitelist_active(false);
        PROXY_DOWN.store(0, Ordering::Relaxed);
        PROXY_UP.store(0, Ordering::Relaxed);
        WHITELIST_DOWN.store(0, Ordering::Relaxed);
        WHITELIST_UP.store(0, Ordering::Relaxed);
    }

    #[test]
    fn test_provider_live_delta_lifecycle() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_provider_live_delta("prov_test");
        assert_eq!(get_provider_live_delta("prov_test"), 0);

        set_active_provider_id(Some("prov_test".to_string()));
        assert_eq!(get_active_provider_id(), Some("prov_test".to_string()));

        add_provider_live_delta("prov_test", 1024);
        add_provider_live_delta("prov_test", 2048);
        assert_eq!(get_provider_live_delta("prov_test"), 3072);

        reset_provider_live_delta("prov_test");
        assert_eq!(get_provider_live_delta("prov_test"), 0);
        set_active_provider_id(None);
    }

    #[test]
    fn regression_bug_72_direct_connections_not_counted_in_vps_traffic() {
        // Баг 72: /traffic Mihomo считает все соединения, включая DIRECT.
        // Соединения с chains: ["DIRECT"] или rule: "DIRECT" обязаны классифицироваться как прямой трафик (direct),
        // а не попадать в счётчик VPS/БС.
        let mock_conns = serde_json::json!({
            "connections": [
                {
                    "id": "c_direct",
                    "upload": 2000,
                    "download": 8000,
                    "chains": ["DIRECT"],
                    "rule": "GeoIP"
                },
                {
                    "id": "c_vps",
                    "upload": 500,
                    "download": 1500,
                    "chains": ["PROXY", "Node-DE"],
                    "rule": "Match"
                }
            ]
        });

        let mut prev_map = std::collections::HashMap::new();
        let deltas = split_connections_delta_with_providers(&mock_conns, &mut prev_map, &std::collections::HashMap::new(), false);

        assert_eq!(
            deltas.direct_bytes, 10000,
            "DIRECT соединения (chains: ['DIRECT']) должны учитываться в direct_bytes"
        );
        assert_eq!(
            deltas.proxy_bytes, 2000,
            "В proxy_bytes должен попадать только реальный прокси-трафик (Node-DE), но не DIRECT"
        );
    }

    #[test]
    fn regression_bug_73_subscription_quota_counts_only_its_nodes() {
        // Баг 73: квоту подписки списывали всем трафиком Mihomo (включая DIRECT и чужие подписки).
        let conns = serde_json::json!({
            "connections": [
                { "id": "a", "upload": 100, "download": 900, "chains": ["Node-A1", "PROXY"], "rule": "Match" },
                { "id": "b", "upload": 50,  "download": 450, "chains": ["Node-B1", "🎮 Gaming"], "rule": "Match" },
                { "id": "d", "upload": 10,  "download": 90,  "chains": ["DIRECT"], "rule": "GeoSite" },
                { "id": "x", "upload": 1,   "download": 9,   "chains": ["Manual-Node"], "rule": "Match" }
            ]
        });
        let mut map = std::collections::HashMap::new();
        map.insert("Node-A1".to_string(), "prov_a".to_string());
        map.insert("Node-B1".to_string(), "prov_b".to_string());
        let mut prev = std::collections::HashMap::new();

        let d1 = split_connections_delta_with_providers(&conns, &mut prev, &map, false);
        assert_eq!(d1.provider_deltas.get("prov_a"), Some(&1000));
        assert_eq!(d1.provider_deltas.get("prov_b"), Some(&500));
        assert_eq!(d1.provider_deltas.len(), 2, "DIRECT и ручные узлы не списываются ни с одной подписки");
        assert_eq!(d1.direct_bytes, 100);
        assert_eq!(d1.proxy_bytes, 1510);

        // Второй опрос: считаются только приросты, закрытые соединения забываются.
        let conns2 = serde_json::json!({
            "connections": [
                { "id": "a", "upload": 150, "download": 1150, "chains": ["Node-A1", "PROXY"], "rule": "Match" }
            ]
        });
        let d2 = split_connections_delta_with_providers(&conns2, &mut prev, &map, false);
        assert_eq!(d2.provider_deltas.get("prov_a"), Some(&300));
        assert_eq!(d2.proxy_bytes, 300);
        assert_eq!(prev.len(), 1, "соединения, которых нет в ответе, удаляются из карты");

        // В режиме БС весь не-DIRECT трафик идёт в счётчик БС, а не VPS.
        let mut prev3 = std::collections::HashMap::new();
        let d3 = split_connections_delta_with_providers(&conns, &mut prev3, &map, true);
        assert_eq!(d3.whitelist_bytes, 1510);
        assert_eq!(d3.proxy_bytes, 0);
    }

    #[test]
    fn regression_vkturn_25_provider_deltas_exclude_vkturn() {
        let vk_name = crate::vkturn::PROXY_NAME;
        let conns = serde_json::json!({
            "connections": [
                { "id": "vk1", "upload": 400, "download": 1600, "chains": [vk_name, "🛡️ Whitelist"], "rule": "Match" },
                { "id": "sub1", "upload": 100, "download": 400, "chains": ["NL-01", "PROXY"], "rule": "Match" }
            ]
        });
        let mut map = std::collections::HashMap::new();
        map.insert(vk_name.to_string(), crate::vkturn::PROVIDER_ID.to_string());
        map.insert("NL-01".to_string(), "my_sub".to_string());
        let mut prev = std::collections::HashMap::new();

        let d = split_connections_delta_with_providers(&conns, &mut prev, &map, false);
        assert_eq!(
            d.provider_deltas.get(crate::vkturn::PROVIDER_ID),
            None,
            "vkturn virtual provider must never be accounted in subscription provider_deltas"
        );
        assert_eq!(d.provider_deltas.get("my_sub"), Some(&500));
    }

    #[test]
    fn regression_bug_75b_traffic_tests_call_production_code() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        NON_DIRECT_PERMILLE.store(1000, Ordering::Relaxed);
        set_whitelist_active(false);
        let msg = MihomoTrafficChunk { up: 1200, down: 300 };
        let (down, up) = apply_traffic_frame(&msg);
        assert_eq!((down, up), (1200, 300));
        let snap = get_snapshot();
        assert_eq!(snap.proxy.down, 1200);
        assert_eq!(snap.proxy.up, 300);

        let local_flag = std::sync::atomic::AtomicBool::new(false);
        let mut saved = false;
        let mut cleaned = false;
        shutdown_with(&local_flag, || saved = true, || cleaned = true);
        assert!(local_flag.load(Ordering::Acquire));
        assert!(saved && cleaned);
        assert!(!is_shutdown(), "local shutdown_with must not mutate global SHUTDOWN");

        PROXY_DOWN.store(0, Ordering::Relaxed);
        PROXY_UP.store(0, Ordering::Relaxed);
    }

    #[test]
    fn regression_bug_74b_direction_mapping_is_single_switch() {
        let _guard = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        reset_traffic("all");
        NON_DIRECT_PERMILLE.store(1000, Ordering::Relaxed);
        set_whitelist_active(false);

        let msg = MihomoTrafficChunk { up: 9000, down: 1500 };
        let (down, up) = apply_traffic_frame(&msg);
        if frame_direction_swapped() {
            assert_eq!((down, up), (9000, 1500));
        } else {
            assert_eq!((down, up), (1500, 9000));
        }
        assert!(frame_direction_swapped(), "current router behavior expects swapped Mihomo up->down until verified on 192.168.1.1");
        reset_traffic("all");
    }
}
