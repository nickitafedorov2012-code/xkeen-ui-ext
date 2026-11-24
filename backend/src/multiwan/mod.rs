pub mod discovery;
pub mod health;
pub mod routing;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Notify, RwLock};

use crate::{
    config::MultiWanWeightMode,
    multiwan::{
        discovery::{WanKind, detect_gateway_collision, discover_wans},
        health::{
            ChannelHealthEntry, WanChannelState, WeightedChannelInput, compute_effective_weights,
            parse_proc_net_dev, probe_wan_canary,
        },
        routing::{
            ActiveWanRoute, DEFAULT_MANAGEMENT_PORTS, RoutingPlan, apply_routing_plan,
            build_routing_plan_full, flush_conntrack_for_dropped_wan, inspect_routing_integrity,
            mark_hex, probe_ipset_available, probe_xt_statistic, repair_routing_in_place,
            teardown_routing,
        },
    },
    AppState,
};

const SUPERVISOR_INTERVAL: Duration = Duration::from_secs(4);
const AUTO_SCALE_MAX_WORKERS: usize = 60;
const AUTO_SCALE_MIN_WORKERS: usize = 20;
const CPU_HIGH_THRESHOLD: f64 = 90.0;
const CPU_LOW_THRESHOLD: f64 = 85.0;
const CPU_HIGH_HOLD_SECS: u64 = 30;
const CPU_STEP_INTERVAL_SECS: u64 = 12;
const CPU_SCALE_STEP: usize = 8;
const DNS_CACHE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiWanChannelStatus {
    pub rci_id: String,
    pub name: String,
    pub kind: WanKind,
    pub linux_iface: String,
    pub ipv4: String,
    pub gateway: String,
    pub slot: u8,
    pub table_id: u32,
    pub fwmark_hex: String,
    pub enabled: bool,
    pub state: WanChannelState,
    pub flapping: bool,
    pub rx_mbps: f64,
    pub tx_mbps: f64,
    pub rtt_ms: Option<u32>,
    pub loss_pct: u8,
    pub manual_weight: u8,
    pub effective_weight: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MultiWanStatus {
    pub enabled: bool,
    pub mode: MultiWanWeightMode,
    pub single_ip_subscriptions: bool,
    pub active_count: usize,
    pub total_rx_mbps: f64,
    pub total_tx_mbps: f64,
    pub wan_marks_spec: String,
    pub gateway_collision_warning: Option<String>,
    pub last_reconcile_ts: i64,
    pub last_error: Option<String>,
    pub channels: Vec<MultiWanChannelStatus>,
}

pub fn format_wan_marks_spec(routes: &[ActiveWanRoute]) -> String {
    routes
        .iter()
        .filter(|r| r.weight > 0)
        .map(|r| {
            let hex = mark_hex(r.wan.fwmark);
            if r.wan.ipv4.is_empty() {
                format!("{hex}:{}", r.weight)
            } else {
                format!("{hex}:{}@{}", r.weight, r.wan.ipv4)
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

/// Плавный CPU-предохранитель для авто-масштабирования потоков csqtt-client (гистерезис >90% >30с).
#[derive(Debug, Clone, Default)]
pub struct CpuAutoScaleGuard {
    pub current_workers: Option<usize>,
    pub high_cpu_since: Option<Instant>,
    pub last_step_at: Option<Instant>,
}

impl CpuAutoScaleGuard {
    pub fn target_cap(active_wans: usize, fallback_workers: usize) -> usize {
        if active_wans >= 3 {
            AUTO_SCALE_MAX_WORKERS
        } else if active_wans == 2 {
            50
        } else {
            fallback_workers.max(10)
        }
    }

    pub fn evaluate(
        &mut self,
        active_wans: usize,
        cpu_pct: f64,
        fallback_workers: usize,
        now: Instant,
    ) -> usize {
        if active_wans < 2 {
            self.high_cpu_since = None;
            self.last_step_at = None;
            self.current_workers = Some(fallback_workers);
            return fallback_workers;
        }

        let cap = Self::target_cap(active_wans, fallback_workers);
        let floor = AUTO_SCALE_MIN_WORKERS.min(cap);
        let cur = self.current_workers.unwrap_or(cap).clamp(floor, cap);

        if cpu_pct > CPU_HIGH_THRESHOLD {
            let since = *self.high_cpu_since.get_or_insert(now);
            let held_over_30s = now.saturating_duration_since(since) >= Duration::from_secs(CPU_HIGH_HOLD_SECS);
            let step_ready = self
                .last_step_at
                .map(|t| now.saturating_duration_since(t) >= Duration::from_secs(CPU_STEP_INTERVAL_SECS))
                .unwrap_or(true);
            if held_over_30s && step_ready && cur > floor {
                let next = cur.saturating_sub(CPU_SCALE_STEP).max(floor);
                self.last_step_at = Some(now);
                self.current_workers = Some(next);
                return next;
            }
            self.current_workers = Some(cur);
            return cur;
        }

        self.high_cpu_since = None;
        if cpu_pct < CPU_LOW_THRESHOLD && cur < cap {
            let step_ready = self
                .last_step_at
                .map(|t| now.saturating_duration_since(t) >= Duration::from_secs(CPU_STEP_INTERVAL_SECS))
                .unwrap_or(true);
            if step_ready {
                let next = cur.saturating_add(CPU_SCALE_STEP).min(cap);
                self.last_step_at = Some(now);
                self.current_workers = Some(next);
                return next;
            }
        }

        self.current_workers = Some(cur);
        cur
    }
}

#[derive(Default)]
struct SupervisorRuntime {
    health: HashMap<String, ChannelHealthEntry>,
    prev_active_wans: HashMap<String, (String, u32)>,
    last_plan: Option<RoutingPlan>,
    last_wan_marks_spec: String,
    last_auto_workers: Option<usize>,
    cpu_guard: CpuAutoScaleGuard,
    dns_cache: HashMap<String, (Instant, Vec<String>)>,
    xt_statistic_ok: Option<bool>,
    ipset_ok: Option<bool>,
    #[allow(dead_code)]
    prev_cpu: Option<(u64, u64)>,
}

pub struct MultiWanManager {
    status: RwLock<MultiWanStatus>,
    runtime: Mutex<SupervisorRuntime>,
    reconcile_lock: Mutex<()>,
    reconcile_notify: Notify,
}

impl MultiWanManager {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            status: RwLock::new(MultiWanStatus::default()),
            runtime: Mutex::new(SupervisorRuntime::default()),
            reconcile_lock: Mutex::new(()),
            reconcile_notify: Notify::new(),
        })
    }

    pub async fn status(&self) -> MultiWanStatus {
        self.status.read().await.clone()
    }

    pub async fn current_wan_marks_spec(&self) -> String {
        self.status.read().await.wan_marks_spec.clone()
    }

    pub async fn live_wan_ips(&self) -> Vec<String> {
        let st = self.status.read().await;
        if !st.enabled {
            return Vec::new();
        }
        let mut ips = Vec::new();
        for ch in &st.channels {
            if !ch.ipv4.is_empty()
                && (ch.state == WanChannelState::Online || ch.effective_weight > 0)
                && !ips.contains(&ch.ipv4)
            {
                ips.push(ch.ipv4.clone());
            }
        }
        ips
    }

    pub fn trigger_reconcile(&self) {
        self.reconcile_notify.notify_one();
    }

    pub fn spawn_supervisor(state: AppState) {
        let cancel = crate::shutdown::global();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(SUPERVISOR_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = interval.tick() => {}
                    _ = state.multiwan.reconcile_notify.notified() => {}
                }
                if cancel.is_cancelled() {
                    break;
                }
                let _ = state.multiwan.reconcile_inner(&state, true).await;
            }
        });
    }

    pub async fn reconcile_now(&self, state: &AppState) -> MultiWanStatus {
        self.reconcile_inner(state, false).await
    }

    pub async fn run_probe_now(&self, state: &AppState) -> MultiWanStatus {
        self.reconcile_inner(state, true).await
    }

    pub async fn shutdown(&self) {
        teardown_routing().await;
    }

    async fn reconcile_inner(&self, state: &AppState, run_probes: bool) -> MultiWanStatus {
        let _guard = self.reconcile_lock.lock().await;
        let cfg = state.cfg_snapshot().await;
        let mw_cfg = &cfg.multiwan;

        let (discovered, lan_cidrs) = discover_wans(&state.http, &cfg).await;
        let gateway_collision_warning = detect_gateway_collision(&discovered);
        let net_dev_raw = read_proc_net_dev().await;
        let net_counters = parse_proc_net_dev(&net_dev_raw);
        let now = Instant::now();

        let panel_port = std::env::var("XKEEN_ROUTE_PORT")
            .ok()
            .and_then(|s| s.parse::<u16>().ok())
            .unwrap_or(1001);

        let (all_proxy_hosts, paid_proxy_hosts) = if mw_cfg.enabled {
            collect_proxy_hosts_from_files(
                std::path::Path::new(&cfg.mihomo.config_path),
                &cfg.mobile_provider_urls,
            )
            .await
        } else {
            (Vec::new(), Vec::new())
        };

        let cached_dns = {
            let rt = self.runtime.lock().await;
            rt.dns_cache.clone()
        };
        let (all_proxy_ips, updated_dns) = if mw_cfg.enabled {
            let mut hosts_to_resolve = all_proxy_hosts.clone();
            if mw_cfg.single_ip_subscriptions {
                for h in &paid_proxy_hosts {
                    if !hosts_to_resolve.contains(h) {
                        hosts_to_resolve.push(h.clone());
                    }
                }
            }
            resolve_hosts_cached(&hosts_to_resolve, cached_dns, now).await
        } else {
            (HashMap::new(), HashMap::new())
        };

        // Канарейки запускаем параллельно без удержания runtime lock
        let probe_results: Vec<(String, (bool, Option<u32>, u8))> = if run_probes && mw_cfg.enabled {
            let futs = discovered.iter().map(|w| {
                let id = w.rci_id.clone();
                let mark = w.fwmark;
                async move { (id, probe_wan_canary(mark).await) }
            });
            futures_util::future::join_all(futs).await
        } else {
            Vec::new()
        };
        let probe_map: HashMap<String, (bool, Option<u32>, u8)> =
            probe_results.into_iter().collect();

        let (channels_status, active_routes, dropped_wans, plan, wan_marks_spec, prev_spec) = {
            let mut rt = self.runtime.lock().await;
            if mw_cfg.enabled {
                rt.dns_cache = updated_dns;
            }
            if rt.xt_statistic_ok.is_none() && mw_cfg.enabled {
                rt.xt_statistic_ok = Some(probe_xt_statistic().await);
            }
            if rt.ipset_ok.is_none() && mw_cfg.enabled {
                rt.ipset_ok = Some(probe_ipset_available().await);
            }
            let use_statistic = rt.xt_statistic_ok.unwrap_or(true);
            let use_ipset = rt.ipset_ok.unwrap_or(true);

            let mut snapshots = Vec::with_capacity(discovered.len());
            for wan in &discovered {
                let entry = rt.health.entry(wan.rci_id.clone()).or_default();
                entry.update_counters(net_counters.get(&wan.linux_iface), now);
                if let Some(&(ok, rtt, loss)) = probe_map.get(&wan.rci_id) {
                    entry.record_probe_outcome(ok, rtt, loss, now);
                }
                snapshots.push(entry.snapshot());
            }

            let cfg_map: HashMap<&str, (bool, u8)> = mw_cfg
                .channels
                .iter()
                .map(|c| (c.rci_id.as_str(), (c.enabled, c.weight.min(100))))
                .collect();

            let inputs: Vec<WeightedChannelInput<'_>> = discovered
                .iter()
                .zip(snapshots.iter())
                .map(|(wan, health)| {
                    let (enabled, manual_weight) = cfg_map
                        .get(wan.rci_id.as_str())
                        .copied()
                        .unwrap_or((true, 50));
                    WeightedChannelInput {
                        wan,
                        health,
                        enabled,
                        manual_weight,
                    }
                })
                .collect();

            let computed = compute_effective_weights(&inputs, mw_cfg.mode);

            let mut channels_status = Vec::with_capacity(discovered.len());
            let mut active_routes = Vec::new();
            let mut current_active_map: HashMap<String, (String, u32)> = HashMap::new();

            for ((wan, health), comp) in discovered
                .iter()
                .zip(snapshots.iter())
                .zip(computed.iter())
            {
                let (enabled, manual_weight) = cfg_map
                    .get(wan.rci_id.as_str())
                    .copied()
                    .unwrap_or((true, 50));
                if mw_cfg.enabled && comp.effective_weight > 0 {
                    active_routes.push(ActiveWanRoute {
                        wan: wan.clone(),
                        weight: comp.effective_weight,
                    });
                    current_active_map.insert(wan.rci_id.clone(), (wan.ipv4.clone(), wan.fwmark));
                }
                channels_status.push(MultiWanChannelStatus {
                    rci_id: wan.rci_id.clone(),
                    name: wan.name.clone(),
                    kind: wan.kind,
                    linux_iface: wan.linux_iface.clone(),
                    ipv4: wan.ipv4.clone(),
                    gateway: wan.gateway.clone(),
                    slot: wan.slot,
                    table_id: wan.table_id,
                    fwmark_hex: mark_hex(wan.fwmark),
                    enabled,
                    state: comp.display_state,
                    flapping: health.flapping,
                    rx_mbps: health.rx_mbps,
                    tx_mbps: health.tx_mbps,
                    rtt_ms: health.rtt_ms,
                    loss_pct: health.loss_pct,
                    manual_weight,
                    effective_weight: comp.effective_weight,
                });
            }

            let dropped: Vec<(String, u32)> = rt
                .prev_active_wans
                .iter()
                .filter(|(id, _)| !current_active_map.contains_key(*id))
                .map(|(_, v)| v.clone())
                .collect();
            rt.prev_active_wans = current_active_map;

            let spec = if mw_cfg.enabled {
                format_wan_marks_spec(&active_routes)
            } else {
                String::new()
            };

            let single_ip_targets = if mw_cfg.enabled && mw_cfg.single_ip_subscriptions {
                flatten_resolved_ips(&paid_proxy_hosts, &all_proxy_ips)
            } else {
                Vec::new()
            };
            let proxy_server_targets = if mw_cfg.enabled {
                flatten_resolved_ips(&all_proxy_hosts, &all_proxy_ips)
            } else {
                Vec::new()
            };

            let mut mgmt_ports = DEFAULT_MANAGEMENT_PORTS.to_vec();
            if !mgmt_ports.contains(&panel_port) {
                mgmt_ports.push(panel_port);
            }

            let plan = if mw_cfg.enabled {
                Some(build_routing_plan_full(
                    active_routes.clone(),
                    lan_cidrs,
                    single_ip_targets,
                    proxy_server_targets,
                    use_statistic,
                    use_ipset,
                    mgmt_ports,
                ))
            } else {
                None
            };

            let prev_spec = rt.last_wan_marks_spec.clone();
            (channels_status, active_routes, dropped, plan, spec, prev_spec)
        };

        // Сбрасываем conntrack только для упавших WAN-каналов
        for (dead_ip, dead_mark) in dropped_wans {
            flush_conntrack_for_dropped_wan(&dead_ip, dead_mark).await;
        }

        // Применяем или идемпотентно чиним Linux-маршрутизацию после сброса ndm
        let mut last_error = None;
        match plan {
            Some(ref new_plan) => {
                let need_apply = {
                    let rt = self.runtime.lock().await;
                    rt.last_plan.as_ref() != Some(new_plan)
                };
                if need_apply {
                    if let Err(e) = apply_routing_plan(new_plan, panel_port).await {
                        crate::log_e!("[MULTI-WAN] Ошибка применения маршрутизации: {e}");
                        last_error = Some(e);
                    } else {
                        self.runtime.lock().await.last_plan = Some(new_plan.clone());
                    }
                } else {
                    let integrity = inspect_routing_integrity(new_plan).await;
                    if !integrity.is_intact() {
                        crate::log_w!(
                            "[MULTI-WAN] Обнаружен сброс маршрутизации ndm ({integrity:?}) — выполняю саморемонт без сброса счётчиков"
                        );
                        if let Err(e) = repair_routing_in_place(new_plan, &integrity, panel_port).await {
                            crate::log_e!("[MULTI-WAN] Ошибка саморемонта маршрутизации: {e}");
                            last_error = Some(e);
                        }
                    }
                }
            }
            None => {
                let had_plan = self.runtime.lock().await.last_plan.take().is_some();
                if had_plan {
                    teardown_routing().await;
                }
            }
        }

        // Без перезапуска csq0: шлём WAN_MARKS| только при изменении
        if wan_marks_spec != prev_spec {
            self.runtime.lock().await.last_wan_marks_spec = wan_marks_spec.clone();
            state.vkturn.send_wan_marks(&wan_marks_spec).await;
        }

        // Плавное авто-масштабирование потоков csqtt-client (гистерезис >90% >30с)
        if mw_cfg.enabled && cfg.vkturn.enabled && cfg.vkturn.streams == 0 {
            let cpu_pct = self.sample_cpu_percent().await;
            let active_len = active_routes.len();
            let fallback_workers = crate::vkturn::effective_workers(&cfg.vkturn) as usize;
            let mut rt = self.runtime.lock().await;
            let desired_workers = rt.cpu_guard.evaluate(active_len, cpu_pct, fallback_workers, now);
            if rt.last_auto_workers != Some(desired_workers) {
                rt.last_auto_workers = Some(desired_workers);
                drop(rt);
                state.vkturn.send_scale_workers(desired_workers).await;
            }
        }

        let total_rx_mbps =
            (channels_status.iter().map(|c| c.rx_mbps).sum::<f64>() * 100.0).round() / 100.0;
        let total_tx_mbps =
            (channels_status.iter().map(|c| c.tx_mbps).sum::<f64>() * 100.0).round() / 100.0;

        let new_status = MultiWanStatus {
            enabled: mw_cfg.enabled,
            mode: mw_cfg.mode,
            single_ip_subscriptions: mw_cfg.single_ip_subscriptions,
            active_count: active_routes.len(),
            total_rx_mbps,
            total_tx_mbps,
            wan_marks_spec,
            gateway_collision_warning,
            last_reconcile_ts: chrono::Utc::now().timestamp(),
            last_error,
            channels: channels_status,
        };
        *self.status.write().await = new_status.clone();
        new_status
    }

    async fn sample_cpu_percent(&self) -> f64 {
        #[cfg(target_os = "linux")]
        {
            if let Ok(raw) = tokio::fs::read_to_string("/proc/stat").await {
                if let Some(line) = raw.lines().next() {
                    let nums: Vec<u64> = line
                        .split_whitespace()
                        .skip(1)
                        .filter_map(|s| s.parse::<u64>().ok())
                        .collect();
                    if nums.len() >= 4 {
                        let idle = nums[3] + nums.get(4).copied().unwrap_or(0);
                        let total: u64 = nums.iter().sum();
                        let mut rt = self.runtime.lock().await;
                        let pct = if let Some((prev_idle, prev_total)) = rt.prev_cpu {
                            let d_total = total.saturating_sub(prev_total);
                            let d_idle = idle.saturating_sub(prev_idle);
                            if d_total > 0 {
                                ((d_total.saturating_sub(d_idle)) as f64 / d_total as f64) * 100.0
                            } else {
                                0.0
                            }
                        } else {
                            0.0
                        };
                        rt.prev_cpu = Some((idle, total));
                        return pct;
                    }
                }
            }
        }
        0.0
    }
}

