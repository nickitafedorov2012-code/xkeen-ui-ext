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
use std::sync::atomic::{AtomicU64, Ordering};
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
use crate::{log_e, log_i, log_w, mihomo, AppState};

pub const FREE_VPN_PROVIDER_ID: &str = "free_vpn";
pub const FREE_WL_PROVIDER_ID: &str = "free_wl";

pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024; // 2 MiB
pub const MAX_PARSED_NODES: usize = 1200;
pub const MAX_PARSED_NODES_PER_SOURCE: usize = 450;
pub const HTTP_FETCH_TIMEOUT_SECS: u64 = 10;
pub const DNS_RESOLVE_TIMEOUT_MS: u64 = 2500;
pub const SOFT_SUBNET_QUOTA: usize = 2;
pub const TCP_MAX_CONCURRENT: usize = 16;
pub const TCP_RATE_INTERVAL_MS: u64 = 25;
pub const TCP_FAST_TIMEOUT_MS: u64 = 700;
pub const TCP_SLOW_TIMEOUT_MS: u64 = 1800;
pub const TCP_PHASE_BUDGET_SECS: u64 = 6;
pub const L7_MAX_CONCURRENT: usize = 8;
pub const L7_BATCH_SIZE: usize = 8;
pub const L7_TIMEOUT_MS: u64 = 3500;
pub const L7_FAST_ACCEPT_SCORE_MS: i64 = 1200;
pub const L7_PENALIZED_GRACE_MS: u64 = 650;
pub const FOREGROUND_L7_MAX_CANDIDATES: usize = 32;
pub const FOREGROUND_L7_BUDGET_SECS: u64 = 20;
pub const BG_L7_MAX_CANDIDATES: usize = 96;
pub const BG_L7_BUDGET_SECS: u64 = 60;
pub const L7_RECENT_FAIL_TTL_SECS: i64 = 600;
pub const BG_REFILL_FAILURE_COOLDOWN_SECS: i64 = 600;
pub const BACKGROUND_BACKUP_TARGET: usize = 20;
pub const BACKGROUND_REFILL_BUDGET_SECS: u64 = 180;
pub const BLACKLIST_TTL_SECS: i64 = 86_400;
pub const WATCHDOG_BLACKLIST_TTL_SECS: i64 = 600;
pub const MAX_PER_ENDPOINT_IN_POOL: usize = 2;
pub const REAL_TRAFFIC_PRIORITY_BYTES: u64 = 1_048_576;
pub const MAX_SESSION_HISTORY: usize = 20;

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
    #[serde(default)]
    pub quality_score: i64,
    pub mode: FreeSubMode,
    #[serde(default)]
    pub high_priority: bool,
    #[serde(default)]
    pub total_bytes: u64,
}

impl CachedWorkingNode {
    pub fn effective_quality_score(&self) -> i64 {
        if self.quality_score > 0 {
            self.quality_score
        } else {
            self.last_delay_ms
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FreeSubSessionRecord {
    pub id: String,
    pub mode: FreeSubMode,
    pub node_name: String,
    pub host: String,
    pub port: u16,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub duration_secs: u64,
    pub down_bytes: u64,
    pub up_bytes: u64,
    pub end_reason: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ModeCache {
    pub last_success_ts: Option<i64>,
    pub working_nodes: Vec<CachedWorkingNode>,
    pub seed_nodes: Vec<Value>,
    #[serde(default)]
    pub blacklist: HashMap<String, i64>,
    #[serde(default)]
    pub sessions: Vec<FreeSubSessionRecord>,
    #[serde(default)]
    pub high_priority_keys: HashMap<String, u64>,
    #[serde(default)]
    pub last_selected_node: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct FreeSubCache {
    pub vpn: ModeCache,
    pub whitelist: ModeCache,
    #[serde(default)]
    pub previous_active_server: Option<String>,
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

static FREE_VPN_STATUS: std::sync::LazyLock<RwLock<FreeSubStatus>> =
    std::sync::LazyLock::new(|| RwLock::new(FreeSubStatus::default()));
static FREE_WL_STATUS: std::sync::LazyLock<RwLock<FreeSubStatus>> =
    std::sync::LazyLock::new(|| RwLock::new(FreeSubStatus { mode: FreeSubMode::Whitelist, ..Default::default() }));
static FREE_VPN_RUN_LOCK: std::sync::LazyLock<Arc<Mutex<()>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(())));
static FREE_WL_RUN_LOCK: std::sync::LazyLock<Arc<Mutex<()>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(())));
static L7_RECENT_FAILS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, i64>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static LAST_BG_REFILL_FAIL_TS: std::sync::LazyLock<std::sync::Mutex<HashMap<String, i64>>> =
    std::sync::LazyLock::new(|| std::sync::Mutex::new(HashMap::new()));
static FREE_VPN_BG_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static FREE_WL_BG_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn bg_active_flag(mode: FreeSubMode) -> &'static std::sync::atomic::AtomicBool {
    match mode {
        FreeSubMode::Vpn => &FREE_VPN_BG_ACTIVE,
        FreeSubMode::Whitelist => &FREE_WL_BG_ACTIVE,
    }
}

pub fn is_bg_refill_active(mode: FreeSubMode) -> bool {
    bg_active_flag(mode).load(Ordering::Acquire)
}

struct BgRefillActiveGuard(FreeSubMode);
impl BgRefillActiveGuard {
    fn acquire(mode: FreeSubMode) -> Self {
        bg_active_flag(mode).store(true, Ordering::Release);
        Self(mode)
    }
}
impl Drop for BgRefillActiveGuard {
    fn drop(&mut self) {
        bg_active_flag(self.0).store(false, Ordering::Release);
    }
}

fn record_l7_recent_fail(node: &Value, now_ts: i64) {
    if let Some(ep) = endpoint_blacklist_key(node) {
        let mut guard = L7_RECENT_FAILS.lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|_, ts| now_ts.saturating_sub(*ts) < L7_RECENT_FAIL_TTL_SECS);
        guard.insert(ep, now_ts);
    }
}

fn clear_l7_recent_fail(node: &Value) {
    if let Some(ep) = endpoint_blacklist_key(node) {
        let mut guard = L7_RECENT_FAILS.lock().unwrap_or_else(|e| e.into_inner());
        guard.remove(&ep);
    }
}

/// Стабильно сдвигает узлы, недавно провалившие L7-проверку (`< L7_RECENT_FAIL_TTL_SECS`), в хвост очереди.
pub fn demote_recent_l7_failed_proxies(
    nodes: Vec<Value>,
    recent_fails: &HashMap<String, i64>,
    now_ts: i64,
) -> Vec<Value> {
    if recent_fails.is_empty() || nodes.len() <= 1 {
        return nodes;
    }
    let mut fresh = Vec::with_capacity(nodes.len());
    let mut failed = Vec::new();
    for node in nodes {
        let recently_failed = endpoint_blacklist_key(&node)
            .and_then(|ep| recent_fails.get(&ep).copied())
            .is_some_and(|ts| now_ts.saturating_sub(ts) < L7_RECENT_FAIL_TTL_SECS);
        if recently_failed {
            failed.push(node);
        } else {
            fresh.push(node);
        }
    }
    fresh.extend(failed);
    fresh
}

fn demote_recent_l7_failed_with_store(nodes: Vec<Value>, now_ts: i64) -> Vec<Value> {
    let snapshot = {
        let mut guard = L7_RECENT_FAILS.lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|_, ts| now_ts.saturating_sub(*ts) < L7_RECENT_FAIL_TTL_SECS);
        guard.clone()
    };
    demote_recent_l7_failed_proxies(nodes, &snapshot, now_ts)
}

fn demote_recent_l7_failed_candidates(
    cands: Vec<CandidateNode>,
    now_ts: i64,
) -> Vec<CandidateNode> {
    let snapshot = {
        let mut guard = L7_RECENT_FAILS.lock().unwrap_or_else(|e| e.into_inner());
        guard.retain(|_, ts| now_ts.saturating_sub(*ts) < L7_RECENT_FAIL_TTL_SECS);
        guard.clone()
    };
    if snapshot.is_empty() || cands.len() <= 1 {
        return cands;
    }
    let mut fresh = Vec::with_capacity(cands.len());
    let mut failed = Vec::new();
    for c in cands {
        let recently_failed = endpoint_blacklist_key(&c.proxy)
            .and_then(|ep| snapshot.get(&ep).copied())
            .is_some_and(|ts| now_ts.saturating_sub(ts) < L7_RECENT_FAIL_TTL_SECS);
        if recently_failed {
            failed.push(c);
        } else {
            fresh.push(c);
        }
    }
    fresh.extend(failed);
    fresh
}

fn record_bg_refill_failure(mode: FreeSubMode, now_ts: i64) {
    let mut guard = LAST_BG_REFILL_FAIL_TS.lock().unwrap_or_else(|e| e.into_inner());
    guard.insert(mode.provider_id().to_string(), now_ts);
}

fn clear_bg_refill_failure(mode: FreeSubMode) {
    let mut guard = LAST_BG_REFILL_FAIL_TS.lock().unwrap_or_else(|e| e.into_inner());
    guard.remove(mode.provider_id());
}

fn is_bg_refill_in_cooldown(mode: FreeSubMode, now_ts: i64) -> bool {
    let guard = LAST_BG_REFILL_FAIL_TS.lock().unwrap_or_else(|e| e.into_inner());
    guard
        .get(mode.provider_id())
        .is_some_and(|&ts| now_ts.saturating_sub(ts) < BG_REFILL_FAILURE_COOLDOWN_SECS)
}

fn status_lock(mode: FreeSubMode) -> &'static RwLock<FreeSubStatus> {
    match mode {
        FreeSubMode::Vpn => &FREE_VPN_STATUS,
        FreeSubMode::Whitelist => &FREE_WL_STATUS,
    }
}

fn run_lock(mode: FreeSubMode) -> &'static Arc<Mutex<()>> {
    match mode {
        FreeSubMode::Vpn => &FREE_VPN_RUN_LOCK,
        FreeSubMode::Whitelist => &FREE_WL_RUN_LOCK,
    }
}

pub async fn get_status_for(mode: FreeSubMode) -> FreeSubStatus {
    status_lock(mode).read().await.clone()
}

pub async fn get_all_statuses() -> (FreeSubStatus, FreeSubStatus) {
    let vpn = FREE_VPN_STATUS.read().await.clone();
    let wl = FREE_WL_STATUS.read().await.clone();
    (vpn, wl)
}

pub fn pick_primary_status(vpn: &FreeSubStatus, wl: &FreeSubStatus) -> FreeSubStatus {
    if wl.running && !vpn.running {
        return wl.clone();
    }
    if vpn.running && !wl.running {
        return vpn.clone();
    }
    let wl_ts = wl.updated_at.unwrap_or(0);
    let vpn_ts = vpn.updated_at.unwrap_or(0);
    if wl_ts > vpn_ts {
        wl.clone()
    } else if vpn_ts > wl_ts {
        vpn.clone()
    } else if wl.stage != FreeSubStage::Idle {
        wl.clone()
    } else {
        vpn.clone()
    }
}

pub async fn get_status() -> FreeSubStatus {
    let (vpn, wl) = get_all_statuses().await;
    pick_primary_status(&vpn, &wl)
}

async fn update_status(mode: FreeSubMode, f: impl FnOnce(&mut FreeSubStatus)) {
    let mut guard = status_lock(mode).write().await;
    f(&mut guard);
    guard.mode = mode;
}

/// Компактные курируемые источники (с CDN-зеркалами jsDelivr и прямыми raw-ссылками).
struct CuratedSource {
    label: &'static str,
    urls: &'static [&'static str],
}

const VPN_SOURCES: &[CuratedSource] = &[
    CuratedSource {
        label: "VLESS/Reality CDN Mirror",
        urls: &[
            "https://cdn.jsdelivr.net/gh/barry-far/V2ray-Config@main/Splitted-By-Protocol/vless.txt",
            "https://raw.githubusercontent.com/barry-far/V2ray-Config/main/Splitted-By-Protocol/vless.txt",
        ],
    },
    CuratedSource {
        label: "Curated Fallback Mirror",
        urls: &[
            "https://cdn.jsdelivr.net/gh/peasoft/NoMoreWalls@master/list_raw.txt",
            "https://raw.githubusercontent.com/peasoft/NoMoreWalls/master/list_raw.txt",
        ],
    },
];

const EMBEDDED_VPN_SEED: &str = include_str!("embedded_vpn_seed.txt");
const EMBEDDED_WHITELIST_SEED: &str = include_str!("embedded_whitelist_seed.txt");

const WHITELIST_SOURCES: &[CuratedSource] = &[
    CuratedSource {
        label: "Yandex Disk БС-зеркало (Документ)",
        urls: &[
            "https://disk.yandex.ru/i/xXnBLHAuIfFpSA",
        ],
    },
    CuratedSource {
        label: "Whitelist/SNI CDN Mirror",
        urls: &[
            "https://cdn.jsdelivr.net/gh/zieng2/wl@main/vless_universal.txt",
            "https://cdn.jsdelivr.net/gh/zieng2/wl@main/vless_lite.txt",
            "https://raw.githubusercontent.com/zieng2/wl/main/vless_universal.txt",
            "https://raw.githubusercontent.com/zieng2/wl/main/vless_lite.txt",
        ],
    },
    CuratedSource {
        label: "Whitelist Fallback Mirror",
        urls: &[
            "https://cdn.jsdelivr.net/gh/igareck/vpn-configs-for-russia@main/Vless-Reality-White-Lists-Rus-Mobile.txt",
            "https://raw.githubusercontent.com/igareck/vpn-configs-for-russia/main/Vless-Reality-White-Lists-Rus-Mobile.txt",
        ],
    },
];

static FREE_SUB_CACHE_LOCK: std::sync::LazyLock<Mutex<()>> =
    std::sync::LazyLock::new(|| Mutex::new(()));
static FREE_SUB_PROBE_LOCK: std::sync::LazyLock<Mutex<()>> =
    std::sync::LazyLock::new(|| Mutex::new(()));
static FREE_VPN_GEN: AtomicU64 = AtomicU64::new(1);
static FREE_WL_GEN: AtomicU64 = AtomicU64::new(1);

fn mode_generation_counter(mode: FreeSubMode) -> &'static AtomicU64 {
    match mode {
        FreeSubMode::Vpn => &FREE_VPN_GEN,
        FreeSubMode::Whitelist => &FREE_WL_GEN,
    }
}

fn current_mode_generation(mode: FreeSubMode) -> u64 {
    mode_generation_counter(mode).load(Ordering::Acquire)
}

fn bump_mode_generation(mode: FreeSubMode) -> u64 {
    mode_generation_counter(mode).fetch_add(1, Ordering::AcqRel) + 1
}

/// Атомарное чтение-изменение-запись дискового кэша `free_sub_cache.json` под сериализующей блокировкой (R12).
async fn update_disk_cache<F, R>(path: &Path, f: F) -> Result<R, String>
where
    F: FnOnce(&mut FreeSubCache) -> R,
{
    let _guard = FREE_SUB_CACHE_LOCK.lock().await;
    let mut cache = load_disk_cache(path).await;
    let res = f(&mut cache);
    save_disk_cache(path, &cache).await?;
    Ok(res)
}

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
    let json_str = serde_json::to_string_pretty(cache).map_err(|e| format!("Serialize cache: {e}"))?;
    crate::api::atomic_write_file(path, &json_str).await
}

pub fn is_yandex_disk_public_url(url: &str) -> bool {
    let lower = url.trim().to_ascii_lowercase();
    lower.starts_with("https://disk.yandex.ru/")
        || lower.starts_with("https://disk.yandex.com/")
        || lower.starts_with("https://yadi.sk/")
}

fn trim_partial_utf8_lines(buf: &[u8]) -> Option<Vec<u8>> {
    if buf.len() < 16 * 1024 || buf.starts_with(b"PK\x03\x04") {
        return None;
    }
    let last_nl = buf.iter().rposition(|&b| b == b'\n')?;
    if last_nl == 0 {
        return None;
    }
    Some(buf[..=last_nl].to_vec())
}

async fn fetch_bounded_bytes(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
    timeout_secs: u64,
) -> Result<Vec<u8>, String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(timeout_secs);
    let mut resp = tokio::time::timeout_at(
        deadline,
        client
            .get(url)
            .header("User-Agent", "v2rayNG/1.8.23")
            .send(),
    )
    .await
    .map_err(|_| format!("HTTP {url}: timeout"))?
    .map_err(|e| format!("HTTP {url}: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("HTTP {} для {url}", resp.status()));
    }
    let is_docx = url.to_ascii_lowercase().contains(".docx")
        || is_yandex_disk_public_url(url);
    if is_docx {
        if let Some(cl) = resp.content_length() {
            if cl > max_bytes as u64 {
                return Err(format!("Превышен лимит размера ответа ({cl} > {max_bytes} байт)"));
            }
        }
    }

    let cap = if is_docx { max_bytes } else { max_bytes.min(512 * 1024) };
    let mut buf = Vec::with_capacity(std::cmp::min(cap, 65536));
    loop {
        match tokio::time::timeout_at(deadline, resp.chunk()).await {
            Ok(Ok(Some(chunk))) => {
                if buf.len().saturating_add(chunk.len()) > cap {
                    let take = cap.saturating_sub(buf.len());
                    buf.extend_from_slice(&chunk[..take]);
                    if !is_docx {
                        if let Some(prefix) = trim_partial_utf8_lines(&buf) {
                            return Ok(prefix);
                        }
                    }
                    return Err(format!("Превышен лимит размера ответа (> {cap} байт)"));
                }
                buf.extend_from_slice(&chunk);
            }
            Ok(Ok(None)) => return Ok(buf),
            Ok(Err(e)) => {
                if !is_docx {
                    if let Some(prefix) = trim_partial_utf8_lines(&buf) {
                        return Ok(prefix);
                    }
                }
                return Err(format!("Chunk {url}: {e}"));
            }
            Err(_) => {
                if !is_docx {
                    if let Some(prefix) = trim_partial_utf8_lines(&buf) {
                        return Ok(prefix);
                    }
                }
                return Err(format!("HTTP {url}: timeout"));
            }
        }
    }
}

async fn resolve_yandex_disk_download_url(
    client: &reqwest::Client,
    public_url: &str,
    timeout_secs: u64,
) -> Result<String, String> {
    let resp = client
        .get("https://cloud-api.yandex.net/v1/disk/public/resources/download")
        .query(&[("public_key", public_url.trim())])
        .header("User-Agent", "v2rayNG/1.8.23")
        .timeout(Duration::from_secs(timeout_secs))
        .send()
        .await
        .map_err(|e| format!("Yandex Disk API: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("Yandex Disk API HTTP {}", resp.status()));
    }
    let val: Value = resp
        .json()
        .await
        .map_err(|e| format!("Yandex Disk JSON: {e}"))?;
    val.get("href")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "В ответе Yandex Disk API отсутствует href".to_string())
}

fn read_u16_le(buf: &[u8], off: usize) -> Option<u16> {
    let slice = buf.get(off..off + 2)?;
    Some(u16::from_le_bytes([slice[0], slice[1]]))
}

fn read_u32_le(buf: &[u8], off: usize) -> Option<u32> {
    let slice = buf.get(off..off + 4)?;
    Some(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
}

/// Извлекает содержимое одного файла из ZIP-архива через Central Directory.
fn extract_zip_entry(zip_bytes: &[u8], target_name: &str, max_uncompressed: usize) -> Result<Vec<u8>, String> {
    use std::io::Read;

    if zip_bytes.len() < 22 {
        return Err("Файл слишком мал для ZIP".into());
    }
    let search_start = zip_bytes.len().saturating_sub(65535 + 22);
    let mut eocd_off = None;
    for i in (search_start..=zip_bytes.len() - 22).rev() {
        if zip_bytes.get(i..i + 4) == Some(b"PK\x05\x06") {
            eocd_off = Some(i);
            break;
        }
    }
    let eocd = eocd_off.ok_or_else(|| "Не найден заголовок EOCD ZIP".to_string())?;
    let total_entries = read_u16_le(zip_bytes, eocd + 10).ok_or("Некорректный EOCD")? as usize;
    let cd_size = read_u32_le(zip_bytes, eocd + 12).ok_or("Некорректный размер CD")? as usize;
    let cd_offset = read_u32_le(zip_bytes, eocd + 16).ok_or("Некорректное смещение CD")? as usize;
    let cd_end = cd_offset
        .checked_add(cd_size)
        .filter(|&e| e <= zip_bytes.len())
        .ok_or_else(|| "Границы Central Directory выходят за пределы архива".to_string())?;

    let mut pos = cd_offset;
    for _ in 0..total_entries {
        if pos + 46 > cd_end || zip_bytes.get(pos..pos + 4) != Some(b"PK\x01\x02") {
            break;
        }
        let method = read_u16_le(zip_bytes, pos + 10).ok_or("ZIP method")?;
        let comp_size = read_u32_le(zip_bytes, pos + 20).ok_or("ZIP comp_size")? as usize;
        let uncomp_size = read_u32_le(zip_bytes, pos + 24).ok_or("ZIP uncomp_size")? as usize;
        let name_len = read_u16_le(zip_bytes, pos + 28).ok_or("ZIP name_len")? as usize;
        let extra_len = read_u16_le(zip_bytes, pos + 30).ok_or("ZIP extra_len")? as usize;
        let comment_len = read_u16_le(zip_bytes, pos + 32).ok_or("ZIP comment_len")? as usize;
        let local_off = read_u32_le(zip_bytes, pos + 42).ok_or("ZIP local_off")? as usize;

        let name_start = pos + 46;
        let name_end = name_start + name_len;
        if name_end > cd_end {
            break;
        }
        let name = std::str::from_utf8(&zip_bytes[name_start..name_end]).unwrap_or("");
        pos = name_end + extra_len + comment_len;

        if name != target_name {
            continue;
        }
        if uncomp_size > max_uncompressed {
            return Err("Превышен лимит распакованного DOCX XML".into());
        }
        if local_off + 30 > zip_bytes.len() || zip_bytes.get(local_off..local_off + 4) != Some(b"PK\x03\x04") {
            return Err("Некорректный Local File Header в ZIP".into());
        }
        let l_name_len = read_u16_le(zip_bytes, local_off + 26).ok_or("LFH name_len")? as usize;
        let l_extra_len = read_u16_le(zip_bytes, local_off + 28).ok_or("LFH extra_len")? as usize;
        let data_start = local_off + 30 + l_name_len + l_extra_len;
        let data_end = data_start
            .checked_add(comp_size)
            .filter(|&e| e <= zip_bytes.len())
            .ok_or_else(|| "Сжатые данные выходят за границы файла".to_string())?;
        let raw_slice = &zip_bytes[data_start..data_end];

        return match method {
            0 => Ok(raw_slice.to_vec()),
            8 => {
                let mut decoder = flate2::read::DeflateDecoder::new(raw_slice);
                let mut out = Vec::new();
                let mut limited = decoder.by_ref().take((max_uncompressed + 1) as u64);
                limited
                    .read_to_end(&mut out)
                    .map_err(|e| format!("Ошибка распаковки Deflate: {e}"))?;
                if out.len() > max_uncompressed {
                    return Err("Превышен лимит распакованного DOCX XML".into());
                }
                Ok(out)
            }
            other => Err(format!("Неподдерживаемый метод сжатия ZIP: {other}")),
        };
    }

    Err(format!("Файл {target_name} не найден в архиве"))
}

fn unescape_xml_entities(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp..];
        if let Some(semi) = after.find(';') {
            let ent = &after[1..semi];
            match ent {
                "amp" => out.push('&'),
                "lt" => out.push('<'),
                "gt" => out.push('>'),
                "quot" => out.push('"'),
                "apos" => out.push('\''),
                _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                    if let Ok(cp) = u32::from_str_radix(&ent[2..], 16) {
                        if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                    }
                }
                _ if ent.starts_with('#') => {
                    if let Ok(cp) = ent[1..].parse::<u32>() {
                        if let Some(ch) = char::from_u32(cp) {
                            out.push(ch);
                        }
                    }
                }
                _ => {
                    out.push('&');
                    out.push_str(ent);
                    out.push(';');
                }
            }
            rest = &after[semi + 1..];
        } else {
            out.push_str(after);
            return out;
        }
    }
    out.push_str(rest);
    out
}

