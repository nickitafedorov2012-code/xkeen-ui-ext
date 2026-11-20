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
pub const BACKGROUND_BACKUP_TARGET: usize = 20;
pub const BLACKLIST_TTL_SECS: i64 = 86_400;
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
    pub mode: FreeSubMode,
    #[serde(default)]
    pub high_priority: bool,
    #[serde(default)]
    pub total_bytes: u64,
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
        label: "GitVerse RU Mirror (Whitelist/SNI)",
        urls: &[
            "https://gitverse.ru/api/repos/strider/xkeen-free-subs/raw/branch/main/whitelist.txt",
            "https://raw.githubusercontent.com/zieng2/wl/main/vless_universal.txt",
            "https://raw.githubusercontent.com/zieng2/wl/main/vless_lite.txt",
        ],
    },
    CuratedSource {
        label: "Whitelist Fallback Mirror",
        urls: &[
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

async fn fetch_bounded_bytes(
    client: &reqwest::Client,
    url: &str,
    max_bytes: usize,
    timeout_secs: u64,
) -> Result<Vec<u8>, String> {
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

    Ok(buf)
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
    let tcp_over_quic = current_is_quic_udp && !candidate_is_quic_udp && candidate_delay_ms < 600;
    much_faster || tcp_over_quic
}

/// Фаза 2: точечная L7-валидация партии кандидатов через отдельный служебный провайдер `__free_sub_probe`.
/// Проверяет всю мини-партию целиком и сортирует успешные узлы по возрастанию задержки.
async fn run_l7_batch_probe(
    state: &AppState,
    batch: &[Value],
    _stop_after_first: bool,
) -> Vec<(Value, i64)> {
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
            let res = mihomo::probe_service_provider_node_l7_stable(
                &http,
                &cfg_c,
                FREE_SUB_PROBE_PROVIDER_ID,
                &name,
                L7_TIMEOUT_MS,
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
        }
    }
    sort_l7_verified_by_delay(&mut verified);
    verified
}

/// Сохраняет активный узел пользователя с прежним именем и параметрами, если он принадлежит целевой подписке.
/// Не допускает более одного узла с одинаковым `(server, port)` в основном пуле.
pub fn merge_verified_with_active_session(
    verified: Vec<Value>,
    existing_nodes: &[Value],
    active_leaf: Option<&str>,
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
            seen_keys.insert(node_dedup_key(active_node));
            if let Some(ep) = endpoint_blacklist_key(active_node) {
                seen_endpoints.insert(ep);
            }
            out.push(active_node.clone());
            preserved = true;
        }
    }

    for node in verified {
        let key = node_dedup_key(&node);
        let ep_ok = endpoint_blacklist_key(&node)
            .map(|ep| seen_endpoints.insert(ep))
            .unwrap_or(true);
        if ep_ok && seen_keys.insert(key) {
            out.push(node);
        }
    }
    ensure_unique_node_names(&mut out);
    (out, preserved)
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
    let expires = now_ts + BLACKLIST_TTL_SECS;
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
            .then_with(|| a.last_delay_ms.cmp(&b.last_delay_ms))
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

async fn sync_mode_whitelist_state(state: &AppState, mode: FreeSubMode, target_node: Option<&str>) -> Result<(), String> {
    let enable_wl = mode == FreeSubMode::Whitelist;
    let mut tx = ConfigTx::begin(state).await?;
    let need_reapply = tx.config().whitelist.enabled != enable_wl || !enable_wl;
    tx.config_mut().whitelist.enabled = enable_wl;
    tx.config_mut().whitelist.target_server = if enable_wl {
        target_node.map(str::to_string)
    } else {
        None
    };

    if need_reapply {
        let yaml = tx.read_yaml().await?;
        let (routed, _) = crate::routing::apply_routing(&yaml, tx.config())?;
        tx.set_yaml(routed)?;
        tx.commit_and_reload().await?;
    } else {
        tx.commit_without_reload().await?;
    }
    drop(tx);

    crate::traffic::set_whitelist_active(enable_wl);
    let _ = crate::api::sync_whitelist_mode_tunnel(state, target_node.unwrap_or("")).await;
    let cfg = state.config.read().await.clone();
    let yaml = tokio::fs::read_to_string(&cfg.mihomo.config_path)
        .await
        .unwrap_or_default();
    let ports = crate::routing::redir_ports(&yaml);
    let dns_port = crate::api::resolve_active_dns_port(&cfg);
    let _ = crate::api::routing::sync_full_tunnel_modes(&cfg, ports.redir, ports.tproxy, dns_port).await;
    Ok(())
}

