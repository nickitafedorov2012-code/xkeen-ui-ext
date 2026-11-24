use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    net::SocketAddr,
    time::{Duration, Instant},
};

use crate::config::MultiWanWeightMode;
use crate::multiwan::discovery::DiscoveredWan;

pub const CANARY_TARGETS: &[&str] = &["77.88.8.8:443", "1.1.1.1:443"];
pub const CANARY_TIMEOUT: Duration = Duration::from_millis(2500);
pub const FLAPPING_WINDOW: Duration = Duration::from_secs(60);
pub const QUARANTINE_DURATION: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WanChannelState {
    Online,
    Offline,
    Standby,
    Quarantine,
}

#[derive(Debug, Clone, Default)]
pub struct ProcNetDevBytes {
    pub rx_bytes: u64,
    pub tx_bytes: u64,
}

pub fn parse_proc_net_dev(raw: &str) -> HashMap<String, ProcNetDevBytes> {
    let mut map = HashMap::new();
    for line in raw.lines().skip(2) {
        let Some((iface_part, rest)) = line.split_once(':') else {
            continue;
        };
        let iface = iface_part.trim();
        let cols: Vec<&str> = rest.split_whitespace().collect();
        if cols.len() >= 9 {
            let rx_bytes = cols[0].parse::<u64>().unwrap_or(0);
            let tx_bytes = cols[8].parse::<u64>().unwrap_or(0);
            map.insert(iface.to_string(), ProcNetDevBytes { rx_bytes, tx_bytes });
        }
    }
    map
}

#[derive(Debug, Clone)]
pub struct ChannelHealthSnapshot {
    pub state: WanChannelState,
    pub flapping: bool,
    pub rx_mbps: f64,
    pub tx_mbps: f64,
    pub rtt_ms: Option<u32>,
    pub loss_pct: u8,
}

#[derive(Debug, Clone)]
pub struct ChannelHealthEntry {
    pub state: WanChannelState,
    pub flapping: bool,
    pub consec_ok: u8,
    pub consec_fail: u8,
    pub drops: VecDeque<Instant>,
    pub quarantine_until: Option<Instant>,
    pub last_rx_bytes: Option<u64>,
    pub last_tx_bytes: Option<u64>,
    pub last_sample_at: Option<Instant>,
    pub rx_mbps: f64,
    pub tx_mbps: f64,
    pub rtt_ms: Option<u32>,
    pub loss_pct: u8,
}

impl Default for ChannelHealthEntry {
    fn default() -> Self {
        Self {
            state: WanChannelState::Online,
            flapping: false,
            consec_ok: 1,
            consec_fail: 0,
            drops: VecDeque::new(),
            quarantine_until: None,
            last_rx_bytes: None,
            last_tx_bytes: None,
            last_sample_at: None,
            rx_mbps: 0.0,
            tx_mbps: 0.0,
            rtt_ms: None,
            loss_pct: 0,
        }
    }
}

impl ChannelHealthEntry {
    pub fn update_counters(&mut self, bytes: Option<&ProcNetDevBytes>, now: Instant) {
        let Some(b) = bytes else {
            return;
        };
        if let (Some(prev_rx), Some(prev_tx), Some(prev_at)) =
            (self.last_rx_bytes, self.last_tx_bytes, self.last_sample_at)
        {
            let dt = now.saturating_duration_since(prev_at).as_secs_f64();
            if dt >= 0.2 {
                let drx = b.rx_bytes.saturating_sub(prev_rx) as f64;
                let dtx = b.tx_bytes.saturating_sub(prev_tx) as f64;
                self.rx_mbps = ((drx * 8.0) / (dt * 1_000_000.0) * 100.0).round() / 100.0;
                self.tx_mbps = ((dtx * 8.0) / (dt * 1_000_000.0) * 100.0).round() / 100.0;
            }
        }
        self.last_rx_bytes = Some(b.rx_bytes);
        self.last_tx_bytes = Some(b.tx_bytes);
        self.last_sample_at = Some(now);
    }