/// Извлекает чистый текст построчно из `.docx` (`word/document.xml`).
pub fn extract_docx_text(docx_bytes: &[u8]) -> Result<String, String> {
    let xml_bytes = extract_zip_entry(docx_bytes, "word/document.xml", 4 * 1024 * 1024)?;
    let xml = String::from_utf8_lossy(&xml_bytes);
    let mut out = String::new();
    let mut pos = 0usize;
    let bytes = xml.as_bytes();

    while pos < bytes.len() {
        if let Some(rel_lt) = xml[pos..].find('<') {
            pos += rel_lt;
            let Some(rel_gt) = xml[pos..].find('>') else {
                break;
            };
            let tag = &xml[pos + 1..pos + rel_gt];
            pos += rel_gt + 1;

            let tag_name = tag
                .split_whitespace()
                .next()
                .unwrap_or("")
                .trim_end_matches('/');
            if tag_name == "w:t" && !tag.ends_with('/') {
                if let Some(end_rel) = xml[pos..].find("</w:t>") {
                    let raw_text = &xml[pos..pos + end_rel];
                    out.push_str(&unescape_xml_entities(raw_text));
                    pos += end_rel + "</w:t>".len();
                }
            } else if tag_name == "/w:p" || tag_name == "w:br" || tag_name == "w:cr" {
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
        } else {
            break;
        }
    }
    Ok(out)
}

/// Декодирует скачанные байты подписки: автоматически распознаёт `.docx` (ZIP) и обычный текст.
pub fn decode_subscription_bytes(bytes: &[u8]) -> String {
    if bytes.starts_with(b"PK\x03\x04") {
        if let Ok(text) = extract_docx_text(bytes) {
            return text;
        }
    }
    String::from_utf8_lossy(bytes).to_string()
}

/// Потоковая загрузка текста (поддерживает прямые URL, публичные ссылки Яндекс Диска и `.docx` / `.txt`).
pub async fn fetch_bounded_text(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
    timeout_secs: u64,
) -> Result<String, String> {
    let target_url = if is_yandex_disk_public_url(url) {
        resolve_yandex_disk_download_url(client, url, timeout_secs).await?
    } else {
        url.to_string()
    };
    let buf = fetch_bounded_bytes(client, &target_url, max_bytes, timeout_secs).await?;
    Ok(decode_subscription_bytes(&buf))
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

fn contains_word_token(haystack_lower: &str, token: &str) -> bool {
    let mut start = 0;
    while let Some(pos) = haystack_lower[start..].find(token) {
        let abs = start + pos;
        let before_ok = abs == 0
            || !haystack_lower[..abs]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_ascii_alphanumeric());
        let after = abs + token.len();
        let after_ok = after >= haystack_lower.len()
            || !haystack_lower[after..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphanumeric());
        if before_ok && after_ok {
            return true;
        }
        start = abs + token.len();
    }
    false
}

/// Мягкий приоритет БС-кандидатов (никогда не отфильтровывает остальные узлы):
/// повышает ранг узлам с RU-SNI, `xhttp` и БС-маркерами (`VK`, `CIDR`, `Beeline`, `MTS`, `Megafon`, `Yandex`, `🇷🇺`).
pub fn bs_candidate_priority(v: &Value) -> u8 {
    let mut score = 0u8;
    if ru_sni_priority(v) > 0 {
        score = score.saturating_add(2);
    }
    let net = v
        .get("network")
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if net == "xhttp" || v.get("xhttp-opts").is_some() {
        score = score.saturating_add(2);
    }
    let name_raw = v.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let name_low = name_raw.to_ascii_lowercase();
    let server_low = v
        .get("server")
        .and_then(|s| s.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    let has_bs_marker = name_raw.contains("🇷🇺")
        || name_low.contains("cidr")
        || name_low.contains("beeline")
        || name_low.contains("megafon")
        || name_low.contains("yandex")
        || name_low.contains("anycast")
        || contains_word_token(&name_low, "vk")
        || contains_word_token(&name_low, "mts")
        || server_low.ends_with(".ru")
        || server_low.ends_with(".su")
        || server_low.contains("yandex")
        || server_low.contains("beeline")
        || server_low.contains("megafon")
        || server_low.contains("vk.com")
        || server_low.contains("vk.ru");
    if has_bs_marker {
        score = score.saturating_add(1);
    }
    score
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
    run_tcp_prefilter_endpoints_budgeted(
        endpoints,
        timeout_ms,
        max_concurrent,
        rate_interval_ms,
        budget,
        None,
    )
    .await
}

async fn run_tcp_prefilter_endpoints_budgeted(
    endpoints: Vec<SocketAddr>,
    timeout_ms: u64,
    max_concurrent: usize,
    rate_interval_ms: u64,
    budget: Duration,
    early_stop_ok: Option<usize>,
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
    let mut futs = futures_util::stream::FuturesUnordered::new();
    let mut ticker = tokio::time::interval(Duration::from_millis(rate_interval_ms));
    let mut outcomes = HashMap::new();
    let mut ok_count = 0usize;

    for ep in unique_eps {
        if started.elapsed() >= budget {
            break;
        }
        if let Some(target_ok) = early_stop_ok {
            if ok_count >= target_ok && started.elapsed() >= Duration::from_millis(1200) {
                break;
            }
        }
        ticker.tick().await;
        while let Some(joined) = futures_util::FutureExt::now_or_never(futs.next()).flatten() {
            if let Ok((done_ep, res)) = joined {
                if res == TcpOutcome::Ok {
                    ok_count += 1;
                }
                outcomes.insert(done_ep, res);
            }
        }
        let Ok(permit) = sem.clone().acquire_owned().await else {
            break;
        };
        futs.push(tokio::spawn(async move {
            let _p = permit;
            let res = tcp_probe_endpoint(ep, timeout_ms).await;
            (ep, res)
        }));
    }

    while let Some(joined) = futs.next().await {
        if let Ok((ep, res)) = joined {
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

/// Stability Gate: проверяет узел двумя последовательными L7-запросами (например, 204 + payload).
pub async fn verify_node_stability<F1, F2, Fut1, Fut2>(
    probe_204: F1,
    probe_payload: F2,
) -> Result<i64, String>
where
    F1: FnOnce() -> Fut1,
    F2: FnOnce() -> Fut2,
    Fut1: std::future::Future<Output = Result<i64, String>>,
    Fut2: std::future::Future<Output = Result<i64, String>>,
{
    let d1 = probe_204().await?;
    let d2 = probe_payload().await?;
    Ok((d1 + d2) / 2)
}

/// Сортирует результаты L7-проверки партии по возрастанию задержки.
pub fn sort_l7_verified_by_delay(verified: &mut [(Value, i64)]) {
    verified.sort_by_key(|(_, d)| *d);
}

/// Приоритет L7-очереди: сначала TCP-кандидаты (с `ru_sni_score == 1` на самом верху),
/// и только после них `is_quic_udp` (`hysteria2`/`tuic`) с сохранением приоритета RU-SNI.
pub fn prioritize_l7_ready_proxies(
    mut tcp_passed: Vec<CandidateNode>,
    mut quic_udp: Vec<CandidateNode>,
) -> Vec<Value> {
    tcp_passed.sort_by_key(|c| std::cmp::Reverse(c.ru_sni_score));
    quic_udp.sort_by_key(|c| std::cmp::Reverse(c.ru_sni_score));
    tcp_passed
        .into_iter()
        .chain(quic_udp)
        .map(|c| c.proxy)
        .collect()
}

/// Решение об авто-переключении активного узла в фоновом доборе на более быстрый узел.
pub fn should_upgrade_active_node(
    current_delay_ms: i64,
    current_is_quic_udp: bool,
    candidate_delay_ms: i64,
    candidate_is_quic_udp: bool,
) -> bool {
    if candidate_delay_ms <= 0 {
        return false;
    }
    let much_faster = current_delay_ms > 0
        && candidate_delay_ms * 2 <= current_delay_ms
        && (current_delay_ms - candidate_delay_ms) >= 300;
    let penalized_upgrade = current_delay_ms >= 3000
        && candidate_delay_ms < 2500
        && candidate_delay_ms + 1000 <= current_delay_ms;
    let tcp_over_quic = current_is_quic_udp
        && !candidate_is_quic_udp
        && (candidate_delay_ms < 1500 || candidate_delay_ms < current_delay_ms);
    much_faster || penalized_upgrade || tcp_over_quic
}

/// Универсальное ядро параллельной L7-проверки партии с поддержкой мгновенного выхода
/// при качественном узле (`quality_score < L7_FAST_ACCEPT_SCORE_MS`) и короткого grace-окна (650 мс)
/// при получении штрафного скора (`>= 1200`), чтобы не отдать победу сломанному/зашейпленному узлу.
pub async fn probe_l7_batch_metrics_with<F, Fut>(
    nodes: Vec<Value>,
    max_concurrent: usize,
    stop_after_first: bool,
    probe_fn: F,
) -> Vec<(Value, mihomo::L7ProbeMetrics)>
where
    F: Fn(Value) -> Fut,
    Fut: std::future::Future<Output = (Value, Result<mihomo::L7ProbeMetrics, String>)>,
{
    let mut verified = Vec::new();
    let mut futs = stream::iter(nodes.into_iter().map(probe_fn)).buffer_unordered(max_concurrent);
    let mut grace_deadline: Option<tokio::time::Instant> = None;

    loop {
        let next_opt = if let Some(dl) = grace_deadline {
            match tokio::time::timeout_at(dl, futs.next()).await {
                Ok(v) => v,
                Err(_) => break,
            }
        } else {
            futs.next().await
        };
        let Some((node, res)) = next_opt else {
            break;
        };
        if let Ok(metrics) = res {
            verified.push((node, metrics));
            if stop_after_first {
                if metrics.quality_score < L7_FAST_ACCEPT_SCORE_MS {
                    break;
                }
                if grace_deadline.is_none() {
                    grace_deadline = Some(
                        tokio::time::Instant::now() + Duration::from_millis(L7_PENALIZED_GRACE_MS),
                    );
                }
            }
        }
    }
    verified.sort_by_key(|(_, m)| m.quality_score);
    verified
}

/// Обратно-совместимая обёртка над `probe_l7_batch_metrics_with`, возвращающая `quality_score`.
pub async fn probe_l7_batch_with<F, Fut>(
    nodes: Vec<Value>,
    max_concurrent: usize,
    stop_after_first: bool,
    probe_fn: F,
) -> Vec<(Value, i64)>
where
    F: Fn(Value) -> Fut,
    Fut: std::future::Future<Output = (Value, Result<i64, String>)>,
{
    probe_l7_batch_metrics_with(nodes, max_concurrent, stop_after_first, |node| {
        let fut = probe_fn(node);
        async move {
            let (n, res) = fut.await;
            (
                n,
                res.map(|score| mihomo::L7ProbeMetrics {
                    rtt_ms: score,
                    quality_score: score,
                }),
            )
        }
    })
    .await
    .into_iter()
    .map(|(n, m)| (n, m.quality_score))
    .collect()
}

/// Фаза 2: точечная L7-валидация партии кандидатов через отдельный служебный провайдер `__free_sub_probe`.
/// При `stop_after_first == true` немедленно возвращает первый качественный узел (или лучший за grace-окно 650 мс)
/// для минимального TTFF, а остальные кандидаты добираются в фоне.
async fn run_l7_batch_probe(
    state: &AppState,
    mode: FreeSubMode,
    batch: &[Value],
    stop_after_first: bool,
) -> Vec<(Value, mihomo::L7ProbeMetrics)> {
    if batch.is_empty() {
        return Vec::new();
    }
    let _probe_guard = FREE_SUB_PROBE_LOCK.lock().await;
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

    let verified = probe_l7_batch_metrics_with(prepared, L7_MAX_CONCURRENT, stop_after_first, |node| {
        let http = state.http.clone();
        let cfg_c = cfg.clone();
        async move {
            let name = node
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let res = mihomo::probe_service_provider_node_l7_metrics(
                &http,
                &cfg_c,
                FREE_SUB_PROBE_PROVIDER_ID,
                &name,
                L7_TIMEOUT_MS,
            )
            .await;
            if res.is_ok() {
                clear_l7_recent_fail(&node);
            } else {
                record_l7_recent_fail(&node, chrono::Utc::now().timestamp());
            }
            update_status(mode, |st| {
                st.stats.l7_checked += 1;
                if res.is_ok() {
                    st.stats.l7_passed += 1;
                }
            })
            .await;
            (node, res)
        }
    })
    .await;

    if !verified.is_empty() {
        let mut cache_map = std::collections::BTreeMap::new();
        for (node, metrics) in &verified {
            if let Some(nm) = node.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                cache_map.insert(nm.to_string(), metrics.rtt_ms);
            }
        }
        if !cache_map.is_empty() {
            mihomo::store_ping_cache(&cache_map);
        }
    }
    verified
}

/// Объединяет проверенные узлы с текущим пулом.
/// При `preserve_existing_pool == true` (фоновый добор) сохраняет ВСЕ не-забаненные `existing_nodes`,
/// дополняя их новыми `verified` узлами (до `BACKGROUND_BACKUP_TARGET`), не сжимая пул.
pub fn merge_verified_with_pool_checked(
    verified: Vec<Value>,
    existing_nodes: &[Value],
    active_leaf: Option<&str>,
    mc: &ModeCache,
    now_ts: i64,
    preserve_existing_pool: bool,
) -> (Vec<Value>, bool) {
    let mut out = Vec::new();
    let mut seen_keys = HashSet::new();
    let mut seen_endpoints = HashSet::new();
    let mut preserved = false;

    if let Some(active_name) = active_leaf.map(str::trim).filter(|s| !s.is_empty()) {
        if let Some(active_node) = existing_nodes
            .iter()
            .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(active_name))
        {
            if !is_node_blacklisted(mc, active_node, active_name, now_ts) {
                seen_keys.insert(node_dedup_key(active_node));
                if let Some(ep) = endpoint_blacklist_key(active_node) {
                    seen_endpoints.insert(ep);
                }
                out.push(active_node.clone());
                preserved = true;
            }
        }
    }

    for node in verified {
        if preserve_existing_pool && out.len() >= BACKGROUND_BACKUP_TARGET {
            break;
        }
        let nm = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
        if is_node_blacklisted(mc, &node, nm, now_ts) {
            continue;
        }
        let key = node_dedup_key(&node);
        let ep_ok = endpoint_blacklist_key(&node)
            .map(|ep| seen_endpoints.insert(ep))
            .unwrap_or(true);
        if ep_ok && seen_keys.insert(key) {
            out.push(node);
        }
    }

    if preserve_existing_pool {
        for node in existing_nodes {
            if out.len() >= BACKGROUND_BACKUP_TARGET {
                break;
            }
            let nm = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if nm.is_empty() || is_node_blacklisted(mc, node, nm, now_ts) {
                continue;
            }
            let key = node_dedup_key(node);
            let ep_ok = endpoint_blacklist_key(node)
                .map(|ep| seen_endpoints.insert(ep))
                .unwrap_or(true);
            if ep_ok && seen_keys.insert(key) {
                out.push(node.clone());
            }
        }
    }

    ensure_unique_node_names(&mut out);
    (out, preserved)
}

/// Сохраняет активный узел пользователя с прежним именем и параметрами, если он принадлежит целевой подписке
/// и НЕ находится в чёрном списке. Не допускает более одного узла с одинаковым `(server, port)` в пуле.
pub fn merge_verified_with_active_session_checked(
    verified: Vec<Value>,
    existing_nodes: &[Value],
    active_leaf: Option<&str>,
    mc: &ModeCache,
    now_ts: i64,
) -> (Vec<Value>, bool) {
    merge_verified_with_pool_checked(verified, existing_nodes, active_leaf, mc, now_ts, false)
}

/// Вставляет/обновляет рабочие узлы в `mc.working_nodes` в начало списка, сохраняя `total_bytes` и `high_priority`,
/// затем сортирует по приоритету и обрезает до `BACKGROUND_BACKUP_TARGET` (не срезая свежие узлы).
pub fn upsert_working_nodes_in_cache(
    mc: &mut ModeCache,
    mode: FreeSubMode,
    items: &[(Value, mihomo::L7ProbeMetrics)],
    now_ts: i64,
) {
    for (node, metrics) in items {
        let dk = node_dedup_key(node);
        let ep = endpoint_blacklist_key(node);
        let mut prev_bytes = mc.high_priority_keys.get(&dk).copied().unwrap_or(0);
        let mut prev_hp = prev_bytes >= REAL_TRAFFIC_PRIORITY_BYTES;
        mc.working_nodes.retain(|w| {
            let same = node_dedup_key(&w.proxy) == dk
                || (ep.is_some() && endpoint_blacklist_key(&w.proxy) == ep);
            if same {
                prev_bytes = prev_bytes.max(w.total_bytes);
                prev_hp |= w.high_priority;
            }
            !same
        });
        mc.working_nodes.insert(
            0,
            CachedWorkingNode {
                proxy: node.clone(),
                last_ok_ts: now_ts,
                last_delay_ms: metrics.rtt_ms,
                quality_score: metrics.quality_score,
                mode,
                high_priority: prev_hp,
                total_bytes: prev_bytes,
            },
        );
    }
    mc.working_nodes.sort_by(|a, b| {
        b.high_priority
            .cmp(&a.high_priority)
            .then_with(|| b.last_ok_ts.cmp(&a.last_ok_ts))
            .then_with(|| a.effective_quality_score().cmp(&b.effective_quality_score()))
    });
    mc.working_nodes.truncate(BACKGROUND_BACKUP_TARGET);
}

/// Сохраняет активный узел пользователя с прежним именем и параметрами, если он принадлежит целевой подписке.
/// Не допускает более одного узла с одинаковым `(server, port)` в основном пуле.
pub fn merge_verified_with_active_session(
    verified: Vec<Value>,
    existing_nodes: &[Value],
    active_leaf: Option<&str>,
) -> (Vec<Value>, bool) {
    let empty_mc = ModeCache::default();
    merge_verified_with_active_session_checked(verified, existing_nodes, active_leaf, &empty_mc, 0)
}

fn unquote_yaml_scalar(raw: &str) -> Value {
    let trimmed = raw.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        if let Ok(s) = serde_json::from_str::<String>(trimmed) {
            return Value::String(s);
        }
        return Value::String(trimmed[1..trimmed.len() - 1].to_string());
    }
    if trimmed.starts_with('\'') && trimmed.ends_with('\'') && trimmed.len() >= 2 {
        return Value::String(trimmed[1..trimmed.len() - 1].replace("''", "'"));
    }
    match trimmed {
        "true" | "True" | "TRUE" => Value::Bool(true),
        "false" | "False" | "FALSE" => Value::Bool(false),
        "null" | "~" => Value::Null,
        _ => {
            if let Ok(i) = trimmed.parse::<i64>() {
                json!(i)
            } else if let Ok(f) = trimmed.parse::<f64>() {
                json!(f)
            } else {
                Value::String(trimmed.to_string())
            }
        }
    }
}

fn parse_yaml_block(lines: &[(usize, &str)], idx: &mut usize, min_indent: usize) -> Value {
    if *idx >= lines.len() {
        return Value::Null;
    }
    let is_seq = lines[*idx].1.starts_with("- ") || lines[*idx].1 == "-";
    if is_seq {
        let mut arr = Vec::new();
        while *idx < lines.len() {
            let (ind, text) = lines[*idx];
            if ind < min_indent || !(text.starts_with("- ") || text == "-") {
                break;
            }
            let after_dash = text.strip_prefix("- ").unwrap_or("").trim();
            *idx += 1;
            if after_dash.is_empty() {
                if *idx < lines.len() && lines[*idx].0 > ind {
                    arr.push(parse_yaml_block(lines, idx, lines[*idx].0));
                }
            } else if let Some((k, v)) = after_dash.split_once(':') {
                let mut map = serde_json::Map::new();
                let key = k.trim().trim_matches('"').trim_matches('\'').to_string();
                let val_trim = v.trim();
                if val_trim.is_empty() && *idx < lines.len() && lines[*idx].0 > ind {
                    map.insert(key, parse_yaml_block(lines, idx, lines[*idx].0));
                } else {
                    map.insert(key, unquote_yaml_scalar(val_trim));
                }
                while *idx < lines.len() && lines[*idx].0 > ind && !lines[*idx].1.starts_with("- ") {
                    let sub_ind = lines[*idx].0;
                    if let Value::Object(extra) = parse_yaml_block(lines, idx, sub_ind) {
                        for (ek, ev) in extra {
                            map.insert(ek, ev);
                        }
                    } else {
                        break;
                    }
                }
                arr.push(Value::Object(map));
            } else {
                arr.push(unquote_yaml_scalar(after_dash));
            }
        }
        Value::Array(arr)
    } else {
        let mut map = serde_json::Map::new();
        while *idx < lines.len() {
            let (ind, text) = lines[*idx];
            if ind < min_indent || text.starts_with("- ") {
                break;
            }
            if let Some((k, v)) = text.split_once(':') {
                let key = k.trim().trim_matches('"').trim_matches('\'').to_string();
                let val_trim = v.trim();
                *idx += 1;
                if val_trim.is_empty() {
                    if *idx < lines.len() && lines[*idx].0 > ind {
                        map.insert(key, parse_yaml_block(lines, idx, lines[*idx].0));
                    } else {
                        map.insert(key, Value::Object(serde_json::Map::new()));
                    }
                } else {
                    map.insert(key, unquote_yaml_scalar(val_trim));
                }
            } else {
                *idx += 1;
            }
        }
        Value::Object(map)
    }
}

/// Извлекает узлы из существующего YAML-файла провайдера с сохранением вложенных карт/массивов и экранирования (R21).
pub fn parse_existing_provider_nodes(yaml_content: &str) -> Vec<Value> {
    let mut parsed_lines: Vec<(usize, &str)> = Vec::new();
    let mut in_proxies = false;
    for raw_line in yaml_content.lines() {
        let trimmed = raw_line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let indent = raw_line.len() - raw_line.trim_start().len();
        if indent == 0 {
            if trimmed == "proxies:" {
                in_proxies = true;
                continue;
            } else if in_proxies {
                break;
            }
        }
        if in_proxies {
            parsed_lines.push((indent, trimmed));
        }
    }
    if parsed_lines.is_empty() {
        return Vec::new();
    }
    let mut idx = 0;
    let block = parse_yaml_block(&parsed_lines, &mut idx, parsed_lines[0].0);
    let Some(arr) = block.as_array() else {
        return Vec::new();
    };
    arr.iter().filter_map(sanitize_proxy_value).collect()
}

/// `(mode, node_name, start_down_bytes, start_up_bytes)`
static ACTIVE_SESSION_TRAFFIC_BASE: std::sync::LazyLock<Mutex<Option<(FreeSubMode, String, u64, u64)>>> =
    std::sync::LazyLock::new(|| Mutex::new(None));
static STALL_WATCHDOG_STREAK: std::sync::LazyLock<Mutex<(Option<String>, u8)>> =
    std::sync::LazyLock::new(|| Mutex::new((None, 0)));
static LAST_MANUAL_POOL_SWITCH_TS: std::sync::atomic::AtomicI64 =
    std::sync::atomic::AtomicI64::new(0);

fn record_manual_pool_switch_ts() {
    LAST_MANUAL_POOL_SWITCH_TS.store(chrono::Utc::now().timestamp(), Ordering::Release);
}

fn is_recent_manual_pool_switch(window_secs: i64) -> bool {
    let ts = LAST_MANUAL_POOL_SWITCH_TS.load(Ordering::Acquire);
    ts > 0 && (chrono::Utc::now().timestamp() - ts) < window_secs
}

fn mode_traffic_counters(mode: FreeSubMode) -> (u64, u64) {
    crate::traffic::get_mode_up_down_bytes(mode == FreeSubMode::Whitelist)
}

pub fn prune_expired_blacklist(mc: &mut ModeCache, now_ts: i64) {
    mc.blacklist.retain(|_, exp| *exp > now_ts);
}

pub fn endpoint_blacklist_key(proxy: &Value) -> Option<String> {
    let server = proxy
        .get("server")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("");
    let port = proxy
        .get("port")
        .and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.trim().parse::<u64>().ok()))
        })
        .unwrap_or(0);
    if server.is_empty() || port == 0 {
        return None;
    }
    Some(format!("ep:{}:{port}", server.to_ascii_lowercase()))
}

pub fn is_node_blacklisted(mc: &ModeCache, proxy: &Value, node_name: &str, now_ts: i64) -> bool {
    let key = node_dedup_key(proxy);
    if mc.blacklist.get(&key).is_some_and(|exp| *exp > now_ts) {
        return true;
    }
    if let Some(ep_key) = endpoint_blacklist_key(proxy) {
        if mc.blacklist.get(&ep_key).is_some_and(|exp| *exp > now_ts) {
            return true;
        }
    }
    mc.blacklist.get(node_name).is_some_and(|exp| *exp > now_ts)
}

pub fn blacklist_node_in_cache(
    mc: &mut ModeCache,
    proxy_opt: Option<&Value>,
    node_name: &str,
    now_ts: i64,
) {
    blacklist_node_in_cache_ttl(mc, proxy_opt, node_name, now_ts, BLACKLIST_TTL_SECS);
}

pub fn blacklist_node_in_cache_ttl(
    mc: &mut ModeCache,
    proxy_opt: Option<&Value>,
    node_name: &str,
    now_ts: i64,
    ttl_secs: i64,
) {
    let expires = now_ts + ttl_secs;
    if !node_name.trim().is_empty() {
        mc.blacklist.insert(node_name.trim().to_string(), expires);
    }
    if let Some(p) = proxy_opt {
        mc.blacklist.insert(node_dedup_key(p), expires);
        if let Some(ep_key) = endpoint_blacklist_key(p) {
            mc.blacklist.insert(ep_key, expires);
        }
    }
}

pub fn unblacklist_node_in_cache(mc: &mut ModeCache, proxy_opt: Option<&Value>, node_name: &str) {
    let trimmed = node_name.trim();
    if !trimmed.is_empty() {
        mc.blacklist.remove(trimmed);
    }
    if let Some(p) = proxy_opt {
        mc.blacklist.remove(&node_dedup_key(p));
        if let Some(ep_key) = endpoint_blacklist_key(p) {
            mc.blacklist.remove(&ep_key);
        }
    }
}

pub fn amnesty_blacklist_gradual(mc: &mut ModeCache, now_ts: i64) -> usize {
    prune_expired_blacklist(mc, now_ts);
    if mc.blacklist.is_empty() {
        return 0;
    }
    let before = mc.blacklist.len();
    let short_keys: Vec<String> = mc
        .blacklist
        .iter()
        .filter(|(_, exp)| **exp - now_ts <= WATCHDOG_BLACKLIST_TTL_SECS)
        .map(|(k, _)| k.clone())
        .collect();
    if !short_keys.is_empty() {
        for k in short_keys {
            mc.blacklist.remove(&k);
        }
        return before - mc.blacklist.len();
    }
    let mut ts_list: Vec<i64> = mc.blacklist.values().copied().collect();
    ts_list.sort_unstable();
    ts_list.dedup();
    let half_idx = (ts_list.len() / 2).saturating_sub(1);
    let cutoff = ts_list[half_idx];
    mc.blacklist.retain(|_, exp| *exp > cutoff);
    before - mc.blacklist.len()
}

pub fn sort_cached_working_prioritized(mc: &mut ModeCache) {
    for w in &mut mc.working_nodes {
        let key = node_dedup_key(&w.proxy);
        if let Some(&bytes) = mc.high_priority_keys.get(&key) {
            w.total_bytes = w.total_bytes.max(bytes);
            if bytes >= REAL_TRAFFIC_PRIORITY_BYTES {
                w.high_priority = true;
            }
        }
    }
    mc.working_nodes.sort_by(|a, b| {
        b.high_priority
            .cmp(&a.high_priority)
            .then_with(|| b.total_bytes.cmp(&a.total_bytes))
            .then_with(|| a.effective_quality_score().cmp(&b.effective_quality_score()))
    });
}

/// Диверсифицирует кандидатов по `(server, port)`: не более `max_per_ep` узлов с одного IP:port в начале очереди.
pub fn diversify_by_endpoint(nodes: Vec<Value>, max_per_ep: usize) -> Vec<Value> {
    if max_per_ep == 0 {
        return nodes;
    }
    let mut counts: HashMap<(String, u16), usize> = HashMap::new();
    let mut primary = Vec::with_capacity(nodes.len());
    let mut overflow = Vec::new();
    for node in nodes {
        let host = node.get("server").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let port = node.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
        let ep = (host, port);
        let cnt = counts.entry(ep).or_insert(0);
        if *cnt < max_per_ep {
            *cnt += 1;
            primary.push(node);
        } else {
            overflow.push(node);
        }
    }
    primary.extend(overflow);
    primary
}

pub fn start_session_in_cache(
    mc: &mut ModeCache,
    node_name: &str,
    host: &str,
    port: u16,
    now_ts: i64,
) {
    close_active_session_in_cache(mc, now_ts, 0, 0, "Переключён");
    let rec = FreeSubSessionRecord {
        id: format!("{now_ts}-{}", mc.sessions.len()),
        mode: if host.is_empty() { FreeSubMode::Vpn } else { mc.working_nodes.first().map(|w| w.mode).unwrap_or(FreeSubMode::Vpn) },
        node_name: node_name.to_string(),
        host: host.to_string(),
        port,
        started_at: now_ts,
        ended_at: None,
        duration_secs: 0,
        down_bytes: 0,
        up_bytes: 0,
        end_reason: "Активен".to_string(),
    };
    mc.sessions.insert(0, rec);
    mc.sessions.truncate(MAX_SESSION_HISTORY);
    mc.last_selected_node = Some(node_name.to_string());
}

pub fn close_active_session_in_cache(
    mc: &mut ModeCache,
    now_ts: i64,
    down_bytes: u64,
    up_bytes: u64,
    reason: &str,
) {
    if let Some(active) = mc.sessions.iter_mut().find(|s| s.ended_at.is_none()) {
        active.ended_at = Some(now_ts);
        active.duration_secs = (now_ts - active.started_at).max(0) as u64;
        active.down_bytes = active.down_bytes.max(down_bytes);
        active.up_bytes = active.up_bytes.max(up_bytes);
        active.end_reason = reason.to_string();
        let total = active.down_bytes + active.up_bytes;
        if total >= REAL_TRAFFIC_PRIORITY_BYTES {
            let nname = active.node_name.clone();
            for w in &mut mc.working_nodes {
                if w.proxy.get("name").and_then(|v| v.as_str()) == Some(nname.as_str()) {
                    w.total_bytes = w.total_bytes.saturating_add(total);
                    w.high_priority = true;
                    mc.high_priority_keys.insert(node_dedup_key(&w.proxy), w.total_bytes);
                }
            }
        }
    }
}

pub fn yaml_has_whitelist_routing(yaml: &str) -> bool {
    yaml.contains(crate::routing::WHITELIST_BEGIN)
        && yaml.contains(crate::routing::WHITELIST_GROUP_NAME)
}

async fn sync_mode_whitelist_state(state: &AppState, mode: FreeSubMode, target_node: Option<&str>) -> Result<(), String> {
    let enable_wl = mode == FreeSubMode::Whitelist;
    let mut tx = ConfigTx::begin(state).await?;
    let yaml_before = tx.read_yaml().await.unwrap_or_default();
    let yaml_missing_wl = enable_wl && !yaml_has_whitelist_routing(&yaml_before);
    let yaml_stale_wl = !enable_wl && yaml_has_whitelist_routing(&yaml_before);
    let need_reapply =
        tx.config().whitelist.enabled != enable_wl || yaml_missing_wl || yaml_stale_wl;
    tx.config_mut().whitelist.enabled = enable_wl;
    tx.config_mut().whitelist.target_server = if enable_wl {
        target_node.map(str::to_string)
    } else {
        None
    };

    if need_reapply {
        let yaml = if yaml_before.is_empty() {
            tx.read_yaml().await?
        } else {
            yaml_before
        };
        let (routed, _) = crate::routing::apply_routing(&yaml, tx.config())?;
        tx.set_yaml(routed)?;
        tx.commit_and_reload().await?;
    } else {
        tx.commit_without_reload().await?;
    }
    drop(tx);

    crate::traffic::set_whitelist_active(enable_wl);
    let _ = crate::api::sync_whitelist_mode_tunnel(state, target_node.unwrap_or("")).await;
    let cfg = state.cfg_snapshot().await;
    let yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path)
        .await
        .unwrap_or_default();
    let ports = crate::routing::redir_ports(&yaml);
    let dns_port = crate::api::resolve_active_dns_port(&cfg);
    let _ = crate::api::routing::sync_full_tunnel_modes(&cfg, ports.redir, ports.tproxy, dns_port).await;
    Ok(())
}

/// Атомарная публикация проверенного набора в рабочий провайдер (`free_vpn` / `free_wl`).
/// При `auto_connect == false` (фоновый добор) обновляет только YAML и пул Mihomo, никогда не перехватывая активный сервер
/// (кроме случая, когда прежний активный узел пула уже попал в blacklist или исчез).
async fn publish_and_connect(
    state: &AppState,
    mode: FreeSubMode,
    verified_nodes: Vec<Value>,
    connect_node_name: Option<String>,
    auto_connect: bool,
    expected_gen: Option<u64>,
) -> Result<(String, bool, usize), String> {
    publish_and_connect_with_mode(
        state,
        mode,
        verified_nodes,
        connect_node_name,
        auto_connect,
        !auto_connect,
        expected_gen,
    )
    .await
}

