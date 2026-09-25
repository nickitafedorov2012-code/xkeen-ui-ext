use std::sync::atomic::{AtomicU64, Ordering};
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
    pub total: TrafficSpeed,
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

    TrafficSnapshot {
        direct: TrafficSpeed { down: d_down, up: d_up },
        proxy: TrafficSpeed { down: p_down, up: p_up },
        total: TrafficSpeed { down: w_down, up: w_up },
        down: p_down,
        up: p_up,
    }
}

pub fn get_snapshot_json() -> serde_json::Value {
    let p_down = PROXY_DOWN.load(Ordering::Relaxed);
    let p_up = PROXY_UP.load(Ordering::Relaxed);
    let d_down = DIRECT_DOWN.load(Ordering::Relaxed);
    let d_up = DIRECT_UP.load(Ordering::Relaxed);
    let w_down = WAN_DOWN.load(Ordering::Relaxed);
    let w_up = WAN_UP.load(Ordering::Relaxed);

    json!({
        "direct": { "down": d_down, "up": d_up },
        "proxy": { "down": p_down, "up": p_up },
        "total": { "down": w_down, "up": w_up },
        "down": p_down,
        "up": p_up,
    })
}

pub static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn shutdown() {
    SHUTDOWN.store(true, Ordering::Release);
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
                                        PROXY_DOWN.store(msg.down, Ordering::Relaxed);
                                        PROXY_UP.store(msg.up, Ordering::Relaxed);
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
                    let nums: Vec<u64> = stats_part
                        .split_whitespace()
                        .filter_map(|s| s.parse::<u64>().ok())
                        .collect();
                    if nums.len() >= 9 {
                        rx += nums[0]; // RX bytes
                        tx += nums[8]; // TX bytes
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
            last_ifaces = get_wan_interfaces();
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

        let d_down = if wan_rx_speed > p_down { wan_rx_speed - p_down } else { 0 };
        let d_up = if wan_tx_speed > p_up { wan_tx_speed - p_up } else { 0 };

        DIRECT_DOWN.store(d_down, Ordering::Relaxed);
        DIRECT_UP.store(d_up, Ordering::Relaxed);
    }
}

pub fn spawn(state: AppState) {
    tokio::spawn(run_mihomo_stream(state.clone()));
    tokio::spawn(run_wan_polling());
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
}