async fn read_proc_net_dev() -> String {
    #[cfg(target_os = "linux")]
    {
        tokio::fs::read_to_string("/proc/net/dev")
            .await
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        String::new()
    }
}

fn is_usable_proxy_host(raw: &str) -> bool {
    let h = raw.trim().trim_matches(|c| c == '"' || c == '\'');
    if h.is_empty() || h.eq_ignore_ascii_case("localhost") {
        return false;
    }
    if let Ok(ip) = h.parse::<std::net::Ipv4Addr>() {
        return !ip.is_loopback() && !ip.is_private() && !ip.is_unspecified() && !ip.is_link_local();
    }
    !h.contains(' ') && !h.contains(':')
}

/// Извлекает значения `server:` из секции `proxies:` (или файла провайдера `proxies:`), поддерживая flow и block YAML.
pub fn extract_proxy_servers_from_yaml(yaml: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_proxies = false;

    for line in yaml.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 && !trimmed.starts_with('-') {
            in_proxies = trimmed.starts_with("proxies:");
            continue;
        }
        if !in_proxies {
            continue;
        }

        // Flow-формат: - { name: ..., server: example.com, port: 443 }
        if let Some(pos) = trimmed.find("server:") {
            let after = &trimmed[pos + "server:".len()..];
            let val = after
                .split(|c| c == ',' || c == '}' || c == '#')
                .next()
                .unwrap_or("")
                .trim()
                .trim_matches(|c| c == '"' || c == '\'');
            if is_usable_proxy_host(val) && !out.iter().any(|x| x == val) {
                out.push(val.to_string());
            }
        }
    }
    out
}