async fn publish_and_connect_with_mode(
    state: &AppState,
    mode: FreeSubMode,
    verified_nodes: Vec<Value>,
    connect_node_name: Option<String>,
    auto_connect: bool,
    preserve_existing_pool: bool,
    expected_gen: Option<u64>,
) -> Result<(String, bool, usize), String> {
    if let Some(g) = expected_gen {
        if current_mode_generation(mode) != g {
            return Err("Операция отменена пользователем".into());
        }
    }

    let provider_id = mode.provider_id();
    let cfg = state.config.read().await.clone();
    let prov_dir = providers_dir(&cfg);
    let prov_path = prov_dir.join(format!("{provider_id}.yaml"));

    let prev_raw_opt = tokio::fs::read_to_string(&prov_path).await.ok();
    let existing_nodes = prev_raw_opt
        .as_deref()
        .map(parse_existing_provider_nodes)
        .unwrap_or_default();

    let cp = cache_file_path(&state.config_path);
    let now_ts = chrono::Utc::now().timestamp();
    let mut cache = {
        let _cg = FREE_SUB_CACHE_LOCK.lock().await;
        load_disk_cache(&cp).await
    };
    prune_expired_blacklist(cache.mode_mut(mode), now_ts);

    let active_srv = mihomo::get_active_server(&state.http, &cfg).await.ok().flatten();
    let active_belongs_to_sub = active_srv.as_ref().is_some_and(|s| {
        s.provider.as_deref() == Some(provider_id)
            || existing_nodes
                .iter()
                .any(|n| n.get("name").and_then(|v| v.as_str()) == Some(s.id.as_str()))
    });
    let active_leaf_owned = if active_belongs_to_sub {
        resolve_pool_leaf_name(state, &cfg, mode, active_srv.as_ref(), &existing_nodes).await
    } else {
        None
    };
    let active_leaf_opt = active_leaf_owned.as_deref();
    let active_is_blacklisted = active_leaf_opt.is_some_and(|leaf| {
        existing_nodes
            .iter()
            .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(leaf))
            .map(|n| is_node_blacklisted(cache.mode_ref(mode), n, leaf, now_ts))
            .unwrap_or_else(|| cache.mode_ref(mode).blacklist.get(leaf).is_some_and(|exp| *exp > now_ts))
    });

    let (final_nodes, preserved_active) = merge_verified_with_pool_checked(
        verified_nodes,
        &existing_nodes,
        active_leaf_opt,
        cache.mode_ref(mode),
        now_ts,
        preserve_existing_pool,
    );

    if final_nodes.is_empty() {
        return Err("Список проверенных узлов пуст; прежняя конфигурация сохранена".into());
    }
    let published_count = final_nodes.len();

    let yaml = proxies_to_mihomo_yaml(&final_nodes)?;
    crate::api::servers::subscriptions::write_mobile_provider_yaml_atomic(&prov_path, &yaml).await?;

    let rollback_provider_file = || async {
        if let Some(prev) = &prev_raw_opt {
            let _ = crate::api::servers::subscriptions::write_mobile_provider_yaml_atomic(&prov_path, prev).await;
        } else {
            let _ = tokio::fs::remove_file(&prov_path).await;
        }
    };

    // Регистрация провайдера в config.yaml и гарантия присутствия в `use:` группы `PROXY`
    let cur_yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path).await.unwrap_or_default();
    let needs_registration = !crate::routing::parse_provider_names(&cur_yaml)
        .iter()
        .any(|p| p.eq_ignore_ascii_case(provider_id));
    let needs_wl_mark = mode == FreeSubMode::Whitelist
        && !cfg
            .whitelist_providers
            .iter()
            .any(|p| p.eq_ignore_ascii_case(provider_id));
    let with_groups_preview = crate::routing::add_provider_to_proxy_groups(&cur_yaml, provider_id);
    let needs_group_use = with_groups_preview != cur_yaml;

    if needs_registration || needs_wl_mark || needs_group_use {
        let tx_res: Result<(), String> = async {
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
            y = crate::routing::add_provider_to_proxy_groups(&y, provider_id);
            if needs_wl_mark {
                tx.config_mut().whitelist_providers.push(provider_id.to_string());
            }
            let (routed, _) = crate::routing::apply_routing(&y, tx.config())?;
            tx.set_yaml(routed)?;
            tx.commit_and_reload().await?;
            Ok(())
        }
        .await;
        if let Err(e) = tx_res {
            rollback_provider_file().await;
            return Err(e);
        }
    }

    let updated_cfg = state.config.read().await.clone();
    if let Err(e) = mihomo::force_update_provider(&state.http, &updated_cfg, provider_id).await {
        rollback_provider_file().await;
        return Err(format!("Не удалось обновить провайдер {provider_id} в Mihomo: {e}"));
    }
    mihomo::invalidate_proxies_cache();

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

    if let Some(g) = expected_gen {
        if current_mode_generation(mode) != g {
            return Err("Операция отменена пользователем".into());
        }
    }

    if auto_connect && !preserved_active && !target_name.is_empty() {
        crate::speedtest::cancel_active_speedtest();
        // Сохраняем предыдущий не-бесплатный сервер для возврата при «Отключить» (R11, R12)
        if let Some(prev) = active_srv.as_ref() {
            if prev.provider.as_deref() != Some(FREE_VPN_PROVIDER_ID)
                && prev.provider.as_deref() != Some(FREE_WL_PROVIDER_ID)
                && !prev.id.is_empty()
            {
                let prev_id = prev.id.clone();
                let _ = update_disk_cache(&cp, |c| {
                    c.previous_active_server = Some(prev_id);
                })
                .await;
            }
        }
        let (prev_wl_enabled, prev_wl_target) = {
            let cur = state.config.read().await;
            (cur.whitelist.enabled, cur.whitelist.target_server.clone())
        };
        // Синхронизируем routing config заранее только если режим БС ещё не был включён (чтобы avoid лишнего reload)
        let pre_synced_wl = if mode == FreeSubMode::Whitelist && !prev_wl_enabled {
            sync_mode_whitelist_state(state, mode, Some(&target_name)).await?;
            true
        } else {
            false
        };
        let updated_cfg = state.config.read().await.clone();
        let mut switch_err: Option<String> = None;
        for attempt in 0..3 {
            mihomo::invalidate_proxies_cache();
            match mihomo::switch_server(&state.http, &updated_cfg, &target_name).await {
                Ok(_) => {
                    switch_err = None;
                    break;
                }
                Err(e) => {
                    switch_err = Some(e);
                    if attempt < 2 {
                        let _ = mihomo::force_update_provider(&state.http, &updated_cfg, provider_id).await;
                        tokio::time::sleep(Duration::from_millis(250)).await;
                    }
                }
            }
        }
        if let Some(e) = switch_err {
            if mode == FreeSubMode::Whitelist {
                if let Ok(mut tx) = crate::transaction::ConfigTx::begin(state).await {
                    tx.config_mut().whitelist.enabled = prev_wl_enabled;
                    tx.config_mut().whitelist.target_server = prev_wl_target;
                    let _ = tx.commit_without_reload().await;
                }
            }
            rollback_provider_file().await;
            return Err(e);
        }
        crate::failover::record_manual_switch(&target_name).await;
        if pre_synced_wl {
            let _ = crate::api::sync_whitelist_mode_tunnel(state, &target_name).await;
        } else {
            sync_mode_whitelist_state(state, mode, Some(&target_name)).await?;
        }
        record_session_start(state, mode, &final_nodes, &target_name).await;
    } else if !auto_connect && (active_belongs_to_sub || active_is_blacklisted) && !preserved_active && !target_name.is_empty() {
        let updated_cfg = state.config.read().await.clone();
        if mihomo::switch_server(&state.http, &updated_cfg, &target_name).await.is_ok() {
            if updated_cfg.whitelist.enabled != (mode == FreeSubMode::Whitelist) {
                let _ = sync_mode_whitelist_state(state, mode, Some(&target_name)).await;
            }
            record_session_start(state, mode, &final_nodes, &target_name).await;
            update_status(mode, |st| {
                if !st.running || st.mode == mode {
                    st.active_node = Some(target_name.clone());
                }
            })
            .await;
            log_i!("[FREE-SUB] Активный узел пула восстановлен/переключён на '{}' после обновления провайдера", target_name);
        }
    }

    Ok((target_name, preserved_active, published_count))
}

async fn record_session_start(state: &AppState, mode: FreeSubMode, pool: &[Value], node_name: &str) {
    let cp = cache_file_path(&state.config_path);
    let now_ts = chrono::Utc::now().timestamp();

    // Закрываем предыдущую активную сессию с дельтой трафика (up/down раздельно, R31)
    let prev_base = ACTIVE_SESSION_TRAFFIC_BASE.lock().await.take();
    let (host, port) = pool
        .iter()
        .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(node_name))
        .map(|n| {
            (
                n.get("server").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                n.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16,
            )
        })
        .unwrap_or_default();

    let _ = update_disk_cache(&cp, |cache| {
        if let Some((prev_mode, _, start_down, start_up)) = prev_base {
            let (cur_down, cur_up) = mode_traffic_counters(prev_mode);
            let down_delta = cur_down.saturating_sub(start_down);
            let up_delta = cur_up.saturating_sub(start_up);
            close_active_session_in_cache(
                cache.mode_mut(prev_mode),
                now_ts,
                down_delta,
                up_delta,
                "Переключён",
            );
        }
        start_session_in_cache(cache.mode_mut(mode), node_name, &host, port, now_ts);
        if let Some(s) = cache.mode_mut(mode).sessions.first_mut() {
            s.mode = mode;
        }
    })
    .await;

    let (cur_down, cur_up) = mode_traffic_counters(mode);
    *ACTIVE_SESSION_TRAFFIC_BASE.lock().await =
        Some((mode, node_name.to_string(), cur_down, cur_up));
}

/// Закрывает активную free-сессию при переключении пользователя на внешний (не-бесплатный) сервер (R32).
pub async fn close_active_free_session_on_external_switch(state: &AppState) {
    let prev_base = ACTIVE_SESSION_TRAFFIC_BASE.lock().await.take();
    if let Some((prev_mode, _, start_down, start_up)) = prev_base {
        let cp = cache_file_path(&state.config_path);
        let now_ts = chrono::Utc::now().timestamp();
        let (cur_down, cur_up) = mode_traffic_counters(prev_mode);
        let down_delta = cur_down.saturating_sub(start_down);
        let up_delta = cur_up.saturating_sub(start_up);
        let _ = update_disk_cache(&cp, |cache| {
            close_active_session_in_cache(
                cache.mode_mut(prev_mode),
                now_ts,
                down_delta,
                up_delta,
                "Переключён",
            );
        })
        .await;
    }
}

/// Загрузка кандидатов из источников (RU-зеркала -> активный туннель -> резервный seed-кэш на диске).
/// Опрашивает источники параллельно с жёстким таймаутом 6 с, чтобы недоступные зеркала не задерживали старт на 40+ с.
/// Смешивает распарсенные узлы из нескольких источников методом Round-Robin,
/// сохраняя приоритетный порядок источников (курируемый документ -> CDN -> fallback).
pub fn interleave_source_nodes(source_batches: Vec<Vec<Value>>, max_total: usize) -> Vec<Value> {
    let mut iters: Vec<std::vec::IntoIter<Value>> =
        source_batches.into_iter().map(|b| b.into_iter()).collect();
    let mut out = Vec::new();
    loop {
        if out.len() >= max_total {
            break;
        }
        let mut progressed = false;
        for it in &mut iters {
            if let Some(node) = it.next() {
                out.push(node);
                progressed = true;
                if out.len() >= max_total {
                    break;
                }
            }
        }
        if !progressed {
            break;
        }
    }
    out
}

async fn fetch_sources_or_cache_with(
    state: &AppState,
    cfg: &AppConfig,
    mode: FreeSubMode,
    cache: &mut FreeSubCache,
    wait_all: bool,
) -> (Vec<Value>, Option<String>, usize) {
    let sources = match mode {
        FreeSubMode::Vpn => VPN_SOURCES,
        FreeSubMode::Whitelist => WHITELIST_SOURCES,
    };
    // Прокси-клиент используем только если реально поднят не-DIRECT прокси, иначе лишний таймаут на 127.0.0.1:7890
    let active_leaf = mihomo::get_active_server(&state.http, cfg)
        .await
        .ok()
        .flatten()
        .map(|s| s.id)
        .unwrap_or_default();
    let has_live_tunnel = !active_leaf.is_empty()
        && !active_leaf.eq_ignore_ascii_case("DIRECT")
        && !active_leaf.eq_ignore_ascii_case("REJECT");
    let proxied = if has_live_tunnel {
        crate::updater::proxied_client(&cfg.mihomo_proxy_url())
    } else {
        None
    };

    let source_timeout_secs: u64 = if wait_all { 10 } else { 6 };

    // Параллельно запрашиваем каждый источник с сохранением его приоритетного индекса:
    // при wait_all=false как только первый источник вернул >= 20 узлов, даём остальным 2.5с добора;
    // при wait_all=true ждём все источники (до 15с) для полного наполнения пула в фоне.
    let mut futs: futures_util::stream::FuturesUnordered<_> = sources
        .iter()
        .enumerate()
        .map(|(src_idx, src)| {
            let http = state.http.clone();
            let proxied_ref = proxied.clone();
            async move {
                for url in src.urls {
                    let mut body_res =
                        fetch_bounded_text(&http, url, MAX_RESPONSE_BYTES, source_timeout_secs)
                            .await;
                    if body_res.is_err() {
                        if let Some(ref p_client) = proxied_ref {
                            body_res = fetch_bounded_text(
                                p_client,
                                url,
                                MAX_RESPONSE_BYTES,
                                source_timeout_secs,
                            )
                            .await;
                        }
                    }
                    if let Ok(text) = body_res {
                        let (nodes, rejected) =
                            parse_subscription_nodes_bounded(&text, MAX_PARSED_NODES_PER_SOURCE);
                        if !nodes.is_empty() {
                            return Some((src_idx, src.label.to_string(), nodes, rejected.len()));
                        }
                    }
                }
                None
            }
        })
        .collect();

    let mut per_source: Vec<(usize, String, Vec<Value>)> = Vec::new();
    let mut total_collected = 0usize;
    let mut rejected_invalid = 0usize;
    let total_deadline = tokio::time::Instant::now() + Duration::from_secs(if wait_all { 15 } else { 8 });
    let mut grace_deadline: Option<tokio::time::Instant> = None;

    loop {
        let deadline = grace_deadline.map_or(total_deadline, |g| g.min(total_deadline));
        let next_res = tokio::time::timeout_at(deadline, futs.next()).await;
        match next_res {
            Ok(Some(Some((src_idx, label, nodes, rej_count)))) => {
                rejected_invalid += rej_count;
                total_collected = total_collected.saturating_add(nodes.len());
                per_source.push((src_idx, label, nodes));
                if !wait_all && total_collected >= 20 && grace_deadline.is_none() {
                    grace_deadline = Some(tokio::time::Instant::now() + Duration::from_millis(2500));
                }
                if total_collected >= MAX_PARSED_NODES {
                    break;
                }
            }
            Ok(Some(None)) => continue,
            Ok(None) | Err(_) => break,
        }
    }

    per_source.sort_by_key(|(idx, _, _)| *idx);
    let used_source = per_source.first().map(|(_, label, _)| label.clone());
    let batches: Vec<Vec<Value>> = per_source.into_iter().map(|(_, _, nodes)| nodes).collect();
    let collected = interleave_source_nodes(batches, MAX_PARSED_NODES);

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

    // 4) Вшитый в бинарник стартовый seed-список (на случай первого запуска в БС без интернета)
    let embedded_raw = match mode {
        FreeSubMode::Vpn => EMBEDDED_VPN_SEED,
        FreeSubMode::Whitelist => EMBEDDED_WHITELIST_SEED,
    };
    let (embedded_nodes, embedded_rejected) = parse_subscription_nodes_bounded(embedded_raw, MAX_PARSED_NODES);
    rejected_invalid += embedded_rejected.len();
    if !embedded_nodes.is_empty() {
        return (embedded_nodes, Some("Вшитый стартовый список".to_string()), rejected_invalid);
    }

    (Vec::new(), None, rejected_invalid)
}

/// Асинхронный запуск с синхронным захватом `run_lock(mode)` и выставлением `running: true` ДО возврата HTTP-ответа (убирает гонку с GET status).
pub async fn spawn_one_click_connect(state: AppState, mode: FreeSubMode) -> Result<FreeSubStatus, String> {
    spawn_free_sub_job(state, mode, true).await
}

pub async fn spawn_background_refill(state: AppState, mode: FreeSubMode) -> Result<FreeSubStatus, String> {
    spawn_free_sub_job(state, mode, false).await
}

fn init_job_status(st: &mut FreeSubStatus, mode: FreeSubMode, auto_connect: bool, stage_label: String) {
    let now_ts = chrono::Utc::now().timestamp();
    if auto_connect {
        *st = FreeSubStatus {
            running: true,
            mode,
            stage: FreeSubStage::Sources,
            stage_label,
            updated_at: Some(now_ts),
            ..Default::default()
        };
    } else {
        let prev_active = st.active_node.clone();
        let prev_preserved = st.preserved_active;
        let prev_working = st.stats.working_count;
        *st = FreeSubStatus {
            running: true,
            mode,
            stage: FreeSubStage::Sources,
            stage_label,
            active_node: prev_active,
            preserved_active: prev_preserved,
            error: None,
            updated_at: Some(now_ts),
            ..Default::default()
        };
        st.stats.working_count = prev_working;
    }
}

async fn apply_job_failure_to_status(
    mode: FreeSubMode,
    auto_connect: bool,
    my_gen: u64,
    msg: &str,
) -> Option<FreeSubStatus> {
    if current_mode_generation(mode) != my_gen {
        return None;
    }
    let now_ts = chrono::Utc::now().timestamp();
    if !auto_connect {
        record_bg_refill_failure(mode, now_ts);
        update_status(mode, |st| {
            st.running = false;
            st.error = None;
            st.stage = if st.active_node.is_some() {
                FreeSubStage::Connected
            } else {
                FreeSubStage::Idle
            };
            st.stage_label = if st.active_node.is_some() {
                "Новые узлы не найдены (текущий узел сохранён)".to_string()
            } else {
                "Новые узлы не найдены".to_string()
            };
            st.updated_at = Some(now_ts);
        })
        .await;
        return Some(get_status_for(mode).await);
    }
    update_status(mode, |st| {
        st.running = false;
        st.stage = FreeSubStage::Error;
        st.stage_label = "Ошибка подключения".to_string();
        st.error = Some(msg.to_string());
        st.updated_at = Some(now_ts);
    })
    .await;
    None
}

async fn reset_mode_status_idle(mode: FreeSubMode) {
    let mut st = status_lock(mode).write().await;
    *st = FreeSubStatus {
        mode,
        stage: FreeSubStage::Idle,
        running: false,
        active_node: None,
        error: None,
        ..Default::default()
    };
}

async fn spawn_free_sub_job(
    state: AppState,
    mode: FreeSubMode,
    auto_connect: bool,
) -> Result<FreeSubStatus, String> {
    let guard = run_lock(mode)
        .clone()
        .try_lock_owned()
        .map_err(|_| "Поиск и подключение бесплатной подписки уже выполняется".to_string())?;

    let my_gen = bump_mode_generation(mode);
    let started = Instant::now();
    let stage_label = if auto_connect {
        "Загрузка компактных источников и кэша…".to_string()
    } else {
        "Фоновый добор свежих узлов…".to_string()
    };
    let status_snapshot = {
        let mut st = status_lock(mode).write().await;
        init_job_status(&mut st, mode, auto_connect, stage_label);
        st.clone()
    };

    tokio::spawn(async move {
        let _ = run_free_sub_job_with_guard(guard, state, mode, auto_connect, my_gen, started).await;
    });

    Ok(status_snapshot)
}

/// Запуск полного конвейера подключения бесплатной мульти-подписки в 1 клик.
pub async fn start_one_click_connect(state: AppState, mode: FreeSubMode) -> Result<FreeSubStatus, String> {
    run_free_sub_job(state, mode, true).await
}

/// Фоновый добор свежих узлов без обрыва и перехвата текущей активной сессии.
pub async fn start_background_refill(state: AppState, mode: FreeSubMode) -> Result<FreeSubStatus, String> {
    run_free_sub_job(state, mode, false).await
}

async fn run_free_sub_job(
    state: AppState,
    mode: FreeSubMode,
    auto_connect: bool,
) -> Result<FreeSubStatus, String> {
    let guard = run_lock(mode)
        .clone()
        .try_lock_owned()
        .map_err(|_| "Поиск и подключение бесплатной подписки уже выполняется".to_string())?;

    let my_gen = bump_mode_generation(mode);
    let started = Instant::now();
    let stage_label = if auto_connect {
        "Загрузка компактных источников и кэша…".to_string()
    } else {
        "Фоновый добор свежих узлов…".to_string()
    };
    update_status(mode, |st| {
        init_job_status(st, mode, auto_connect, stage_label);
    })
    .await;

    run_free_sub_job_with_guard(guard, state, mode, auto_connect, my_gen, started).await
}

async fn run_free_sub_job_with_guard(
    _guard: tokio::sync::OwnedMutexGuard<()>,
    state: AppState,
    mode: FreeSubMode,
    auto_connect: bool,
    my_gen: u64,
    started: Instant,
) -> Result<FreeSubStatus, String> {
    match run_free_sub_job_inner(state, mode, auto_connect, my_gen, started).await {
        Ok(status) => Ok(status),
        Err(msg) => {
            if let Some(non_destructive) =
                apply_job_failure_to_status(mode, auto_connect, my_gen, &msg).await
            {
                log_w!("[FREE-SUB] Фоновый добор не нашёл новых узлов (mode={mode:?}): {msg}");
                Ok(non_destructive)
            } else if current_mode_generation(mode) != my_gen {
                log_i!("[FREE-SUB] Задача отменена сменой поколения (mode={mode:?})");
                Err(msg)
            } else {
                log_e!("[FREE-SUB] Ошибка конвейера (mode={mode:?}): {msg}");
                Err(msg)
            }
        }
    }
}

