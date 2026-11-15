//! Модуль «Подключить бесплатную мульти-подписку в 1 клик» (Обычный VPN / Белые списки РФ).
//!
//! Архитектура:
//! 1. Компактные курируемые источники (зеркала RU / активный туннель / локальный seed-кэш на диске)
//!    с жёсткими лимитами размера ответа, числа записей и таймаута HTTP.
//! 2. Нормализация через `subscription_parser` (только разрешённые поля прокси, без правил/настроек ядра),
//!    отсев запрещённых IP (включая результат DNS-резолва доменов), поддержка портов < 80,
//!    полная дедупликация по всем значимым параметрам и мягкая квота по подсети `/24`.
//! 3. Конвейер проверки (Time-to-First-Working-Node):
//!    - Шаг A: Fast-path по дисковому кэшу (сразу L7 без TCP-предфильтра).
//!    - Шаг B: Дешёвый TCP-предфильтр (только для TCP-транспортов, дедупликация IP:port,
//!      <= 16 сокетов, rate limit, волна 700 мс и повтор 1800 мс, прямой WAN-маршрут).
//!    - Шаг C: Точечная L7-валидация через отдельный служебный провайдер `__free_sub_probe`
//!      (небольшие партии, <= 4 одновременных, HTTPS-проверка с независимой резервной целью).
//! 4. Атомарная публикация в `free_vpn.yaml` / `free_wl.yaml` с сохранением активной сессии
//!    и откатом/сохранением прежней конфигурации при 0 рабочих узлов.

use futures_util::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock, Semaphore};

use crate::config::AppConfig;
use crate::routing::FREE_SUB_PROBE_PROVIDER_ID;
use crate::subscription_parser::{
    is_forbidden_ip, node_dedup_key, parse_subscription_nodes_bounded, proxies_to_mihomo_yaml,
    sanitize_proxy_value,
};
use crate::transaction::ConfigTx;
use crate::{log_i, log_w, mihomo, AppState};

pub const FREE_VPN_PROVIDER_ID: &str = "free_vpn";
pub const FREE_WL_PROVIDER_ID: &str = "free_wl";

pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024; // 2 MiB
pub const MAX_PARSED_NODES: usize = 600;
pub const HTTP_FETCH_TIMEOUT_SECS: u64 = 10;
pub const DNS_RESOLVE_TIMEOUT_MS: u64 = 2500;
pub const SOFT_SUBNET_QUOTA: usize = 2;
pub const TCP_MAX_CONCURRENT: usize = 16;
pub const TCP_RATE_INTERVAL_MS: u64 = 25;
pub const TCP_FAST_TIMEOUT_MS: u64 = 700;
pub const TCP_SLOW_TIMEOUT_MS: u64 = 1800;
pub const TCP_PHASE_BUDGET_SECS: u64 = 12;
pub const L7_MAX_CONCURRENT: usize = 4;
pub const L7_BATCH_SIZE: usize = 8;
pub const L7_TIMEOUT_MS: u64 = 4500;
pub const BACKGROUND_BACKUP_TARGET: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreeSubMode {
    Vpn,
    Whitelist,
}

impl Default for FreeSubMode {
    fn default() -> Self {
        Self::Vpn
    }
}

impl FreeSubMode {
    pub fn provider_id(self) -> &'static str {
        match self {
            Self::Vpn => FREE_VPN_PROVIDER_ID,
            Self::Whitelist => FREE_WL_PROVIDER_ID,
        }
    }

    pub fn display_alias(self) -> &'static str {
        match self {
            Self::Vpn => "Бесплатный VPN (Авто)",
            Self::Whitelist => "Бесплатный обход БС (Авто)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreeSubStage {
    Idle,
    Sources,
    Prefilter,
    L7,
    Publishing,
    Connected,
    Error,
}

impl Default for FreeSubStage {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FreeSubStats {
    pub total_raw: usize,
    pub rejected_invalid: usize,
    pub rejected_dns: usize,
    pub deduplicated: usize,
    pub tcp_checked: usize,
    pub tcp_passed: usize,
    pub tcp_retried: usize,
    pub l7_checked: usize,
    pub l7_passed: usize,
    pub working_count: usize,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FreeSubStatus {
    pub running: bool,
    pub mode: FreeSubMode,
    pub stage: FreeSubStage,
    pub stage_label: String,
    pub ttff_ms: Option<u64>,
    pub stats: FreeSubStats,
    pub active_node: Option<String>,
    pub preserved_active: bool,
    pub source_used: Option<String>,
    pub error: Option<String>,
    pub updated_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedWorkingNode {
    pub proxy: Value,
    pub last_ok_ts: i64,
    pub last_delay_ms: i64,
    pub mode: FreeSubMode,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModeCache {
    pub last_success_ts: Option<i64>,
    pub working_nodes: Vec<CachedWorkingNode>,
    pub seed_nodes: Vec<Value>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FreeSubCache {
    pub vpn: ModeCache,
    pub whitelist: ModeCache,
}

impl FreeSubCache {
    pub fn mode_mut(&mut self, mode: FreeSubMode) -> &mut ModeCache {
        match mode {
            FreeSubMode::Vpn => &mut self.vpn,
            FreeSubMode::Whitelist => &mut self.whitelist,
        }
    }

    pub fn mode_ref(&self, mode: FreeSubMode) -> &ModeCache {
        match mode {
            FreeSubMode::Vpn => &self.vpn,
            FreeSubMode::Whitelist => &self.whitelist,
        }
    }
}

static FREE_SUB_STATUS: std::sync::LazyLock<RwLock<FreeSubStatus>> =
    std::sync::LazyLock::new(|| RwLock::new(FreeSubStatus::default()));
static FREE_SUB_RUN_LOCK: std::sync::LazyLock<Mutex<()>> =
    std::sync::LazyLock::new(|| Mutex::new(()));

pub async fn get_status() -> FreeSubStatus {
    FREE_SUB_STATUS.read().await.clone()
}

async fn update_status(f: impl FnOnce(&mut FreeSubStatus)) {
    let mut guard = FREE_SUB_STATUS.write().await;
    f(&mut guard);
}

/// Компактные курируемые источники (с российскими зеркалами gitverse.ru на первом месте).
struct CuratedSource {
    label: &'static str,
    urls: &'static [&'static str],
}

const VPN_SOURCES: &[CuratedSource] = &[
    CuratedSource {
        label: "GitVerse RU Mirror (VLESS/Reality)",
        urls: &[
            "https://gitverse.ru/api/repos/strider/xkeen-free-subs/raw/branch/main/vpn.txt",
            "https://raw.githubusercontent.com/barry-far/V2ray-Config/main/Splitted-By-Protocol/vless.txt",
        ],
    },
    CuratedSource {
        label: "RU Curated Fallback Mirror",
        urls: &[
            "https://raw.githubusercontent.com/Epodonios/v2ray-configs/main/Splitted-By-Protocol/vless.txt",
            "https://raw.githubusercontent.com/mahdibland/V2RayAggregator/master/sub/splitted/vless.txt",
        ],
    },
];

const WHITELIST_SOURCES: &[CuratedSource] = &[
    CuratedSource {
        label: "GitVerse RU Mirror (Whitelist/SNI)",
        urls: &[
            "https://gitverse.ru/api/repos/strider/xkeen-free-subs/raw/branch/main/whitelist.txt",
            "https://raw.githubusercontent.com/zieng2/wl/main/vless_lite.txt",
        ],
    },
    CuratedSource {
        label: "Whitelist Fallback Mirror",
        urls: &[
            "https://raw.githubusercontent.com/zieng2/wl/main/vless.txt",
        ],
    },
];

pub fn cache_file_path(config_path: &Path) -> PathBuf {
    config_path
        .parent()
        .map(|p| p.join("free_sub_cache.json"))
        .unwrap_or_else(|| PathBuf::from("free_sub_cache.json"))
}

pub async fn load_disk_cache(path: &Path) -> FreeSubCache {
    match tokio::fs::read_to_string(path).await {
        Ok(raw) => serde_json::from_str::<FreeSubCache>(&raw).unwrap_or_default(),
        Err(_) => FreeSubCache::default(),
    }
}

pub async fn save_disk_cache(path: &Path, cache: &FreeSubCache) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    let tmp = path.with_extension("json.tmp");
    let json_str = serde_json::to_string_pretty(cache).map_err(|e| format!("Serialize cache: {e}"))?;
    tokio::fs::write(&tmp, json_str)
        .await
        .map_err(|e| format!("Write cache tmp: {e}"))?;
    tokio::fs::rename(&tmp, path)
        .await
        .map_err(|e| format!("Rename cache: {e}"))?;
    Ok(())
}

/// Потоковая загрузка текста с жёстким лимитом размера и таймаутом.
pub async fn fetch_bounded_text(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
    timeout_secs: u64,
) -> Result<String, String> {
    let mut resp = client
        .get(url)
        .header("User-Agent", "v2rayNG/1.8.23")
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|e| format!("HTTP {url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {} для {url}", resp.status()));
    }
    if let Some(cl) = resp.content_length() {
        if cl > max_bytes as u64 {
            return Err(format!("Превышен лимит размера ответа ({cl} > {max_bytes} байт)"));
        }
    }