/// Атомарная публикация проверенного набора в рабочий провайдер (`free_vpn` / `free_wl`).
/// При `auto_connect == false` (фоновый добор) обновляет только YAML и пул Mihomo, никогда не перехватывая активный сервер.
async fn publish_and_connect(
    state: &AppState,
    mode: FreeSubMode,
    verified_nodes: Vec<Value>,
    connect_node_name: Option<String>,
    auto_connect: bool,
    expected_gen: Option<u64>,
) -> Result<(String, bool), String> {
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
                let cp = cache_file_path(&state.config_path);
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
        if mode == FreeSubMode::Whitelist {
            sync_mode_whitelist_state(state, mode, Some(&target_name)).await?;
        }
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
        sync_mode_whitelist_state(state, mode, Some(&target_name)).await?;
        record_session_start(state, mode, &final_nodes, &target_name).await;
    }

    Ok((target_name, preserved_active))
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

    const FAST_SOURCE_TIMEOUT_SECS: u64 = 6;

    // Параллельно запрашиваем каждый источник (перебирая зеркала внутри источника)
    let futs = sources.iter().map(|src| {
        let http = state.http.clone();
        let proxied_ref = proxied.clone();
        async move {
            for url in src.urls {
                let mut body_res =
                    fetch_bounded_text(&http, url, MAX_RESPONSE_BYTES, FAST_SOURCE_TIMEOUT_SECS).await;
                if body_res.is_err() {
                    if let Some(ref p_client) = proxied_ref {
                        body_res =
                            fetch_bounded_text(p_client, url, MAX_RESPONSE_BYTES, FAST_SOURCE_TIMEOUT_SECS).await;
                    }
                }
                if let Ok(text) = body_res {
                    let (nodes, rejected) = parse_subscription_nodes_bounded(&text, MAX_PARSED_NODES);
                    if !nodes.is_empty() {
                        return Some((src.label.to_string(), nodes, rejected.len()));
                    }
                }
            }
            None
        }
    });

    let results = futures_util::future::join_all(futs).await;
    let mut collected = Vec::new();
    let mut rejected_invalid = 0usize;
    let mut used_source: Option<String> = None;

    for item in results.into_iter().flatten() {
        let (label, nodes, rej_count) = item;
        rejected_invalid += rej_count;
        let remaining = MAX_PARSED_NODES.saturating_sub(collected.len());
        if remaining == 0 {
            break;
        }
        collected.extend(nodes.into_iter().take(remaining));
        if used_source.is_none() {
            used_source = Some(label);
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
    let _guard = FREE_SUB_RUN_LOCK
        .try_lock()
        .map_err(|_| "Поиск и подключение бесплатной подписки уже выполняется".to_string())?;

    let my_gen = bump_mode_generation(mode);
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

    match run_free_sub_job_inner(state, mode, auto_connect, my_gen, started).await {
        Ok(status) => Ok(status),
        Err(msg) => {
            log_e!("[FREE-SUB] Ошибка конвейера (mode={mode:?}): {msg}");
            update_status(|st| {
                st.running = false;
                st.stage = FreeSubStage::Error;
                st.stage_label = "Ошибка подключения".to_string();
                st.error = Some(msg.clone());
                st.updated_at = Some(chrono::Utc::now().timestamp());
            })
            .await;
            Err(msg)
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

    let mut first_verified: Option<(Value, i64)> = None;
    if auto_connect && !cached_working.is_empty() {
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

    if auto_connect {
        if let Some((first_node, first_delay)) = first_verified {
            let ttff = started.elapsed().as_millis() as u64;
            update_status(|st| {
                st.ttff_ms = Some(ttff);
                st.stage = FreeSubStage::Publishing;
                st.stage_label = "Публикация и подключение рабочего узла из кэша…".to_string();
            })
            .await;
            let first_name = first_node.get("name").and_then(|v| v.as_str()).map(str::to_string);
            // Сохраняем остальные рабочие узлы из кэша и текущего файла провайдера (R27)
            let prov_path = providers_dir(&cfg).join(format!("{}.yaml", mode.provider_id()));
            let existing_pool = tokio::fs::read_to_string(&prov_path)
                .await
                .map(|r| parse_existing_provider_nodes(&r))
                .unwrap_or_default();
            let mut fast_pool = vec![first_node.clone()];
            let mut seen_k: HashSet<String> = [node_dedup_key(&first_node)].into_iter().collect();
            for n in cache
                .mode_ref(mode)
                .working_nodes
                .iter()
                .filter_map(|w| sanitize_proxy_value(&w.proxy))
                .chain(existing_pool.into_iter())
            {
                let k = node_dedup_key(&n);
                let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
                if !is_node_blacklisted(cache.mode_ref(mode), &n, nm, now_ts) && seen_k.insert(k) {
                    fast_pool.push(n);
                    if fast_pool.len() >= BACKGROUND_BACKUP_TARGET {
                        break;
                    }
                }
            }
            let fast_pool_len = fast_pool.len();
            if let Ok((active_name, preserved)) =
                publish_and_connect(&state, mode, fast_pool, first_name, true, Some(my_gen)).await
            {
                update_status(|st| {
                    st.running = false;
                    st.stage = FreeSubStage::Connected;
                    st.stage_label = format!("Подключено ({active_name}) за {ttff} мс");
                    st.active_node = Some(active_name);
                    st.preserved_active = preserved;
                    st.stats.working_count = fast_pool_len;
                    st.source_used = Some("Быстрый кэш".to_string());
                    st.updated_at = Some(chrono::Utc::now().timestamp());
                })
                .await;
                let _ = first_delay;
                return Ok(get_status().await);
            }
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
    let (deduped_raw, dup_count) = deduplicate_proxies(raw_nodes);
    let deduped_nodes: Vec<Value> = deduped_raw
        .into_iter()
        .filter(|p| {
            let n = p.get("name").and_then(|v| v.as_str()).unwrap_or("");
            !is_node_blacklisted(cache.mode_ref(mode), p, n, now_ts)
        })
        .collect();
    let deduped_nodes = diversify_by_endpoint(deduped_nodes, MAX_PER_ENDPOINT_IN_POOL);
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
    let wave1_outcomes = run_tcp_prefilter_endpoints(
        tcp_endpoints_wave1,
        TCP_FAST_TIMEOUT_MS,
        TCP_MAX_CONCURRENT,
        TCP_RATE_INTERVAL_MS,
        Duration::from_secs(TCP_PHASE_BUDGET_SECS),
    )
    .await;

    let mut tcp_passed_candidates = Vec::new();
    let mut timed_out_candidates = Vec::new();
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
        }
    }
    let tcp_passed_count = tcp_passed_candidates.len();
    let l7_ready_proxies = prioritize_l7_ready_proxies(tcp_passed_candidates, quic_udp_candidates);
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
                let verified_keys: HashSet<String> =
                    res.iter().map(|(n, _)| node_dedup_key(n)).collect();
                // Остальные кандидаты текущей партии + следующие партии пойдут в фон (R26)
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
        let _ = update_disk_cache(&cache_path, |c| {
            *c.mode_mut(mode) = cache.mode_ref(mode).clone();
        })
        .await;
        let msg = "Ни один узел не прошёл L7-проверку. Предыдущая конфигурация сохранена без изменений.".to_string();
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
    let _ = update_disk_cache(&cache_path, |disk| {
        let mc = disk.mode_mut(mode);
        for (node, delay) in &verified_nodes {
            mc.working_nodes.retain(|w| node_dedup_key(&w.proxy) != node_dedup_key(node));
            mc.working_nodes.insert(
                0,
                CachedWorkingNode {
                    proxy: node.clone(),
                    last_ok_ts: now_ts,
                    last_delay_ms: *delay,
                    mode,
                    high_priority: false,
                    total_bytes: 0,
                },
            );
        }
        mc.working_nodes.truncate(BACKGROUND_BACKUP_TARGET);
    })
    .await;

    let initial_active_delay = verified_nodes.first().map(|(_, d)| *d).unwrap_or(999);
    let initial_active_is_quic = verified_nodes
        .first()
        .is_some_and(|(n, _)| is_quic_udp_node(n));

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

    let (active_name, preserved) = publish_and_connect(
        &state,
        mode,
        first_proxies.clone(),
        connect_name,
        auto_connect,
        Some(my_gen),
    )
    .await?;
    log_i!("[FREE-SUB] ✓ Опубликовано (auto_connect={auto_connect}) узел '{active_name}' за {ttff} мс (mode={mode:?})");

    update_status(|st| {
        st.running = false;
        st.stage = FreeSubStage::Connected;
        st.stage_label = if auto_connect {
            format!("Подключено ({active_name}) за {ttff} мс")
        } else {
            format!("Пул обновлён ({} узл.) за {ttff} мс", first_proxies.len())
        };
        st.active_node = if auto_connect { Some(active_name) } else { st.active_node.clone() };
        st.preserved_active = preserved;
        st.stats.working_count = first_proxies.len();
        st.updated_at = Some(chrono::Utc::now().timestamp());
    })
    .await;

    // Фоновый добор до BACKGROUND_BACKUP_TARGET (20) резервных узлов;
    // если найден существенно более быстрый узел (или TCP вместо QUIC/UDP), автоматически переключаемся на него.
    if !remaining_for_bg.is_empty() {
        let bg_state = state.clone();
        let bg_cache_path = cache_path.clone();
        tokio::spawn(async move {
            let mut extra = first_proxies;
            let mut seen_eps: HashSet<String> = extra
                .iter()
                .filter_map(endpoint_blacklist_key)
                .collect();
            let mut delay_by_key: HashMap<String, i64> = verified_nodes
                .iter()
                .map(|(n, d)| (node_dedup_key(n), *d))
                .collect();
            let mut cur_active_delay = initial_active_delay;
            let mut cur_active_is_quic = initial_active_is_quic;
            let mut last_published_len = extra.len();
            for chunk in remaining_for_bg.chunks(L7_BATCH_SIZE).take(10) {
                if current_mode_generation(mode) != my_gen || extra.len() >= BACKGROUND_BACKUP_TARGET {
                    break;
                }
                let res = run_l7_batch_probe(&bg_state, chunk, false).await;
                let mut upgrade_target: Option<(String, i64, bool)> = None;
                for (n, delay) in res {
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
                    delay_by_key.insert(node_dedup_key(&n), delay);
                    if auto_connect
                        && upgrade_target.is_none()
                        && !n_name.is_empty()
                        && should_upgrade_active_node(cur_active_delay, cur_active_is_quic, delay, n_is_quic)
                    {
                        upgrade_target = Some((n_name, delay, n_is_quic));
                    }
                    extra.push(n);
                }
                if current_mode_generation(mode) != my_gen {
                    return;
                }
                if extra.len() > last_published_len || upgrade_target.is_some() {
                    let count = extra.len();
                    let switch_name = upgrade_target.as_ref().map(|(nm, _, _)| nm.clone());
                    let do_connect = switch_name.is_some();
                    if publish_and_connect(&bg_state, mode, extra.clone(), switch_name.clone(), do_connect, Some(my_gen))
                        .await
                        .is_ok()
                    {
                        last_published_len = count;
                        if let Some((ref new_name, new_delay, new_quic)) = upgrade_target {
                            log_i!(
                                "[FREE-SUB] Фоновый добор: авто-переключение на более быстрый узел '{}' ({} мс вместо {} мс)",
                                new_name,
                                new_delay,
                                cur_active_delay
                            );
                            cur_active_delay = new_delay;
                            cur_active_is_quic = new_quic;
                            update_status(|st| {
                                if !st.running || st.mode == mode {
                                    st.active_node = Some(new_name.clone());
                                }
                            })
                            .await;
                        }
                        let ts = chrono::Utc::now().timestamp();
                        let _ = update_disk_cache(&bg_cache_path, |c| {
                            let mc = c.mode_mut(mode);
                            for n in &extra {
                                let dk = node_dedup_key(n);
                                if !mc.working_nodes.iter().any(|w| node_dedup_key(&w.proxy) == dk) {
                                    let node_delay = delay_by_key.get(&dk).copied().unwrap_or(999);
                                    mc.working_nodes.push(CachedWorkingNode {
                                        proxy: n.clone(),
                                        last_ok_ts: ts,
                                        last_delay_ms: node_delay,
                                        mode,
                                        high_priority: false,
                                        total_bytes: 0,
                                    });
                                }
                            }
                            mc.working_nodes.truncate(BACKGROUND_BACKUP_TARGET);
                        })
                        .await;
                        update_status(|st| {
                            if !st.running || st.mode == mode {
                                st.stats.working_count = count;
                            }
                        })
                        .await;
                        log_i!("[FREE-SUB] Фоновый добор: опубликовано {count} рабочих узлов");
                    }
                }
            }
        });
    }

    Ok(get_status().await)
}

/// Добавить обе виртуальные подписки (`free_vpn` и `free_wl`) в config.yaml в 1 клик.
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

    let mut tx = ConfigTx::begin(state).await?;
    let mut y = tx.read_yaml().await?;
    let existing = crate::routing::parse_provider_names(&y);
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
    Ok(())
}

/// Отключить активную бесплатную подписку (возвращает предыдущий сервер пользователя или `DIRECT`).
pub async fn disconnect_free_sub(state: &AppState, mode: FreeSubMode) -> Result<Option<String>, String> {
    bump_mode_generation(mode);
    let cfg = state.config.read().await.clone();
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
        let _ = sync_mode_whitelist_state(state, FreeSubMode::Vpn, None).await;
    }

    update_status(|st| {
        if st.mode == mode {
            st.active_node = None;
            st.stage = FreeSubStage::Idle;
            st.stage_label = format!("Отключено (возврат на {fallback_used})");
        }
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

    let prov_path = providers_dir(&*state.config.read().await).join(format!("{pid}.yaml"));
    let _ = tokio::fs::remove_file(&prov_path).await;
    Ok(())
}

/// Переключение узла внутри пула (`next`, `prev`, либо конкретное имя узла).
pub async fn switch_pool_node(
    state: &AppState,
    mode: FreeSubMode,
    target: &str,
) -> Result<String, String> {
    crate::speedtest::cancel_active_speedtest();
    let pid = mode.provider_id();
    let cfg = state.config.read().await.clone();
    let prov_path = providers_dir(&cfg).join(format!("{pid}.yaml"));
    let raw = tokio::fs::read_to_string(&prov_path).await.unwrap_or_default();
    let nodes = parse_existing_provider_nodes(&raw);

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

    if names.is_empty() {
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
        "next" => {
            let idx = cur_name
                .as_ref()
                .and_then(|c| names.iter().position(|n| n == c))
                .map(|i| (i + 1) % names.len())
                .unwrap_or(0);
            names[idx].clone()
        }
        "prev" => {
            let idx = cur_name
                .as_ref()
                .and_then(|c| names.iter().position(|n| n == c))
                .map(|i| if i == 0 { names.len() - 1 } else { i - 1 })
                .unwrap_or(0);
            names[idx].clone()
        }
        explicit => {
            if names.iter().any(|n| n == explicit) {
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
    let _ = sync_mode_whitelist_state(state, mode, Some(&chosen)).await;
    record_session_start(state, mode, &nodes, &chosen).await;
    update_status(|st| {
        st.mode = mode;
        st.stage = FreeSubStage::Connected;
        st.active_node = Some(chosen.clone());
        st.stage_label = format!("Подключено ({chosen})");
    })
    .await;
    Ok(chosen)
}

/// Добавить текущий (или указанный) узел в чёрный список на 24ч и переключиться на следующий.
pub async fn blacklist_and_next(
    state: &AppState,
    mode: FreeSubMode,
    node_name: Option<&str>,
) -> Result<String, String> {
    let cfg = state.config.read().await.clone();
    let prov_path = providers_dir(&cfg).join(format!("{}.yaml", mode.provider_id()));
    let raw = tokio::fs::read_to_string(&prov_path).await.unwrap_or_default();
    let nodes = parse_existing_provider_nodes(&raw);

    let cp = cache_file_path(&state.config_path);
    let mut cache = load_disk_cache(&cp).await;
    let now_ts = chrono::Utc::now().timestamp();

    let target_name = node_name
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(|| cache.mode_ref(mode).last_selected_node.clone())
        .or_else(|| nodes.first().and_then(|n| n.get("name").and_then(|v| v.as_str())).map(str::to_string))
        .ok_or_else(|| "Не определён узел для добавления в чёрный список".to_string())?;

    let proxy_opt = nodes
        .iter()
        .find(|n| n.get("name").and_then(|v| v.as_str()) == Some(target_name.as_str()));
    blacklist_node_in_cache(cache.mode_mut(mode), proxy_opt, &target_name, now_ts);
    let _ = save_disk_cache(&cp, &cache).await;

    switch_pool_node(state, mode, "next").await
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

    let available: Vec<Value> = nodes
        .iter()
        .filter(|n| {
            let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
            !nm.is_empty() && !is_node_blacklisted(cache.mode_ref(mode), n, nm, now_ts)
        })
        .map(|n| {
            let nm = n.get("name").and_then(|v| v.as_str()).unwrap_or("");
            json!({
                "name": nm,
                "host": n.get("server").and_then(|v| v.as_str()).unwrap_or(""),
                "port": n.get("port").and_then(|v| v.as_u64()).unwrap_or(0),
                "ping_ms": mihomo::cached_ping(nm).unwrap_or(-1),
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

/// Определяет, является ли текущий тик Smart Watchdog подозрительным на зависание:
/// узел пропал из живых в провайдере ИЛИ при активных соединениях скорость в обе стороны ниже 1 КБ/с (капельный трафик).
pub fn is_stall_watchdog_tick(
    alive_in_provider: bool,
    node_conns: usize,
    down_speed: u64,
    up_speed: u64,
) -> bool {
    !alive_in_provider || (node_conns > 0 && down_speed < 1024 && up_speed < 1024)
}

/// Smart Watchdog: при зависании активного бесплатного узла (есть соединения через ЭТОТ узел,
/// 0 Б/с входящего и исходящего трафика >= 3 тика ~15-20с, либо Mihomo пометил узел `alive: false`,
/// либо периодическая L7-проба при нулевом трафике упала)
/// автоматически отправляет мёртвый узел в 24ч blacklist, переключает на следующий рабочий узел
/// (или запускает переподбор при исчерпании пула) и фиксирует "Обрыв" в истории.
pub async fn check_free_sub_stall_watchdog(state: &AppState) {
    let cfg = state.config.read().await.clone();
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
        if idle.1 >= 6 {
            idle.1 = 0;
            periodic_probe_due = true;
        }
    }

    if count >= 3 || periodic_probe_due {
        // Активная L7-проверка реального листового узла (включая доступ к youtube.com) перед признанием обрыва
        if mihomo::probe_service_provider_node_l7_stable(
            &state.http,
            &cfg,
            mode.provider_id(),
            &leaf_name,
            L7_TIMEOUT_MS,
        )
        .await
        .is_ok()
        {
            *STALL_WATCHDOG_STREAK.lock().await = (Some(leaf_name.clone()), 0);
            *IDLE_PROBE_TICKS.lock().await = (Some(leaf_name), 0);
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
        if let Err(err) = blacklist_and_next(state, mode, Some(&leaf_name)).await {
            log_w!(
                "[FREE-SUB] Smart Watchdog: пул {:?} исчерпан ({err}) — запускаем переподбор",
                mode
            );
            let bg_state = state.clone();
            tokio::spawn(async move {
                let _ = start_one_click_connect(bg_state, mode).await;
            });
        }
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
            mode: FreeSubMode::Whitelist,
            high_priority: false,
            total_bytes: 0,
        });
        mc.working_nodes.push(CachedWorkingNode {
            proxy: n2.clone(),
            last_ok_ts: 100,
            last_delay_ms: 80,
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
        let marker = "if !remaining_for_bg.is_empty()";
        let start = src.find(marker).expect("bg refill block");
        let end = src[start..]
            .find("Ok(get_status().await)")
            .map(|o| start + o)
            .unwrap_or(src.len());
        let block = &src[start..end];
        let loop_pos = block.find("for chunk in remaining_for_bg.chunks").expect("batch loop");
        let pub_pos = block.find("publish_and_connect(&bg_state, mode, extra.clone()").expect("publish_and_connect");
        assert!(pub_pos > loop_pos && block.contains("last_published_len"), "publish_and_connect должен вызываться инкрементально внутри цикла батчей");
    }

    #[tokio::test]
    async fn regression_stall_watchdog_resolves_virtual_pool_leaf_blacklists_and_switches() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

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
}