async fn run_free_sub_job_inner(
    state: AppState,
    mode: FreeSubMode,
    auto_connect: bool,
    my_gen: u64,
    started: Instant,
) -> Result<FreeSubStatus, String> {
    let cfg = state.config.read().await.clone();
    let cache_path = cache_file_path(&state.config_path);
    let mut cache = {
        let _cg = FREE_SUB_CACHE_LOCK.lock().await;
        load_disk_cache(&cache_path).await
    };
    let now_ts = chrono::Utc::now().timestamp();
    prune_expired_blacklist(cache.mode_mut(mode), now_ts);
    sort_cached_working_prioritized(cache.mode_mut(mode));

    // Убеждаемся, что служебный провайдер __free_sub_probe готов
    if let Err(e) = ensure_service_probe_provider(&state).await {
        log_w!("[FREE-SUB] Не удалось подготовить служебный провайдер: {e}");
    }

    // Шаг A: Real-Traffic Fast-Path по ранее работавшим узлам из кэша (сразу L7 без TCP-предфильтра)
    let cached_working: Vec<Value> = cache
        .mode_ref(mode)
        .working_nodes
        .iter()
        .filter(|w| {
            let n = w.proxy.get("name").and_then(|v| v.as_str()).unwrap_or("");
            !is_node_blacklisted(cache.mode_ref(mode), &w.proxy, n, now_ts)
        })
        .take(4)
        .filter_map(|w| sanitize_proxy_value(&w.proxy))
        .collect();

    let mut fast_verified: Vec<(Value, mihomo::L7ProbeMetrics)> = Vec::new();
    if auto_connect && !cached_working.is_empty() {
        update_status(mode, |st| {
            st.stage = FreeSubStage::L7;
            st.stage_label = "Быстрая L7-проверка ранее работавших узлов из кэша…".to_string();
        })
        .await;
        let fast_res = run_l7_batch_probe(&state, mode, &cached_working, true).await;
        let mut seen_k: HashSet<String> = HashSet::new();
        let mut seen_eps: HashSet<String> = HashSet::new();
        for (node, metrics) in fast_res {
            let nm = node.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if is_node_blacklisted(cache.mode_ref(mode), &node, nm, now_ts) {
                continue;
            }
            if !seen_k.insert(node_dedup_key(&node)) {
                continue;
            }
            let ep_ok = endpoint_blacklist_key(&node)
                .map(|ep| seen_eps.insert(ep))
                .unwrap_or(true);
            if ep_ok {
                fast_verified.push((node, metrics));
            }
        }
    }

    if auto_connect && !fast_verified.is_empty() {
        let ttff = started.elapsed().as_millis() as u64;
        update_status(mode, |st| {
            st.ttff_ms = Some(ttff);
            st.stage = FreeSubStage::Publishing;
            st.stage_label = "Публикация и подключение рабочего узла из кэша…".to_string();
        })
        .await;
        // Публикуем ТОЛЬКО узлы, реально прошедшие L7-проверку в fast_res (без слепого подмешивания старых узлов).
        let fast_pool: Vec<Value> = fast_verified.iter().map(|(n, _)| n.clone()).collect();
        let first_name = fast_pool
            .first()
            .and_then(|n| n.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string);
        if let Ok((active_name, preserved, published_count)) =
            publish_and_connect(&state, mode, fast_pool.clone(), first_name, true, Some(my_gen)).await
        {
            update_status(mode, |st| {
                st.running = false;
                st.stage = FreeSubStage::Connected;
                st.stage_label = format!("Подключено ({active_name}) за {ttff} мс");
                st.active_node = Some(active_name);
                st.preserved_active = preserved;
                st.stats.working_count = published_count;
                st.source_used = Some("Быстрый кэш".to_string());
                st.updated_at = Some(chrono::Utc::now().timestamp());
            })
            .await;

            // После быстрого старта из кэша обязательно запускаем фоновый добор свежих узлов
            // из онлайн-источников до BACKGROUND_BACKUP_TARGET (20) и авто-апгрейд скорости.
            if fast_verified.len() < BACKGROUND_BACKUP_TARGET {
                let bg_state = state.clone();
                let bg_cfg = cfg.clone();
                let bg_cache_path = cache_path.clone();
                let mut bg_cache = cache.clone();
                tokio::spawn(async move {
                    if current_mode_generation(mode) != my_gen {
                        return;
                    }
                    let bg_now = chrono::Utc::now().timestamp();
                    let (raw_nodes, _source_used, _rejected) =
                        fetch_sources_or_cache_with(&bg_state, &bg_cfg, mode, &mut bg_cache, true).await;
                    let (deduped_raw, _) = deduplicate_proxies(raw_nodes);
                    let existing_keys: HashSet<String> =
                        fast_verified.iter().map(|(n, _)| node_dedup_key(n)).collect();
                    let existing_eps: HashSet<String> = fast_verified
                        .iter()
                        .filter_map(|(n, _)| endpoint_blacklist_key(n))
                        .collect();
                    let deduped_nodes: Vec<Value> = deduped_raw
                        .into_iter()
                        .filter(|p| {
                            let n = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                            if is_node_blacklisted(bg_cache.mode_ref(mode), p, n, bg_now) {
                                return false;
                            }
                            if existing_keys.contains(&node_dedup_key(p)) {
                                return false;
                            }
                            if let Some(ep) = endpoint_blacklist_key(p) {
                                if existing_eps.contains(&ep) {
                                    return false;
                                }
                            }
                            true
                        })
                        .collect();
                    let deduped_nodes = diversify_by_endpoint(deduped_nodes, MAX_PER_ENDPOINT_IN_POOL);
                    if deduped_nodes.is_empty() || current_mode_generation(mode) != my_gen {
                        return;
                    }
                    let dns_futs = stream::iter(deduped_nodes.into_iter().map(|proxy| async move {
                        let server = proxy.get("server").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let port = proxy.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                        let name = proxy.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                        let res = resolve_and_validate_host(&server, port).await;
                        (proxy, name, server, port, res)
                    }))
                    .buffer_unordered(16);
                    let mut candidates = Vec::new();
                    for (proxy, name, server, port, res) in dns_futs.collect::<Vec<_>>().await {
                        if let Ok(addrs) = res {
                            let subnet_key = addrs
                                .first()
                                .map(|a| subnet_prefix_key(a.ip()))
                                .unwrap_or_else(|| "unknown".into());
                            candidates.push(CandidateNode {
                                is_quic_udp: is_quic_udp_node(&proxy),
                                ru_sni_score: if mode == FreeSubMode::Whitelist {
                                    bs_candidate_priority(&proxy)
                                } else {
                                    ru_sni_priority(&proxy)
                                },
                                proxy,
                                name,
                                server,
                                port,
                                resolved_addrs: addrs,
                                subnet_key,
                            });
                        }
                    }
                    let (primary_q, deferred_q) = apply_soft_subnet_quota(
                        candidates,
                        SOFT_SUBNET_QUOTA,
                        mode == FreeSubMode::Whitelist,
                    );
                    let mut quic_udp_candidates = Vec::new();
                    let mut tcp_candidates = Vec::new();
                    for c in primary_q.into_iter().chain(deferred_q) {
                        if c.is_quic_udp {
                            quic_udp_candidates.push(c);
                        } else {
                            tcp_candidates.push(c);
                        }
                    }
                    let tcp_endpoints_wave1: Vec<SocketAddr> = tcp_candidates
                        .iter()
                        .flat_map(|c| c.resolved_addrs.iter().take(2).copied())
                        .collect();
                    let wave1_outcomes = run_tcp_prefilter_endpoints(
                        tcp_endpoints_wave1,
                        TCP_FAST_TIMEOUT_MS,
                        TCP_MAX_CONCURRENT,
                        TCP_RATE_INTERVAL_MS,
                        Duration::from_secs(12),
                    )
                    .await;
                    let tcp_passed_candidates: Vec<CandidateNode> = tcp_candidates
                        .into_iter()
                        .filter(|c| {
                            c.resolved_addrs
                                .iter()
                                .take(2)
                                .any(|ep| wave1_outcomes.get(ep) == Some(&TcpOutcome::Ok))
                        })
                        .collect();
                    let remaining_for_bg = demote_recent_l7_failed_with_store(
                        prioritize_l7_ready_proxies(tcp_passed_candidates, quic_udp_candidates),
                        bg_now,
                    );
                    if !remaining_for_bg.is_empty() && current_mode_generation(mode) == my_gen {
                        run_background_l7_refill(
                            bg_state,
                            bg_cfg,
                            bg_cache_path,
                            mode,
                            my_gen,
                            true,
                            fast_verified,
                            remaining_for_bg,
                            Vec::new(),
                        )
                        .await;
                    }
                });
            }
            return Ok(get_status_for(mode).await);
        }
    }

    // Загрузка источников / резервного seed-кэша
    update_status(mode, |st| {
        st.stage = FreeSubStage::Sources;
        st.stage_label = "Загрузка источников (RU-зеркала / туннель / seed-кэш)…".to_string();
    })
    .await;

    let initial_pool_nodes: Vec<Value> = if !auto_connect {
        let prov_path = providers_dir(&cfg).join(format!("{}.yaml", mode.provider_id()));
        tokio::fs::read_to_string(&prov_path)
            .await
            .map(|r| parse_existing_provider_nodes(&r))
            .unwrap_or_default()
            .into_iter()
            .filter(|n| {
                let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
                !nm.is_empty() && !is_node_blacklisted(cache.mode_ref(mode), n, nm, now_ts)
            })
            .collect()
    } else {
        Vec::new()
    };
    let initial_pool_eps: HashSet<String> = initial_pool_nodes
        .iter()
        .filter_map(endpoint_blacklist_key)
        .collect();
    let initial_pool_keys: HashSet<String> = initial_pool_nodes
        .iter()
        .map(node_dedup_key)
        .collect();

    let (raw_nodes, source_used, rejected_invalid) =
        fetch_sources_or_cache_with(&state, &cfg, mode, &mut cache, !auto_connect).await;
    let total_raw = raw_nodes.len();
    let (deduped_raw, dup_count) = deduplicate_proxies(raw_nodes);
    let mut deduped_nodes: Vec<Value> = deduped_raw
        .iter()
        .filter(|p| {
            let n = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
            if is_node_blacklisted(cache.mode_ref(mode), p, n, now_ts) {
                return false;
            }
            if !auto_connect {
                if initial_pool_keys.contains(&node_dedup_key(p)) {
                    return false;
                }
                if let Some(ep) = endpoint_blacklist_key(p) {
                    if initial_pool_eps.contains(&ep) {
                        return false;
                    }
                }
            }
            true
        })
        .cloned()
        .collect();
    if deduped_nodes.is_empty() && !deduped_raw.is_empty() && !cache.mode_ref(mode).blacklist.is_empty() {
        log_w!("[FREE-SUB] Все {} узлов в blacklist — выполняем градуальную амнистию ЧС (mode={mode:?})", deduped_raw.len());
        amnesty_blacklist_gradual(cache.mode_mut(mode), now_ts);
        let _ = update_disk_cache(&cache_path, |c| {
            amnesty_blacklist_gradual(c.mode_mut(mode), now_ts);
        })
        .await;
        deduped_nodes = deduped_raw
            .iter()
            .filter(|p| {
                let n = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                !is_node_blacklisted(cache.mode_ref(mode), p, n, now_ts)
            })
            .cloned()
            .collect();
        if deduped_nodes.is_empty() {
            deduped_nodes = deduped_raw.clone();
        }
    }
    let deduped_nodes = diversify_by_endpoint(deduped_nodes, MAX_PER_ENDPOINT_IN_POOL);
    update_status(mode, |st| {
        st.source_used = source_used.clone();
        st.stats.total_raw = total_raw;
        st.stats.rejected_invalid = rejected_invalid;
        st.stats.deduplicated = dup_count;
    })
    .await;

    if deduped_nodes.is_empty() {
        return Err("Не удалось получить ни одного валидного узла из источников и локального кэша. Прежняя конфигурация сохранена.".to_string());
    }

    // DNS-резолв + отбрасывание loopback/LAN/приватных IP
    update_status(mode, |st| {
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
                    ru_sni_score: if mode == FreeSubMode::Whitelist {
                        bs_candidate_priority(&proxy)
                    } else {
                        ru_sni_priority(&proxy)
                    },
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
    update_status(mode, |st| st.stats.rejected_dns = rejected_dns).await;

    // Мягкая квота /24 (диверсификация порядка очереди, без потери узлов)
    let (primary_q, deferred_q) = apply_soft_subnet_quota(
        candidates,
        SOFT_SUBNET_QUOTA,
        mode == FreeSubMode::Whitelist,
    );
    let ordered_candidates: Vec<CandidateNode> = demote_recent_l7_failed_candidates(
        primary_q.into_iter().chain(deferred_q).collect(),
        now_ts,
    );

    // Разделение: TCP/VLESS проходят через TCP-предфильтр и ставятся в начало очереди (с приоритетом RU-SNI),
    // а QUIC/UDP (`hysteria2`/`tuic`) идут только после прошедших TCP-предфильтр узлов.
    let mut quic_udp_candidates: Vec<CandidateNode> = Vec::new();
    let mut tcp_candidates: Vec<CandidateNode> = Vec::new();
    for c in ordered_candidates {
        if c.is_quic_udp {
            quic_udp_candidates.push(c);
        } else {
            tcp_candidates.push(c);
        }
    }

    // Шаг B: Волна 1 TCP-предфильтра (700 мс, 16 сокетов, rate limit, до 2 адресов на хост — R28)
    let tcp_endpoints_wave1: Vec<SocketAddr> = tcp_candidates
        .iter()
        .flat_map(|c| c.resolved_addrs.iter().take(2).copied())
        .collect();
    let tcp_budget_secs = if auto_connect { TCP_PHASE_BUDGET_SECS } else { 12 };
    let tcp_early_stop = if auto_connect { 24 } else { BG_L7_MAX_CANDIDATES };
    let wave1_outcomes = run_tcp_prefilter_endpoints_budgeted(
        tcp_endpoints_wave1,
        TCP_FAST_TIMEOUT_MS,
        TCP_MAX_CONCURRENT,
        TCP_RATE_INTERVAL_MS,
        Duration::from_secs(tcp_budget_secs),
        Some(tcp_early_stop),
    )
    .await;

    let tcp_candidates_total = tcp_candidates.len();
    let mut tcp_passed_candidates = Vec::new();
    let mut timed_out_candidates = Vec::new();
    let mut unchecked_tcp_candidates = Vec::new();
    for c in tcp_candidates {
        let any_ok = c
            .resolved_addrs
            .iter()
            .take(2)
            .any(|ep| wave1_outcomes.get(ep) == Some(&TcpOutcome::Ok));
        if any_ok {
            tcp_passed_candidates.push(c);
        } else if c
            .resolved_addrs
            .iter()
            .take(2)
            .any(|ep| wave1_outcomes.get(ep) == Some(&TcpOutcome::TimedOutFast))
        {
            timed_out_candidates.push(c);
        } else if !c
            .resolved_addrs
            .iter()
            .take(2)
            .any(|ep| wave1_outcomes.contains_key(ep))
        {
            unchecked_tcp_candidates.push(c);
        }
    }
    let tcp_checked_candidates = tcp_candidates_total.saturating_sub(unchecked_tcp_candidates.len());
    let tcp_passed_count = tcp_passed_candidates.len();
    let l7_ready_proxies = demote_recent_l7_failed_with_store(
        prioritize_l7_ready_proxies(tcp_passed_candidates, quic_udp_candidates),
        now_ts,
    );
    update_status(mode, |st| {
        st.stats.tcp_checked = tcp_checked_candidates;
        st.stats.tcp_passed = tcp_passed_count;
    })
    .await;

    // Шаг C: L7-проверка партиями до первого рабочего узла (с бюджетом 32 узла / 20с для быстрого TTFF,
    // и расширенным бюджетом 96 узлов / 60с для фонового добора)
    update_status(mode, |st| {
        st.stage = FreeSubStage::L7;
        st.stage_label = format!("Точечная L7-проверка ({} кандидатов)…", l7_ready_proxies.len());
    })
    .await;

    let mut verified_nodes: Vec<(Value, mihomo::L7ProbeMetrics)> = Vec::new();
    let mut remaining_for_bg: Vec<Value> = Vec::new();
    let fg_l7_started = Instant::now();
    let max_l7_cands = if auto_connect {
        FOREGROUND_L7_MAX_CANDIDATES
    } else {
        BG_L7_MAX_CANDIDATES
    };
    let l7_budget_secs = if auto_connect {
        FOREGROUND_L7_BUDGET_SECS
    } else {
        BG_L7_BUDGET_SECS
    };
    let fg_limit = l7_ready_proxies.len().min(max_l7_cands);

    for (batch_idx, chunk) in l7_ready_proxies[..fg_limit].chunks(L7_BATCH_SIZE).enumerate() {
        if current_mode_generation(mode) != my_gen {
            return Err("Операция отменена пользователем".into());
        }
        if fg_l7_started.elapsed() >= Duration::from_secs(l7_budget_secs) {
            let rest_start = batch_idx * L7_BATCH_SIZE;
            if rest_start < l7_ready_proxies.len() {
                remaining_for_bg.extend_from_slice(&l7_ready_proxies[rest_start..]);
            }
            break;
        }
        let res = run_l7_batch_probe(&state, mode, chunk, true).await;
        if !res.is_empty() {
            let verified_keys: HashSet<String> =
                res.iter().map(|(n, _)| node_dedup_key(n)).collect();
            for n in chunk {
                if !verified_keys.contains(&node_dedup_key(n)) {
                    remaining_for_bg.push(n.clone());
                }
            }
            verified_nodes.extend(res);
            let rest_start = (batch_idx + 1) * L7_BATCH_SIZE;
            if rest_start < l7_ready_proxies.len() {
                remaining_for_bg.extend_from_slice(&l7_ready_proxies[rest_start..]);
            }
            break;
        }
    }

    // Быстрая мягкая амнистия ЧС до долгой Wave 2, если Wave 1 не нашла ни одного живого узла
    if verified_nodes.is_empty() && !cache.mode_ref(mode).blacklist.is_empty() {
        if current_mode_generation(mode) != my_gen {
            return Err("Операция отменена пользователем".into());
        }
        log_w!("[FREE-SUB] Wave 1 не дала живых узлов при непустом blacklist — выполняем градуальную амнистию ЧС (mode={mode:?})");
        let prev_working: Vec<Value> = cache
            .mode_ref(mode)
            .working_nodes
            .iter()
            .filter_map(|w| sanitize_proxy_value(&w.proxy))
            .take(8)
            .collect();
        let amnesty_ts = chrono::Utc::now().timestamp();
        amnesty_blacklist_gradual(cache.mode_mut(mode), amnesty_ts);
        let _ = update_disk_cache(&cache_path, |c| {
            amnesty_blacklist_gradual(c.mode_mut(mode), amnesty_ts);
        })
        .await;
        let amnesty_pool: Vec<Value> = if !prev_working.is_empty() {
            prev_working
                .into_iter()
                .filter(|p| {
                    let n = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    !is_node_blacklisted(cache.mode_ref(mode), p, n, amnesty_ts)
                })
                .collect()
        } else {
            deduped_raw
                .iter()
                .filter(|p| {
                    let n = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    !is_node_blacklisted(cache.mode_ref(mode), p, n, amnesty_ts)
                })
                .take(16)
                .cloned()
                .collect()
        };
        for chunk in amnesty_pool.chunks(L7_BATCH_SIZE) {
            if current_mode_generation(mode) != my_gen {
                return Err("Операция отменена пользователем".into());
            }
            let res = run_l7_batch_probe(&state, mode, chunk, true).await;
            if !res.is_empty() {
                verified_nodes.extend(res);
                break;
            }
        }
    }

    // Повторная волна TCP (>= 1500 мс) если не найдено ни одного рабочего узла (не более 16 кандидатов / 10 с)
    if verified_nodes.is_empty() && !timed_out_candidates.is_empty() {
        if current_mode_generation(mode) != my_gen {
            return Err("Операция отменена пользователем".into());
        }
        update_status(mode, |st| {
            st.stage = FreeSubStage::Prefilter;
            st.stage_label = format!(
                "Повторный TCP-предфильтр с увеличенным таймаутом ({} узлов)…",
                timed_out_candidates.len()
            );
        })
        .await;
        let retry_eps: Vec<SocketAddr> = timed_out_candidates
            .iter()
            .flat_map(|c| c.resolved_addrs.iter().take(2).copied())
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
            if c.resolved_addrs
                .iter()
                .take(2)
                .any(|ep| wave2_outcomes.get(ep) == Some(&TcpOutcome::Ok))
            {
                retry_l7.push(c.proxy.clone());
            }
        }
        let retry_cap = retry_l7.len().min(16);
        let wave2_checked_count = timed_out_candidates.len();
        update_status(mode, |st| {
            st.stats.tcp_retried = wave2_checked_count;
            st.stats.tcp_checked += wave2_checked_count;
            st.stats.tcp_passed = (st.stats.tcp_passed + retry_l7.len()).min(st.stats.tcp_checked);
            st.stage = FreeSubStage::L7;
            st.stage_label = format!("L7-проверка после повтора ({retry_cap} кандидатов)…");
        })
        .await;

        let w2_started = Instant::now();
        for (batch_idx, chunk) in retry_l7[..retry_cap].chunks(L7_BATCH_SIZE).enumerate() {
            if current_mode_generation(mode) != my_gen {
                return Err("Операция отменена пользователем".into());
            }
            if w2_started.elapsed() >= Duration::from_secs(10) {
                break;
            }
            let res = run_l7_batch_probe(&state, mode, chunk, true).await;
            if !res.is_empty() {
                let verified_keys: HashSet<String> =
                    res.iter().map(|(n, _)| node_dedup_key(n)).collect();
                for n in chunk {
                    if !verified_keys.contains(&node_dedup_key(n)) {
                        remaining_for_bg.push(n.clone());
                    }
                }
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
        let now_fail_ts = chrono::Utc::now().timestamp();
        let _ = update_disk_cache(&cache_path, |c| {
            *c.mode_mut(mode) = cache.mode_ref(mode).clone();
            if c.mode_ref(mode).blacklist.len() >= 10 {
                amnesty_blacklist_gradual(c.mode_mut(mode), now_fail_ts);
            }
        })
        .await;
        let msg = "Ни один узел не прошёл L7-проверку. Предыдущая конфигурация сохранена без изменений.".to_string();
        return Err(msg);
    }

    let ttff = started.elapsed().as_millis() as u64;
    update_status(mode, |st| {
        st.ttff_ms = Some(ttff);
        st.stage = FreeSubStage::Publishing;
        st.stage_label = "Атомарная публикация и подключение…".to_string();
    })
    .await;

    let now_ts = chrono::Utc::now().timestamp();
    {
        let mut ping_map = std::collections::BTreeMap::new();
        for (node, metrics) in &verified_nodes {
            if metrics.rtt_ms > 0 {
                if let Some(nm) = node.get("name").and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
                    ping_map.insert(nm.to_string(), metrics.rtt_ms);
                }
            }
        }
        if !ping_map.is_empty() {
            mihomo::store_ping_cache(&ping_map);
        }
    }
    let _ = update_disk_cache(&cache_path, |disk| {
        upsert_working_nodes_in_cache(disk.mode_mut(mode), mode, &verified_nodes, now_ts);
    })
    .await;

    let first_proxies: Vec<Value> = verified_nodes.iter().map(|(n, _)| n.clone()).collect();
    let connect_name = if auto_connect {
        first_proxies
            .first()
            .and_then(|n| n.get("name"))
            .and_then(|v| v.as_str())
            .map(str::to_string)
    } else {
        None
    };

    let (active_name, preserved, published_count) = publish_and_connect(
        &state,
        mode,
        first_proxies.clone(),
        connect_name,
        auto_connect,
        Some(my_gen),
    )
    .await?;
    let added_unique_here = verified_nodes
        .iter()
        .filter(|(n, _)| {
            endpoint_blacklist_key(n)
                .map(|ep| !initial_pool_eps.contains(&ep))
                .unwrap_or_else(|| !initial_pool_keys.contains(&node_dedup_key(n)))
        })
        .count();
    if auto_connect || added_unique_here > 0 {
        clear_bg_refill_failure(mode);
    }
    log_i!("[FREE-SUB] ✓ Опубликовано (auto_connect={auto_connect}) узел '{active_name}' (пул={published_count}) за {ttff} мс (mode={mode:?})");

    update_status(mode, |st| {
        st.running = false;
        st.stage = FreeSubStage::Connected;
        st.stage_label = if auto_connect {
            format!("Подключено ({active_name}) за {ttff} мс")
        } else {
            format!("Пул обновлён ({published_count} узл.) за {ttff} мс")
        };
        st.active_node = if auto_connect { Some(active_name) } else { st.active_node.clone() };
        st.preserved_active = preserved;
        st.stats.working_count = published_count;
        st.updated_at = Some(chrono::Utc::now().timestamp());
    })
    .await;

    // Фоновый добор до BACKGROUND_BACKUP_TARGET (20) резервных узлов;
    // проверяет остаток Wave 1, все непроверенные из-за раннего стопа TCP-кандидаты и догружает все источники (wait_all=true).
    if published_count < BACKGROUND_BACKUP_TARGET {
        let bg_state = state.clone();
        let bg_cfg = cfg.clone();
        let bg_cache_path = cache_path.clone();
        let mut bg_extra_candidates = unchecked_tcp_candidates;
        bg_extra_candidates.extend(timed_out_candidates);
        tokio::spawn(async move {
            run_background_l7_refill(
                bg_state,
                bg_cfg,
                bg_cache_path,
                mode,
                my_gen,
                auto_connect,
                verified_nodes,
                remaining_for_bg,
                bg_extra_candidates,
            )
            .await;
        });
    }

    Ok(get_status_for(mode).await)
}

/// Вспомогательная проверка L7-кандидатов партиями с инкрементальной публикацией, авто-апгрейдом и записью пингов в кэш.
#[allow(clippy::too_many_arguments)]
async fn run_bg_l7_chunks(
    bg_state: &AppState,
    bg_cache_path: &Path,
    mode: FreeSubMode,
    my_gen: u64,
    auto_connect: bool,
    bg_started: Instant,
    proxies: &[Value],
    extra: &mut Vec<Value>,
    seen_eps: &mut HashSet<String>,
    metrics_by_key: &mut HashMap<String, mihomo::L7ProbeMetrics>,
    cur_active_score: &mut i64,
    cur_active_is_quic: &mut bool,
    last_published_len: &mut usize,
) {
    for chunk in proxies.chunks(L7_BATCH_SIZE) {
        if current_mode_generation(mode) != my_gen
            || extra.len() >= BACKGROUND_BACKUP_TARGET
            || bg_started.elapsed() >= Duration::from_secs(BACKGROUND_REFILL_BUDGET_SECS)
        {
            break;
        }
        let res = run_l7_batch_probe(bg_state, mode, chunk, false).await;
        let mut upgrade_target: Option<(String, i64, bool)> = None;
        let mut ping_map = std::collections::BTreeMap::new();
        for (n, metrics) in res {
            if extra.len() >= BACKGROUND_BACKUP_TARGET {
                break;
            }
            let ep_ok = endpoint_blacklist_key(&n)
                .map(|ep| seen_eps.insert(ep))
                .unwrap_or(true);
            if !ep_ok {
                continue;
            }
            let n_is_quic = is_quic_udp_node(&n);
            let n_name = n
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if metrics.rtt_ms > 0 && !n_name.is_empty() {
                ping_map.insert(n_name.clone(), metrics.rtt_ms);
            }
            metrics_by_key.insert(node_dedup_key(&n), metrics);
            if auto_connect
                && upgrade_target.is_none()
                && !n_name.is_empty()
                && !is_recent_manual_pool_switch(120)
                && should_upgrade_active_node(
                    *cur_active_score,
                    *cur_active_is_quic,
                    metrics.quality_score,
                    n_is_quic,
                )
            {
                upgrade_target = Some((n_name, metrics.quality_score, n_is_quic));
            }
            extra.push(n);
        }
        if !ping_map.is_empty() {
            mihomo::store_ping_cache(&ping_map);
        }
        if current_mode_generation(mode) != my_gen {
            return;
        }
        if extra.len() > *last_published_len || upgrade_target.is_some() {
            let count = extra.len();
            let switch_name = upgrade_target.as_ref().map(|(nm, _, _)| nm.clone());
            let do_connect = switch_name.is_some();
            if let Ok((active_after, _, published_count)) =
                publish_and_connect_with_mode(
                    bg_state,
                    mode,
                    extra.clone(),
                    switch_name.clone(),
                    do_connect,
                    true,
                    Some(my_gen),
                )
                .await
            {
                *last_published_len = count;
                if let Some((ref new_name, new_score, new_quic)) = upgrade_target {
                    log_i!(
                        "[FREE-SUB] Фоновый добор: авто-переключение на более быстрый узел '{}' (score {} мс вместо {} мс)",
                        new_name,
                        new_score,
                        *cur_active_score
                    );
                    *cur_active_score = new_score;
                    *cur_active_is_quic = new_quic;
                    update_status(mode, |st| {
                        st.active_node = Some(new_name.clone());
                    })
                    .await;
                }
                let ts = chrono::Utc::now().timestamp();
                let upsert_items: Vec<(Value, mihomo::L7ProbeMetrics)> = extra
                    .iter()
                    .map(|n| {
                        let dk = node_dedup_key(n);
                        let m = metrics_by_key.get(&dk).copied().unwrap_or(mihomo::L7ProbeMetrics {
                            rtt_ms: 999,
                            quality_score: 999,
                        });
                        (n.clone(), m)
                    })
                    .collect();
                let _ = update_disk_cache(bg_cache_path, |c| {
                    upsert_working_nodes_in_cache(c.mode_mut(mode), mode, &upsert_items, ts);
                })
                .await;
                update_status(mode, |st| {
                    st.stats.working_count = published_count;
                    st.updated_at = Some(ts);
                    if !st.running {
                        st.stage_label = if auto_connect {
                            format!("Подключено ({active_after}) · пул {published_count} узл.")
                        } else {
                            format!("Пул обновлён ({published_count} узл.)")
                        };
                    }
                })
                .await;
                log_i!("[FREE-SUB] Фоновый добор: опубликовано {published_count} рабочих узлов");
            }
        }
    }
}

/// Фоновый L7-добор резервных узлов до `BACKGROUND_BACKUP_TARGET` (20):
/// 1) остаток TCP-проверенных узлов Wave 1;
/// 2) все оставшиеся непроверенные кандидаты (снятие 24-IP потолка);
/// 3) дозагрузка всех источников (`wait_all=true`), если в пуле всё ещё < 20 уникальных серверов.
#[allow(clippy::too_many_arguments)]
async fn run_background_l7_refill(
    bg_state: AppState,
    bg_cfg: Arc<AppConfig>,
    bg_cache_path: PathBuf,
    mode: FreeSubMode,
    my_gen: u64,
    auto_connect: bool,
    verified_nodes: Vec<(Value, mihomo::L7ProbeMetrics)>,
    remaining_for_bg: Vec<Value>,
    unchecked_candidates: Vec<CandidateNode>,
) {
    let _bg_active_guard = BgRefillActiveGuard::acquire(mode);
    let bg_started = Instant::now();
    let mut extra: Vec<Value> = verified_nodes.iter().map(|(n, _)| n.clone()).collect();
    if !auto_connect {
        let prov_path = providers_dir(&bg_cfg).join(format!("{}.yaml", mode.provider_id()));
        if let Ok(raw) = tokio::fs::read_to_string(&prov_path).await {
            let existing = parse_existing_provider_nodes(&raw);
            let now_ts = chrono::Utc::now().timestamp();
            let cache = load_disk_cache(&bg_cache_path).await;
            let (merged, _) = merge_verified_with_pool_checked(
                extra,
                &existing,
                None,
                cache.mode_ref(mode),
                now_ts,
                true,
            );
            extra = merged;
        }
    }
    let initial_extra_len = extra.len();
    let mut seen_eps: HashSet<String> = extra
        .iter()
        .filter_map(endpoint_blacklist_key)
        .collect();
    let mut metrics_by_key: HashMap<String, mihomo::L7ProbeMetrics> = verified_nodes
        .iter()
        .map(|(n, m)| (node_dedup_key(n), *m))
        .collect();
    let mut cur_active_score = verified_nodes
        .first()
        .map(|(_, m)| m.quality_score)
        .unwrap_or(999);
    let mut cur_active_is_quic = verified_nodes
        .first()
        .is_some_and(|(n, _)| is_quic_udp_node(n));
    let mut last_published_len = extra.len();

    // Фаза 1: уже прошедшие TCP-предфильтр кандидаты из Wave 1
    if !remaining_for_bg.is_empty() {
        run_bg_l7_chunks(
            &bg_state,
            &bg_cache_path,
            mode,
            my_gen,
            auto_connect,
            bg_started,
            &remaining_for_bg,
            &mut extra,
            &mut seen_eps,
            &mut metrics_by_key,
            &mut cur_active_score,
            &mut cur_active_is_quic,
            &mut last_published_len,
        )
        .await;
    }

    // Фаза 2: все непроверенные из-за раннего стопа TCP на 24 IP кандидаты (без 24-IP потолка)
    if current_mode_generation(mode) == my_gen
        && extra.len() < BACKGROUND_BACKUP_TARGET
        && !unchecked_candidates.is_empty()
        && bg_started.elapsed() < Duration::from_secs(BACKGROUND_REFILL_BUDGET_SECS)
    {
        let novel_candidates: Vec<CandidateNode> = unchecked_candidates
            .into_iter()
            .filter(|c| {
                endpoint_blacklist_key(&c.proxy)
                    .map(|ep| !seen_eps.contains(&ep))
                    .unwrap_or(true)
            })
            .collect();
        if !novel_candidates.is_empty() {
            let mut tcp_cands = Vec::new();
            let mut quic_cands = Vec::new();
            for c in novel_candidates {
                if c.is_quic_udp {
                    quic_cands.push(c);
                } else {
                    tcp_cands.push(c);
                }
            }
            let eps: Vec<SocketAddr> = tcp_cands
                .iter()
                .flat_map(|c| c.resolved_addrs.iter().take(2).copied())
                .collect();
            let outcomes = run_tcp_prefilter_endpoints(
                eps,
                1000,
                TCP_MAX_CONCURRENT,
                TCP_RATE_INTERVAL_MS,
                Duration::from_secs(12),
            )
            .await;
            let passed: Vec<CandidateNode> = tcp_cands
                .into_iter()
                .filter(|c| {
                    c.resolved_addrs
                        .iter()
                        .take(2)
                        .any(|ep| outcomes.get(ep) == Some(&TcpOutcome::Ok))
                })
                .collect();
            let ordered = demote_recent_l7_failed_with_store(
                prioritize_l7_ready_proxies(passed, quic_cands),
                chrono::Utc::now().timestamp(),
            );
            if !ordered.is_empty() {
                run_bg_l7_chunks(
                    &bg_state,
                    &bg_cache_path,
                    mode,
                    my_gen,
                    auto_connect,
                    bg_started,
                    &ordered,
                    &mut extra,
                    &mut seen_eps,
                    &mut metrics_by_key,
                    &mut cur_active_score,
                    &mut cur_active_is_quic,
                    &mut last_published_len,
                )
                .await;
            }
        }
    }

    // Фаза 3: если всё ещё < 20 серверов — догружаем ВСЕ источники (wait_all=true) и проверяем новые
    if current_mode_generation(mode) == my_gen
        && extra.len() < BACKGROUND_BACKUP_TARGET
        && bg_started.elapsed() < Duration::from_secs(BACKGROUND_REFILL_BUDGET_SECS)
    {
        let bg_now = chrono::Utc::now().timestamp();
        let mut bg_cache = load_disk_cache(&bg_cache_path).await;
        let (all_raw, _, _) =
            fetch_sources_or_cache_with(&bg_state, &bg_cfg, mode, &mut bg_cache, true).await;
        let (deduped_raw, _) = deduplicate_proxies(all_raw);
        let novel_proxies: Vec<Value> = deduped_raw
            .into_iter()
            .filter(|p| {
                let n = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if is_node_blacklisted(bg_cache.mode_ref(mode), p, n, bg_now) {
                    return false;
                }
                endpoint_blacklist_key(p)
                    .map(|ep| !seen_eps.contains(&ep))
                    .unwrap_or(true)
            })
            .collect();
        let novel_proxies = diversify_by_endpoint(novel_proxies, MAX_PER_ENDPOINT_IN_POOL);
        if !novel_proxies.is_empty() && current_mode_generation(mode) == my_gen {
            let dns_futs = stream::iter(novel_proxies.into_iter().map(|proxy| async move {
                let server = proxy.get("server").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let port = proxy.get("port").and_then(|v| v.as_u64()).unwrap_or(0) as u16;
                let name = proxy.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let res = resolve_and_validate_host(&server, port).await;
                (proxy, name, server, port, res)
            }))
            .buffer_unordered(16);
            let mut tcp_cands = Vec::new();
            let mut quic_cands = Vec::new();
            for (proxy, name, server, port, res) in dns_futs.collect::<Vec<_>>().await {
                if let Ok(addrs) = res {
                    let subnet_key = addrs
                        .first()
                        .map(|a| subnet_prefix_key(a.ip()))
                        .unwrap_or_else(|| "unknown".into());
                    let cand = CandidateNode {
                        is_quic_udp: is_quic_udp_node(&proxy),
                        ru_sni_score: if mode == FreeSubMode::Whitelist {
                            bs_candidate_priority(&proxy)
                        } else {
                            ru_sni_priority(&proxy)
                        },
                        proxy,
                        name,
                        server,
                        port,
                        resolved_addrs: addrs,
                        subnet_key,
                    };
                    if cand.is_quic_udp {
                        quic_cands.push(cand);
                    } else {
                        tcp_cands.push(cand);
                    }
                }
            }
            let eps: Vec<SocketAddr> = tcp_cands
                .iter()
                .flat_map(|c| c.resolved_addrs.iter().take(2).copied())
                .collect();
            let outcomes = run_tcp_prefilter_endpoints(
                eps,
                1000,
                TCP_MAX_CONCURRENT,
                TCP_RATE_INTERVAL_MS,
                Duration::from_secs(12),
            )
            .await;
            let passed: Vec<CandidateNode> = tcp_cands
                .into_iter()
                .filter(|c| {
                    c.resolved_addrs
                        .iter()
                        .take(2)
                        .any(|ep| outcomes.get(ep) == Some(&TcpOutcome::Ok))
                })
                .collect();
            let ordered = demote_recent_l7_failed_with_store(
                prioritize_l7_ready_proxies(passed, quic_cands),
                chrono::Utc::now().timestamp(),
            );
            if !ordered.is_empty() {
                run_bg_l7_chunks(
                    &bg_state,
                    &bg_cache_path,
                    mode,
                    my_gen,
                    auto_connect,
                    bg_started,
                    &ordered,
                    &mut extra,
                    &mut seen_eps,
                    &mut metrics_by_key,
                    &mut cur_active_score,
                    &mut cur_active_is_quic,
                    &mut last_published_len,
                )
                .await;
            }
        }
    }

    let added_in_bg = extra.len().saturating_sub(initial_extra_len);
    if current_mode_generation(mode) == my_gen {
        if added_in_bg > 0 {
            clear_bg_refill_failure(mode);
        } else if extra.len() < BACKGROUND_BACKUP_TARGET {
            record_bg_refill_failure(mode, chrono::Utc::now().timestamp());
            let final_count = extra.len();
            update_status(mode, |st| {
                if !st.running {
                    st.stats.working_count = final_count;
                    if !auto_connect {
                        st.stage_label = format!("Пул актуален ({final_count} узл., новых не найдено)");
                    }
                    st.updated_at = Some(chrono::Utc::now().timestamp());
                }
            })
            .await;
        }
    }

    log_i!(
        "[FREE-SUB] Фоновый добор завершён: {}/{} уникальных рабочих узлов (новых={}, mode={:?})",
        extra.len(),
        BACKGROUND_BACKUP_TARGET,
        added_in_bg,
        mode
    );
}

/// Добавить обе виртуальные подписки (`free_vpn` и `free_wl`) и служебный `__free_sub_probe` в config.yaml в 1 клик.
pub async fn enable_both_free_subs(state: &AppState) -> Result<(), String> {
    let cfg = state.config.read().await.clone();
    let prov_dir = providers_dir(&cfg);
    let _ = tokio::fs::create_dir_all(&prov_dir).await;

    for mode in [FreeSubMode::Vpn, FreeSubMode::Whitelist] {
        let pid = mode.provider_id();
        let prov_path = prov_dir.join(format!("{pid}.yaml"));
        if !prov_path.exists() {
            let _ = crate::api::servers::subscriptions::write_mobile_provider_yaml_atomic(
                &prov_path,
                "proxies: []\n",
            )
            .await;
        }
    }
    let probe_path = prov_dir.join(format!("{FREE_SUB_PROBE_PROVIDER_ID}.yaml"));
    if !probe_path.exists() {
        let placeholder = "proxies:\n  - name: \"__probe_placeholder\"\n    type: direct\n";
        let _ = crate::api::servers::subscriptions::write_mobile_provider_yaml_atomic(
            &probe_path,
            placeholder,
        )
        .await;
    }

    let mut tx = ConfigTx::begin(state).await?;
    let mut y = tx.read_yaml().await?;
    let existing = crate::routing::parse_provider_names(&y);
    if !existing
        .iter()
        .any(|p| p.eq_ignore_ascii_case(FREE_SUB_PROBE_PROVIDER_ID))
    {
        y = crate::routing::add_service_file_provider_to_yaml(
            &y,
            FREE_SUB_PROBE_PROVIDER_ID,
            &format!("./providers/{FREE_SUB_PROBE_PROVIDER_ID}.yaml"),
        )?;
    }
    for mode in [FreeSubMode::Vpn, FreeSubMode::Whitelist] {
        let pid = mode.provider_id();
        if !existing.iter().any(|p| p.eq_ignore_ascii_case(pid)) {
            y = crate::routing::add_file_provider_to_yaml(
                &y,
                pid,
                &format!("./providers/{pid}.yaml"),
                Some(tx.config().health_check_url()),
                Some(tx.config().mihomo.health_check_interval),
            )?;
        }
        y = crate::routing::add_provider_to_proxy_groups(&y, pid);
        tx.config_mut()
            .provider_aliases
            .insert(pid.to_string(), mode.display_alias().to_string());
        if mode == FreeSubMode::Whitelist
            && !tx
                .config()
                .whitelist_providers
                .iter()
                .any(|p| p.eq_ignore_ascii_case(pid))
        {
            tx.config_mut().whitelist_providers.push(pid.to_string());
        }
    }
    let (routed, _) = crate::routing::apply_routing(&y, tx.config())?;
    tx.set_yaml(routed)?;
    tx.commit_and_reload().await?;
    for mode in [FreeSubMode::Vpn, FreeSubMode::Whitelist] {
        bump_mode_generation(mode);
        clear_bg_refill_failure(mode);
        reset_mode_status_idle(mode).await;
    }
    Ok(())
}

/// Отключить активную бесплатную подписку (возвращает предыдущий сервер пользователя или `DIRECT`).
pub async fn disconnect_free_sub(state: &AppState, mode: FreeSubMode) -> Result<Option<String>, String> {
    bump_mode_generation(mode);
    if !is_mode_active(state, mode).await {
        if mode == FreeSubMode::Whitelist {
            let cfg = state.cfg_snapshot().await;
            if cfg.whitelist.enabled {
                let active_srv = mihomo::get_active_server(&state.http, &cfg).await.ok().flatten();
                let is_bs_active = active_srv.as_ref().is_some_and(|s| {
                    !s.id.eq_ignore_ascii_case("DIRECT")
                        && crate::failover::is_whitelist_server_full(&s.name, &s.id, s.provider.as_deref(), &cfg)
                });
                if !is_bs_active {
                    sync_mode_whitelist_state(state, FreeSubMode::Vpn, None).await?;
                }
            }
        }
        update_status(mode, |st| {
            st.running = false;
            st.active_node = None;
            st.error = None;
            st.stage = FreeSubStage::Idle;
            st.stage_label = "Отключено".to_string();
        })
        .await;
        return Ok(None);
    }

    let cfg = state.cfg_snapshot().await;
    let cp = cache_file_path(&state.config_path);
    let now_ts = chrono::Utc::now().timestamp();

    let restore_target = {
        let _cg = FREE_SUB_CACHE_LOCK.lock().await;
        load_disk_cache(&cp)
            .await
            .previous_active_server
            .filter(|s| !s.is_empty() && s != FREE_VPN_PROVIDER_ID && s != FREE_WL_PROVIDER_ID)
            .unwrap_or_else(|| "DIRECT".to_string())
    };

    // Переключаем ДО очистки previous_active_server и закрытия сессии (R17)
    mihomo::invalidate_proxies_cache();
    let fallback_used = match mihomo::switch_server(&state.http, &cfg, &restore_target).await {
        Ok(_) => restore_target,
        Err(err) => {
            if restore_target != "DIRECT" {
                mihomo::switch_server(&state.http, &cfg, "DIRECT")
                    .await
                    .map_err(|d_err| format!("Не удалось вернуть сервер '{restore_target}' ({err}) и DIRECT ({d_err})"))?;
                "DIRECT".to_string()
            } else {
                return Err(err);
            }
        }
    };
    if fallback_used.eq_ignore_ascii_case("DIRECT") {
        mihomo::clear_selected_tunnel_server();
    }
    crate::failover::record_manual_switch(&fallback_used).await;

    let prev_base = ACTIVE_SESSION_TRAFFIC_BASE.lock().await.take();
    let _ = update_disk_cache(&cp, |cache| {
        if let Some((prev_mode, _, start_down, start_up)) = prev_base {
            let (cur_down, cur_up) = mode_traffic_counters(prev_mode);
            let down_delta = cur_down.saturating_sub(start_down);
            let up_delta = cur_up.saturating_sub(start_up);
            close_active_session_in_cache(
                cache.mode_mut(prev_mode),
                now_ts,
                down_delta,
                up_delta,
                "Отключён",
            );
        } else {
            close_active_session_in_cache(cache.mode_mut(mode), now_ts, 0, 0, "Отключён");
        }
        cache.previous_active_server = None;
    })
    .await;

    if mode == FreeSubMode::Whitelist {
        let fallback_is_bs = !fallback_used.eq_ignore_ascii_case("DIRECT")
            && crate::failover::is_whitelist_server_full(&fallback_used, &fallback_used, None, &cfg);
        if !fallback_is_bs {
            sync_mode_whitelist_state(state, FreeSubMode::Vpn, None).await?;
        }
    }

    update_status(mode, |st| {
        st.running = false;
        st.active_node = None;
        st.error = None;
        st.stage = FreeSubStage::Idle;
        st.stage_label = format!("Отключено (возврат на {fallback_used})");
    })
    .await;

    Ok(Some(fallback_used))
}

/// Полностью удалить/выключить бесплатную подписку (`free_vpn` или `free_wl`) из `config.yaml` и списка карточек.
pub async fn remove_free_sub(state: &AppState, mode: FreeSubMode) -> Result<(), String> {
    bump_mode_generation(mode);
    let pid = mode.provider_id();
    let cfg = state.config.read().await.clone();

    // Отключаем только если активный сервер действительно принадлежит удаляемому mode (R16)
    let active_srv = mihomo::get_active_server(&state.http, &cfg).await.ok().flatten();
    let prov_path_pre = providers_dir(&cfg).join(format!("{pid}.yaml"));
    let existing_nodes = tokio::fs::read_to_string(&prov_path_pre)
        .await
        .map(|r| parse_existing_provider_nodes(&r))
        .unwrap_or_default();
    let belongs_to_mode = active_srv.as_ref().is_some_and(|s| {
        s.provider.as_deref() == Some(pid)
            || existing_nodes
                .iter()
                .any(|n| n.get("name").and_then(|v| v.as_str()) == Some(s.id.as_str()))
    });
    if belongs_to_mode {
        let _ = disconnect_free_sub(state, mode).await;
    } else {
        // Закрываем возможную незакрытую запись в истории только для этого mode, не трогая чужой сервер
        let cp = cache_file_path(&state.config_path);
        let now_ts = chrono::Utc::now().timestamp();
        let _ = update_disk_cache(&cp, |cache| {
            close_active_session_in_cache(cache.mode_mut(mode), now_ts, 0, 0, "Удалён");
        })
        .await;
    }

    let mut tx = ConfigTx::begin(state).await?;
    let y = tx.read_yaml().await?;
    let cleaned = crate::routing::delete_provider_from_yaml(&y, pid)?;
    tx.config_mut().provider_aliases.remove(pid);
    tx.config_mut()
        .whitelist_providers
        .retain(|p| !p.eq_ignore_ascii_case(pid));
    let (routed, _) = crate::routing::apply_routing(&cleaned, tx.config())?;
    tx.set_yaml(routed)?;
    tx.commit_and_reload().await?;

    let prov_path = providers_dir(&*state.cfg_snapshot().await).join(format!("{pid}.yaml"));
    let _ = tokio::fs::remove_file(&prov_path).await;
    clear_bg_refill_failure(mode);
    reset_mode_status_idle(mode).await;
    Ok(())
}

/// Проверяет, является ли сейчас указанный режим (`free_vpn` / `free_wl`) активным сервером в Mihomo.
pub async fn is_mode_active(state: &AppState, mode: FreeSubMode) -> bool {
    let pid = mode.provider_id();
    let cfg = state.config.read().await.clone();
    let prov_path = providers_dir(&cfg).join(format!("{pid}.yaml"));
    let raw = tokio::fs::read_to_string(&prov_path).await.unwrap_or_default();
    let nodes = parse_existing_provider_nodes(&raw);
    let active_srv = mihomo::get_active_server(&state.http, &cfg).await.ok().flatten();
    active_srv.as_ref().is_some_and(|s| {
        s.provider.as_deref() == Some(pid)
            || s.id == pid
            || nodes
                .iter()
                .any(|n| n.get("name").and_then(|v| v.as_str()) == Some(s.id.as_str()))
    })
}

/// Подключает первый доступный узел пула (или `last_selected_node`) при клике по неактивной карточке.
pub async fn connect_pool_node(state: &AppState, mode: FreeSubMode) -> Result<String, String> {
    switch_pool_node_internal(state, mode, "connect", true).await
}

/// Переключение узла внутри пула (`next`, `prev`, `connect`, либо конкретное имя узла).
pub async fn switch_pool_node(
    state: &AppState,
    mode: FreeSubMode,
    target: &str,
) -> Result<String, String> {
    switch_pool_node_internal(state, mode, target, true).await
}

/// Умный выбор лучшего следующего узла при аварийном/ручном бане:
/// предпочитает узлы с `alive == true` в провайдере Mihomo, затем TCP над QUIC/UDP и минимальный `quality_score`.
pub fn select_best_next_pool_node(
    names: &[String],
    nodes: &[Value],
    mc: &ModeCache,
    cur_name: Option<&str>,
    provider_proxies: Option<&std::collections::BTreeMap<String, Value>>,
) -> Option<String> {
    let mut best: Option<((u8, u8, i64, usize), String)> = None;
    for (idx, nm) in names.iter().enumerate() {
        if cur_name.is_some_and(|c| c == nm) {
            continue;
        }
        let node_val = nodes
            .iter()
            .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(nm.as_str()));
        let alive_tier = match provider_proxies.and_then(|m| m.get(nm)).and_then(|v| v.get("alive")).and_then(|a| a.as_bool()) {
            Some(true) => 0u8,
            None => 1u8,
            Some(false) => 2u8,
        };
        let is_quic = if node_val.is_some_and(is_quic_udp_node) { 1u8 } else { 0u8 };
        let dk = node_val.map(node_dedup_key);
        let ep = node_val.and_then(endpoint_blacklist_key);
        let score = mc
            .working_nodes
            .iter()
            .find(|w| {
                w.proxy.get("name").and_then(|v| v.as_str()) == Some(nm.as_str())
                    || dk.as_ref().is_some_and(|d| &node_dedup_key(&w.proxy) == d)
                    || ep.as_ref().is_some_and(|e| endpoint_blacklist_key(&w.proxy).as_ref() == Some(e))
            })
            .map(CachedWorkingNode::effective_quality_score)
            .filter(|&s| s > 0)
            .unwrap_or(i64::MAX / 2);

        let key = (alive_tier, is_quic, score, idx);
        if best.as_ref().is_none_or(|(bk, _)| key < *bk) {
            best = Some((key, nm.clone()));
        }
    }
    best.map(|(_, nm)| nm)
}

async fn switch_pool_node_internal(
    state: &AppState,
    mode: FreeSubMode,
    target: &str,
    is_manual: bool,
) -> Result<String, String> {
    crate::speedtest::cancel_active_speedtest();
    let pid = mode.provider_id();
    let cfg = state.config.read().await.clone();
    let prov_path = providers_dir(&cfg).join(format!("{pid}.yaml"));
    let raw = tokio::fs::read_to_string(&prov_path).await.unwrap_or_default();
    let mut nodes = parse_existing_provider_nodes(&raw);

    let cp = cache_file_path(&state.config_path);
    let now_ts = chrono::Utc::now().timestamp();
    let cache = update_disk_cache(&cp, |c| {
        prune_expired_blacklist(c.mode_mut(mode), now_ts);
        c.clone()
    })
    .await?;

    let names: Vec<String> = nodes
        .iter()
        .filter(|n| {
            let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
            !nm.is_empty() && !is_node_blacklisted(cache.mode_ref(mode), n, nm, now_ts)
        })
        .filter_map(|n| n.get("name").and_then(|v| v.as_str()).map(str::to_string))
        .collect();

    if names.is_empty() && target.trim() != "connect" && target.trim() != "next" && target.trim() != "prev" && target.trim() != "best_next" {
        // Разрешаем explicit reconnect из working_nodes даже если активный файл пула пуст
    } else if names.is_empty() {
        return Err("В пуле нет доступных рабочих узлов — нажмите «Добрать ещё»".into());
    }

    let active_srv = mihomo::get_active_server(&state.http, &cfg).await.ok().flatten();
    let cur_name = resolve_pool_leaf_name(state, &cfg, mode, active_srv.as_ref(), &nodes)
        .await
        .filter(|leaf| names.iter().any(|n| n == leaf))
        .or_else(|| {
            cache
                .mode_ref(mode)
                .last_selected_node
                .clone()
                .filter(|leaf| names.iter().any(|n| n == leaf))
        });

    let chosen = match target.trim() {
        "connect" => {
            if let Some(ref last) = cache.mode_ref(mode).last_selected_node {
                if names.iter().any(|n| n == last) {
                    last.clone()
                } else {
                    names[0].clone()
                }
            } else {
                names[0].clone()
            }
        }
        "best_next" => {
            let prov_proxies = mihomo::get_provider_proxies(&state.http, &cfg).await.ok();
            match select_best_next_pool_node(
                &names,
                &nodes,
                cache.mode_ref(mode),
                cur_name.as_deref(),
                prov_proxies.as_ref(),
            ) {
                Some(picked) => picked,
                None => {
                    if !get_status_for(mode).await.running {
                        let _ = spawn_background_refill(state.clone(), mode).await;
                    }
                    return Err("В пуле нет другого рабочего узла кроме текущего — запущен добор свежих узлов".into());
                }
            }
        }
        "next" => {
            let others_exist = match cur_name.as_deref() {
                Some(cur) => names.iter().any(|n| n != cur),
                None => !names.is_empty(),
            };
            if !others_exist {
                if !get_status_for(mode).await.running {
                    let _ = spawn_background_refill(state.clone(), mode).await;
                }
                return Err("В пуле нет другого рабочего узла кроме текущего — запущен добор свежих узлов".into());
            }
            let start_idx = cur_name
                .as_ref()
                .and_then(|c| names.iter().position(|n| n == c))
                .map(|i| (i + 1) % names.len())
                .unwrap_or(0);
            let mut picked = names[start_idx].clone();
            if cur_name.as_deref() == Some(picked.as_str()) {
                if let Some(alt) = names.iter().find(|n| Some(n.as_str()) != cur_name.as_deref()) {
                    picked = alt.clone();
                } else {
                    if !get_status_for(mode).await.running {
                        let _ = spawn_background_refill(state.clone(), mode).await;
                    }
                    return Err("В пуле нет другого рабочего узла кроме текущего — запущен добор свежих узлов".into());
                }
            }
            picked
        }
        "prev" => {
            let others_exist = match cur_name.as_deref() {
                Some(cur) => names.iter().any(|n| n != cur),
                None => !names.is_empty(),
            };
            if !others_exist {
                if !get_status_for(mode).await.running {
                    let _ = spawn_background_refill(state.clone(), mode).await;
                }
                return Err("В пуле нет другого рабочего узла кроме текущего — запущен добор свежих узлов".into());
            }
            let start_idx = cur_name
                .as_ref()
                .and_then(|c| names.iter().position(|n| n == c))
                .map(|i| if i == 0 { names.len() - 1 } else { i - 1 })
                .unwrap_or(0);
            let mut picked = names[start_idx].clone();
            if cur_name.as_deref() == Some(picked.as_str()) {
                if let Some(alt) = names.iter().rev().find(|n| Some(n.as_str()) != cur_name.as_deref()) {
                    picked = alt.clone();
                } else {
                    if !get_status_for(mode).await.running {
                        let _ = spawn_background_refill(state.clone(), mode).await;
                    }
                    return Err("В пуле нет другого рабочего узла кроме текущего — запущен добор свежих узлов".into());
                }
            }
            picked
        }
        explicit => {
            if names.iter().any(|n| n == explicit) {
                explicit.to_string()
            } else if let Some(raw_node) = nodes
                .iter()
                .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(explicit))
            {
                // Ручное «Подключить снова» из истории: снимаем узел из ЧС по явному выбору пользователя
                let _ = update_disk_cache(&cp, |c| {
                    unblacklist_node_in_cache(c.mode_mut(mode), Some(raw_node), explicit);
                })
                .await;
                explicit.to_string()
            } else if let Some(cached_node) = cache
                .mode_ref(mode)
                .working_nodes
                .iter()
                .find(|w| w.proxy.get("name").and_then(|v| v.as_str()) == Some(explicit))
                .map(|w| w.proxy.clone())
            {
                // Узел был вытеснен из {pid}.yaml, но остался в истории/working_nodes — восстанавливаем в пул
                let _ = update_disk_cache(&cp, |c| {
                    unblacklist_node_in_cache(c.mode_mut(mode), Some(&cached_node), explicit);
                })
                .await;
                let mut restored_nodes = vec![cached_node];
                restored_nodes.extend(nodes.clone());
                let (merged, _) = merge_verified_with_pool_checked(
                    restored_nodes,
                    &nodes,
                    None,
                    cache.mode_ref(mode),
                    now_ts,
                    true,
                );
                if let Ok(yaml) = proxies_to_mihomo_yaml(&merged) {
                    if crate::api::servers::subscriptions::write_mobile_provider_yaml_atomic(&prov_path, &yaml)
                        .await
                        .is_ok()
                    {
                        let _ = mihomo::force_update_provider(&state.http, &cfg, pid).await;
                        nodes = merged;
                    }
                }
                explicit.to_string()
            } else {
                return Err(format!("Узел '{explicit}' не найден в активном пуле {pid}"));
            }
        }
    };

    // Если раньше был активен внешний сервер, запоминаем его для «Отключить»
    if let Some(prev) = active_srv.as_ref() {
        if prev.provider.as_deref() != Some(FREE_VPN_PROVIDER_ID)
            && prev.provider.as_deref() != Some(FREE_WL_PROVIDER_ID)
            && !prev.id.is_empty()
        {
            let prev_id = prev.id.clone();
            let _ = update_disk_cache(&cp, |c| {
                c.previous_active_server = Some(prev_id);
            })
            .await;
        }
    }

    mihomo::invalidate_proxies_cache();
    mihomo::switch_server(&state.http, &cfg, &chosen).await?;
    crate::failover::record_manual_switch(&chosen).await;
    if is_manual {
        record_manual_pool_switch_ts();
    }
    let yaml_now = tokio::fs::read_to_string(&cfg.mihomo.config_path)
        .await
        .unwrap_or_default();
    let has_wl_yaml = yaml_has_whitelist_routing(&yaml_now);
    let mode_synced = match mode {
        FreeSubMode::Whitelist => cfg.whitelist.enabled && has_wl_yaml,
        FreeSubMode::Vpn => !cfg.whitelist.enabled && !has_wl_yaml,
    };
    if mode_synced {
        if mode == FreeSubMode::Whitelist {
            if let Ok(mut tx) = ConfigTx::begin(state).await {
                tx.config_mut().whitelist.target_server = Some(chosen.clone());
                let _ = tx.commit_without_reload().await;
            }
            let _ = crate::api::sync_whitelist_mode_tunnel(state, &chosen).await;
        }
    } else {
        let _ = sync_mode_whitelist_state(state, mode, Some(&chosen)).await;
    }
    record_session_start(state, mode, &nodes, &chosen).await;
    update_status(mode, |st| {
        st.stage = FreeSubStage::Connected;
        st.active_node = Some(chosen.clone());
        st.stage_label = format!("Подключено ({chosen})");
    })
    .await;
    Ok(chosen)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BlacklistNextResult {
    pub active_node: Option<String>,
    pub refill_started: bool,
    pub message: String,
}

/// Добавить текущий (или указанный) узел в чёрный список на 24ч и переключиться на следующий.
/// Если пул исчерпан — автоматически запускает автоподбор нового рабочего узла (с авто-амнистией при необходимости).
pub async fn blacklist_and_next(
    state: &AppState,
    mode: FreeSubMode,
    node_name: Option<&str>,
) -> Result<BlacklistNextResult, String> {
    blacklist_and_next_with_ttl(state, mode, node_name, BLACKLIST_TTL_SECS).await
}

pub async fn blacklist_and_next_with_ttl(
    state: &AppState,
    mode: FreeSubMode,
    node_name: Option<&str>,
    ttl_secs: i64,
) -> Result<BlacklistNextResult, String> {
    let cfg = state.cfg_snapshot().await;
    let prov_path = providers_dir(&cfg).join(format!("{}.yaml", mode.provider_id()));
    let raw = tokio::fs::read_to_string(&prov_path).await.unwrap_or_default();
    let nodes = parse_existing_provider_nodes(&raw);

    let active_srv = mihomo::get_active_server(&state.http, &cfg).await.ok().flatten();
    let resolved_leaf = resolve_pool_leaf_name(state, &cfg, mode, active_srv.as_ref(), &nodes).await;
    let pool_was_active = resolved_leaf.is_some();

    let cp = cache_file_path(&state.config_path);
    let now_ts = chrono::Utc::now().timestamp();

    let target_name = {
        let pre_cache = load_disk_cache(&cp).await;
        node_name
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .or(resolved_leaf)
            .or_else(|| pre_cache.mode_ref(mode).last_selected_node.clone())
            .or_else(|| nodes.first().and_then(|n| n.get("name").and_then(|v| v.as_str())).map(str::to_string))
            .ok_or_else(|| "Не определён узел для добавления в чёрный список".to_string())?
    };

    let proxy_opt = nodes
        .iter()
        .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(target_name.as_str()));
    let reason = if ttl_secs >= BLACKLIST_TTL_SECS { "ЧС" } else { "Обрыв" };
    update_disk_cache(&cp, |cache| {
        blacklist_node_in_cache_ttl(cache.mode_mut(mode), proxy_opt, &target_name, now_ts, ttl_secs);
        if pool_was_active {
            close_active_session_in_cache(cache.mode_mut(mode), now_ts, 0, 0, reason);
        }
    })
    .await?;

    if !pool_was_active && active_srv.is_some() {
        return Ok(BlacklistNextResult {
            active_node: None,
            refill_started: false,
            message: format!("Узел '{target_name}' добавлен в ЧС"),
        });
    }

    match switch_pool_node_internal(state, mode, "best_next", ttl_secs >= BLACKLIST_TTL_SECS).await {
        Ok(next) => Ok(BlacklistNextResult {
            active_node: Some(next.clone()),
            refill_started: false,
            message: format!("Узел '{target_name}' добавлен в ЧС, активен: {next}"),
        }),
        Err(_) => {
            let started = match spawn_one_click_connect(state.clone(), mode).await {
                Ok(_) => true,
                Err(_) => get_status_for(mode).await.running,
            };
            Ok(BlacklistNextResult {
                active_node: None,
                refill_started: started,
                message: format!(
                    "Узел '{target_name}' добавлен в ЧС — пул исчерпан, запущен автоподбор нового рабочего узла…"
                ),
            })
        }
    }
}