    let mut buf = Vec::with_capacity(std::cmp::min(max_bytes, 65536));
    while let Some(chunk) = resp.chunk().await.map_err(|e| format!("Chunk {url}: {e}"))? {
        if buf.len().saturating_add(chunk.len()) > max_bytes {
            return Err(format!("Превышен лимит размера ответа (> {max_bytes} байт)"));
        }
        buf.extend_from_slice(&chunk);
    }

    Ok(String::from_utf8_lossy(&buf).to_string())
}

#[derive(Clone, Debug)]
pub struct CandidateNode {
    pub proxy: Value,
    pub name: String,
    pub server: String,
    pub port: u16,
    pub is_quic_udp: bool,
    pub resolved_addrs: Vec<SocketAddr>,
    pub subnet_key: String,
    pub ru_sni_score: u8,
}

pub fn is_quic_udp_node(v: &Value) -> bool {
    let typ = v
        .get("type")
        .and_then(|t| t.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(typ.as_str(), "hysteria2" | "tuic" | "wireguard") {
        return true;
    }
    let net = v
        .get("network")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    net == "quic"
}

/// Слабый сигнал приоритизации очереди для российских SNI (никогда не используется как жёсткий фильтр).
pub fn ru_sni_priority(v: &Value) -> u8 {
    let mut candidates = Vec::new();
    for key in ["servername", "sni"] {
        if let Some(s) = v.get(key).and_then(|x| x.as_str()) {
            candidates.push(s);
        }
    }
    if let Some(h) = v
        .get("ws-opts")
        .and_then(|o| o.get("headers"))
        .and_then(|h| h.get("Host"))
        .and_then(|s| s.as_str())
    {
        candidates.push(h);
    }
    if let Some(h) = v.get("xhttp-opts").and_then(|o| o.get("host")).and_then(|s| s.as_str()) {
        candidates.push(h);
    }

    for raw in candidates {
        let s = raw.trim().to_ascii_lowercase();
        if s.is_empty() {
            continue;
        }
        if s.ends_with(".ru")
            || s.ends_with(".su")
            || s.ends_with(".xn--p1ai")
            || s.contains("yandex")
            || s.contains("vk.com")
            || s.contains("vk.ru")
            || s.contains("mail.ru")
            || s.contains("ozon")
            || s.contains("wildberries")
            || s.contains("wb.ru")
            || s.contains("sberbank")
            || s.contains("gosuslugi")
            || s.contains("cdnvideo")
            || s.contains("edgecenter")
            || s.contains("selectel")
            || s.contains("timeweb")
        {
            return 1;
        }
    }
    0
}

pub fn subnet_prefix_key(ip: IpAddr) -> String {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            format!("{}.{}.{}.0/24", o[0], o[1], o[2])
        }
        IpAddr::V6(v6) => {
            let s = v6.segments();
            format!("{:x}:{:x}:{:x}::/48", s[0], s[1], s[2])
        }
    }
}

/// DNS-разрешение хоста с обязательной проверкой фактических IP (отбрасывает loopback/LAN/приватные сети).
pub async fn resolve_and_validate_host(server: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    if port == 0 {
        return Err("нулевой порт (0) запрещён".into());
    }
    if let Ok(ip) = server.parse::<IpAddr>() {
        if is_forbidden_ip(ip) {
            return Err(format!("запрещённый IP {ip}"));
        }
        return Ok(vec![SocketAddr::new(ip, port)]);
    }

    let lookup_fut = tokio::net::lookup_host((server, port));
    let addrs: Vec<SocketAddr> = tokio::time::timeout(Duration::from_millis(DNS_RESOLVE_TIMEOUT_MS), lookup_fut)
        .await
        .map_err(|_| format!("таймаут DNS для {server}"))?
        .map_err(|e| format!("DNS ошибка для {server}: {e}"))?
        .collect();

    if addrs.is_empty() {
        return Err(format!("DNS не вернул адресов для {server}"));
    }
    for addr in &addrs {
        if is_forbidden_ip(addr.ip()) {
            return Err(format!("домен {server} разрешился в запрещённый IP {}", addr.ip()));
        }
    }
    // IPv4 первыми (согласовано с поведением Mihomo по умолчанию на Keenetic)
    let mut sorted = addrs;
    sorted.sort_by_key(|a| if a.is_ipv4() { 0 } else { 1 });
    Ok(sorted)
}