/// Собирает хосты прокси-серверов из `config.yaml` и `providers/*.yaml`.
/// Возвращает `(all_proxy_hosts, paid_proxy_hosts)`.
pub async fn collect_proxy_hosts_from_files(
    mihomo_config_path: &std::path::Path,
    mobile_provider_urls: &HashMap<String, String>,
) -> (Vec<String>, Vec<String>) {
    let mut all_hosts: Vec<String> = Vec::new();
    let mut paid_hosts: Vec<String> = Vec::new();
    let mut push_host = |h: String, is_paid: bool| {
        if !all_hosts.contains(&h) {
            all_hosts.push(h.clone());
        }
        if is_paid && !paid_hosts.contains(&h) {
            paid_hosts.push(h);
        }
    };

    if let Ok(main_yaml) = tokio::fs::read_to_string(mihomo_config_path).await {
        for h in extract_proxy_servers_from_yaml(&main_yaml) {
            push_host(h, true);
        }
    }

    if let Some(cfg_dir) = mihomo_config_path.parent() {
        let providers_dir = cfg_dir.join("providers");
        if let Ok(mut entries) = tokio::fs::read_dir(&providers_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                if !ext.eq_ignore_ascii_case("yaml") && !ext.eq_ignore_ascii_case("yml") {
                    continue;
                }
                let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if stem == "__free_sub_probe" {
                    continue;
                }
                let is_paid = stem != "free_wl" && stem != "free_vpn" && stem != "vkturn";
                if let Ok(content) = tokio::fs::read_to_string(&path).await {
                    for h in extract_proxy_servers_from_yaml(&content) {
                        push_host(h, is_paid);
                    }
                }
            }
        }
    }

    for (prov_id, url) in mobile_provider_urls {
        if prov_id == "free_wl" || prov_id == "free_vpn" || prov_id == "vkturn" || prov_id == "__free_sub_probe" {
            continue;
        }
        if let Ok(parsed) = reqwest::Url::parse(url) {
            if let Some(h) = parsed.host_str() {
                if is_usable_proxy_host(h) {
                    push_host(h.to_string(), true);
                }
            }
        }
    }

    (all_hosts, paid_hosts)
}