/// Полный сброс runtime-состояния, фоновых задач и дискового кэша `free_sub` (используется при POST /api/settings/reset).
pub async fn reset_free_sub_runtime_and_cache(state: &AppState) {
    bump_mode_generation(FreeSubMode::Vpn);
    bump_mode_generation(FreeSubMode::Whitelist);
    *FREE_VPN_STATUS.write().await = FreeSubStatus {
        mode: FreeSubMode::Vpn,
        ..Default::default()
    };
    *FREE_WL_STATUS.write().await = FreeSubStatus {
        mode: FreeSubMode::Whitelist,
        ..Default::default()
    };
    *ACTIVE_SESSION_TRAFFIC_BASE.lock().await = None;
    *STALL_WATCHDOG_STREAK.lock().await = (None, 0);
    *IDLE_PROBE_TICKS.lock().await = (None, 0);
    LAST_AUTO_UPGRADE_TS.lock().await.clear();
    L7_RECENT_FAILS.lock().unwrap_or_else(|e| e.into_inner()).clear();
    LAST_BG_REFILL_FAIL_TS.lock().unwrap_or_else(|e| e.into_inner()).clear();

    let cp = cache_file_path(&state.config_path);
    let _cg = FREE_SUB_CACHE_LOCK.lock().await;
    let _ = tokio::fs::remove_file(&cp).await;
}

/// Разрешает имя реального листового узла для виртуального пула бесплатной подписки (`free_wl`/`free_vpn`).
async fn resolve_pool_leaf_name(
    state: &AppState,
    cfg: &AppConfig,
    mode: FreeSubMode,
    active_srv: Option<&mihomo::Server>,
    nodes: &[Value],
) -> Option<String> {
    let pid = mode.provider_id();
    let is_mode_active = active_srv.is_some_and(|s| {
        s.provider.as_deref() == Some(pid)
            || nodes
                .iter()
                .any(|n| n.get("name").and_then(|v| v.as_str()) == Some(s.id.as_str()))
    });
    if !is_mode_active {
        return None;
    }

    if let Some(s) = active_srv {
        if !s.is_pool && !s.id.is_empty() && !mihomo::is_virtual_free_sub_pool(&s.id) {
            return Some(s.id.clone());
        }
    }

    if let Some((leaf, _)) = mihomo::resolve_virtual_pool_active_leaf(&state.http, cfg, pid).await {
        if !leaf.is_empty() && !mihomo::is_virtual_free_sub_pool(&leaf) {
            return Some(leaf);
        }
    }

    if let Some(s) = active_srv {
        let host = s.host.trim();
        if !host.is_empty()
            && host != "Пул бесплатного VPN"
            && host != "Пул обхода БС"
            && !mihomo::is_virtual_free_sub_pool(host)
        {
            return Some(host.to_string());
        }
    }

    let cp = cache_file_path(&state.config_path);
    load_disk_cache(&cp)
        .await
        .mode_ref(mode)
        .last_selected_node
        .clone()
        .filter(|n| !n.is_empty() && !mihomo::is_virtual_free_sub_pool(n))
}

/// Получить пул узлов, индекс активного узла и историю сессий для карточки бесплатной подписки.
pub async fn get_pool_and_history(state: &AppState, mode: FreeSubMode) -> Value {
    let pid = mode.provider_id();
    let cfg = state.config.read().await.clone();
    let prov_path = providers_dir(&cfg).join(format!("{pid}.yaml"));
    let raw = tokio::fs::read_to_string(&prov_path).await.unwrap_or_default();
    let nodes = parse_existing_provider_nodes(&raw);

    let cp = cache_file_path(&state.config_path);
    let mut cache = load_disk_cache(&cp).await;
    let now_ts = chrono::Utc::now().timestamp();
    prune_expired_blacklist(cache.mode_mut(mode), now_ts);

    let active_srv = mihomo::get_active_server(&state.http, &cfg).await.ok().flatten();
    let active_leaf = resolve_pool_leaf_name(state, &cfg, mode, active_srv.as_ref(), &nodes).await;

    let disk_delays_by_name: HashMap<String, i64> = cache
        .mode_ref(mode)
        .working_nodes
        .iter()
        .filter(|w| w.last_delay_ms > 0)
        .filter_map(|w| {
            w.proxy
                .get("name")
                .and_then(|v| v.as_str())
                .map(|nm| (nm.to_string(), w.last_delay_ms))
        })
        .collect();
    let disk_delays_by_ep: HashMap<String, i64> = cache
        .mode_ref(mode)
        .working_nodes
        .iter()
        .filter(|w| w.last_delay_ms > 0)
        .filter_map(|w| endpoint_blacklist_key(&w.proxy).map(|ep| (ep, w.last_delay_ms)))
        .collect();

    let available: Vec<Value> = nodes
        .iter()
        .filter(|n| {
            let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
            !nm.is_empty() && !is_node_blacklisted(cache.mode_ref(mode), n, nm, now_ts)
        })
        .map(|n| {
            let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let ping_ms = mihomo::cached_ping(nm)
                .filter(|&d| d > 0)
                .or_else(|| disk_delays_by_name.get(nm).copied())
                .or_else(|| {
                    endpoint_blacklist_key(n).and_then(|ep| disk_delays_by_ep.get(&ep).copied())
                })
                .unwrap_or(-1);
            json!({
                "name": nm,
                "host": n.get("server").and_then(|v| v.as_str()).unwrap_or(""),
                "port": n.get("port").and_then(|v| v.as_u64()).unwrap_or(0),
                "ping_ms": ping_ms,
                "is_active": active_leaf.as_deref() == Some(nm),
            })
        })
        .collect();

    let active_index = active_leaf
        .as_ref()
        .and_then(|al| available.iter().position(|x| x.get("name").and_then(|v| v.as_str()) == Some(al.as_str())))
        .map(|i| i + 1)
        .unwrap_or(0);

    // Обновляем живую длительность и дельту трафика для активной сессии в ответе
    let base_guard = ACTIVE_SESSION_TRAFFIC_BASE.lock().await;
    let mut sessions = cache.mode_ref(mode).sessions.clone();
    if let Some((b_mode, b_node, start_down, start_up)) = &*base_guard {
        if *b_mode == mode {
            let (cur_down, cur_up) = mode_traffic_counters(mode);
            let live_down = cur_down.saturating_sub(*start_down);
            let live_up = cur_up.saturating_sub(*start_up);
            if let Some(s) = sessions.iter_mut().find(|s| s.ended_at.is_none() && &s.node_name == b_node) {
                s.duration_secs = (now_ts - s.started_at).max(0) as u64;
                s.down_bytes = s.down_bytes.max(live_down);
                s.up_bytes = s.up_bytes.max(live_up);
            }
        }
    }

    json!({
        "mode": mode,
        "provider_id": pid,
        "active_node": active_leaf,
        "pool_index": active_index,
        "pool_total": available.len(),
        "blacklist_count": cache.mode_ref(mode).blacklist.len(),
        "nodes": available,
        "history": sessions,
    })
}

static IDLE_PROBE_TICKS: std::sync::LazyLock<Mutex<(Option<String>, u8)>> =
    std::sync::LazyLock::new(|| Mutex::new((None, 0)));
static REFILL_CHECK_TICKS: std::sync::LazyLock<Mutex<u8>> =
    std::sync::LazyLock::new(|| Mutex::new(0));
static LAST_AUTO_UPGRADE_TS: std::sync::LazyLock<Mutex<HashMap<String, i64>>> =
    std::sync::LazyLock::new(|| Mutex::new(HashMap::new()));
const AUTO_UPGRADE_COOLDOWN_SECS: i64 = 600;

/// Определяет, является ли текущий тик Smart Watchdog подозрительным на зависание:
/// узел пропал из живых в провайдере ИЛИ при активных соединениях клиент шлёт трафик (up > 0),
/// а входящая скорость ниже 1 КБ/с (капельный трафик). Полный простой (0/0) от idle push-сокетов не считается зависанием.
pub fn is_stall_watchdog_tick(
    alive_in_provider: bool,
    node_conns: usize,
    down_speed: u64,
    up_speed: u64,
) -> bool {
    !alive_in_provider || (node_conns > 0 && down_speed < 1024 && up_speed > 0 && up_speed < 1024)
}