/// Разделяет кандидатов на основную очередь (с мягкой квотой по подсети `/24`)
/// и отложенную очередь добора (никогда не удаляет узлы навсегда).
pub fn apply_soft_subnet_quota(
    mut candidates: Vec<CandidateNode>,
    quota_per_subnet: usize,
    prefer_ru_sni: bool,
) -> (Vec<CandidateNode>, Vec<CandidateNode>) {
    if prefer_ru_sni {
        candidates.sort_by(|a, b| b.ru_sni_score.cmp(&a.ru_sni_score));
    }

    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut primary = Vec::new();
    let mut deferred = Vec::new();

    for c in candidates {
        let cnt = counts.entry(c.subnet_key.clone()).or_insert(0);
        if *cnt < quota_per_subnet {
            *cnt += 1;
            primary.push(c);
        } else {
            deferred.push(c);
        }
    }

    (primary, deferred)
}

/// Дедупликация распарсенных и санитайзированных прокси по полному набору значимых параметров.
pub fn deduplicate_proxies(proxies: Vec<Value>) -> (Vec<Value>, usize) {
    let mut seen = HashSet::new();
    let mut out = Vec::with_capacity(proxies.len());
    let mut dup_count = 0usize;
    for p in proxies {
        let key = node_dedup_key(&p);
        if seen.insert(key) {
            out.push(p);
        } else {
            dup_count += 1;
        }
    }
    (out, dup_count)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TcpOutcome {
    Ok,
    TimedOutFast,
    Failed,
}

async fn tcp_probe_endpoint(addr: SocketAddr, timeout_ms: u64) -> TcpOutcome {
    match tokio::time::timeout(Duration::from_millis(timeout_ms), tokio::net::TcpStream::connect(addr)).await {
        Ok(Ok(_stream)) => TcpOutcome::Ok,
        Ok(Err(_)) => TcpOutcome::Failed,
        Err(_) => TcpOutcome::TimedOutFast,
    }
}

/// Фаза 1: дешёвый TCP-предфильтр только для TCP-транспортов с дедупликацией по `IP:port`,
/// лимитом сокетов (16), rate limit и общим бюджетом времени.
async fn run_tcp_prefilter_endpoints(
    endpoints: Vec<SocketAddr>,
    timeout_ms: u64,
    max_concurrent: usize,
    rate_interval_ms: u64,
    budget: Duration,
) -> HashMap<SocketAddr, TcpOutcome> {
    let mut unique_eps = Vec::new();
    let mut seen = HashSet::new();
    for ep in endpoints {
        if seen.insert(ep) {
            unique_eps.push(ep);
        }
    }

    let sem = Arc::new(Semaphore::new(max_concurrent));
    let started = Instant::now();
    let mut handles = Vec::with_capacity(unique_eps.len());
    let mut ticker = tokio::time::interval(Duration::from_millis(rate_interval_ms));

    for ep in unique_eps {
        if started.elapsed() >= budget {
            break;
        }
        ticker.tick().await;
        let Ok(permit) = sem.clone().acquire_owned().await else {
            break;
        };
        handles.push(tokio::spawn(async move {
            let _p = permit;
            let res = tcp_probe_endpoint(ep, timeout_ms).await;
            (ep, res)
        }));
    }

    let mut outcomes = HashMap::new();
    for h in handles {
        if let Ok((ep, res)) = h.await {
            outcomes.insert(ep, res);
        }
    }
    outcomes
}

/// Гарантирует уникальные имена узлов внутри временного/рабочего YAML-файла.
fn ensure_unique_node_names(nodes: &mut [Value]) {
    let mut used = HashSet::new();
    for (idx, node) in nodes.iter_mut().enumerate() {
        if let Some(obj) = node.as_object_mut() {
            let base = obj
                .get("name")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("FreeNode")
                .to_string();
            let mut candidate = base.clone();
            let mut suffix = 2usize;
            while !used.insert(candidate.clone()) {
                candidate = format!("{base} #{suffix}");
                suffix += 1;
            }
            if candidate.is_empty() {
                candidate = format!("FreeNode-{}", idx + 1);
            }
            obj.insert("name".into(), Value::String(candidate));
        }
    }
}

fn providers_dir(cfg: &AppConfig) -> PathBuf {
    Path::new(&cfg.mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| PathBuf::from("/opt/etc/mihomo/providers"))
}

/// Убеждается, что служебный провайдер `__free_sub_probe` зарегистрирован в `config.yaml` (вне рабочих групп).
async fn ensure_service_probe_provider(state: &AppState) -> Result<(), String> {
    let cfg = state.config.read().await.clone();
    let probe_path = providers_dir(&cfg).join(format!("{FREE_SUB_PROBE_PROVIDER_ID}.yaml"));
    if let Some(parent) = probe_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if !probe_path.exists() {
        let placeholder = "proxies:\n  - name: \"__probe_placeholder\"\n    type: direct\n";
        let _ = tokio::fs::write(&probe_path, placeholder).await;
    }

    let yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    if crate::routing::parse_provider_names(&yaml)
        .iter()
        .any(|p| p.eq_ignore_ascii_case(FREE_SUB_PROBE_PROVIDER_ID))
    {
        return Ok(());
    }

    let mut tx = ConfigTx::begin(state).await?;
    let cur_yaml = tx.read_yaml().await?;
    let new_yaml = crate::routing::add_service_file_provider_to_yaml(
        &cur_yaml,
        FREE_SUB_PROBE_PROVIDER_ID,
        &format!("./providers/{FREE_SUB_PROBE_PROVIDER_ID}.yaml"),
    )?;
    tx.set_yaml(new_yaml)?;
    tx.commit_and_reload().await?;
    Ok(())
}

/// Фаза 2: точечная L7-валидация партии кандидатов через отдельный служебный провайдер `__free_sub_probe`.
async fn run_l7_batch_probe(
    state: &AppState,
    batch: &[Value],
    stop_after_first: bool,
) -> Vec<(Value, i64)> {
    if batch.is_empty() {
        return Vec::new();
    }
    let cfg = state.config.read().await.clone();
    let mut prepared = batch.to_vec();
    ensure_unique_node_names(&mut prepared);

    let yaml = match proxies_to_mihomo_yaml(&prepared) {
        Ok(y) => y,
        Err(_) => return Vec::new(),
    };
    let probe_path = providers_dir(&cfg).join(format!("{FREE_SUB_PROBE_PROVIDER_ID}.yaml"));
    if crate::api::servers::subscriptions::write_mobile_provider_yaml_atomic(&probe_path, &yaml)
        .await
        .is_err()
    {
        return Vec::new();
    }
    if mihomo::force_update_provider(&state.http, &cfg, FREE_SUB_PROBE_PROVIDER_ID)
        .await
        .is_err()
    {
        return Vec::new();
    }

    let mut verified = Vec::new();
    let mut futs = stream::iter(prepared.into_iter().map(|node| {
        let http = state.http.clone();
        let cfg_c = cfg.clone();
        async move {
            let name = node
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let res = mihomo::probe_service_provider_node_l7(
                &http,
                &cfg_c,
                FREE_SUB_PROBE_PROVIDER_ID,
                &name,
                L7_TIMEOUT_MS,
                None,
                None,
            )
            .await;
            (node, res)
        }
    }))
    .buffer_unordered(L7_MAX_CONCURRENT);

    while let Some((node, res)) = futs.next().await {
        update_status(|st| st.stats.l7_checked += 1).await;
        if let Ok(delay) = res {
            update_status(|st| st.stats.l7_passed += 1).await;
            verified.push((node, delay));
            if stop_after_first {
                break;
            }
        }
    }
    verified
}

/// Сохраняет активный узел пользователя с прежним именем и параметрами, если он принадлежит целевой подписке.
pub fn merge_verified_with_active_session(
    verified: Vec<Value>,
    existing_nodes: &[Value],
    active_leaf: Option<&str>,
) -> (Vec<Value>, bool) {
    let mut out = Vec::new();
    let mut seen_keys = HashSet::new();
    let mut preserved = false;

    if let Some(active_name) = active_leaf.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(active_node) = existing_nodes
            .iter()
            .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(active_name))
        {
            seen_keys.insert(node_dedup_key(active_node));
            out.push(active_node.clone());
            preserved = true;
        }
    }

    for node in verified {
        let key = node_dedup_key(&node);
        if seen_keys.insert(key) {
            out.push(node);
        }
    }
    ensure_unique_node_names(&mut out);
    (out, preserved)
}