async fn resolve_hosts_cached(
    hosts: &[String],
    mut cache: HashMap<String, (Instant, Vec<String>)>,
    now: Instant,
) -> (HashMap<String, Vec<String>>, HashMap<String, (Instant, Vec<String>)>) {
    let mut resolved_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut to_lookup: Vec<String> = Vec::new();

    for host in hosts.iter().take(48) {
        if let Ok(ip) = host.parse::<std::net::Ipv4Addr>() {
            resolved_map.insert(host.clone(), vec![ip.to_string()]);
            continue;
        }
        if let Some((ts, ips)) = cache.get(host) {
            if now.saturating_duration_since(*ts) < DNS_CACHE_TTL {
                resolved_map.insert(host.clone(), ips.clone());
                continue;
            }
        }
        to_lookup.push(host.clone());
    }

    let futs = to_lookup.into_iter().map(|host| async move {
        let mut ips = Vec::new();
        if let Ok(Ok(addrs)) = tokio::time::timeout(
            Duration::from_millis(800),
            tokio::net::lookup_host((host.as_str(), 443)),
        )
        .await
        {
            for addr in addrs {
                if let std::net::IpAddr::V4(v4) = addr.ip() {
                    let s = v4.to_string();
                    if !ips.contains(&s) {
                        ips.push(s);
                    }
                }
            }
        }
        (host, ips)
    });
    let results = futures_util::future::join_all(futs).await;
    for (host, ips) in results {
        cache.insert(host.clone(), (now, ips.clone()));
        resolved_map.insert(host, ips);
    }

    (resolved_map, cache)
}

fn flatten_resolved_ips(hosts: &[String], map: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut out = Vec::new();
    for h in hosts {
        if let Some(ips) = map.get(h) {
            for ip in ips {
                if !out.contains(ip) {
                    out.push(ip.clone());
                }
            }
        }
    }
    out
}