/// Подбирает лучший узел в пуле для авто-апгрейда медленного или QUIC/UDP активного узла.
/// Никогда не подставляет выдуманную задержку — использует только реальный замер из `PING_CACHE`
/// или `mc.working_nodes.last_delay_ms`.
pub fn pick_faster_pool_candidate(
    nodes: &[Value],
    mc: &ModeCache,
    active_leaf: &str,
    active_score: i64,
    active_is_quic: bool,
    now_ts: i64,
    cached_ping_fn: impl Fn(&str) -> Option<i64>,
) -> Option<(String, i64)> {
    let mut best: Option<(String, i64)> = None;
    for n in nodes {
        let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("").trim();
        if nm.is_empty() || nm == active_leaf || is_node_blacklisted(mc, n, nm, now_ts) {
            continue;
        }
        let cand_is_quic = is_quic_udp_node(n);
        let dk = node_dedup_key(n);
        let ep = endpoint_blacklist_key(n);
        let matched_w = mc.working_nodes.iter().find(|w| {
            w.proxy.get("name").and_then(|v| v.as_str()) == Some(nm)
                || node_dedup_key(&w.proxy) == dk
                || (ep.is_some() && endpoint_blacklist_key(&w.proxy) == ep)
        });
        let cand_score = matched_w
            .map(|w| w.quality_score)
            .filter(|&q| q > 0)
            .or_else(|| cached_ping_fn(nm).filter(|&d| d > 0))
            .or_else(|| matched_w.map(|w| w.last_delay_ms).filter(|&d| d > 0))
            .unwrap_or(0);
        if cand_score <= 0 {
            continue;
        }
        if should_upgrade_active_node(active_score, active_is_quic, cand_score, cand_is_quic) {
            if best.as_ref().is_none_or(|(_, b)| cand_score < *b) {
                best = Some((nm.to_string(), cand_score));
            }
        }
    }
    best
}

async fn ensure_pool_backup_refill(state: &AppState, mode: FreeSubMode, cfg: &AppConfig) {
    if get_status_for(mode).await.running || is_bg_refill_active(mode) {
        return;
    }
    let prov_path = providers_dir(cfg).join(format!("{}.yaml", mode.provider_id()));
    let raw = tokio::fs::read_to_string(&prov_path).await.unwrap_or_default();
    let nodes = parse_existing_provider_nodes(&raw);
    let cp = cache_file_path(&state.config_path);
    let now_ts = chrono::Utc::now().timestamp();
    if is_bg_refill_in_cooldown(mode, now_ts) {
        return;
    }
    let (avail_count, bl_count) = {
        let mut cache = load_disk_cache(&cp).await;
        prune_expired_blacklist(cache.mode_mut(mode), now_ts);
        let avail = nodes
            .iter()
            .filter(|n| {
                let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
                !nm.is_empty() && !is_node_blacklisted(cache.mode_ref(mode), n, nm, now_ts)
            })
            .count();
        (avail, cache.mode_ref(mode).blacklist.len())
    };
    if avail_count < 3 {
        if avail_count <= 1 && bl_count >= 15 {
            log_i!("[FREE-SUB] Smart Watchdog: в пуле {avail_count} узлов и {bl_count} в blacklist — градуальная амнистия перед авто-добором");
            let _ = update_disk_cache(&cp, |c| {
                amnesty_blacklist_gradual(c.mode_mut(mode), now_ts);
            })
            .await;
        }
        let bg_state = state.clone();
        tokio::spawn(async move {
            let _ = start_background_refill(bg_state, mode).await;
        });
    }
}