/// Извлекает узлы из существующего YAML-файла провайдера (для сохранения активной сессии).
pub fn parse_existing_provider_nodes(yaml_content: &str) -> Vec<Value> {
    // Простой построчный разбор `proxies:` из ранее сгенерированного YAML-файла провайдера.
    let mut out = Vec::new();
    let mut cur: Option<serde_json::Map<String, Value>> = None;
    let mut sub_key: Option<String> = None;
    let mut sub_map = serde_json::Map::new();

    let flush_sub = |cur: &mut Option<serde_json::Map<String, Value>>,
                     sub_key: &mut Option<String>,
                     sub_map: &mut serde_json::Map<String, Value>| {
        if let (Some(obj), Some(sk)) = (cur.as_mut(), sub_key.take()) {
            if !sub_map.is_empty() {
                obj.insert(sk, Value::Object(std::mem::take(sub_map)));
            }
        }
    };

    for line in yaml_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "proxies:" {
            continue;
        }
        if line.starts_with("  - ") {
            flush_sub(&mut cur, &mut sub_key, &mut sub_map);
            if let Some(obj) = cur.take() {
                if let Some(clean) = sanitize_proxy_value(&Value::Object(obj)) {
                    out.push(clean);
                }
            }
            let mut m = serde_json::Map::new();
            if let Some((k, v)) = trimmed.trim_start_matches("- ").split_once(':') {
                let val_s = v.trim().trim_matches('"').trim_matches('\'').to_string();
                m.insert(k.trim().to_string(), Value::String(val_s));
            }
            cur = Some(m);
            continue;
        }
        if line.starts_with("      ") {
            if let Some((k, v)) = trimmed.split_once(':') {
                let val_raw = v.trim().trim_matches('"').trim_matches('\'');
                let val = if val_raw == "true" {
                    Value::Bool(true)
                } else if val_raw == "false" {
                    Value::Bool(false)
                } else {
                    Value::String(val_raw.to_string())
                };
                sub_map.insert(k.trim().to_string(), val);
            }
            continue;
        }
        if line.starts_with("    ") {
            flush_sub(&mut cur, &mut sub_key, &mut sub_map);
            if let Some(obj) = cur.as_mut() {
                if let Some((k, v)) = trimmed.split_once(':') {
                    let key = k.trim().to_string();
                    let v_trim = v.trim();
                    if v_trim.is_empty() {
                        sub_key = Some(key);
                    } else {
                        let val_clean = v_trim.trim_matches('"').trim_matches('\'');
                        let val = if key == "port" || key == "alterId" {
                            val_clean.parse::<u64>().map(|n| json!(n)).unwrap_or_else(|_| Value::String(val_clean.to_string()))
                        } else if val_clean == "true" {
                            Value::Bool(true)
                        } else if val_clean == "false" {
                            Value::Bool(false)
                        } else {
                            Value::String(val_clean.to_string())
                        };
                        obj.insert(key, val);
                    }
                }
            }
        }
    }
    flush_sub(&mut cur, &mut sub_key, &mut sub_map);
    if let Some(obj) = cur.take() {
        if let Some(clean) = sanitize_proxy_value(&Value::Object(obj)) {
            out.push(clean);
        }
    }
    out
}