    pub fn record_probe_outcome(&mut self, ok: bool, rtt_ms: Option<u32>, loss_pct: u8, now: Instant) {
        self.rtt_ms = rtt_ms;
        self.loss_pct = loss_pct;

        while let Some(&front) = self.drops.front() {
            if now.saturating_duration_since(front) > FLAPPING_WINDOW {
                self.drops.pop_front();
            } else {
                break;
            }
        }

        if let Some(until) = self.quarantine_until {
            if now < until {
                self.state = WanChannelState::Quarantine;
                self.flapping = true;
                return;
            }
            self.quarantine_until = None;
            self.flapping = false;
            self.state = WanChannelState::Offline;
            self.consec_ok = 0;
        }

        if ok {
            self.consec_fail = 0;
            self.consec_ok = self.consec_ok.saturating_add(1);
            if self.state != WanChannelState::Online && self.consec_ok >= 2 {
                self.state = WanChannelState::Online;
                self.flapping = false;
            }
        } else {
            self.consec_ok = 0;
            self.consec_fail = self.consec_fail.saturating_add(1);
            if self.state == WanChannelState::Online && self.consec_fail >= 2 {
                self.state = WanChannelState::Offline;
                self.drops.push_back(now);
                if self.drops.len() >= 3 {
                    self.state = WanChannelState::Quarantine;
                    self.flapping = true;
                    self.quarantine_until = Some(now + QUARANTINE_DURATION);
                }
            }
        }
    }

    pub fn snapshot(&self) -> ChannelHealthSnapshot {
        ChannelHealthSnapshot {
            state: self.state,
            flapping: self.flapping,
            rx_mbps: self.rx_mbps,
            tx_mbps: self.tx_mbps,
            rtt_ms: self.rtt_ms,
            loss_pct: self.loss_pct,
        }
    }
}

#[cfg(target_os = "linux")]
async fn probe_single_target(addr: SocketAddr, fwmark: u32) -> Option<u32> {
    use std::os::unix::io::AsRawFd;
    let sock = tokio::net::TcpSocket::new_v4().ok()?;
    let mark: libc::c_uint = fwmark;
    let rc = unsafe {
        libc::setsockopt(
            sock.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_MARK,
            (&mark as *const libc::c_uint).cast::<libc::c_void>(),
            std::mem::size_of::<libc::c_uint>() as libc::socklen_t,
        )
    };
    if rc != 0 {
        return None;
    }
    let started = Instant::now();
    match tokio::time::timeout(CANARY_TIMEOUT, sock.connect(addr)).await {
        Ok(Ok(_stream)) => Some(started.elapsed().as_millis().max(1) as u32),
        _ => None,
    }
}

#[cfg(not(target_os = "linux"))]
async fn probe_single_target(_addr: SocketAddr, _fwmark: u32) -> Option<u32> {
    Some(15)
}

/// Запускает активную TCP-канарейку через `SO_MARK` конкретного WAN-канала.
pub async fn probe_wan_canary(fwmark: u32) -> (bool, Option<u32>, u8) {
    let addrs: Vec<SocketAddr> = CANARY_TARGETS
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect();
    if addrs.is_empty() {
        return (false, None, 100);
    }
    let futs = addrs.iter().map(|&a| probe_single_target(a, fwmark));
    let results = futures_util::future::join_all(futs).await;
    let total = results.len();
    let ok_rtts: Vec<u32> = results.into_iter().flatten().collect();
    let ok_count = ok_rtts.len();
    let any_ok = ok_count > 0;
    let min_rtt = ok_rtts.into_iter().min();
    let loss_pct = (((total - ok_count) * 100) / total) as u8;
    (any_ok, min_rtt, loss_pct)
}

