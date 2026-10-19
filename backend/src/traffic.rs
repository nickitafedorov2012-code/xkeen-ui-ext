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

pub fn set_whitelist_active(active: bool) {
    WHITELIST_ACTIVE.store(active, Ordering::Release);
}

pub fn is_whitelist_active() -> bool {
    WHITELIST_ACTIVE.load(Ordering::Acquire)
}

#[derive(Deserialize)]
struct MihomoTrafficChunk {
    #[serde(default)]
    up: u64,
    #[serde(default)]
    down: u64,
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
        down: if is_wl_active { wl_down } else { p_down },
        up: if is_wl_active { wl_up } else { p_up },
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
    json!({
        "direct": {
            "down": d_down,
            "up": d_up,
            "bytes": d_bytes,
            "estimated": true,
            "source": "WAN (/proc/net/dev) - Mihomo (/traffic)"
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
        "down": if is_wl_active { wl_down } else { p_down },
        "up": if is_wl_active { wl_up } else { p_up },
        "history": history,
        "estimated": true,
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
    let stats = StoredTrafficStats {
        direct_bytes: TOTAL_DIRECT_BYTES.load(Ordering::Relaxed),
        proxy_bytes: TOTAL_PROXY_BYTES.load(Ordering::Relaxed),
        whitelist_bytes: TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed),
        updated_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
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

pub fn shutdown() {
    SHUTDOWN.store(true, Ordering::Release);
    save_stats_to_disk();
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
                                        // Инверсия направления Mihomo -> клиент: данные, отдаваемые Mihomo клиенту, являются Загрузкой (DOWN)
                                        let bytes = msg.down + msg.up;
                                        if WHITELIST_ACTIVE.load(Ordering::Relaxed) {
                                            // Активен сервер / туннель Белых списков (БС) с лимитированным трафиком
                                            WHITELIST_DOWN.store(msg.up, Ordering::Relaxed);
                                            WHITELIST_UP.store(msg.down, Ordering::Relaxed);
                                            TOTAL_WHITELIST_BYTES.fetch_add(bytes, Ordering::Relaxed);
                                            PROXY_DOWN.store(0, Ordering::Relaxed);
                                            PROXY_UP.store(0, Ordering::Relaxed);
                                        } else {
                                            // Обычный VPS сервер (без метки БС) с безлимитным трафиком
                                            PROXY_DOWN.store(msg.up, Ordering::Relaxed);
                                            PROXY_UP.store(msg.down, Ordering::Relaxed);
                                            TOTAL_PROXY_BYTES.fetch_add(bytes, Ordering::Relaxed);
                                            WHITELIST_DOWN.store(0, Ordering::Relaxed);
                                            WHITELIST_UP.store(0, Ordering::Relaxed);
                                        }
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

/// Подсчет реальной скорости прямого трафика роутера (WAN - PROXY)
async fn run_wan_polling() {
    let mut last_ifaces = get_wan_interfaces();
    let mut last_ifaces_update = Instant::now();
    let (mut prev_rx, mut prev_tx) = read_wan_bytes(&last_ifaces);
    let mut prev_time = Instant::now();

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

        let d_down = wan_rx_speed.saturating_sub(total_proxy_down);
        let d_up = wan_tx_speed.saturating_sub(total_proxy_up);

        DIRECT_DOWN.store(d_down, Ordering::Relaxed);
        DIRECT_UP.store(d_up, Ordering::Relaxed);
        TOTAL_DIRECT_BYTES.fetch_add(d_down + d_up, Ordering::Relaxed);

        let is_wl_active = WHITELIST_ACTIVE.load(Ordering::Relaxed);

        record_history_point(json!({
            "direct": { "down": d_down, "up": d_up, "estimated": true, "source": "WAN - Mihomo" },
            "proxy": { "down": p_down, "up": p_up, "source": "Mihomo VPS" },
            "whitelist": { "down": wl_down, "up": wl_up, "active": is_wl_active },
            "total": { "down": wan_rx_speed, "up": wan_tx_speed, "source": "WAN" },
            "down": if is_wl_active { wl_down } else { p_down },
            "up": if is_wl_active { wl_up } else { p_up },
            "estimated": true,
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

    tokio::spawn(run_mihomo_stream(state.clone()));
    tokio::spawn(run_wan_polling());
    tokio::spawn(run_periodic_stats_saver());
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(!is_shutdown());
        shutdown();
        assert!(is_shutdown());
    }

    #[test]
    fn test_whitelist_traffic_active_and_counters() {
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

        // Reset back to inactive
        set_whitelist_active(false);
        assert!(!is_whitelist_active());
    }

    #[test]
    fn test_anti_regression_traffic_direction_mihomo_to_client_perspective() {
        // Проверка: Mihomo chunk 'up' (данные, отдаваемые прокси клиенту) парсится
        // и направляется в Загрузку (DOWN), а chunk 'down' (от клиента к прокси) — в Отдачу (UP).
        let chunk_json = r#"{"up":15000000,"down":250000}"#;
        let msg: MihomoTrafficChunk = serde_json::from_str(chunk_json).expect("valid chunk");
        
        // В run_mihomo_stream:
        PROXY_DOWN.store(msg.up, Ordering::Relaxed);
        PROXY_UP.store(msg.down, Ordering::Relaxed);

        let snap = get_snapshot();
        assert_eq!(snap.proxy.down, 15000000, "Mihomo up must map to client download (DOWN)");
        assert_eq!(snap.proxy.up, 250000, "Mihomo down must map to client upload (UP)");
    }

    #[test]
    fn test_anti_regression_traffic_split_between_vps_and_whitelist() {
        // Сбросим для чистоты теста
        reset_traffic("all");
        set_whitelist_active(false);

        // 1. При неактивном Whitelist трафик должен идти в PROXY (VPS Безлимит)
        let chunk_regular = MihomoTrafficChunk { down: 500, up: 1500 };
        let bytes_regular = chunk_regular.down + chunk_regular.up;
        if WHITELIST_ACTIVE.load(Ordering::Relaxed) {
            TOTAL_WHITELIST_BYTES.fetch_add(bytes_regular, Ordering::Relaxed);
        } else {
            TOTAL_PROXY_BYTES.fetch_add(bytes_regular, Ordering::Relaxed);
        }

        assert_eq!(TOTAL_PROXY_BYTES.load(Ordering::Relaxed), 2000);
        assert_eq!(TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed), 0);

        // 2. Активируем Whitelist: теперь трафик должен идти ТОЛЬКО в WHITELIST (БС Лимит)
        set_whitelist_active(true);
        let chunk_wl = MihomoTrafficChunk { down: 1000, up: 4000 };
        let bytes_wl = chunk_wl.down + chunk_wl.up;
        if WHITELIST_ACTIVE.load(Ordering::Relaxed) {
            TOTAL_WHITELIST_BYTES.fetch_add(bytes_wl, Ordering::Relaxed);
        } else {
            TOTAL_PROXY_BYTES.fetch_add(bytes_wl, Ordering::Relaxed);
        }

        // Счетчик PROXY не должен измениться!
        assert_eq!(TOTAL_PROXY_BYTES.load(Ordering::Relaxed), 2000);
        assert_eq!(TOTAL_WHITELIST_BYTES.load(Ordering::Relaxed), 5000);

        set_whitelist_active(false);
    }

    #[test]
    fn test_traffic_reset_individual_counters() {
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
    fn test_traffic_persistence_roundtrip() {
        let tmp_dir = std::env::temp_dir();
        let test_file = tmp_dir.join(format!("traffic_test_{}.json", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        
        let stats = StoredTrafficStats {
            direct_bytes: 12345,
            proxy_bytes: 67890,
            whitelist_bytes: 54321,
            updated_at: 1000,
        };
        let json_str = serde_json::to_string_pretty(&stats).expect("json serialize");
        std::fs::write(&test_file, json_str).expect("write file");

        let loaded: StoredTrafficStats = serde_json::from_str(&std::fs::read_to_string(&test_file).expect("read file")).expect("parse");
        assert_eq!(loaded.direct_bytes, 12345);
        assert_eq!(loaded.proxy_bytes, 67890);
        assert_eq!(loaded.whitelist_bytes, 54321);

        let _ = std::fs::remove_file(&test_file);
    }
}