/// Атомарная публикация проверенного набора в рабочий провайдер (`free_vpn` / `free_wl`)
/// и подключение к первому рабочему узлу (если текущая сессия не активна на узле этой подписки).
async fn publish_and_connect(
    state: &AppState,
    mode: FreeSubMode,
    verified_nodes: Vec<Value>,
    connect_node_name: Option<String>,
) -> Result<(String, bool), String> {
    let provider_id = mode.provider_id();
    let cfg = state.config.read().await.clone();
    let prov_dir = providers_dir(&cfg);
    let prov_path = prov_dir.join(format!("{provider_id}.yaml"));

    let existing_nodes = match tokio::fs::read_to_string(&prov_path).await {
        Ok(raw) => parse_existing_provider_nodes(&raw),
        Err(_) => Vec::new(),
    };

    let active_srv = mihomo::get_active_server(&state.http, &cfg).await.ok().flatten();
    let active_belongs_to_sub = active_srv.as_ref().is_some_and(|s| {
        s.provider.as_deref() == Some(provider_id)
            || existing_nodes
                .iter()
                .any(|n| n.get("name").and_then(|v| v.as_str()) == Some(s.id.as_str()))
    });
    let active_leaf_opt = if active_belongs_to_sub {
        active_srv.as_ref().map(|s| s.id.as_str())
    } else {
        None
    };

    let (final_nodes, preserved_active) =
        merge_verified_with_active_session(verified_nodes, &existing_nodes, active_leaf_opt);

    if final_nodes.is_empty() {
        return Err("Список проверенных узлов пуст; прежняя конфигурация сохранена".into());
    }

    let yaml = proxies_to_mihomo_yaml(&final_nodes)?;
    crate::api::servers::subscriptions::write_mobile_provider_yaml_atomic(&prov_path, &yaml).await?;

    // Регистрация провайдера в config.yaml при первом запуске
    let cur_yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let needs_registration = !crate::routing::parse_provider_names(&cur_yaml)
        .iter()
        .any(|p| p.eq_ignore_ascii_case(provider_id));
    let needs_wl_mark = mode == FreeSubMode::Whitelist
        && !cfg
            .whitelist_providers
            .iter()
            .any(|p| p.eq_ignore_ascii_case(provider_id));

    if needs_registration || needs_wl_mark {
        let mut tx = ConfigTx::begin(state).await?;
        let mut y = tx.read_yaml().await?;
        if needs_registration {
            y = crate::routing::add_file_provider_to_yaml(
                &y,
                provider_id,
                &format!("./providers/{provider_id}.yaml"),
                Some(tx.config().health_check_url()),
                Some(tx.config().mihomo.health_check_interval),
            )?;
            tx.config_mut()
                .provider_aliases
                .insert(provider_id.to_string(), mode.display_alias().to_string());
        }
        if needs_wl_mark {
            tx.config_mut().whitelist_providers.push(provider_id.to_string());
        }
        let (routed, _) = crate::routing::apply_routing(&y, tx.config())?;
        tx.set_yaml(routed)?;
        tx.commit_and_reload().await?;
    }

    let updated_cfg = state.config.read().await.clone();
    let _ = mihomo::force_update_provider(&state.http, &updated_cfg, provider_id).await;

    let target_name = if preserved_active {
        active_leaf_opt.unwrap_or_default().to_string()
    } else {
        connect_node_name
            .or_else(|| {
                final_nodes
                    .first()
                    .and_then(|n| n.get("name"))
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_default()
    };

    if !preserved_active && !target_name.is_empty() {
        let _ = mihomo::switch_server(&state.http, &updated_cfg, &target_name).await;
    }

    Ok((target_name, preserved_active))
}

/// Загрузка кандидатов из источников (RU-зеркала -> активный туннель -> резервный seed-кэш на диске).
async fn fetch_sources_or_cache(
    state: &AppState,
    cfg: &AppConfig,
    mode: FreeSubMode,
    cache: &mut FreeSubCache,
) -> (Vec<Value>, Option<String>, usize) {
    let sources = match mode {
        FreeSubMode::Vpn => VPN_SOURCES,
        FreeSubMode::Whitelist => WHITELIST_SOURCES,
    };
    let proxied = crate::updater::proxied_client(&cfg.mihomo_proxy_url());
    let mut collected = Vec::new();
    let mut rejected_invalid = 0usize;
    let mut used_source: Option<String> = None;

    for src in sources {
        if collected.len() >= MAX_PARSED_NODES {
            break;
        }
        for url in src.urls {
            // 1) Прямой запрос (в т.ч. к российскому зеркалу gitverse.ru)
            let mut body_res = fetch_bounded_text(&state.http, url, MAX_RESPONSE_BYTES, HTTP_FETCH_TIMEOUT_SECS).await;
            // 2) Если напрямую не вышло и поднят активный туннель — пробуем через него
            if body_res.is_err() {
                if let Some(ref p_client) = proxied {
                    body_res = fetch_bounded_text(p_client, url, MAX_RESPONSE_BYTES, HTTP_FETCH_TIMEOUT_SECS).await;
                }
            }
            if let Ok(text) = body_res {
                let remaining = MAX_PARSED_NODES.saturating_sub(collected.len());
                let (nodes, rejected) = parse_subscription_nodes_bounded(&text, remaining);
                rejected_invalid += rejected.len();
                if !nodes.is_empty() {
                    collected.extend(nodes);
                    if used_source.is_none() {
                        used_source = Some(src.label.to_string());
                    }
                    break;
                }
            }
        }
    }

    if !collected.is_empty() {
        let now_ts = chrono::Utc::now().timestamp();
        let mc = cache.mode_mut(mode);
        mc.last_success_ts = Some(now_ts);
        mc.seed_nodes = collected.iter().take(200).cloned().collect();
        return (collected, used_source, rejected_invalid);
    }

    // 3) Локальный резервный seed-кэш на диске
    let mc = cache.mode_ref(mode);
    if !mc.seed_nodes.is_empty() {
        let sanitized: Vec<Value> = mc.seed_nodes.iter().filter_map(sanitize_proxy_value).collect();
        if !sanitized.is_empty() {
            return (sanitized, Some("Локальный seed-кэш".to_string()), rejected_invalid);
        }
    }

    (Vec::new(), None, rejected_invalid)
}

/// Запуск полного конвейера подключения бесплатной мульти-подписки в 1 клик.
pub async fn start_one_click_connect(state: AppState, mode: FreeSubMode) -> Result<FreeSubStatus, String> {
    let _guard = FREE_SUB_RUN_LOCK
        .try_lock()
        .map_err(|_| "Поиск и подключение бесплатной подписки уже выполняется".to_string())?;

    let started = Instant::now();
    update_status(|st| {
        *st = FreeSubStatus {
            running: true,
            mode,
            stage: FreeSubStage::Sources,
            stage_label: "Загрузка компактных источников и кэша…".to_string(),
            updated_at: Some(chrono::Utc::now().timestamp()),
            ..Default::default()
        };
    })
    .await;

    let cfg = state.config.read().await.clone();
    let cache_path = cache_file_path(&state.config_path);
    let mut cache = load_disk_cache(&cache_path).await;

    // Убеждаемся, что служебный провайдер __free_sub_probe готов
    if let Err(e) = ensure_service_probe_provider(&state).await {
        log_w!("[FREE-SUB] Не удалось подготовить служебный провайдер: {e}");
    }

    // Шаг A: Fast-path по ранее работавшим узлам из кэша (сразу L7 без TCP-предфильтра)
    let cached_working: Vec<Value> = cache
        .mode_ref(mode)
        .working_nodes
        .iter()
        .take(4)
        .filter_map(|w| sanitize_proxy_value(&w.proxy))
        .collect();

    let mut first_verified: Option<(Value, i64)> = None;
    if !cached_working.is_empty() {
        update_status(|st| {
            st.stage = FreeSubStage::L7;
            st.stage_label = "Быстрая L7-проверка ранее работавших узлов из кэша…".to_string();
        })
        .await;
        let fast_res = run_l7_batch_probe(&state, &cached_working, true).await;
        if let Some((node, delay)) = fast_res.into_iter().next() {
            first_verified = Some((node, delay));
        }
    }

    if let Some((first_node, first_delay)) = first_verified {
        let ttff = started.elapsed().as_millis() as u64;
        update_status(|st| {
            st.ttff_ms = Some(ttff);
            st.stage = FreeSubStage::Publishing;
            st.stage_label = "Публикация и подключение рабочего узла из кэша…".to_string();
        })
        .await;
        let first_name = first_node.get("name").and_then(|v| v.as_str()).map(str::to_string);
        if let Ok((active_name, preserved)) = publish_and_connect(&state, mode, vec![first_node.clone()], first_name).await {
            update_status(|st| {
                st.running = false;
                st.stage = FreeSubStage::Connected;
                st.stage_label = format!("Подключено ({active_name}) за {ttff} мс");
                st.active_node = Some(active_name);
                st.preserved_active = preserved;
                st.stats.working_count = 1;
                st.source_used = Some("Быстрый кэш".to_string());
                st.updated_at = Some(chrono::Utc::now().timestamp());
            })
            .await;
            let _ = first_delay;
            return Ok(get_status().await);
        }
    }

    // Загрузка источников / резервного seed-кэша
    update_status(|st| {
        st.stage = FreeSubStage::Sources;
        st.stage_label = "Загрузка источников (RU-зеркала / туннель / seed-кэш)…".to_string();
    })
    .await;

    let (raw_nodes, source_used, rejected_invalid) = fetch_sources_or_cache(&state, &cfg, mode, &mut cache).await;
    let total_raw = raw_nodes.len();
    let (deduped_nodes, dup_count) = deduplicate_proxies(raw_nodes);
    update_status(|st| {
        st.source_used = source_used.clone();
        st.stats.total_raw = total_raw;
        st.stats.rejected_invalid = rejected_invalid;
        st.stats.deduplicated = dup_count;
    })
    .await;

    if deduped_nodes.is_empty() {
        let msg = "Не удалось получить ни одного валидного узла из источников и локального кэша. Прежняя конфигурация сохранена.".to_string();
        update_status(|st| {
            st.running = false;
            st.stage = FreeSubStage::Error;
            st.stage_label = "Нет доступных узлов".to_string();
            st.error = Some(msg.clone());
            st.updated_at = Some(chrono::Utc::now().timestamp());
        })
        .await;
        return Err(msg);
    }

    // DNS-резолв + отбрасывание loopback/LAN/приватных IP
    update_status(|st| {
        st.stage = FreeSubStage::Prefilter;
        st.stage_label = format!("DNS-проверка и предфильтр ({} кандидатов)…", deduped_nodes.len());
    })
    .await;

    let mut candidates = Vec::new();
    let mut rejected_dns = 0usize;
    let dns_futs = stream::iter(deduped_nodes.into_iter().map(|proxy| async move {
        let server = proxy.get("server").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let port = proxy.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
        let name = proxy.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let res = resolve_and_validate_host(&server, port).await;
        (proxy, name, server, port, res)
    }))
    .buffer_unordered(16);

    let dns_results: Vec<_> = dns_futs.collect().await;
    for (proxy, name, server, port, res) in dns_results {
        match res {
            Ok(addrs) => {
                let subnet_key = addrs
                    .first()
                    .map(|a| subnet_prefix_key(a.ip()))
                    .unwrap_or_else(|| "unknown".into());
                candidates.push(CandidateNode {
                    is_quic_udp: is_quic_udp_node(&proxy),
                    ru_sni_score: ru_sni_priority(&proxy),
                    proxy,
                    name,
                    server,
                    port,
                    resolved_addrs: addrs,
                    subnet_key,
                });
            }
            Err(_) => {
                rejected_dns += 1;
            }
        }
    }
    update_status(|st| st.stats.rejected_dns = rejected_dns).await;

    // Мягкая квота /24 (диверсификация порядка очереди, без потери узлов)
    let (primary_q, deferred_q) = apply_soft_subnet_quota(
        candidates,
        SOFT_SUBNET_QUOTA,
        mode == FreeSubMode::Whitelist,
    );
    let ordered_candidates: Vec<CandidateNode> = primary_q.into_iter().chain(deferred_q).collect();

    // Разделение: QUIC/UDP минуют TCP-предфильтр, TCP проходят через TCP-предфильтр
    let mut l7_ready_proxies: Vec<Value> = Vec::new();
    let mut tcp_candidates: Vec<CandidateNode> = Vec::new();
    for c in ordered_candidates {
        if c.is_quic_udp {
            l7_ready_proxies.push(c.proxy);
        } else {
            tcp_candidates.push(c);
        }
    }

    // Шаг B: Волна 1 TCP-предфильтра (700 мс, 16 сокетов, rate limit)
    let tcp_endpoints_wave1: Vec<SocketAddr> = tcp_candidates
        .iter()
        .filter_map(|c| c.resolved_addrs.first().copied())
        .collect();
    let wave1_outcomes = run_tcp_prefilter_endpoints(
        tcp_endpoints_wave1,
        TCP_FAST_TIMEOUT_MS,
        TCP_MAX_CONCURRENT,
        TCP_RATE_INTERVAL_MS,
        Duration::from_secs(TCP_PHASE_BUDGET_SECS),
    )
    .await;

    let mut tcp_passed_count = 0usize;
    let mut timed_out_candidates = Vec::new();
    for c in &tcp_candidates {
        if let Some(ep) = c.resolved_addrs.first() {
            match wave1_outcomes.get(ep) {
                Some(TcpOutcome::Ok) => {
                    tcp_passed_count += 1;
                    l7_ready_proxies.push(c.proxy.clone());
                }
                Some(TcpOutcome::TimedOutFast) => {
                    timed_out_candidates.push(c.clone());
                }
                _ => {}
            }
        }
    }
    update_status(|st| {
        st.stats.tcp_checked = wave1_outcomes.len();
        st.stats.tcp_passed = tcp_passed_count;
    })
    .await;

    // Шаг C: L7-проверка партиями до первого рабочего узла
    update_status(|st| {
        st.stage = FreeSubStage::L7;
        st.stage_label = format!("Точечная L7-проверка ({} кандидатов)…", l7_ready_proxies.len());
    })
    .await;

    let mut verified_nodes: Vec<(Value, i64)> = Vec::new();
    let mut remaining_for_bg: Vec<Value> = Vec::new();

    for (batch_idx, chunk) in l7_ready_proxies.chunks(L7_BATCH_SIZE).enumerate() {
        if verified_nodes.is_empty() {
            let res = run_l7_batch_probe(&state, chunk, true).await;
            if !res.is_empty() {
                verified_nodes.extend(res);
                // Остальные партии пойдут в фон для добора 3-5 резервов
                let rest_start = (batch_idx + 1) * L7_BATCH_SIZE;
                if rest_start < l7_ready_proxies.len() {
                    remaining_for_bg.extend_from_slice(&l7_ready_proxies[rest_start..]);
                }
                break;
            }
        }
    }

    // Повторная волна TCP (>= 1500 мс) если не найдено ни одного рабочего узла
    if verified_nodes.is_empty() && !timed_out_candidates.is_empty() {
        update_status(|st| {
            st.stage = FreeSubStage::Prefilter;
            st.stage_label = format!(
                "Повторный TCP-предфильтр с увеличенным таймаутом ({} узлов)…",
                timed_out_candidates.len()
            );
        })
        .await;
        let retry_eps: Vec<SocketAddr> = timed_out_candidates
            .iter()
            .filter_map(|c| c.resolved_addrs.first().copied())
            .collect();
        let wave2_outcomes = run_tcp_prefilter_endpoints(
            retry_eps,
            TCP_SLOW_TIMEOUT_MS,
            TCP_MAX_CONCURRENT,
            TCP_RATE_INTERVAL_MS,
            Duration::from_secs(TCP_PHASE_BUDGET_SECS),
        )
        .await;
        let mut retry_l7 = Vec::new();
        for c in &timed_out_candidates {
            if let Some(ep) = c.resolved_addrs.first() {
                if wave2_outcomes.get(ep) == Some(&TcpOutcome::Ok) {
                    retry_l7.push(c.proxy.clone());
                }
            }
        }
        update_status(|st| {
            st.stats.tcp_retried = wave2_outcomes.len();
            st.stats.tcp_passed += retry_l7.len();
            st.stage = FreeSubStage::L7;
            st.stage_label = format!("L7-проверка после повтора ({} кандидатов)…", retry_l7.len());
        })
        .await;

        for (batch_idx, chunk) in retry_l7.chunks(L7_BATCH_SIZE).enumerate() {
            let res = run_l7_batch_probe(&state, chunk, true).await;
            if !res.is_empty() {
                verified_nodes.extend(res);
                let rest_start = (batch_idx + 1) * L7_BATCH_SIZE;
                if rest_start < retry_l7.len() {
                    remaining_for_bg.extend_from_slice(&retry_l7[rest_start..]);
                }
                break;
            }
        }
    }

    if verified_nodes.is_empty() {
        let _ = save_disk_cache(&cache_path, &cache).await;
        let msg = "Ни один узел не прошёл L7-проверку. Предыдущая конфигурация сохранена без изменений.".to_string();
        update_status(|st| {
            st.running = false;
            st.stage = FreeSubStage::Error;
            st.stage_label = "Рабочие узлы не найдены".to_string();
            st.error = Some(msg.clone());
            st.updated_at = Some(chrono::Utc::now().timestamp());
        })
        .await;
        return Err(msg);
    }

    let ttff = started.elapsed().as_millis() as u64;
    update_status(|st| {
        st.ttff_ms = Some(ttff);
        st.stage = FreeSubStage::Publishing;
        st.stage_label = "Атомарная публикация и подключение…".to_string();
    })
    .await;

    let now_ts = chrono::Utc::now().timestamp();
    for (node, delay) in &verified_nodes {
        let mc = cache.mode_mut(mode);
        mc.working_nodes.retain(|w| node_dedup_key(&w.proxy) != node_dedup_key(node));
        mc.working_nodes.insert(
            0,
            CachedWorkingNode {
                proxy: node.clone(),
                last_ok_ts: now_ts,
                last_delay_ms: *delay,
                mode,
            },
        );
        mc.working_nodes.truncate(15);
    }
    let _ = save_disk_cache(&cache_path, &cache).await;

    let first_proxies: Vec<Value> = verified_nodes.iter().map(|(n, _)| n.clone()).collect();
    let connect_name = first_proxies
        .first()
        .and_then(|n| n.get("name"))
        .and_then(|v| v.as_str())
        .map(str::to_string);

    let (active_name, preserved) = publish_and_connect(&state, mode, first_proxies.clone(), connect_name).await?;
    log_i!("[FREE-SUB] ✓ Подключён рабочий узел '{active_name}' за {ttff} мс (mode={mode:?})");

    update_status(|st| {
        st.running = false;
        st.stage = FreeSubStage::Connected;
        st.stage_label = format!("Подключено ({active_name}) за {ttff} мс");
        st.active_node = Some(active_name);
        st.preserved_active = preserved;
        st.stats.working_count = first_proxies.len();
        st.updated_at = Some(chrono::Utc::now().timestamp());
    })
    .await;

    // Фоновый добор 3-5 резервных узлов без блокировки пользователя
    if !remaining_for_bg.is_empty() {
        let bg_state = state.clone();
        let bg_cache_path = cache_path.clone();
        tokio::spawn(async move {
            let mut extra = first_proxies;
            for chunk in remaining_for_bg.chunks(L7_BATCH_SIZE).take(3) {
                if extra.len() >= BACKGROUND_BACKUP_TARGET {
                    break;
                }
                let res = run_l7_batch_probe(&bg_state, chunk, false).await;
                for (n, _) in res {
                    if extra.len() >= BACKGROUND_BACKUP_TARGET {
                        break;
                    }
                    extra.push(n);
                }
            }
            if extra.len() > 1 {
                let count = extra.len();
                if publish_and_connect(&bg_state, mode, extra.clone(), None).await.is_ok() {
                    let mut c = load_disk_cache(&bg_cache_path).await;
                    let ts = chrono::Utc::now().timestamp();
                    for n in &extra {
                        let mc = c.mode_mut(mode);
                        if !mc.working_nodes.iter().any(|w| node_dedup_key(&w.proxy) == node_dedup_key(n)) {
                            mc.working_nodes.push(CachedWorkingNode {
                                proxy: n.clone(),
                                last_ok_ts: ts,
                                last_delay_ms: 100,
                                mode,
                            });
                        }
                    }
                    c.mode_mut(mode).working_nodes.truncate(15);
                    let _ = save_disk_cache(&bg_cache_path, &c).await;
                    update_status(|st| st.stats.working_count = count).await;
                    log_i!("[FREE-SUB] Фоновый добор завершён: {count} рабочих узлов");
                }
            }
        });
    }

    Ok(get_status().await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subscription_parser::parse_vless_uri;

    #[tokio::test]
    async fn test_1_rejects_loopback_lan_private_ips_and_allows_ports_below_80() {
        // 1a. Запрещённые IPv4/IPv6 и нулевой порт отклоняются
        assert!(is_forbidden_ip("127.0.0.1".parse().unwrap()));
        assert!(is_forbidden_ip("0.0.0.0".parse().unwrap()));
        assert!(is_forbidden_ip("10.0.0.1".parse().unwrap()));
        assert!(is_forbidden_ip("172.16.5.10".parse().unwrap()));
        assert!(is_forbidden_ip("192.168.1.1".parse().unwrap()));
        assert!(is_forbidden_ip("169.254.1.1".parse().unwrap()));
        assert!(is_forbidden_ip("100.64.0.1".parse().unwrap()));
        assert!(is_forbidden_ip("::1".parse().unwrap()));
        assert!(is_forbidden_ip("fe80::1".parse().unwrap()));
        assert!(!is_forbidden_ip("8.8.8.8".parse().unwrap()));

        // 1b. DNS-резолв localhost должен отклоняться как loopback
        let dns_local = resolve_and_validate_host("localhost", 443).await;
        assert!(dns_local.is_err(), "localhost must be rejected after DNS resolution");

        // 1c. Порты < 80 (например, 22 и 53) РАЗРЕШЕНЫ, порт 0 запрещён
        let raw = "vless://11111111-1111-1111-1111-111111111111@1.1.1.1:22?security=reality&pbk=k1&sid=s1#Port22\n\
                   vless://11111111-1111-1111-1111-111111111111@8.8.8.8:53?security=tls&sni=example.com#Port53\n\
                   vless://11111111-1111-1111-1111-111111111111@127.0.0.1:443#Loopback\n\
                   vless://11111111-1111-1111-1111-111111111111@192.168.1.1:443#LAN\n\
                   vless://11111111-1111-1111-1111-111111111111@9.9.9.9:0#PortZero\n";
        let (nodes, rejected) = parse_subscription_nodes_bounded(raw, 50);
        assert_eq!(nodes.len(), 2, "ports < 80 (22, 53) must be kept, loopback/LAN/port-0 rejected");
        assert_eq!(nodes[0]["port"], 22);
        assert_eq!(nodes[1]["port"], 53);
        assert_eq!(rejected.len(), 3);
    }

    #[test]
    fn test_2_full_vless_deduplication_across_significant_fields() {
        let base = "vless://11111111-1111-1111-1111-111111111111@198.51.100.10:443?security=reality&sni=ya.ru&pbk=PUB1&sid=aa11&flow=xtls-rprx-vision&type=tcp#A";
        let dup_name_only = "vless://11111111-1111-1111-1111-111111111111@198.51.100.10:443?security=reality&sni=ya.ru&pbk=PUB1&sid=aa11&flow=xtls-rprx-vision&type=tcp#B_DifferentName";
        let diff_sid = "vless://11111111-1111-1111-1111-111111111111@198.51.100.10:443?security=reality&sni=ya.ru&pbk=PUB1&sid=bb22&flow=xtls-rprx-vision&type=tcp#DiffSid";
        let diff_flow = "vless://11111111-1111-1111-1111-111111111111@198.51.100.10:443?security=reality&sni=ya.ru&pbk=PUB1&sid=aa11&type=tcp#DiffFlow";
        let diff_pbk = "vless://11111111-1111-1111-1111-111111111111@198.51.100.10:443?security=reality&sni=ya.ru&pbk=PUB2&sid=aa11&flow=xtls-rprx-vision&type=tcp#DiffPbk";
        let diff_sni = "vless://11111111-1111-1111-1111-111111111111@198.51.100.10:443?security=reality&sni=vk.com&pbk=PUB1&sid=aa11&flow=xtls-rprx-vision&type=tcp#DiffSni";
        let diff_transport_ws = "vless://11111111-1111-1111-1111-111111111111@198.51.100.10:443?security=tls&sni=ya.ru&type=ws&path=/ws1#DiffWs1";
        let diff_path_ws = "vless://11111111-1111-1111-1111-111111111111@198.51.100.10:443?security=tls&sni=ya.ru&type=ws&path=/ws2#DiffWs2";

        let list: Vec<Value> = [
            base,
            dup_name_only,
            diff_sid,
            diff_flow,
            diff_pbk,
            diff_sni,
            diff_transport_ws,
            diff_path_ws,
        ]
        .iter()
        .filter_map(|u| parse_vless_uri(u))
        .collect();

        let (deduped, dup_count) = deduplicate_proxies(list);
        assert_eq!(dup_count, 1, "only exact duplicate with different fragment name must be collapsed");
        assert_eq!(deduped.len(), 7, "all variants differing in sid/flow/pbk/sni/transport/path must be kept");
    }

    #[test]
    fn test_3_soft_subnet_quota_diversifies_and_never_discards() {
        let make_cand = |name: &str, ip: &str, ru_score: u8| CandidateNode {
            proxy: json!({"name": name, "type": "vless", "server": ip, "port": 443}),
            name: name.to_string(),
            server: ip.to_string(),
            port: 443,
            is_quic_udp: false,
            resolved_addrs: vec![SocketAddr::new(ip.parse().unwrap(), 443)],
            subnet_key: subnet_prefix_key(ip.parse().unwrap()),
            ru_sni_score: ru_score,
        };

        let cands = vec![
            make_cand("S1-a", "45.10.10.1", 0),
            make_cand("S1-b", "45.10.10.2", 0),
            make_cand("S1-c", "45.10.10.3", 0),
            make_cand("S1-d", "45.10.10.4", 0),
            make_cand("S2-a", "46.20.20.1", 1),
        ];

        let (primary, deferred) = apply_soft_subnet_quota(cands, 2, true);
        assert_eq!(primary.len(), 3, "primary must take 2 from 45.10.10.0/24 and 1 from 46.20.20.0/24");
        assert_eq!(primary[0].name, "S2-a", "RU SNI weak signal prioritizes S2-a in primary queue");
        assert_eq!(deferred.len(), 2, "excess nodes from saturated /24 go to deferred queue, not discarded");
        assert!(TCP_SLOW_TIMEOUT_MS >= 1500, "retry timeout must be >= 1500ms (RFC 6298 initial RTO)");
    }

    #[test]
    fn test_4_l7_healthcheck_response_strict_validation() {
        // Отказ при HTTP != 2xx
        assert!(mihomo::parse_l7_healthcheck_response(reqwest::StatusCode::GATEWAY_TIMEOUT, r#"{"delay":120}"#).is_err());
        // Отказ при ошибке в JSON даже при 200 OK
        assert!(mihomo::parse_l7_healthcheck_response(reqwest::StatusCode::OK, r#"{"message":"status code mismatch"}"#).is_err());
        // Отказ при нулевом delay
        assert!(mihomo::parse_l7_healthcheck_response(reqwest::StatusCode::OK, r#"{"delay":0}"#).is_err());
        // Успех только при 2xx и delay > 0 без ошибки
        assert_eq!(
            mihomo::parse_l7_healthcheck_response(reqwest::StatusCode::OK, r#"{"delay":87}"#).unwrap(),
            87
        );
    }

    #[test]
    fn test_5_preserve_active_session_and_service_provider_isolation() {
        let existing = vec![
            json!({"name": "ActiveFreeNode", "type": "vless", "server": "1.2.3.4", "port": 443, "uuid": "u-active"}),
            json!({"name": "OldNode", "type": "vless", "server": "5.6.7.8", "port": 443, "uuid": "u-old"}),
        ];
        let verified = vec![
            json!({"name": "NewFreshNode", "type": "vless", "server": "9.9.9.9", "port": 443, "uuid": "u-new"}),
        ];

        let (merged, preserved) = merge_verified_with_active_session(verified, &existing, Some("ActiveFreeNode"));
        assert!(preserved, "active node from this subscription must be preserved");
        assert_eq!(merged[0]["name"], "ActiveFreeNode");
        assert_eq!(merged[0]["uuid"], "u-active");
        assert_eq!(merged.len(), 2);

        // Служебный провайдер __free_sub_probe не попадает в parse_user_provider_names
        let yaml = "proxy-providers:\n  __free_sub_probe:\n    type: file\n    path: ./providers/__free_sub_probe.yaml\n  free_wl:\n    type: file\n    path: ./providers/free_wl.yaml\n";
        let user_provs = crate::routing::parse_user_provider_names(yaml);
        assert_eq!(user_provs, vec!["free_wl".to_string()]);
    }
}