/// Smart Watchdog: при зависании активного бесплатного узла (есть соединения через ЭТОТ узел,
/// 0 Б/с входящего трафика при наличии исходящего >= 3 тика ~15с, либо Mihomo пометил узел `alive: false`,
/// либо периодическая L7-проба при простое упала)
/// автоматически отправляет мёртвый узел в 24ч blacklist, переключает на следующий рабочий узел
/// (или запускает переподбор с амнистией blacklist при исчерпании пула) и фиксирует "Обрыв" в истории.
/// Также при работе на медленном/QUIC узле автоматически переключается на более быстрый TCP-узел из пула
/// и поддерживает минимальный запас узлов в фоне.
pub async fn check_free_sub_stall_watchdog(state: &AppState) {
    let cfg = state.cfg_snapshot().await;
    let Ok(Some(active)) = mihomo::get_active_server(&state.http, &cfg).await else {
        return;
    };
    let mode = match active.provider.as_deref() {
        Some(FREE_VPN_PROVIDER_ID) => FreeSubMode::Vpn,
        Some(FREE_WL_PROVIDER_ID) => FreeSubMode::Whitelist,
        _ => {
            *STALL_WATCHDOG_STREAK.lock().await = (None, 0);
            *IDLE_PROBE_TICKS.lock().await = (None, 0);
            return;
        }
    };
    // Не мешаем активному поиску/подключению или фоновому добору этого же режима
    if get_status_for(mode).await.running || is_bg_refill_active(mode) {
        return;
    }

    let (leaf_name, alive_in_provider) =
        match mihomo::resolve_virtual_pool_active_leaf(&state.http, &cfg, mode.provider_id()).await {
            Some((leaf, alive)) => (leaf, alive),
            None => {
                let fallback_leaf = if !active.is_pool && !active.id.is_empty() {
                    active.id.clone()
                } else if !active.host.trim().is_empty()
                    && active.host != "Пул бесплатного VPN"
                    && active.host != "Пул обхода БС"
                    && !mihomo::is_virtual_free_sub_pool(&active.host)
                {
                    active.host.clone()
                } else {
                    let cp = cache_file_path(&state.config_path);
                    load_disk_cache(&cp)
                        .await
                        .mode_ref(mode)
                        .last_selected_node
                        .clone()
                        .unwrap_or_default()
                };
                if fallback_leaf.is_empty() || mihomo::is_virtual_free_sub_pool(&fallback_leaf) {
                    return;
                }
                (fallback_leaf, true)
            }
        };

    if mode == FreeSubMode::Whitelist {
        let yaml_now = tokio::fs::read_to_string(&cfg.mihomo.config_path)
            .await
            .unwrap_or_default();
        if !cfg.whitelist.enabled || !yaml_has_whitelist_routing(&yaml_now) {
            log_w!(
                "[FREE-SUB] Smart Watchdog: активен free_wl ('{}'), но БС-маршрутизация отсутствует (whitelist.enabled={}, yaml_has_wl={}) — восстанавливаем",
                leaf_name,
                cfg.whitelist.enabled,
                yaml_has_whitelist_routing(&yaml_now)
            );
            let _ = sync_mode_whitelist_state(state, FreeSubMode::Whitelist, Some(&leaf_name)).await;
        }
    }

    let snap = crate::traffic::get_snapshot();
    let (down_speed, up_speed) = match mode {
        FreeSubMode::Whitelist => (snap.whitelist.down, snap.whitelist.up),
        FreeSubMode::Vpn => (snap.proxy.down, snap.proxy.up),
    };
    let node_conns = mihomo::get_node_connections_count(&state.http, &cfg, &leaf_name)
        .await
        .unwrap_or(0);

    let mut streak = STALL_WATCHDOG_STREAK.lock().await;
    if streak.0.as_deref() != Some(leaf_name.as_str()) {
        *streak = (Some(leaf_name.clone()), 0);
    }
    if is_stall_watchdog_tick(alive_in_provider, node_conns, down_speed, up_speed) {
        streak.1 = streak.1.saturating_add(1);
    } else {
        streak.1 = 0;
    }
    let count = streak.1;
    drop(streak);

    let mut periodic_probe_due = false;
    {
        let mut idle = IDLE_PROBE_TICKS.lock().await;
        if idle.0.as_deref() != Some(leaf_name.as_str()) {
            *idle = (Some(leaf_name.clone()), 0);
        }
        idle.1 = idle.1.saturating_add(1);
        if idle.1 >= 24 {
            idle.1 = 0;
            periodic_probe_due = true;
        }
    }

    let mut refill_due = false;
    {
        let mut rt = REFILL_CHECK_TICKS.lock().await;
        *rt = rt.saturating_add(1);
        if *rt >= 24 {
            *rt = 0;
            refill_due = true;
        }
    }

    if count >= 3 || periodic_probe_due {
        // Активная L7-проверка реального листового узла (включая доступ к youtube.com) перед признанием обрыва
        let mut l7_res = mihomo::probe_service_provider_node_l7_metrics(
            &state.http,
            &cfg,
            mode.provider_id(),
            &leaf_name,
            L7_TIMEOUT_MS,
        )
        .await;
        if l7_res.is_err() && count < 3 {
            // Защита от ложного бана при единичном скачке под нагрузкой (плановая проверка): 2 повтора с паузой 2с
            for _ in 0..2 {
                tokio::time::sleep(Duration::from_millis(2000)).await;
                l7_res = mihomo::probe_service_provider_node_l7_metrics(
                    &state.http,
                    &cfg,
                    mode.provider_id(),
                    &leaf_name,
                    L7_TIMEOUT_MS,
                )
                .await;
                if l7_res.is_ok() {
                    break;
                }
            }
        }
        if let Ok(mut active_metrics) = l7_res {
            let mut ping_map = std::collections::BTreeMap::new();
            ping_map.insert(leaf_name.clone(), active_metrics.rtt_ms);
            mihomo::store_ping_cache(&ping_map);

            // Реальный замер скорости скачивания 50 КБ через mixed-port (выполняется редко — раз в 120 с при плановой пробе)
            if periodic_probe_due {
                if let Some(p_client) = crate::updater::proxied_client(&cfg.mihomo_proxy_url()) {
                    let tp_start = Instant::now();
                    if let Ok(Ok(resp)) = tokio::time::timeout(
                        Duration::from_secs(4),
                        p_client.get(mihomo::FREE_SUB_L7_THROUGHPUT_URL).send(),
                    )
                    .await
                    {
                        if resp.status().is_success() {
                            if let Ok(bytes) = tokio::time::timeout(Duration::from_secs(4), resp.bytes()).await {
                                let elapsed_ms = tp_start.elapsed().as_millis() as i64;
                                if bytes.is_ok() && elapsed_ms > 850 {
                                    // < 60 КБ/с: честный штраф к скору активного узла
                                    active_metrics.quality_score = active_metrics
                                        .quality_score
                                        .saturating_add((elapsed_ms - 850) * 2);
                                }
                            }
                        }
                    }
                }
            }

            let active_score = active_metrics.quality_score;
            let prov_path = providers_dir(&cfg).join(format!("{}.yaml", mode.provider_id()));
            let raw = tokio::fs::read_to_string(&prov_path).await.unwrap_or_default();
            let nodes = parse_existing_provider_nodes(&raw);
            let cp = cache_file_path(&state.config_path);
            let now_ts = chrono::Utc::now().timestamp();
            if let Some(active_node_val) = nodes
                .iter()
                .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(leaf_name.as_str()))
                .cloned()
            {
                let _ = update_disk_cache(&cp, |c| {
                    upsert_working_nodes_in_cache(
                        c.mode_mut(mode),
                        mode,
                        &[(active_node_val, active_metrics)],
                        now_ts,
                    );
                })
                .await;
            }

            *STALL_WATCHDOG_STREAK.lock().await = (Some(leaf_name.clone()), 0);
            *IDLE_PROBE_TICKS.lock().await = (Some(leaf_name.clone()), 0);

            // Авто-апгрейд: если текущий узел медленный (> 900 мс) или QUIC/UDP, а в пуле есть более быстрый TCP-узел — переключаемся
            let active_is_quic = nodes
                .iter()
                .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(leaf_name.as_str()))
                .is_some_and(is_quic_udp_node);
            if active_is_quic || active_score > 900 {
                let mut cache = load_disk_cache(&cp).await;
                let upgrade_cooldown_ok = {
                    let guard = LAST_AUTO_UPGRADE_TS.lock().await;
                    guard
                        .get(mode.provider_id())
                        .is_none_or(|&ts| now_ts.saturating_sub(ts) >= AUTO_UPGRADE_COOLDOWN_SECS)
                };
                if upgrade_cooldown_ok && !is_recent_manual_pool_switch(120) {
                    if active_score >= 2500 {
                        let missing_cands: Vec<Value> = nodes
                            .iter()
                            .filter(|n| {
                                let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
                                !nm.is_empty()
                                    && nm != leaf_name
                                    && !is_node_blacklisted(cache.mode_ref(mode), n, nm, now_ts)
                                    && !cache.mode_ref(mode).working_nodes.iter().any(|w| {
                                        w.proxy.get("name").and_then(|v| v.as_str()) == Some(nm)
                                            && w.quality_score > 0
                                    })
                            })
                            .take(2)
                            .cloned()
                            .collect();
                        if !missing_cands.is_empty() {
                            let probed = run_l7_batch_probe(state, mode, &missing_cands, false).await;
                            if !probed.is_empty() {
                                if let Ok(updated) = update_disk_cache(&cp, |c| {
                                    upsert_working_nodes_in_cache(c.mode_mut(mode), mode, &probed, now_ts);
                                    c.clone()
                                })
                                .await
                                {
                                    cache = updated;
                                }
                            }
                        }
                    }
                    if let Some((cand_name, cand_ms)) = pick_faster_pool_candidate(
                        &nodes,
                        cache.mode_ref(mode),
                        &leaf_name,
                        active_score,
                        active_is_quic,
                        now_ts,
                        mihomo::cached_ping,
                    ) {
                        log_i!(
                            "[FREE-SUB] Smart Watchdog: авто-апгрейд с медленного/QUIC узла '{}' (score={} мс) на '{}' ({} мс)",
                            leaf_name,
                            active_score,
                            cand_name,
                            cand_ms
                        );
                        if switch_pool_node_internal(state, mode, &cand_name, false).await.is_ok() {
                            LAST_AUTO_UPGRADE_TS
                                .lock()
                                .await
                                .insert(mode.provider_id().to_string(), now_ts);
                            return;
                        }
                    }
                }
            }
            if refill_due {
                ensure_pool_backup_refill(state, mode, &cfg).await;
            }
            return;
        }

        // N16: перечитываем активный сервер и лист после L7-проверки, чтобы не перехватить ручной выбор пользователя
        let current_active = mihomo::get_active_server(&state.http, &cfg).await.ok().flatten();
        if current_active.as_ref().and_then(|s| s.provider.as_deref()) != Some(mode.provider_id()) {
            *STALL_WATCHDOG_STREAK.lock().await = (None, 0);
            *IDLE_PROBE_TICKS.lock().await = (None, 0);
            return;
        }
        if let Some((cur_leaf, _)) =
            mihomo::resolve_virtual_pool_active_leaf(&state.http, &cfg, mode.provider_id()).await
        {
            if cur_leaf != leaf_name {
                *STALL_WATCHDOG_STREAK.lock().await = (Some(cur_leaf.clone()), 0);
                *IDLE_PROBE_TICKS.lock().await = (Some(cur_leaf), 0);
                return;
            }
        }

        *STALL_WATCHDOG_STREAK.lock().await = (None, 0);
        *IDLE_PROBE_TICKS.lock().await = (None, 0);
        let cp = cache_file_path(&state.config_path);
        let now_ts = chrono::Utc::now().timestamp();
        let prev_base = ACTIVE_SESSION_TRAFFIC_BASE.lock().await.take();
        let (down_delta, up_delta) = prev_base
            .map(|(pm, _, sd, su)| {
                let (cd, cu) = mode_traffic_counters(pm);
                (cd.saturating_sub(sd), cu.saturating_sub(su))
            })
            .unwrap_or((0, 0));
        let _ = update_disk_cache(&cp, |cache| {
            close_active_session_in_cache(
                cache.mode_mut(mode),
                now_ts,
                down_delta,
                up_delta,
                "Обрыв",
            );
        })
        .await;
        log_w!(
            "[FREE-SUB] Smart Watchdog: зависание узла '{}' (alive={}, {} с 0 Б/с при {} соед., L7 проба упала) — в blacklist и переключение",
            leaf_name,
            alive_in_provider,
            count * 5,
            node_conns
        );
        match blacklist_and_next_with_ttl(state, mode, Some(&leaf_name), WATCHDOG_BLACKLIST_TTL_SECS).await {
            Ok(res) if res.refill_started => {
                log_w!(
                    "[FREE-SUB] Smart Watchdog: пул {:?} исчерпан — запущен автоподбор нового узла",
                    mode
                );
            }
            Ok(_) => {}
            Err(err) => {
                log_w!(
                    "[FREE-SUB] Smart Watchdog: ошибка переключения после бана ({err}) — градуальная амнистия blacklist и запуск переподбора",
                    err = err
                );
                let now_err_ts = chrono::Utc::now().timestamp();
                let _ = update_disk_cache(&cp, |c| {
                    amnesty_blacklist_gradual(c.mode_mut(mode), now_err_ts);
                })
                .await;
                let _ = spawn_one_click_connect(state.clone(), mode).await;
            }
        }
    } else if refill_due {
        ensure_pool_backup_refill(state, mode, &cfg).await;
    }
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

    #[test]
    fn test_6_publish_and_connect_syncs_whitelist_mode_tunnel() {
        let src = include_str!("mod.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
        assert!(
            prod.contains("sync_mode_whitelist_state(state, mode, Some(&target_name))")
                && prod.contains("crate::api::sync_whitelist_mode_tunnel"),
            "publish_and_connect must call sync_mode_whitelist_state -> sync_whitelist_mode_tunnel after switching server so whitelist/VPN mode and iptables stay in sync"
        );
    }

    #[test]
    fn test_7_yandex_disk_url_detection_and_docx_extraction() {
        use std::io::Write;
        assert!(is_yandex_disk_public_url("https://disk.yandex.ru/i/xXnBLHAuIfFpSA"));
        assert!(is_yandex_disk_public_url("https://yadi.sk/d/abc123"));
        assert!(!is_yandex_disk_public_url("https://raw.githubusercontent.com/zieng2/wl/main/vless.txt"));

        // Собираем минимальный валидный ZIP (DOCX) в памяти со сжатием Deflate для word/document.xml
        let doc_xml = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:r><w:t>vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443?security=reality&amp;sni=vk.com&amp;pbk=abc#NodeOne</w:t></w:r></w:p>
    <w:p><w:r><w:t>vless://22222222-2222-2222-2222-222222222222@5.6.7.8:443?security=tls&amp;sni=yandex.ru#NodeTwo</w:t></w:r></w:p>
  </w:body>
</w:document>"#;

        let mut encoder = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(doc_xml.as_bytes()).unwrap();
        let compressed = encoder.finish().unwrap();
        let name = b"word/document.xml";

        let mut zip = Vec::new();
        // Local File Header
        zip.extend_from_slice(b"PK\x03\x04");
        zip.extend_from_slice(&20u16.to_le_bytes()); // version
        zip.extend_from_slice(&0u16.to_le_bytes()); // flags
        zip.extend_from_slice(&8u16.to_le_bytes()); // method = deflate
        zip.extend_from_slice(&0u16.to_le_bytes()); // time
        zip.extend_from_slice(&0u16.to_le_bytes()); // date
        zip.extend_from_slice(&0u32.to_le_bytes()); // crc32 (unused here)
        zip.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(doc_xml.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // extra len
        zip.extend_from_slice(name);
        zip.extend_from_slice(&compressed);

        let cd_offset = zip.len();
        // Central Directory Header
        zip.extend_from_slice(b"PK\x01\x02");
        zip.extend_from_slice(&20u16.to_le_bytes());
        zip.extend_from_slice(&20u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&8u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u32.to_le_bytes());
        zip.extend_from_slice(&(compressed.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(doc_xml.len() as u32).to_le_bytes());
        zip.extend_from_slice(&(name.len() as u16).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes()); // extra
        zip.extend_from_slice(&0u16.to_le_bytes()); // comment
        zip.extend_from_slice(&0u16.to_le_bytes()); // disk
        zip.extend_from_slice(&0u16.to_le_bytes()); // int attr
        zip.extend_from_slice(&0u32.to_le_bytes()); // ext attr
        zip.extend_from_slice(&0u32.to_le_bytes()); // local header offset
        zip.extend_from_slice(name);
        let cd_size = zip.len() - cd_offset;

        // EOCD
        zip.extend_from_slice(b"PK\x05\x06");
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());
        zip.extend_from_slice(&1u16.to_le_bytes());
        zip.extend_from_slice(&1u16.to_le_bytes());
        zip.extend_from_slice(&(cd_size as u32).to_le_bytes());
        zip.extend_from_slice(&(cd_offset as u32).to_le_bytes());
        zip.extend_from_slice(&0u16.to_le_bytes());

        let decoded = decode_subscription_bytes(&zip);
        let (nodes, rejected) = parse_subscription_nodes_bounded(&decoded, 10);
        assert!(rejected.is_empty(), "rejected: {:?}", rejected);
        assert_eq!(nodes.len(), 2);
        assert_eq!(nodes[0]["server"], "1.2.3.4");
        assert_eq!(nodes[1]["server"], "5.6.7.8");
    }

    #[test]
    fn test_8_embedded_seeds_contain_valid_nodes() {
        let (wl_nodes, _) = parse_subscription_nodes_bounded(EMBEDDED_WHITELIST_SEED, MAX_PARSED_NODES);
        assert!(
            !wl_nodes.is_empty(),
            "embedded_whitelist_seed.txt must contain valid fallback nodes"
        );
        let (vpn_nodes, _) = parse_subscription_nodes_bounded(EMBEDDED_VPN_SEED, MAX_PARSED_NODES);
        assert!(
            !vpn_nodes.is_empty(),
            "embedded_vpn_seed.txt must contain valid fallback nodes"
        );
    }

    #[tokio::test]
    async fn test_9_stability_gate_requires_both_204_and_payload() {
        let ok = verify_node_stability(
            || async { Ok(120) },
            || async { Ok(180) },
        )
        .await;
        assert_eq!(ok.unwrap(), 150);

        let econnreset = verify_node_stability(
            || async { Ok(95) },
            || async { Err("ECONNRESET on cdn-cgi/trace".into()) },
        )
        .await;
        assert!(econnreset.is_err(), "Stability gate must reject if payload GET fails");
    }

    #[test]
    fn test_10_blacklist_24h_ttl_and_pruning() {
        let mut mc = ModeCache::default();
        let node = json!({"name": "n1", "type": "vless", "server": "1.2.3.4", "port": 443, "uuid": "u1"});
        let now = 1_700_000_000i64;
        blacklist_node_in_cache(&mut mc, Some(&node), "n1", now);
        assert!(is_node_blacklisted(&mc, &node, "n1", now + 3600));
        assert!(!is_node_blacklisted(&mc, &node, "n1", now + BLACKLIST_TTL_SECS + 1));

        prune_expired_blacklist(&mut mc, now + BLACKLIST_TTL_SECS + 1);
        assert!(mc.blacklist.is_empty());
    }

    #[test]
    fn test_11_diversify_by_endpoint_limits_same_ip_head() {
        let nodes = vec![
            json!({"name": "a1", "server": "185.178.208.167", "port": 443}),
            json!({"name": "a2", "server": "185.178.208.167", "port": 443}),
            json!({"name": "a3", "server": "185.178.208.167", "port": 443}),
            json!({"name": "a4", "server": "185.178.208.167", "port": 443}),
            json!({"name": "b1", "server": "94.183.164.146", "port": 443}),
        ];
        let diversified = diversify_by_endpoint(nodes, 2);
        let head_names: Vec<&str> = diversified[..3]
            .iter()
            .filter_map(|v| v.get("name").and_then(|s| s.as_str()))
            .collect();
        assert_eq!(head_names, vec!["a1", "a2", "b1"]);
        assert_eq!(diversified.len(), 5);
    }

    #[test]
    fn test_12_session_history_and_real_traffic_priority() {
        let mut mc = ModeCache::default();
        let n1 = json!({"name": "node-1", "type": "vless", "server": "8.8.4.4", "port": 443, "uuid": "u1"});
        let n2 = json!({"name": "node-2", "type": "vless", "server": "9.9.9.9", "port": 443, "uuid": "u2"});
        mc.working_nodes.push(CachedWorkingNode {
            proxy: n1.clone(),
            last_ok_ts: 100,
            last_delay_ms: 250,
            quality_score: 0,
            mode: FreeSubMode::Whitelist,
            high_priority: false,
            total_bytes: 0,
        });
        mc.working_nodes.push(CachedWorkingNode {
            proxy: n2.clone(),
            last_ok_ts: 100,
            last_delay_ms: 80,
            quality_score: 0,
            mode: FreeSubMode::Whitelist,
            high_priority: false,
            total_bytes: 0,
        });

        start_session_in_cache(&mut mc, "node-1", "8.8.4.4", 443, 1000);
        assert_eq!(mc.sessions.len(), 1);
        assert_eq!(mc.sessions[0].end_reason, "Активен");

        close_active_session_in_cache(&mut mc, 1060, 2_500_000, 100_000, "Переключён");
        assert_eq!(mc.sessions[0].duration_secs, 60);
        assert_eq!(mc.sessions[0].down_bytes, 2_500_000);
        assert_eq!(mc.sessions[0].end_reason, "Переключён");

        sort_cached_working_prioritized(&mut mc);
        assert_eq!(mc.working_nodes[0].proxy["name"], "node-1", "High real-traffic node must sort first");
        assert!(mc.working_nodes[0].high_priority);
    }

    #[test]
    fn test_13_no_hijack_and_backup_target_contract() {
        assert!(BACKGROUND_BACKUP_TARGET >= 20);
        let src = include_str!("mod.rs");
        let fn_start = src.find("async fn publish_and_connect(").expect("publish_and_connect");
        let fn_end = src[fn_start..].find("async fn record_session_start(").map(|o| fn_start + o).unwrap_or(src.len());
        let fn_body = &src[fn_start..fn_end];
        assert!(fn_body.contains("auto_connect: bool"));
        assert!(fn_body.contains("if auto_connect && !preserved_active"));
        assert!(fn_body.contains("add_provider_to_proxy_groups"));
        assert!(fn_body.contains("invalidate_proxies_cache"));
        assert!(fn_body.contains("cache_file_path(&state.config_path)"));
    }

    #[test]
    fn regression_r21_yaml_roundtrip_preserves_nested_ws_headers_and_escaped_quotes() {
        let original = json!({
            "name": "a\"b",
            "type": "vless",
            "server": "1.2.3.4",
            "port": 443,
            "uuid": "11111111-1111-1111-1111-111111111111",
            "network": "ws",
            "ws-opts": {
                "path": "/ws",
                "headers": {
                    "Host": "example.com"
                }
            }
        });
        let yaml = proxies_to_mihomo_yaml(&[original.clone()]).expect("serialize yaml");
        let parsed = parse_existing_provider_nodes(&yaml);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["name"], "a\"b");
        assert_eq!(parsed[0]["ws-opts"]["path"], "/ws");
        assert_eq!(parsed[0]["ws-opts"]["headers"]["Host"], "example.com");
    }

    #[tokio::test]
    async fn regression_r28_tcp_prefilter_checks_multiple_resolved_ips() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ok_addr = listener.local_addr().unwrap();
        let bad_addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let accept_task = tokio::spawn(async move {
            let _ = listener.accept().await;
        });
        let outcomes = run_tcp_prefilter_endpoints(
            vec![bad_addr, ok_addr],
            500,
            4,
            1,
            Duration::from_secs(2),
        )
        .await;
        let _ = accept_task.await;
        assert_eq!(outcomes.get(&ok_addr), Some(&TcpOutcome::Ok));
    }

    #[tokio::test]
    async fn regression_n03_r18_sync_mode_whitelist_state_atomic_and_full_tunnel_sync() {
        let dir = std::env::temp_dir().join(format!("xkeen_n03_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");
        let yaml_path = dir.join("missing_config.yaml");

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.whitelist.enabled = false;
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let config_arc = std::sync::Arc::new(tokio::sync::RwLock::new(std::sync::Arc::new(cfg)));
        let ag = std::sync::Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: std::sync::Arc::new(cfg_path.clone()),
            http: reqwest::Client::new(),
            failover_log: std::sync::Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            config_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        let res = sync_mode_whitelist_state(&state, FreeSubMode::Whitelist, Some("Node-1")).await;
        assert!(res.is_err(), "N03: при ошибке чтения/применения YAML должен возвращаться Err");
        assert!(
            !state.config.read().await.whitelist.enabled,
            "N03: при ошибке YAML флаг whitelist.enabled в AppState не должен меняться"
        );
        let on_disk: crate::config::AppConfig = serde_json::from_str(
            &tokio::fs::read_to_string(&cfg_path).await.unwrap(),
        )
        .unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(
            !on_disk.whitelist.enabled,
            "N03: при ошибке YAML флаг whitelist.enabled в config.json не должен меняться"
        );
    }

    #[test]
    fn regression_sync_mode_whitelist_calls_sync_full_tunnel_modes_unconditionally() {
        let src = include_str!("mod.rs");
        let fn_start = src.find("async fn sync_mode_whitelist_state(").expect("sync_mode_whitelist_state");
        let fn_end = src[fn_start..]
            .find("async fn publish_and_connect(")
            .map(|o| fn_start + o)
            .unwrap_or(src.len());
        let fn_body = &src[fn_start..fn_end];
        assert!(
            fn_body.contains("sync_full_tunnel_modes("),
            "sync_mode_whitelist_state должен вызывать sync_full_tunnel_modes"
        );
        assert!(
            !fn_body.contains("if !enable_wl"),
            "sync_full_tunnel_modes не должен быть обёрнут в `if !enable_wl`, иначе при включении free_wl не ставятся правила iptables"
        );
    }

    #[test]
    fn regression_bg_refill_publishes_incrementally_inside_batch_loop() {
        let src = include_str!("mod.rs");
        let marker = "async fn run_bg_l7_chunks(";
        let start = src.find(marker).expect("run_bg_l7_chunks fn");
        let end = src[start..]
            .find("pub async fn enable_both_free_subs")
            .map(|o| start + o)
            .unwrap_or(src.len());
        let block = &src[start..end];
        let loop_pos = block.find("for chunk in proxies.chunks").expect("batch loop");
        let pub_pos = block.find("publish_and_connect_with_mode(").expect("publish_and_connect");
        assert!(pub_pos > loop_pos && block.contains("last_published_len"), "publish_and_connect должен вызываться инкрементально внутри цикла батчей");
    }

    #[test]
    fn regression_fast_path_publishes_only_l7_verified_nodes_and_starts_background_refill() {
        let src = include_str!("mod.rs");
        let start = src.find("if auto_connect && !fast_verified.is_empty()").expect("fast-path block");
        let end = src[start..]
            .find("// Загрузка источников / резервного seed-кэша")
            .map(|o| start + o)
            .unwrap_or(src.len());
        let block = &src[start..end];
        assert!(
            !block.contains("parse_existing_provider_nodes"),
            "Fast-Path не должен слепо подмешивать непроверенные узлы из existing_pool"
        );
        assert!(
            block.contains("run_background_l7_refill("),
            "После Fast-Path должен обязательно запускаться фоновый добор свежих узлов"
        );
    }

    #[test]
    fn regression_bg_refill_has_no_take_10_cap_and_uses_time_budget() {
        let src = include_str!("mod.rs");
        let start = src.find("async fn run_background_l7_refill(").expect("run_background_l7_refill fn");
        let end = src[start..]
            .find("pub async fn enable_both_free_subs")
            .map(|o| start + o)
            .unwrap_or(src.len());
        let block = &src[start..end];
        assert!(
            !block.contains(".take(10)"),
            "Фоновый добор не должен искусственно обрезаться на .take(10) (80 кандидатов)"
        );
        assert!(
            block.contains("BACKGROUND_REFILL_BUDGET_SECS"),
            "Фоновый добор должен использовать общий бюджет времени вместо жёсткого лимита партий"
        );
    }

    #[test]
    fn regression_sources_have_no_broken_gitverse_urls_and_valid_mirrors() {
        for src in VPN_SOURCES.iter().chain(WHITELIST_SOURCES.iter()) {
            assert!(!src.urls.is_empty(), "Источник {} не должен быть пустым", src.label);
            for url in src.urls {
                assert!(url.starts_with("https://"), "URL должен быть https: {url}");
                assert!(
                    !url.contains("gitverse.ru"),
                    "Нерабочее зеркало gitverse.ru должно быть удалено: {url}"
                );
            }
        }
    }

    #[tokio::test]
    async fn regression_stall_watchdog_resolves_virtual_pool_leaf_blacklists_and_switches() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::mihomo::invalidate_proxies_cache();

        crate::mihomo::invalidate_proxies_cache();
        *STALL_WATCHDOG_STREAK.lock().await = (None, 0);
        *IDLE_PROBE_TICKS.lock().await = (None, 0);

        let dir = std::env::temp_dir().join(format!("xkeen_watchdog_virt_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prov_dir = dir.join("providers");
        std::fs::create_dir_all(&prov_dir).unwrap();
        let cfg_path = dir.join("config.json");
        let yaml_path = dir.join("config.yaml");
        let prov_yaml_path = prov_dir.join("free_wl.yaml");

        let prov_yaml = "proxies:\n  - name: \"dead-leaf\"\n    type: vless\n    server: 1.2.3.4\n    port: 443\n    uuid: 11111111-1111-1111-1111-111111111111\n  - name: \"alive-leaf\"\n    type: vless\n    server: 5.6.7.8\n    port: 443\n    uuid: 22222222-2222-2222-2222-222222222222\n";
        tokio::fs::write(&prov_yaml_path, prov_yaml).await.unwrap();

        let main_yaml = "mixed-port: 7892\nproxy-providers:\n  free_wl:\n    type: file\n    path: providers/free_wl.yaml\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\n    use: [free_wl]\n  - name: \"🛡️ Whitelist\"\n    type: select\n    proxies: [DIRECT]\n    use: [free_wl]\n";
        tokio::fs::write(&yaml_path, main_yaml).await.unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let leaf_probe_hits = Arc::new(AtomicUsize::new(0));
        let bad_pool_probe_hits = Arc::new(AtomicUsize::new(0));
        let switched_to = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));

        let leaf_probe_srv = leaf_probe_hits.clone();
        let bad_pool_srv = bad_pool_probe_hits.clone();
        let switched_srv = switched_to.clone();

        let srv_handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                let leaf_probe_srv = leaf_probe_srv.clone();
                let bad_pool_srv = bad_pool_srv.clone();
                let switched_srv = switched_srv.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req_str = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req_str.lines().next().unwrap_or("");
                    let body_part = req_str.split("\r\n\r\n").nth(1).unwrap_or("");
                    let (status, body) = if first_line.starts_with("GET /proxies ") {
                        let j = serde_json::json!({
                            "proxies": {
                                "PROXY": { "type": "Selector", "now": "dead-leaf", "all": ["dead-leaf", "alive-leaf"] },
                                crate::routing::WHITELIST_GROUP_NAME: {
                                    "type": "Selector",
                                    "now": "dead-leaf",
                                    "all": ["dead-leaf", "alive-leaf"]
                                },
                                "dead-leaf": { "type": "Vless", "server": "1.2.3.4", "history": [] },
                                "alive-leaf": { "type": "Vless", "server": "5.6.7.8", "history": [] }
                            }
                        })
                        .to_string();
                        ("200 OK", j)
                    } else if first_line.starts_with("GET /providers/proxies/free_wl/dead-leaf/healthcheck") {
                        leaf_probe_srv.fetch_add(1, Ordering::SeqCst);
                        ("504 Gateway Timeout", "{\"message\":\"timeout\"}".to_string())
                    } else if first_line.starts_with("GET /providers/proxies/free_wl/free_wl/healthcheck") {
                        bad_pool_srv.fetch_add(1, Ordering::SeqCst);
                        ("404 Not Found", "{\"message\":\"not found\"}".to_string())
                    } else if first_line.starts_with("GET /providers/proxies ") {
                        let j = serde_json::json!({
                            "providers": {
                                "free_wl": {
                                    "vehicleType": "File",
                                    "proxies": [
                                        { "name": "dead-leaf", "server": "1.2.3.4", "port": 443, "type": "vless", "alive": false, "history": [] },
                                        { "name": "alive-leaf", "server": "5.6.7.8", "port": 443, "type": "vless", "alive": true, "history": [{ "delay": 180 }] }
                                    ]
                                }
                            }
                        })
                        .to_string();
                        ("200 OK", j)
                    } else if first_line.starts_with("GET /connections") {
                        ("200 OK", "{\"connections\":[]}".to_string())
                    } else if first_line.starts_with("PUT /proxies/") {
                        if let Ok(v) = serde_json::from_str::<Value>(body_part) {
                            if let Some(nm) = v.get("name").and_then(|x| x.as_str()) {
                                switched_srv.lock().await.push(nm.to_string());
                            }
                        }
                        ("204 No Content", String::new())
                    } else if first_line.starts_with("DELETE /connections") {
                        ("204 No Content", String::new())
                    } else {
                        ("404 Not Found", "{\"message\":\"not found\"}".to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.port = port;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.whitelist.enabled = true;
        cfg.whitelist_providers = vec!["free_wl".into()];
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap())
            .await
            .unwrap();

        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path.clone()),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        // Сбрасываем глобальное состояние статуса на случай параллельных тестов free_sub
        *FREE_WL_STATUS.write().await = FreeSubStatus {
            mode: FreeSubMode::Whitelist,
            ..Default::default()
        };
        *STALL_WATCHDOG_STREAK.lock().await = (None, 0);
        *IDLE_PROBE_TICKS.lock().await = (None, 0);

        // 3 тика при alive == false в провайдере Mihomo
        for _ in 0..3 {
            check_free_sub_stall_watchdog(&state).await;
        }

        srv_handle.abort();
        assert_eq!(bad_pool_probe_hits.load(Ordering::SeqCst), 0, "watchdog не должен пробовать имя виртуального пула free_wl");
        assert!(leaf_probe_hits.load(Ordering::SeqCst) >= 1, "watchdog должен проверить реальный лист dead-leaf через L7");
        let switches = switched_to.lock().await.clone();
        assert!(switches.iter().any(|s| s == "alive-leaf"), "watchdog должен переключить пул на живой узел alive-leaf, got {switches:?}");

        let cp = cache_file_path(&cfg_path);
        let cache = load_disk_cache(&cp).await;
        assert!(
            is_node_blacklisted(
                cache.mode_ref(FreeSubMode::Whitelist),
                &json!({"name": "dead-leaf", "type": "vless", "server": "1.2.3.4", "port": 443}),
                "dead-leaf",
                chrono::Utc::now().timestamp()
            ),
            "мёртвый узел dead-leaf должен попасть в 24ч blacklist"
        );
        let _ = std::fs::remove_dir_all(&dir);
        crate::mihomo::invalidate_proxies_cache();
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)]
    async fn regression_get_pool_and_history_resolves_virtual_pool_active_leaf() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        crate::mihomo::invalidate_proxies_cache();

        let dir = std::env::temp_dir().join(format!("xkeen_free_sub_pool_leaf_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prov_dir = dir.join("providers");
        std::fs::create_dir_all(&prov_dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        std::fs::write(&yaml_path, "mixed-port: 7892\n").unwrap();
        let wl_prov_yaml = "\
proxies:
  - name: \"node-a\"
    type: vless
    server: 93.184.216.34
    port: 443
    uuid: 11111111-1111-1111-1111-111111111111
  - name: \"node-b\"
    type: vless
    server: 93.184.216.35
    port: 443
    uuid: 11111111-1111-1111-1111-111111111112
";
        std::fs::write(prov_dir.join("free_wl.yaml"), wl_prov_yaml).unwrap();
        let cfg_path = dir.join("config.json");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv_handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req.lines().next().unwrap_or("");
                    let (status, body) = if first_line.starts_with("GET /proxies ") {
                        let json_body = json!({
                            "proxies": {
                                "PROXY": { "type": "Selector", "now": "DIRECT", "all": ["DIRECT"] },
                                crate::routing::WHITELIST_GROUP_NAME: { "type": "Selector", "now": "node-b", "all": ["node-a", "node-b"] },
                                "node-a": { "type": "Vless", "alive": true, "history": [] },
                                "node-b": { "type": "Vless", "alive": true, "history": [] }
                            }
                        }).to_string();
                        ("200 OK", json_body)
                    } else if first_line.starts_with("GET /providers/proxies ") {
                        let json_body = json!({
                            "providers": {
                                "free_wl": {
                                    "vehicleType": "HTTP",
                                    "proxies": [
                                        { "name": "node-a", "alive": true, "history": [] },
                                        { "name": "node-b", "alive": true, "history": [] }
                                    ]
                                }
                            }
                        }).to_string();
                        ("200 OK", json_body)
                    } else {
                        ("404 Not Found", "{}".to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.port = port;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.whitelist.enabled = true;
        cfg.whitelist_providers = vec!["free_wl".into()];
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path.clone()),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        let res = get_pool_and_history(&state, FreeSubMode::Whitelist).await;
        srv_handle.abort();

        assert_eq!(res.get("active_node").and_then(|v| v.as_str()), Some("node-b"));
        assert_eq!(res.get("pool_index").and_then(|v| v.as_u64()), Some(2), "res={res}");
        let nodes = res.get("nodes").and_then(|v| v.as_array()).unwrap();
        assert_eq!(nodes[0].get("is_active").and_then(|v| v.as_bool()), Some(false));
        assert_eq!(nodes[1].get("is_active").and_then(|v| v.as_bool()), Some(true));

        let _ = std::fs::remove_dir_all(&dir);
        crate::mihomo::invalidate_proxies_cache();
    }

    #[test]
    fn regression_tcp_ru_sni_prioritized_ahead_of_quic_udp_and_sorted_by_delay() {
        let make_cand = |name: &str, is_quic_udp: bool, ru_sni_score: u8| CandidateNode {
            proxy: json!({ "name": name, "type": if is_quic_udp { "hysteria2" } else { "vless" }, "server": "1.2.3.4", "port": 443 }),
            name: name.to_string(),
            server: "1.2.3.4".to_string(),
            port: 443,
            is_quic_udp,
            resolved_addrs: Vec::new(),
            subnet_key: "1.2.3.0/24".to_string(),
            ru_sni_score,
        };
        let tcp_passed = vec![
            make_cand("tcp-foreign", false, 0),
            make_cand("tcp-ru-vk", false, 1),
        ];
        let quic_udp = vec![
            make_cand("hy2-foreign", true, 0),
            make_cand("hy2-ru", true, 1),
        ];
        let ordered = prioritize_l7_ready_proxies(tcp_passed, quic_udp);
        let names: Vec<&str> = ordered
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(names, vec!["tcp-ru-vk", "tcp-foreign", "hy2-ru", "hy2-foreign"]);

        let mut verified = vec![
            (json!({"name": "slow"}), 1200),
            (json!({"name": "fast"}), 210),
            (json!({"name": "mid"}), 480),
        ];
        sort_l7_verified_by_delay(&mut verified);
        let sorted_names: Vec<&str> = verified
            .iter()
            .filter_map(|(v, _)| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(sorted_names, vec!["fast", "mid", "slow"]);

        assert!(should_upgrade_active_node(1200, false, 350, false));
        assert!(should_upgrade_active_node(800, true, 450, false));
        assert!(!should_upgrade_active_node(1200, false, 1000, false));
    }

    #[test]
    fn regression_blacklist_blocks_all_clones_sharing_same_server_port() {
        let clone1 = json!({
            "name": "Clone-A",
            "type": "vless",
            "server": "185.151.243.186",
            "port": 443,
            "uuid": "11111111-1111-1111-1111-111111111111"
        });
        let clone2 = json!({
            "name": "Clone-B",
            "type": "vless",
            "server": "185.151.243.186",
            "port": 443,
            "uuid": "22222222-2222-2222-2222-222222222222"
        });
        let other = json!({
            "name": "Other-Node",
            "type": "vless",
            "server": "91.108.4.1",
            "port": 443,
            "uuid": "33333333-3333-3333-3333-333333333333"
        });

        let mut mc = ModeCache::default();
        let now_ts = 1_700_000_000;
        blacklist_node_in_cache(&mut mc, Some(&clone1), "Clone-A", now_ts);

        assert!(is_node_blacklisted(&mc, &clone1, "Clone-A", now_ts + 10));
        assert!(
            is_node_blacklisted(&mc, &clone2, "Clone-B", now_ts + 10),
            "Клон с тем же server:port должен быть заблокирован в blacklist"
        );
        assert!(!is_node_blacklisted(&mc, &other, "Other-Node", now_ts + 10));

        let (merged, _) = merge_verified_with_active_session(
            vec![clone1, clone2, other],
            &[],
            None,
        );
        let merged_names: Vec<&str> = merged
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(merged_names, vec!["Clone-A", "Other-Node"]);
    }

    #[tokio::test]
    async fn regression_stall_watchdog_triggers_on_trickle_traffic_and_switch_pool_resolves_leaf() {
        assert!(is_stall_watchdog_tick(true, 4, 350, 180));
        assert!(is_stall_watchdog_tick(false, 0, 5000, 5000));
        assert!(!is_stall_watchdog_tick(true, 4, 2048, 128));
        assert!(!is_stall_watchdog_tick(true, 0, 0, 0));

        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::mihomo::invalidate_proxies_cache();
        let dir = std::env::temp_dir().join(format!("xkeen_free_sub_switch_leaf_{}", std::process::id()));
        let prov_dir = dir.join("providers");
        let _ = std::fs::create_dir_all(&prov_dir);
        let yaml_path = dir.join("config.yaml");
        let cfg_path = dir.join("config.json");
        std::fs::write(&yaml_path, "proxies: []\n").unwrap();
        let pool_yaml = proxies_to_mihomo_yaml(&[
            json!({"name": "node-a", "type": "vless", "server": "1.1.1.1", "port": 443, "uuid": "u1"}),
            json!({"name": "node-b", "type": "vless", "server": "2.2.2.2", "port": 443, "uuid": "u2"}),
        ])
        .unwrap();
        std::fs::write(prov_dir.join("free_wl.yaml"), pool_yaml).unwrap();

        let switched_to = Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
        let switched_clone = switched_to.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv_handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let sw = switched_clone.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).to_string();
                    let first_line = req.lines().next().unwrap_or("");
                    let (status, body) = if first_line.starts_with("PUT /proxies/") {
                        if let Some(body_part) = req.split("\r\n\r\n").nth(1) {
                            if let Ok(v) = serde_json::from_str::<Value>(body_part) {
                                if let Some(name) = v.get("name").and_then(|x| x.as_str()) {
                                    sw.lock().await.push(name.to_string());
                                }
                            }
                        }
                        ("204 No Content", String::new())
                    } else if first_line.contains("GET /providers/proxies") {
                        let json_body = serde_json::json!({
                            "providers": {
                                "free_wl": {
                                    "name": "free_wl",
                                    "vehicleType": "File",
                                    "proxies": [
                                        { "name": "node-a", "type": "VLESS", "alive": true },
                                        { "name": "node-b", "type": "VLESS", "alive": true }
                                    ]
                                }
                            }
                        }).to_string();
                        ("200 OK", json_body)
                    } else if first_line.contains("GET /proxies") {
                        let json_body = serde_json::json!({
                            "proxies": {
                                "PROXY": { "name": "PROXY", "type": "Selector", "now": "DIRECT", "all": ["DIRECT", "node-a", "node-b"] },
                                "🛡️ Whitelist": { "name": "🛡️ Whitelist", "type": "Selector", "now": "node-a", "all": ["node-a", "node-b"] },
                                "node-a": { "name": "node-a", "type": "VLESS", "alive": true },
                                "node-b": { "name": "node-b", "type": "VLESS", "alive": true }
                            }
                        }).to_string();
                        ("200 OK", json_body)
                    } else {
                        ("404 Not Found", "{}".to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.port = port;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.whitelist.enabled = true;
        cfg.whitelist_providers = vec!["free_wl".into()];
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        let chosen = switch_pool_node(&state, FreeSubMode::Whitelist, "next").await.unwrap();
        srv_handle.abort();
        assert_eq!(chosen, "node-b", "switch_pool_node('next') должен переключить с node-a на node-b");
        let calls = switched_to.lock().await;
        assert!(calls.iter().any(|c| c == "node-b"), "calls={calls:?}");

        let _ = std::fs::remove_dir_all(&dir);
        crate::mihomo::invalidate_proxies_cache();
    }

    #[test]
    fn regression_idle_push_sockets_do_not_trigger_stall_and_quic_upgrades_to_tcp() {
        // 1. Фоновые push-соединения (0 B/s up, 0 B/s down) при живом узле не должны считаться зависанием
        assert!(!is_stall_watchdog_tick(true, 12, 0, 0));
        // Капельный трафик при наличии исходящих запросов (up > 0) по-прежнему детектируется
        assert!(is_stall_watchdog_tick(true, 4, 300, 150));

        // 2. Переход с медленного QUIC/UDP (Hysteria2) на рабочий TCP-узел (даже при скоре 750 мс)
        assert!(should_upgrade_active_node(1200, true, 750, false));

        // 3. pick_faster_pool_candidate выбирает быстрый незабаненный TCP-узел вместо медленного Hysteria2
        let nodes = vec![
            json!({"name": "slow-hy2", "type": "hysteria2", "server": "1.1.1.1", "port": 443}),
            json!({"name": "banned-vless", "type": "vless", "server": "2.2.2.2", "port": 443}),
            json!({"name": "fast-vless", "type": "vless", "server": "3.3.3.3", "port": 443}),
        ];
        let mut mc = ModeCache::default();
        let now_ts = 1_700_000_000;
        blacklist_node_in_cache(&mut mc, Some(&nodes[1]), "banned-vless", now_ts);

        let picked = pick_faster_pool_candidate(
            &nodes,
            &mc,
            "slow-hy2",
            1250,
            true,
            now_ts + 10,
            |nm| match nm {
                "banned-vless" => Some(80),
                "fast-vless" => Some(280),
                _ => None,
            },
        );
        assert_eq!(picked, Some(("fast-vless".to_string(), 280)));
    }

    #[tokio::test]
    async fn regression_switch_pool_node_never_switches_to_same_node_or_writes_fake_history() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::mihomo::invalidate_proxies_cache();

        let dir = std::env::temp_dir().join(format!("xkeen_no_self_switch_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prov_dir = dir.join("providers");
        std::fs::create_dir_all(&prov_dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        let cfg_path = dir.join("config.json");
        std::fs::write(&yaml_path, "proxies: []\n").unwrap();
        let pool_yaml = proxies_to_mihomo_yaml(&[
            json!({"name": "only-node", "type": "vless", "server": "1.1.1.1", "port": 443, "uuid": "u1"}),
        ])
        .unwrap();
        std::fs::write(prov_dir.join("free_wl.yaml"), pool_yaml).unwrap();

        let put_calls = Arc::new(AtomicUsize::new(0));
        let put_srv = put_calls.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv_handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                let pc = put_srv.clone();
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req.lines().next().unwrap_or("");
                    let (status, body) = if first_line.starts_with("PUT /proxies/") {
                        pc.fetch_add(1, Ordering::SeqCst);
                        ("204 No Content", String::new())
                    } else if first_line.contains("GET /providers/proxies") {
                        ("200 OK", json!({
                            "providers": {
                                "free_wl": {
                                    "name": "free_wl",
                                    "vehicleType": "File",
                                    "proxies": [{ "name": "only-node", "type": "VLESS", "alive": true }]
                                }
                            }
                        }).to_string())
                    } else if first_line.contains("GET /proxies") {
                        ("200 OK", json!({
                            "proxies": {
                                "PROXY": { "name": "PROXY", "type": "Selector", "now": "DIRECT", "all": ["DIRECT", "only-node"] },
                                "🛡️ Whitelist": { "name": "🛡️ Whitelist", "type": "Selector", "now": "only-node", "all": ["only-node"] },
                                "only-node": { "name": "only-node", "type": "VLESS", "alive": true }
                            }
                        }).to_string())
                    } else {
                        ("404 Not Found", "{}".to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.port = port;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.whitelist.enabled = true;
        cfg.whitelist_providers = vec!["free_wl".into()];
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path.clone()),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        // Занимаем run-lock чтобы не запускать реальные сетевые источники в фоне
        let _run_guard = run_lock(FreeSubMode::Whitelist).lock().await;
        let res_next = switch_pool_node(&state, FreeSubMode::Whitelist, "next").await;
        let res_prev = switch_pool_node(&state, FreeSubMode::Whitelist, "prev").await;
        drop(_run_guard);
        srv_handle.abort();

        assert!(res_next.is_err(), "next при 1 узле должен возвращать ошибку, а не переключать на себя");
        assert!(res_prev.is_err(), "prev при 1 узле должен возвращать ошибку, а не переключать на себя");
        assert_eq!(put_calls.load(Ordering::SeqCst), 0, "PUT /proxies/ не должен вызываться при 1 узле");
        let cache = load_disk_cache(&cache_file_path(&cfg_path)).await;
        assert!(cache.mode_ref(FreeSubMode::Whitelist).sessions.is_empty(), "не должно создаваться фейковых записей в history");

        let _ = std::fs::remove_dir_all(&dir);
        crate::mihomo::invalidate_proxies_cache();
    }

    #[tokio::test]
    async fn regression_blacklist_and_next_last_node_triggers_refill_and_never_preserves_banned() {
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::mihomo::invalidate_proxies_cache();
        drop(run_lock(FreeSubMode::Whitelist).lock().await);
        reset_mode_status_idle(FreeSubMode::Whitelist).await;
        clear_bg_refill_failure(FreeSubMode::Whitelist);
        let dir = std::env::temp_dir().join(format!("xkeen_bl_last_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prov_dir = dir.join("providers");
        std::fs::create_dir_all(&prov_dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        let cfg_path = dir.join("config.json");
        std::fs::write(&yaml_path, "proxies: []\n").unwrap();

        let banned_node = json!({"name": "banned-leaf", "type": "vless", "server": "1.2.3.4", "port": 443, "uuid": "u1"});
        let fresh_node = json!({"name": "fresh-leaf", "type": "vless", "server": "5.6.7.8", "port": 443, "uuid": "u2"});
        std::fs::write(
            prov_dir.join("free_wl.yaml"),
            proxies_to_mihomo_yaml(&[banned_node.clone()]).unwrap(),
        )
        .unwrap();

        let mut mc = ModeCache::default();
        let now_ts = chrono::Utc::now().timestamp();
        blacklist_node_in_cache(&mut mc, Some(&banned_node), "banned-leaf", now_ts);
        let (merged, preserved) = merge_verified_with_active_session_checked(
            vec![fresh_node.clone()],
            &[banned_node.clone()],
            Some("banned-leaf"),
            &mc,
            now_ts + 5,
        );
        assert!(!preserved, "забаненный активный узел не должен сохраняться через preserved_active");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0]["name"], "fresh-leaf");

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.mihomo.port = 1; // unreachable mock port
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path.clone()),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        let res = blacklist_and_next(&state, FreeSubMode::Whitelist, Some("banned-leaf"))
            .await
            .expect("blacklist_and_next на последнем узле не должен падать");
        assert_eq!(res.active_node, None);
        assert!(res.refill_started, "при исчерпании пула должен автоматически запускаться подбор нового узла");
        bump_mode_generation(FreeSubMode::Whitelist);
        reset_mode_status_idle(FreeSubMode::Whitelist).await;
        crate::mihomo::invalidate_proxies_cache();

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn regression_l7_batch_probe_stop_after_first_returns_immediately() {
        let nodes: Vec<Value> = (0..8)
            .map(|i| json!({ "name": format!("n-{i}") }))
            .collect();
        let started = Instant::now();
        let res = probe_l7_batch_with(nodes, 4, true, |node| async move {
            let nm = node.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if nm == "n-0" {
                tokio::time::sleep(Duration::from_millis(15)).await;
                (node, Ok(15))
            } else {
                tokio::time::sleep(Duration::from_millis(4000)).await;
                (node, Err("timeout".into()))
            }
        })
        .await;
        let elapsed = started.elapsed();
        assert_eq!(res.len(), 1);
        assert_eq!(res[0].0["name"], "n-0");
        assert!(
            elapsed < Duration::from_millis(500),
            "stop_after_first=true должен возвращаться сразу после первого рабочего узла, а не ждать 4000мс (elapsed={elapsed:?})"
        );
    }

    #[tokio::test]
    async fn regression_enable_both_free_subs_creates_probe_provider_immediately() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = std::env::temp_dir().join(format!("xkeen_enable_probe_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");
        let yaml_path = dir.join("config.yaml");
        let base_yaml = "mixed-port: 7892\nproxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";
        std::fs::write(&yaml_path, base_yaml).unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let _ = socket.read(&mut buf).await;
                    let resp = "HTTP/1.1 204 No Content\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.port = port;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        enable_both_free_subs(&state).await.expect("enable_both_free_subs ok");
        srv.abort();

        let updated_yaml = tokio::fs::read_to_string(&yaml_path).await.unwrap();
        let names = crate::routing::parse_provider_names(&updated_yaml);
        assert!(names.iter().any(|p| p == FREE_SUB_PROBE_PROVIDER_ID), "names={names:?}");
        assert!(names.iter().any(|p| p == FREE_WL_PROVIDER_ID), "names={names:?}");
        assert!(names.iter().any(|p| p == FREE_VPN_PROVIDER_ID), "names={names:?}");
        assert!(dir.join("providers").join(format!("{FREE_SUB_PROBE_PROVIDER_ID}.yaml")).exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn regression_stall_watchdog_skips_while_job_is_running_and_uses_short_ttl() {
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        update_status(FreeSubMode::Whitelist, |st| {
            st.running = true;
        })
        .await;
        // Watchdog должен мгновенно выйти при running == true, не делая HTTP-запросов
        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.port = 1;
        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(std::env::temp_dir().join("xkeen_wd_skip.json")),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };
        check_free_sub_stall_watchdog(&state).await;
        update_status(FreeSubMode::Whitelist, |st| {
            st.running = false;
        })
        .await;

        let mut mc = ModeCache::default();
        let node = json!({"name": "wd-node", "type": "vless", "server": "1.2.3.4", "port": 443, "uuid": "u1"});
        let now = 1_700_000_000i64;
        blacklist_node_in_cache_ttl(&mut mc, Some(&node), "wd-node", now, WATCHDOG_BLACKLIST_TTL_SECS);
        assert!(is_node_blacklisted(&mc, &node, "wd-node", now + 300));
        assert!(!is_node_blacklisted(&mc, &node, "wd-node", now + WATCHDOG_BLACKLIST_TTL_SECS + 1));
    }

    #[tokio::test]
    async fn regression_explicit_switch_pool_node_unblacklists_manual_reconnect() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::mihomo::invalidate_proxies_cache();

        let dir = std::env::temp_dir().join(format!("xkeen_unblacklist_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prov_dir = dir.join("providers");
        std::fs::create_dir_all(&prov_dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        let cfg_path = dir.join("config.json");
        std::fs::write(&yaml_path, "proxies: []\n").unwrap();

        let n_a = json!({"name": "node-a", "type": "vless", "server": "1.1.1.1", "port": 443, "uuid": "u1"});
        let n_b = json!({"name": "node-b", "type": "vless", "server": "2.2.2.2", "port": 443, "uuid": "u2"});
        std::fs::write(prov_dir.join("free_wl.yaml"), proxies_to_mihomo_yaml(&[n_a.clone(), n_b.clone()]).unwrap()).unwrap();

        let cp = cache_file_path(&cfg_path);
        let now_ts = chrono::Utc::now().timestamp();
        let mut cache = FreeSubCache::default();
        blacklist_node_in_cache(&mut cache.whitelist, Some(&n_b), "node-b", now_ts);
        save_disk_cache(&cp, &cache).await.unwrap();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { break };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first_line = req.lines().next().unwrap_or("");
                    let (status, body) = if first_line.starts_with("PUT /proxies/") {
                        ("204 No Content", String::new())
                    } else if first_line.contains("GET /providers/proxies") {
                        ("200 OK", json!({
                            "providers": {
                                "free_wl": {
                                    "name": "free_wl",
                                    "vehicleType": "File",
                                    "proxies": [
                                        { "name": "node-a", "type": "VLESS", "alive": true },
                                        { "name": "node-b", "type": "VLESS", "alive": true }
                                    ]
                                }
                            }
                        }).to_string())
                    } else if first_line.contains("GET /proxies") {
                        ("200 OK", json!({
                            "proxies": {
                                "PROXY": { "name": "PROXY", "type": "Selector", "now": "DIRECT", "all": ["DIRECT", "node-a", "node-b"] },
                                "🛡️ Whitelist": { "name": "🛡️ Whitelist", "type": "Selector", "now": "node-a", "all": ["node-a", "node-b"] },
                                "node-a": { "name": "node-a", "type": "VLESS", "alive": true },
                                "node-b": { "name": "node-b", "type": "VLESS", "alive": true }
                            }
                        }).to_string())
                    } else {
                        ("404 Not Found", "{}".to_string())
                    };
                    let resp = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.port = port;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.whitelist.enabled = true;
        tokio::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).await.unwrap();

        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path.clone()),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        let chosen = switch_pool_node(&state, FreeSubMode::Whitelist, "node-b").await.unwrap();
        srv.abort();
        assert_eq!(chosen, "node-b");
        let after_cache = load_disk_cache(&cp).await;
        assert!(
            !is_node_blacklisted(after_cache.mode_ref(FreeSubMode::Whitelist), &n_b, "node-b", now_ts + 10),
            "ручное переключение на узел из истории должно снимать его из ЧС"
        );

        let _ = std::fs::remove_dir_all(&dir);
        crate::mihomo::invalidate_proxies_cache();
    }

    #[test]
    fn regression_generation_cancellation_stops_l7_loop_immediately() {
        let src = include_str!("mod.rs");
        let prod = &src[..src.find("#[cfg(test)]").unwrap_or(src.len())];
        assert!(prod.contains("FOREGROUND_L7_MAX_CANDIDATES"));
        assert!(prod.contains("FOREGROUND_L7_BUDGET_SECS"));
        assert!(prod.contains("WATCHDOG_BLACKLIST_TTL_SECS"));
        assert!(prod.matches("if current_mode_generation(mode) != my_gen").count() >= 5);
    }

    #[test]
    fn regression_pick_faster_pool_candidate_never_fabricates_450ms_for_uncached_node() {
        let nodes = vec![
            json!({"name": "slow-vless", "type": "vless", "server": "1.1.1.1", "port": 443}),
            json!({"name": "uncached-vless", "type": "vless", "server": "2.2.2.2", "port": 443}),
        ];
        let mut mc = ModeCache::default();
        let now_ts = 1_700_000_000;

        // Без замера в PING_CACHE и без last_delay_ms на диске — НЕ выдумывать 450 мс!
        let picked = pick_faster_pool_candidate(
            &nodes,
            &mc,
            "slow-vless",
            2500,
            false,
            now_ts,
            |_| None,
        );
        assert_eq!(picked, None, "must not fabricate 450ms for uncached candidate");

        // Если реальный last_delay_ms есть в working_nodes и он действительно быстрее (>= 2x), он используется
        mc.working_nodes.push(CachedWorkingNode {
            proxy: nodes[1].clone(),
            last_ok_ts: now_ts,
            last_delay_ms: 300,
            quality_score: 0,
            mode: FreeSubMode::Whitelist,
            high_priority: false,
            total_bytes: 0,
        });
        let picked_disk = pick_faster_pool_candidate(
            &nodes,
            &mc,
            "slow-vless",
            2500,
            false,
            now_ts,
            |_| None,
        );
        assert_eq!(picked_disk, Some(("uncached-vless".to_string(), 300)));
    }

    #[tokio::test]
    async fn regression_get_pool_and_history_uses_disk_delay_when_ping_cache_expired() {
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::mihomo::invalidate_proxies_cache();

        let dir = std::env::temp_dir().join(format!("xkeen_pool_ping_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let prov_dir = dir.join("providers");
        std::fs::create_dir_all(&prov_dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        let cfg_path = dir.join("config.json");
        std::fs::write(&yaml_path, "proxies: []\n").unwrap();

        let node_a = json!({"name": "disk-node-a", "type": "vless", "server": "11.22.33.44", "port": 443});
        let yaml = crate::subscription_parser::proxies_to_mihomo_yaml(&[node_a.clone()]).unwrap();
        std::fs::write(prov_dir.join("free_wl.yaml"), yaml).unwrap();

        let cp = cache_file_path(&cfg_path);
        let now_ts = chrono::Utc::now().timestamp();
        let mut cache = FreeSubCache::default();
        cache.whitelist.working_nodes.push(CachedWorkingNode {
            proxy: node_a,
            last_ok_ts: now_ts,
            last_delay_ms: 615,
            quality_score: 0,
            mode: FreeSubMode::Whitelist,
            high_priority: false,
            total_bytes: 0,
        });
        save_disk_cache(&cp, &cache).await.unwrap();

        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.port = 1;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        let pool = get_pool_and_history(&state, FreeSubMode::Whitelist).await;
        let nodes_arr = pool.get("nodes").and_then(|v| v.as_array()).unwrap();
        assert_eq!(nodes_arr.len(), 1);
        assert_eq!(nodes_arr[0].get("ping_ms").and_then(|v| v.as_i64()), Some(615));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn regression_vpn_and_whitelist_status_and_run_locks_are_isolated() {
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = std::env::temp_dir().join(format!("xkeen_free_sub_iso_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");
        let cfg = crate::config::AppConfig::default();
        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        reset_free_sub_runtime_and_cache(&state).await;

        // 1. Занятый run_lock(Whitelist) не блокирует run_lock(Vpn)
        let _wl_guard = run_lock(FreeSubMode::Whitelist).lock().await;
        assert!(
            run_lock(FreeSubMode::Vpn).try_lock().is_ok(),
            "фоновый добор Whitelist не должен блокировать запуск Vpn"
        );

        // 2. Параллельные обновления статусов Vpn и Whitelist не перетирают друг друга
        let vpn_task = async {
            for i in 1..=50usize {
                update_status(FreeSubMode::Vpn, |st| {
                    st.running = true;
                    st.stage = FreeSubStage::L7;
                    st.stats.working_count = i;
                    st.active_node = Some(format!("vpn-node-{i}"));
                    st.updated_at = Some(100);
                })
                .await;
                tokio::task::yield_now().await;
            }
        };
        let wl_task = async {
            for i in 1..=50usize {
                update_status(FreeSubMode::Whitelist, |st| {
                    st.running = true;
                    st.stage = FreeSubStage::Prefilter;
                    st.stats.working_count = i + 100;
                    st.active_node = Some(format!("wl-node-{i}"));
                    st.updated_at = Some(200);
                })
                .await;
                tokio::task::yield_now().await;
            }
        };
        tokio::join!(vpn_task, wl_task);

        let vpn_st = get_status_for(FreeSubMode::Vpn).await;
        let wl_st = get_status_for(FreeSubMode::Whitelist).await;
        assert_eq!(vpn_st.mode, FreeSubMode::Vpn);
        assert_eq!(vpn_st.stage, FreeSubStage::L7);
        assert_eq!(vpn_st.stats.working_count, 50);
        assert_eq!(vpn_st.active_node.as_deref(), Some("vpn-node-50"));

        assert_eq!(wl_st.mode, FreeSubMode::Whitelist);
        assert_eq!(wl_st.stage, FreeSubStage::Prefilter);
        assert_eq!(wl_st.stats.working_count, 150);
        assert_eq!(wl_st.active_node.as_deref(), Some("wl-node-50"));

        // 3. HTTP-хэндлер get_free_sub_status поддерживает ?mode= и возвращает оба statuses
        let resp_vpn = crate::api::servers::subscriptions::get_free_sub_status(
            axum::extract::Query(crate::api::servers::subscriptions::FreeSubStatusQuery {
                mode: Some(FreeSubMode::Vpn),
            }),
        )
        .await;
        let bytes_vpn = axum::body::to_bytes(resp_vpn.into_body(), 65536).await.unwrap();
        let val_vpn: serde_json::Value = serde_json::from_slice(&bytes_vpn).unwrap();
        assert_eq!(val_vpn["data"]["status"]["mode"], "vpn");
        assert_eq!(val_vpn["data"]["status"]["active_node"], "vpn-node-50");
        assert_eq!(val_vpn["data"]["statuses"]["whitelist"]["active_node"], "wl-node-50");

        // 4. reset_free_sub_runtime_and_cache сбрасывает оба режима
        drop(_wl_guard);
        reset_free_sub_runtime_and_cache(&state).await;
        let (after_vpn, after_wl) = get_all_statuses().await;
        assert!(!after_vpn.running && after_vpn.stage == FreeSubStage::Idle && after_vpn.mode == FreeSubMode::Vpn);
        assert!(!after_wl.running && after_wl.stage == FreeSubStage::Idle && after_wl.mode == FreeSubMode::Whitelist);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn regression_interleave_source_nodes_prioritizes_and_round_robins() {
        let s1 = vec![json!({"name": "yd-1"}), json!({"name": "yd-2"})];
        let s2 = vec![json!({"name": "gh-1"}), json!({"name": "gh-2"}), json!({"name": "gh-3"})];
        let mixed = interleave_source_nodes(vec![s1, s2], 10);
        let names: Vec<&str> = mixed
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(names, vec!["yd-1", "gh-1", "yd-2", "gh-2", "gh-3"]);
        let capped = interleave_source_nodes(
            vec![vec![json!({"name": "a1"}), json!({"name": "a2"})], vec![json!({"name": "b1"})]],
            2,
        );
        assert_eq!(capped.len(), 2);
    }

    #[tokio::test]
    async fn regression_l7_batch_grace_window_prefers_healthy_node_over_fast_penalized_node() {
        let nodes = vec![json!({"name": "penalized-fast"}), json!({"name": "healthy-node"})];
        let res = probe_l7_batch_metrics_with(nodes, 2, true, |node| async move {
            let nm = node.get("name").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if nm == "penalized-fast" {
                tokio::time::sleep(Duration::from_millis(15)).await;
                (
                    node,
                    Ok(mihomo::L7ProbeMetrics {
                        rtt_ms: 100,
                        quality_score: 4300,
                    }),
                )
            } else {
                tokio::time::sleep(Duration::from_millis(120)).await;
                (
                    node,
                    Ok(mihomo::L7ProbeMetrics {
                        rtt_ms: 180,
                        quality_score: 320,
                    }),
                )
            }
        })
        .await;
        assert_eq!(res.len(), 2);
        assert_eq!(res[0].0["name"], "healthy-node");
        assert_eq!(res[0].1.quality_score, 320);
    }

    #[test]
    fn regression_pick_faster_pool_candidate_compares_quality_score_not_raw_ping() {
        let nodes = vec![
            json!({"name": "active-node", "type": "vless", "server": "1.1.1.1", "port": 443}),
            json!({"name": "dead-tg-cand", "type": "vless", "server": "2.2.2.2", "port": 443}),
        ];
        let mut mc = ModeCache::default();
        let now_ts = 1_700_000_000;
        mc.working_nodes.push(CachedWorkingNode {
            proxy: nodes[1].clone(),
            last_ok_ts: now_ts,
            last_delay_ms: 90,
            quality_score: 4200,
            mode: FreeSubMode::Whitelist,
            high_priority: false,
            total_bytes: 0,
        });
        let picked = pick_faster_pool_candidate(
            &nodes,
            &mc,
            "active-node",
            1100,
            false,
            now_ts,
            |_| Some(90),
        );
        assert_eq!(
            picked, None,
            "нельзя переключаться на кандидата с quality_score=4200 только из-за низкого raw RTT 90 мс"
        );
    }

    #[test]
    fn regression_select_best_next_pool_node_prefers_alive_tcp_and_lowest_score() {
        let names = vec![
            "cur-node".to_string(),
            "dead-node".to_string(),
            "quic-node".to_string(),
            "alive-slow".to_string(),
            "alive-fast".to_string(),
        ];
        let nodes = vec![
            json!({"name": "cur-node", "type": "vless", "server": "1.1.1.1", "port": 443}),
            json!({"name": "dead-node", "type": "vless", "server": "2.2.2.2", "port": 443}),
            json!({"name": "quic-node", "type": "hysteria2", "server": "3.3.3.3", "port": 443}),
            json!({"name": "alive-slow", "type": "vless", "server": "4.4.4.4", "port": 443}),
            json!({"name": "alive-fast", "type": "vless", "server": "5.5.5.5", "port": 443}),
        ];
        let mut mc = ModeCache::default();
        mc.working_nodes.push(CachedWorkingNode {
            proxy: nodes[1].clone(),
            last_ok_ts: 100,
            last_delay_ms: 80,
            quality_score: 120,
            mode: FreeSubMode::Whitelist,
            high_priority: false,
            total_bytes: 0,
        });
        mc.working_nodes.push(CachedWorkingNode {
            proxy: nodes[3].clone(),
            last_ok_ts: 100,
            last_delay_ms: 350,
            quality_score: 700,
            mode: FreeSubMode::Whitelist,
            high_priority: false,
            total_bytes: 0,
        });
        mc.working_nodes.push(CachedWorkingNode {
            proxy: nodes[4].clone(),
            last_ok_ts: 100,
            last_delay_ms: 140,
            quality_score: 260,
            mode: FreeSubMode::Whitelist,
            high_priority: false,
            total_bytes: 0,
        });
        let mut prov_proxies = std::collections::BTreeMap::new();
        prov_proxies.insert("dead-node".to_string(), json!({"alive": false}));
        prov_proxies.insert("quic-node".to_string(), json!({"alive": true}));
        prov_proxies.insert("alive-slow".to_string(), json!({"alive": true}));
        prov_proxies.insert("alive-fast".to_string(), json!({"alive": true}));

        let picked = select_best_next_pool_node(
            &names,
            &nodes,
            &mc,
            Some("cur-node"),
            Some(&prov_proxies),
        );
        assert_eq!(picked.as_deref(), Some("alive-fast"));
    }

    #[test]
    fn regression_amnesty_blacklist_gradual_preserves_manual_bans_and_newest_entries() {
        let mut mc = ModeCache::default();
        let now_ts = 1_700_000_000;
        mc.blacklist.insert("expired".into(), now_ts - 10);
        mc.blacklist.insert("wd-short".into(), now_ts + WATCHDOG_BLACKLIST_TTL_SECS);
        mc.blacklist.insert("manual-old".into(), now_ts + BLACKLIST_TTL_SECS - 100);
        mc.blacklist.insert("manual-new".into(), now_ts + BLACKLIST_TTL_SECS);

        let removed1 = amnesty_blacklist_gradual(&mut mc, now_ts);
        assert!(removed1 >= 1);
        assert!(!mc.blacklist.contains_key("wd-short"));
        assert!(mc.blacklist.contains_key("manual-old"));
        assert!(mc.blacklist.contains_key("manual-new"));

        let removed2 = amnesty_blacklist_gradual(&mut mc, now_ts);
        assert_eq!(removed2, 1);
        assert!(!mc.blacklist.contains_key("manual-old"));
        assert!(mc.blacklist.contains_key("manual-new"));
    }

    #[tokio::test]
    async fn regression_sync_mode_whitelist_reapplies_yaml_when_whitelist_block_missing() {
        use axum::{extract::Request, http::StatusCode, response::IntoResponse, Json, Router};
        use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrd};

        let dir = std::env::temp_dir().join(format!(
            "xkeen_wl_reapply_test_{}_{}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg_path = dir.join("config.json");
        let yaml_path = dir.join("config.yaml");

        let stale_yaml = "mixed-port: 7892\nproxies: []\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - DIRECT\n      - '🇷🇺 VK — #2'\nrules:\n  - MATCH,DIRECT\n";
        std::fs::write(&yaml_path, stale_yaml).unwrap();

        let reload_calls = Arc::new(AtomicUsize::new(0));
        let reload_calls_srv = reload_calls.clone();
        let app = Router::new().fallback(move |req: Request| {
            let reload_calls_srv = reload_calls_srv.clone();
            async move {
                let method = req.method().clone();
                let path = req.uri().path().to_string();
                if method == axum::http::Method::PUT && path == "/configs" {
                    reload_calls_srv.fetch_add(1, AtomicOrd::SeqCst);
                    return StatusCode::NO_CONTENT.into_response();
                }
                if method == axum::http::Method::GET && path == "/proxies" {
                    return Json(json!({
                        "proxies": {
                            "PROXY": { "type": "Selector", "now": "🇷🇺 VK — #2", "all": ["DIRECT", "🇷🇺 VK — #2"] },
                            "🛡️ Whitelist": { "type": "Selector", "now": "🇷🇺 VK — #2", "all": ["DIRECT", "🇷🇺 VK — #2"] },
                        }
                    }))
                    .into_response();
                }
                StatusCode::NO_CONTENT.into_response()
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let mut cfg = AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.port = port;
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.whitelist.enabled = true;
        cfg.whitelist.strategy = crate::config::WhitelistStrategy::StrictXKeen;
        std::fs::write(&cfg_path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();

        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(cfg_path),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        sync_mode_whitelist_state(&state, FreeSubMode::Whitelist, Some("🇷🇺 VK — #2"))
            .await
            .unwrap();

        let updated_yaml = std::fs::read_to_string(&yaml_path).unwrap();
        assert!(yaml_has_whitelist_routing(&updated_yaml));
        assert!(updated_yaml.contains(crate::routing::WHITELIST_BEGIN));
        assert!(updated_yaml.contains(crate::routing::WHITELIST_GROUP_NAME));
        assert!(reload_calls.load(AtomicOrd::SeqCst) >= 1);

        srv.abort();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn regression_bg_refill_failure_and_cancellation_do_not_clobber_active_status() {
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let _lock = run_lock(FreeSubMode::Whitelist).lock().await;
        clear_bg_refill_failure(FreeSubMode::Whitelist);
        reset_mode_status_idle(FreeSubMode::Whitelist).await;

        update_status(FreeSubMode::Whitelist, |st| {
            st.running = false;
            st.stage = FreeSubStage::Connected;
            st.stage_label = "Подключено (🇷🇺 VK — #2)".into();
            st.active_node = Some("🇷🇺 VK — #2".into());
            st.stats.working_count = 3;
            st.error = None;
        })
        .await;

        // 1. Фоновый добор сохраняет active_node и working_count при старте
        {
            let mut st = status_lock(FreeSubMode::Whitelist).write().await;
            init_job_status(
                &mut st,
                FreeSubMode::Whitelist,
                false,
                "Фоновый добор свежих узлов…".into(),
            );
            assert!(st.running);
            assert_eq!(st.active_node.as_deref(), Some("🇷🇺 VK — #2"));
            assert_eq!(st.stats.working_count, 3);
            assert_eq!(st.error, None);
        }

        // 2. Провал фонового добора не выставляет FreeSubStage::Error и error, а сохраняет Connected + cooldown
        let gen1 = bump_mode_generation(FreeSubMode::Whitelist);
        let res = apply_job_failure_to_status(FreeSubMode::Whitelist, false, gen1, "0 / 96")
            .await
            .expect("non-destructive refill failure returns status");
        assert!(!res.running);
        assert_eq!(res.stage, FreeSubStage::Connected);
        assert_eq!(res.active_node.as_deref(), Some("🇷🇺 VK — #2"));
        assert_eq!(res.error, None);
        assert!(is_bg_refill_in_cooldown(
            FreeSubMode::Whitelist,
            chrono::Utc::now().timestamp()
        ));

        // 3. Отменённая задача (устаревшее поколение) не трогает статус
        let _gen2 = bump_mode_generation(FreeSubMode::Whitelist);
        let cancelled =
            apply_job_failure_to_status(FreeSubMode::Whitelist, true, gen1, "Операция отменена пользователем")
                .await;
        assert!(cancelled.is_none());
        let after_cancel = get_status_for(FreeSubMode::Whitelist).await;
        assert_eq!(after_cancel.stage, FreeSubStage::Connected);
        assert_eq!(after_cancel.error, None);

        // 4. Сброс в чистый Idle при удалении / повторном добавлении
        reset_mode_status_idle(FreeSubMode::Whitelist).await;
        clear_bg_refill_failure(FreeSubMode::Whitelist);
        let idle = get_status_for(FreeSubMode::Whitelist).await;
        assert_eq!(idle.stage, FreeSubStage::Idle);
        assert!(!idle.running);
        assert_eq!(idle.active_node, None);
        assert_eq!(idle.error, None);
    }

    #[test]
    fn regression_l7_recent_fails_demotes_dead_endpoints_to_queue_tail() {
        let now_ts = 1_700_000_000;
        let nodes = vec![
            json!({"name": "dead-1", "server": "1.1.1.1", "port": 443}),
            json!({"name": "fresh-1", "server": "2.2.2.2", "port": 443}),
            json!({"name": "expired-fail", "server": "3.3.3.3", "port": 443}),
            json!({"name": "dead-2", "server": "4.4.4.4", "port": 443}),
            json!({"name": "fresh-2", "server": "5.5.5.5", "port": 443}),
        ];
        let mut recent = HashMap::new();
        recent.insert("ep:1.1.1.1:443".into(), now_ts - 30);
        recent.insert("ep:4.4.4.4:443".into(), now_ts - 120);
        recent.insert("ep:3.3.3.3:443".into(), now_ts - L7_RECENT_FAIL_TTL_SECS - 5);

        let reordered = demote_recent_l7_failed_proxies(nodes, &recent, now_ts);
        let names: Vec<&str> = reordered
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(
            names,
            vec!["fresh-1", "expired-fail", "fresh-2", "dead-1", "dead-2"]
        );
    }

    #[test]
    fn regression_bs_priority_ranks_xhttp_vk_cidr_nodes_ahead_of_generic_eu_nodes() {
        let vk_xhttp = json!({
            "name": "🇷🇺 VK — #2",
            "type": "vless",
            "server": "185.86.145.173",
            "port": 443,
            "network": "xhttp",
            "servername": "wholebeam.com"
        });
        let cidr_ws = json!({
            "name": "Anycast | [*CIDR]",
            "type": "vless",
            "server": "185.178.208.167",
            "port": 443,
            "network": "ws",
            "servername": "sw-api.telemetryapi.org"
        });
        let eu_reality = json!({
            "name": "🇩🇪 Germany — #18",
            "type": "vless",
            "server": "176.9.141.110",
            "port": 443,
            "network": "tcp",
            "servername": "www.cloudflare.com"
        });

        assert!(bs_candidate_priority(&vk_xhttp) > bs_candidate_priority(&eu_reality));
        assert!(bs_candidate_priority(&cidr_ws) > bs_candidate_priority(&eu_reality));
        assert_eq!(bs_candidate_priority(&eu_reality), 0);

        let make_cand = |p: Value| {
            let ip: IpAddr = p["server"].as_str().unwrap().parse().unwrap();
            CandidateNode {
                is_quic_udp: false,
                ru_sni_score: bs_candidate_priority(&p),
                name: p["name"].as_str().unwrap().to_string(),
                server: ip.to_string(),
                port: 443,
                resolved_addrs: vec![SocketAddr::new(ip, 443)],
                subnet_key: subnet_prefix_key(ip),
                proxy: p,
            }
        };
        let (primary, deferred) = apply_soft_subnet_quota(
            vec![make_cand(eu_reality), make_cand(cidr_ws), make_cand(vk_xhttp)],
            SOFT_SUBNET_QUOTA,
            true,
        );
        let ordered = prioritize_l7_ready_proxies(
            primary.into_iter().chain(deferred).collect(),
            Vec::new(),
        );
        let names: Vec<&str> = ordered
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(names, vec!["🇷🇺 VK — #2", "Anycast | [*CIDR]", "🇩🇪 Germany — #18"]);
    }

    #[test]
    fn regression_refill_preserves_existing_non_blacklisted_pool_nodes() {
        let now_ts = 1_700_000_000;
        let n1 = json!({"name": "Node-1", "type": "vless", "server": "1.1.1.1", "port": 443});
        let n2 = json!({"name": "Node-2", "type": "vless", "server": "2.2.2.2", "port": 443});
        let n3 = json!({"name": "Node-3", "type": "vless", "server": "3.3.3.3", "port": 443});
        let n4 = json!({"name": "Node-4", "type": "vless", "server": "4.4.4.4", "port": 443});
        let mc = ModeCache::default();

        // При preserve_existing_pool = true (auto_connect = false) пул НЕ сжимается 3 -> 1
        let (merged, preserved) = merge_verified_with_pool_checked(
            vec![n4.clone()],
            &[n1.clone(), n2.clone(), n3.clone()],
            Some("Node-1"),
            &mc,
            now_ts,
            true,
        );
        assert!(preserved);
        let names: Vec<&str> = merged
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        assert_eq!(names, vec!["Node-1", "Node-4", "Node-2", "Node-3"]);
    }

    #[test]
    fn regression_bg_refill_sets_cooldown_when_zero_new_nodes_added_and_updates_stage_label() {
        let mut mc = ModeCache::default();
        let now_ts = 1_700_000_000;
        for i in 0..20 {
            let p = json!({"name": format!("Old-{i}"), "type": "vless", "server": format!("10.20.0.{}", i + 1), "port": 443});
            mc.working_nodes.push(CachedWorkingNode {
                proxy: p,
                last_ok_ts: now_ts - 1000,
                last_delay_ms: 300,
                quality_score: 300,
                mode: FreeSubMode::Whitelist,
                high_priority: i == 0,
                total_bytes: if i == 0 { 2_000_000 } else { 0 },
            });
        }
        let fresh = json!({"name": "New-Fresh", "type": "vless", "server": "8.8.4.4", "port": 443});
        let fresh_metrics = mihomo::L7ProbeMetrics { rtt_ms: 450, quality_score: 650 };
        upsert_working_nodes_in_cache(&mut mc, FreeSubMode::Whitelist, &[(fresh, fresh_metrics)], now_ts);
        assert_eq!(mc.working_nodes.len(), 20);
        assert!(
            mc.working_nodes.iter().any(|w| w.proxy.get("name").and_then(|v| v.as_str()) == Some("New-Fresh")),
            "Новый узел не должен обрезаться при truncate(20)"
        );
        assert!(
            mc.working_nodes.iter().any(|w| w.proxy.get("name").and_then(|v| v.as_str()) == Some("Old-0") && w.high_priority && w.total_bytes == 2_000_000),
            "high_priority и total_bytes не должны затираться"
        );

        let src = include_str!("mod.rs");
        let bg_start = src.find("async fn run_background_l7_refill(").expect("run_background_l7_refill");
        let bg_end = src[bg_start..]
            .find("pub async fn enable_both_free_subs(")
            .map(|o| bg_start + o)
            .unwrap_or(src.len());
        let bg_body = &src[bg_start..bg_end];
        assert!(bg_body.contains("record_bg_refill_failure(mode"), "При 0 новых узлов должен ставиться cooldown");
        assert!(bg_body.contains("BgRefillActiveGuard::acquire(mode)"), "Фон должен защищаться BgRefillActiveGuard");
    }

    #[test]
    fn regression_free_sub_reconnect_restores_evicted_history_node_and_partial_source_stream() {
        // 1. Проверка частичного префикса для больших списков (>16 КБ)
        let mut big = String::new();
        for i in 0..300 {
            big.push_str(&format!(
                "vless://11111111-1111-1111-1111-111111111111@1.2.3.{}:443#Node-{i}\n",
                (i % 200) + 1
            ));
        }
        big.push_str("vless://truncated-without-newline");
        let trimmed = trim_partial_utf8_lines(big.as_bytes()).expect("prefix must be extracted");
        let text = String::from_utf8(trimmed).unwrap();
        assert!(text.ends_with('\n'));
        assert!(!text.contains("truncated-without-newline"));

        // 2. Вшитый VPN сид не должен содержать loopback 127.0.0.53
        let (_parsed, rejected) = parse_subscription_nodes_bounded(EMBEDDED_VPN_SEED, MAX_PARSED_NODES);
        assert!(rejected.is_empty(), "embedded_vpn_seed.txt не должен содержать запрещённых узлов: {:?}", rejected);

        // 3. Проверка наличия ветки восстановления вытесненных узлов из working_nodes в switch_pool_node_internal
        let src = include_str!("mod.rs");
        let sw_start = src.find("async fn switch_pool_node_internal(").expect("switch_pool_node_internal");
        let sw_end = src[sw_start..]
            .find("pub async fn blacklist_and_next(")
            .map(|o| sw_start + o)
            .unwrap_or(src.len());
        let sw_body = &src[sw_start..sw_end];
        assert!(sw_body.contains(".working_nodes") && sw_body.contains("merge_verified_with_pool_checked"));
    }

    #[tokio::test]
    async fn regression_disconnect_free_sub_preserves_bs_fallback_and_ignores_inactive_mode() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let _cache_guard = crate::mihomo::PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        crate::mihomo::invalidate_proxies_cache();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let srv_handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else { return };
                tokio::spawn(async move {
                    let mut buf = [0u8; 4096];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    let first = req.lines().next().unwrap_or("");
                    let (status, body) = if first.starts_with("PUT /configs") {
                        ("204 No Content", "")
                    } else if first.starts_with("GET /proxies") {
                        ("200 OK", r#"{"proxies":{"PROXY":{"type":"Selector","now":"DIRECT","all":["DIRECT"]}}}"#)
                    } else {
                        ("200 OK", "{}")
                    };
                    let resp = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                    let _ = socket.write_all(resp.as_bytes()).await;
                    let _ = socket.shutdown().await;
                });
            }
        });

        let dir = std::env::temp_dir().join(format!("free_sub_disc_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let yaml_path = dir.join("config.yaml");
        let json_path = dir.join("config.json");
        std::fs::write(&yaml_path, "port: 7890\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies:\n      - DIRECT\nrules:\n  - MATCH,PROXY\n").unwrap();
        let mut cfg = AppConfig::default();
        cfg.mihomo.host = "127.0.0.1".into();
        cfg.mihomo.config_path = yaml_path.to_string_lossy().to_string();
        cfg.mihomo.port = port;
        crate::config::save(&json_path, &cfg).await.unwrap();

        let config_arc = Arc::new(RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        let state = AppState {
            config: config_arc,
            config_path: Arc::new(json_path),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(Mutex::new(())),
            config_lock: Arc::new(Mutex::new(())),
            zapret_lock: Arc::new(Mutex::new(())),
            speedtest_lock: Arc::new(Mutex::new(())),
            antigravity: ag,
            vkturn: crate::vkturn::VkTurnManager::new(),
        };

        // Неактивный режим не должен падать или перехватывать сервер
        let res = disconnect_free_sub(&state, FreeSubMode::Vpn).await.unwrap();
        assert_eq!(res, None);

        // Залипший whitelist.enabled=true при active_srv=None/DIRECT должен очищаться при disconnect(Whitelist)
        {
            let mut tx = ConfigTx::begin(&state).await.unwrap();
            tx.config_mut().whitelist.enabled = true;
            tx.commit_without_reload().await.unwrap();
        }
        let res_wl = disconnect_free_sub(&state, FreeSubMode::Whitelist).await.unwrap();
        assert_eq!(res_wl, None);
        assert!(!state.cfg_snapshot().await.whitelist.enabled, "disconnect(Whitelist) должен сбрасывать залипший whitelist.enabled при active=DIRECT/None");

        let src = include_str!("mod.rs");
        let d_start = src.find("pub async fn disconnect_free_sub(").expect("disconnect_free_sub");
        let d_body = &src[d_start..d_start + 3500.min(src.len() - d_start)];
        assert!(d_body.contains("fallback_is_bs"), "При возврате на другой [БС]-сервер нельзя выключать БС");
        assert!(d_body.contains("sync_mode_whitelist_state(state, FreeSubMode::Vpn, None).await?"), "Ошибка sync_mode_whitelist_state не должна проглатываться");
        srv_handle.abort();
        crate::mihomo::invalidate_proxies_cache();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