pub struct WeightedChannelInput<'a> {
    pub wan: &'a DiscoveredWan,
    pub health: &'a ChannelHealthSnapshot,
    pub enabled: bool,
    pub manual_weight: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputedChannelWeight {
    pub rci_id: String,
    pub effective_weight: u8,
    pub display_state: WanChannelState,
}

fn normalize_scores_to_100(scores: &[f64]) -> Vec<u8> {
    let sum: f64 = scores.iter().copied().sum();
    if sum <= 0.0 || scores.is_empty() {
        return vec![0; scores.len()];
    }
    let mut out = vec![0u8; scores.len()];
    let mut rem: Vec<(usize, f64)> = Vec::with_capacity(scores.len());
    let mut total_int = 0u32;
    for (i, &s) in scores.iter().enumerate() {
        let exact = (s / sum) * 100.0;
        let base = exact.floor() as u8;
        out[i] = base;
        total_int += u32::from(base);
        rem.push((i, exact - f64::from(base)));
    }
    rem.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (i, _) in rem.into_iter().take(100usize.saturating_sub(total_int as usize)) {
        out[i] = out[i].saturating_add(1);
    }
    out
}

/// Вычисляет эффективные веса (`0..=100`, сумма 100 среди активных) и отображаемый статус.
pub fn compute_effective_weights(
    inputs: &[WeightedChannelInput<'_>],
    mode: MultiWanWeightMode,
) -> Vec<ComputedChannelWeight> {
    let mut result: Vec<ComputedChannelWeight> = inputs
        .iter()
        .map(|i| ComputedChannelWeight {
            rci_id: i.wan.rci_id.clone(),
            effective_weight: 0,
            display_state: i.health.state,
        })
        .collect();

    let eligible_indices: Vec<usize> = inputs
        .iter()
        .enumerate()
        .filter(|(_, i)| i.enabled && i.health.state == WanChannelState::Online)
        .map(|(idx, _)| idx)
        .collect();

    if eligible_indices.is_empty() {
        return result;
    }

    match mode {
        MultiWanWeightMode::Auto => {
            let scores: Vec<f64> = eligible_indices
                .iter()
                .map(|&idx| {
                    let h = inputs[idx].health;
                    let rtt = h.rtt_ms.unwrap_or(80).max(15) as f64;
                    let loss_factor = (100.0 - f64::from(h.loss_pct)).max(10.0) / 100.0;
                    let throughput = (h.rx_mbps + h.tx_mbps).min(200.0);
                    (1000.0 / rtt) * loss_factor + throughput * 0.5
                })
                .collect();
            let weights = normalize_scores_to_100(&scores);
            for (pos, &idx) in eligible_indices.iter().enumerate() {
                result[idx].effective_weight = weights[pos];
            }
        }
        MultiWanWeightMode::Manual => {
            let primary_indices: Vec<usize> = eligible_indices
                .iter()
                .copied()
                .filter(|&idx| inputs[idx].manual_weight > 0)
                .collect();

            if !primary_indices.is_empty() {
                let scores: Vec<f64> = primary_indices
                    .iter()
                    .map(|&idx| f64::from(inputs[idx].manual_weight))
                    .collect();
                let weights = normalize_scores_to_100(&scores);
                for (pos, &idx) in primary_indices.iter().enumerate() {
                    result[idx].effective_weight = weights[pos];
                }
                for &idx in &eligible_indices {
                    if inputs[idx].manual_weight == 0 {
                        result[idx].display_state = WanChannelState::Standby;
                    }
                }
            } else {
                // Все основные каналы с весом > 0 упали: автоматически поднимаем живые Standby-каналы!
                let scores = vec![1.0f64; eligible_indices.len()];
                let weights = normalize_scores_to_100(&scores);
                for (pos, &idx) in eligible_indices.iter().enumerate() {
                    result[idx].effective_weight = weights[pos];
                    result[idx].display_state = WanChannelState::Online;
                }
            }
        }
    }

    result
}
