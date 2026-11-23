//! Синхронизация принудительно проксируемых доменов (force_domains)
//! с ipset geo_override ядра Linux и файлом /opt/etc/xkeen/ipset/ru_exclude_override.lst.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;
use std::time::Duration;

pub const OVERRIDE_FILE: &str = "/opt/etc/xkeen/ipset/ru_exclude_override.lst";
pub const OVERRIDE_DIR: &str = "/opt/etc/xkeen/ipset";
pub const XKEEN_CONF_FILE: &str = "/opt/etc/xkeen/xkeen.conf";
pub const SYSTEM_CRONTAB_FILE: &str = "/opt/etc/crontab";
pub const MARKER_BEGIN: &str = "# --- XKEEN-ROUTE-OVERRIDE-BEGIN ---";
pub const MARKER_END: &str = "# --- XKEEN-ROUTE-OVERRIDE-END ---";

/// Можно ли класть IPv4 в geo_override: только публичные адреса (баг 97).
/// Fake-ip Mihomo (198.18.0.0/15), частные, CGNAT, link-local и служебные диапазоны отбрасываются.
pub fn is_override_eligible_v4(ip: &Ipv4Addr) -> bool {
    let o = ip.octets();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_private()
        || ip.is_link_local()
        || ip.is_broadcast()
        || ip.is_multicast()
        || ip.is_documentation()
        || o[0] == 0
        || o[0] >= 240
        || (o[0] == 198 && (o[1] == 18 || o[1] == 19))
        || (o[0] == 100 && (64..=127).contains(&o[1])))
}

/// Можно ли класть IPv6 в geo_override: только глобальные unicast-адреса (баг 97).
pub fn is_override_eligible_v6(ip: &Ipv6Addr) -> bool {
    let seg0 = ip.segments()[0];
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || (seg0 & 0xfe00) == 0xfc00 // ULA fc00::/7
        || (seg0 & 0xffc0) == 0xfe80 // link-local fe80::/10
        || ip.to_ipv4_mapped().is_some())
}


/// Асинхронно резолвит каждый домен в список его валидных публичных IP.
pub async fn resolve_domains_map(
    domains: &[String],
) -> (std::collections::HashMap<String, Vec<IpAddr>>, usize) {
    let mut set = tokio::task::JoinSet::new();

    for (idx, d) in domains.iter().enumerate() {
        let clean = d.trim().trim_start_matches("www.").to_string();
        if clean.is_empty() {
            continue;
        }
        let domain_orig = d.clone();
        for host in [clean.clone(), format!("www.{clean}")] {
            let orig = domain_orig.clone();
            set.spawn(async move {
                let mut ips = Vec::new();
                let addr = format!("{host}:443");
                let res = tokio::time::timeout(Duration::from_millis(1500), tokio::net::lookup_host(addr)).await;
                if let Ok(Ok(iter)) = res {
                    for sa in iter {
                        match sa.ip() {
                            IpAddr::V4(ip) => {
                                if is_override_eligible_v4(&ip) {
                                    ips.push(IpAddr::V4(ip));
                                }
                            }
                            IpAddr::V6(ip) => {
                                if is_override_eligible_v6(&ip) {
                                    ips.push(IpAddr::V6(ip));
                                }
                            }
                        }
                    }
                }
                (idx, orig, ips)
            });
        }
    }

    let mut map: std::collections::HashMap<String, Vec<IpAddr>> = std::collections::HashMap::new();
    let mut resolved_indices = std::collections::HashSet::new();

    while let Some(res) = set.join_next().await {
        if let Ok((idx, orig, ip_list)) = res {
            if !ip_list.is_empty() {
                resolved_indices.insert(idx);
                let entry = map.entry(orig).or_default();
                for ip in ip_list {
                    if !entry.contains(&ip) {
                        entry.push(ip);
                    }
                }
            }
        }
    }

    let count = resolved_indices.len();
    (map, count)
}

/// Путь к файлу дискового кэша DNS geo_override (баг 97, баг 13)
#[cfg(target_os = "linux")]
pub const DEFAULT_OVERRIDE_CACHE_FILE: &str = "/opt/etc/xkeen-route/override_cache.json";
#[cfg(not(target_os = "linux"))]
pub const DEFAULT_OVERRIDE_CACHE_FILE: &str = "xkeen-route.override_cache.json";

static OVERRIDE_CACHE_PATH_OVERRIDE: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

pub fn get_override_cache_path() -> std::path::PathBuf {
    let guard = OVERRIDE_CACHE_PATH_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref p) = *guard {
        p.clone()
    } else {
        std::path::PathBuf::from(DEFAULT_OVERRIDE_CACHE_FILE)
    }
}


/// Загрузка кэша домен -> IP с диска
pub fn load_override_cache(path: &Path) -> std::collections::HashMap<String, Vec<IpAddr>> {
    if !path.exists() {
        return std::collections::HashMap::new();
    }
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(e) => {
            crate::log_w!("[OVERRIDE] Не удалось прочитать кэш {}: {e}", path.display());
            std::collections::HashMap::new()
        }
    }
}

/// Атомарное сохранение кэша домен -> IP на диск (tmp + rename)
pub fn save_override_cache(
    path: &Path,
    cache: &std::collections::HashMap<String, Vec<IpAddr>>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let serialized = serde_json::to_vec(cache).map_err(|e| e.to_string())?;
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), nonce));

    std::fs::write(&tmp, &serialized).map_err(|e| format!("Не удалось записать {}: {e}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("Не удалось переименовать {} в {}: {e}", tmp.display(), path.display()));
    }
    Ok(())
}

/// Чистая функция слияния DNS-ответов с кэшем последних успешных IP (баг 97).
/// - Для разрешённых сейчас доменов обновляет кэш и включает их новые IP.
/// - Для неразрешённых доменов берёт последние известные IP из кэша.
/// - Отсекает fake-ip (198.18.0.0/15), частные и немаршрутизируемые адреса.
pub fn merge_with_last_good(
    requested: &[String],
    resolved_now: &std::collections::HashMap<String, Vec<IpAddr>>,
    cache: &mut std::collections::HashMap<String, Vec<IpAddr>>,
) -> Vec<IpAddr> {
    let mut out_set = std::collections::BTreeSet::new();

    for d in requested {
        let clean = d.trim().trim_start_matches("www.").to_lowercase();
        if clean.is_empty() {
            continue;
        }

        if let Some(ips_now) = resolved_now.get(d).or_else(|| resolved_now.get(&clean)) {
            let mut valid_ips = Vec::new();
            for ip in ips_now {
                match ip {
                    IpAddr::V4(v4) => {
                        if is_override_eligible_v4(v4) {
                            valid_ips.push(IpAddr::V4(*v4));
                            out_set.insert(IpAddr::V4(*v4));
                        }
                    }
                    IpAddr::V6(v6) => {
                        if is_override_eligible_v6(v6) {
                            valid_ips.push(IpAddr::V6(*v6));
                            out_set.insert(IpAddr::V6(*v6));
                        }
                    }
                }
            }
            if !valid_ips.is_empty() {
                cache.insert(d.clone(), valid_ips);
            }
        } else if let Some(cached_ips) = cache.get(d).or_else(|| cache.get(&clean)) {
            for ip in cached_ips {
                match ip {
                    IpAddr::V4(v4) => {
                        if is_override_eligible_v4(v4) {
                            out_set.insert(IpAddr::V4(*v4));
                        }
                    }
                    IpAddr::V6(v6) => {
                        if is_override_eligible_v6(v6) {
                            out_set.insert(IpAddr::V6(*v6));
                        }
                    }
                }
            }
        }
    }

    out_set.into_iter().collect()
}

static OVERRIDE_CACHE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, Vec<IpAddr>>>> =
    std::sync::LazyLock::new(|| {
        let path = get_override_cache_path();
        std::sync::Mutex::new(load_override_cache(&path))
    });


/// Скрипт для `ipset restore` (баг 97): весь набор заполняется одним процессом вместо процесса на каждый IP.
/// Строки, не являющиеся IP/CIDR нужного семейства, пропускаются: одна битая строка сорвала бы весь restore.
pub fn build_ipset_restore_script(tmp: &str, family: &str, entries: &[String]) -> String {
    let mut out = format!("create {tmp} hash:net family {family} -exist\nflush {tmp}\n");
    for e in entries {
        let t = e.trim();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let (addr, prefix) = match t.split_once('/') {
            Some((a, p)) => (a, Some(p)),
            None => (t, None),
        };
        let Ok(ip) = addr.parse::<IpAddr>() else { continue };
        let max = if ip.is_ipv4() { 32 } else { 128 };
        if let Some(p) = prefix {
            match p.parse::<u8>() {
                Ok(n) if n <= max => {}
                _ => continue,
            }
        }
        if (family == "inet") != ip.is_ipv4() {
            continue;
        }
        out.push_str(&format!("add {tmp} {t} -exist\n"));
    }
    out
}


#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct AutoCdnEntry {
    cdn: String,
    #[serde(default)]
    source_domains: Vec<String>,
    #[serde(default)]
    updated_at: u64,
}

const AUTO_CDN_TTL_SECS: u64 = 7 * 86_400;

fn auto_cdn_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

static IPSET_SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CACHED_AUTO_CDNS: std::sync::RwLock<Vec<AutoCdnEntry>> = std::sync::RwLock::new(Vec::new());
const DEFAULT_AUTO_CDNS_CACHE_FILE: &str = "/opt/etc/xkeen-route.auto_cdns.json";
static AUTO_CDNS_CACHE_PATH_OVERRIDE: std::sync::Mutex<Option<std::path::PathBuf>> =
    std::sync::Mutex::new(None);

fn get_auto_cdns_cache_path() -> std::path::PathBuf {
    let guard = AUTO_CDNS_CACHE_PATH_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(ref p) = *guard {
        p.clone()
    } else {
        std::path::PathBuf::from(DEFAULT_AUTO_CDNS_CACHE_FILE)
    }
}

fn save_auto_cdns_cache_to_path(path: &Path, entries: &[AutoCdnEntry]) {
    if cfg!(test) && path == Path::new(DEFAULT_AUTO_CDNS_CACHE_FILE) {
        return;
    }
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(serialized) = serde_json::to_vec(entries) else {
        return;
    };
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = path.with_extension(format!("tmp.{}.{}", std::process::id(), nonce));
    if std::fs::write(&tmp, &serialized).is_ok() {
        if std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
}

fn parse_auto_cdn_entries(bytes: &[u8]) -> Vec<AutoCdnEntry> {
    if let Ok(entries) = serde_json::from_slice::<Vec<AutoCdnEntry>>(bytes) {
        return entries;
    }
    if let Ok(legacy) = serde_json::from_slice::<Vec<String>>(bytes) {
        let now = auto_cdn_now_secs();
        return legacy
            .into_iter()
            .filter_map(|c| {
                let clean = c.trim().to_lowercase();
                if clean.is_empty() {
                    None
                } else {
                    Some(AutoCdnEntry {
                        cdn: clean,
                        source_domains: Vec::new(),
                        updated_at: now,
                    })
                }
            })
            .collect();
    }
    Vec::new()
}

/// Загрузка кэша найденных CDN с диска в память (баг 13 + R53).
pub fn load_cached_auto_cdns() -> Vec<String> {
    let path = get_auto_cdns_cache_path();
    let now = auto_cdn_now_secs();
    let loaded = if path.exists() {
        std::fs::read(&path)
            .ok()
            .map(|b| parse_auto_cdn_entries(&b))
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    if !loaded.is_empty() {
        if let Ok(mut cache) = CACHED_AUTO_CDNS.write() {
            cache.retain(|e| e.updated_at == 0 || now.saturating_sub(e.updated_at) <= AUTO_CDN_TTL_SECS);
            for entry in loaded {
                let clean = entry.cdn.trim().to_lowercase();
                if clean.is_empty() {
                    continue;
                }
                if entry.updated_at != 0 && now.saturating_sub(entry.updated_at) > AUTO_CDN_TTL_SECS {
                    continue;
                }
                if let Some(existing) = cache.iter_mut().find(|e| e.cdn == clean) {
                    for src in entry.source_domains {
                        let s = src.trim().to_lowercase();
                        if !s.is_empty() && !existing.source_domains.contains(&s) {
                            existing.source_domains.push(s);
                        }
                    }
                    existing.updated_at = existing.updated_at.max(entry.updated_at);
                } else {
                    cache.push(AutoCdnEntry {
                        cdn: clean,
                        source_domains: entry
                            .source_domains
                            .into_iter()
                            .map(|s| s.trim().to_lowercase())
                            .filter(|s| !s.is_empty())
                            .collect(),
                        updated_at: if entry.updated_at == 0 { now } else { entry.updated_at },
                    });
                }
            }
            return cache.iter().map(|e| e.cdn.clone()).collect();
        }
    }
    Vec::new()
}

/// Обновление глобального кэша найденных CDN доменов с привязкой к source_domains (R53).
pub fn sync_cached_auto_cdns_for_sources(cdns: &[String], sources: &[String]) {
    let now = auto_cdn_now_secs();
    let clean_sources: Vec<String> = sources
        .iter()
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    let snapshot = if let Ok(mut cache) = CACHED_AUTO_CDNS.write() {
        let prev_len = cache.len();
        cache.retain(|e| e.updated_at == 0 || now.saturating_sub(e.updated_at) <= AUTO_CDN_TTL_SECS);
        let mut changed = cache.len() != prev_len;
        for c in cdns {
            let clean = c.trim().to_lowercase();
            if clean.is_empty() {
                continue;
            }
            if let Some(existing) = cache.iter_mut().find(|e| e.cdn == clean) {
                for s in &clean_sources {
                    if !existing.source_domains.contains(s) {
                        existing.source_domains.push(s.clone());
                        changed = true;
                    }
                }
                if now > existing.updated_at {
                    existing.updated_at = now;
                    changed = true;
                }
            } else {
                cache.push(AutoCdnEntry {
                    cdn: clean,
                    source_domains: clean_sources.clone(),
                    updated_at: now,
                });
                changed = true;
            }
        }
        if changed {
            Some(cache.clone())
        } else {
            None
        }
    } else {
        None
    };
    if let Some(ref items) = snapshot {
        save_auto_cdns_cache_to_path(&get_auto_cdns_cache_path(), items);
    }
}

/// Обновление глобального кэша найденных CDN доменов в памяти и на диске (баг 13)
pub fn sync_cached_auto_cdns(cdns: &[String]) {
    sync_cached_auto_cdns_for_sources(cdns, &[]);
}

/// R53: сохраняет найденные CDN с привязкой к конкретному исходному домену.
pub fn sync_cached_auto_cdns_by_source(
    by_source: &std::collections::BTreeMap<String, std::collections::BTreeSet<String>>,
) {
    for (source, cdns) in by_source {
        let cdns_vec: Vec<String> = cdns.iter().cloned().collect();
        sync_cached_auto_cdns_for_sources(&cdns_vec, std::slice::from_ref(source));
    }
}

pub fn get_cached_auto_cdns() -> Vec<String> {
    let now = auto_cdn_now_secs();
    CACHED_AUTO_CDNS
        .read()
        .map(|c| {
            c.iter()
                .filter(|e| e.updated_at == 0 || now.saturating_sub(e.updated_at) <= AUTO_CDN_TTL_SECS)
                .map(|e| e.cdn.clone())
                .collect()
        })
        .unwrap_or_default()
}

fn domain_matches_any_force_domain(domain: &str, force_domains: &[String]) -> bool {
    let d = domain.trim().to_lowercase();
    force_domains.iter().any(|f| {
        let fl = f.trim().to_lowercase();
        !fl.is_empty() && (d == fl || d.ends_with(&format!(".{fl}")))
    })
}

fn get_cached_auto_cdns_for_force_domains(force_domains: &[String]) -> Vec<String> {
    if force_domains.is_empty() {
        return Vec::new();
    }
    let now = auto_cdn_now_secs();
    CACHED_AUTO_CDNS
        .read()
        .map(|c| {
            c.iter()
                .filter(|e| e.updated_at == 0 || now.saturating_sub(e.updated_at) <= AUTO_CDN_TTL_SECS)
                .filter(|e| {
                    if e.source_domains.is_empty() {
                        domain_matches_any_force_domain(&e.cdn, force_domains)
                    } else {
                        e.source_domains
                            .iter()
                            .any(|src| domain_matches_any_force_domain(src, force_domains))
                    }
                })
                .map(|e| e.cdn.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Синхронизирует IP-адреса force_domains с ru_exclude_override.lst и живыми ipset ядра.
pub async fn sync_geo_override(domains: &[String]) -> Result<usize, String> {
    if !Path::new(OVERRIDE_DIR).exists() {
        return Ok(0);
    }

    let _lock = IPSET_SYNC_LOCK.lock().await;

    let requested_domains: Vec<String> = domains
        .iter()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
        .collect();
    let requested = requested_domains.len();

    let (resolved_map, resolved) = resolve_domains_map(&requested_domains).await;

    let merged_ips = {
        let cache_path = get_override_cache_path();
        let mut cache_guard = OVERRIDE_CACHE.lock().unwrap_or_else(|e| e.into_inner());

        // Если кэш в памяти пуст, пробуем прочитать его с диска
        if cache_guard.is_empty() && cache_path.exists() {
            *cache_guard = load_override_cache(&cache_path);
        }

        let ips = merge_with_last_good(&requested_domains, &resolved_map, &mut cache_guard);

        // Сохраняем обновлённый кэш на диск
        if let Err(e) = save_override_cache(&cache_path, &cache_guard) {
            crate::log_w!("[OVERRIDE] Не удалось сохранить кэш geo_override: {e}");
        }
        ips
    };

    // Если ни один домен не разрешился и в кэше пусто (например первый старт без сети),
    // действует предохранитель: старый файл и ipset не трогаем
    if requested > 0 && merged_ips.is_empty() {
        crate::log_w!(
            "[OVERRIDE] 0 из {requested} доменов имеют доступные IP (DNS сбой, кэш пуст) — geo_override не трогаем"
        );
        return Err(format!(
            "DNS не ответил для всех {requested} доменов и кэш пуст: geo_override сохранён без изменений"
        ));
    }

    let mut v4 = BTreeSet::new();
    let mut v6 = BTreeSet::new();
    for ip in merged_ips {
        match ip {
            IpAddr::V4(addr) => { v4.insert(addr); }
            IpAddr::V6(addr) => { v6.insert(addr); }
        }
    }
    let total_ips = v4.len() + v6.len();

    // 1. Потоково обновляем файл ru_exclude_override.lst без буферизации всего содержимого в RAM
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp_file = format!("{OVERRIDE_FILE}.{}.{nonce}.tmp", std::process::id());
    write_override_file_streaming(OVERRIDE_FILE, &tmp_file, &v4, &v6).await?;
    if let Err(e) = tokio::fs::rename(&tmp_file, OVERRIDE_FILE).await {
        let _ = tokio::fs::remove_file(&tmp_file).await;
        return Err(format!("Не удалось переименовать {tmp_file}: {e}"));
    }

    // 2. Атомарно обновляем ipset ядра geo_override (IPv4)
    sync_ipset_family("geo_override", "inet").await?;

    // 3. Атомарно обновляем ipset ядра geo_override6 (IPv6)
    if let Err(e) = sync_ipset_family("geo_override6", "inet6").await {
        crate::log_w!("[OVERRIDE] Ошибка синхронизации IPv6 ipset: {e}");
    }

    crate::log_i!("[OVERRIDE] Синхронизировано {total_ips} IP для {} доменов (разрешено сейчас: {resolved})", domains.len());
    Ok(total_ips)
}

/// Баг 13: Единый сбор всех доменов для geo_override из полного конфига.
/// Объединяет force_domains, статические CDN-бандлы, обнаруженные auto_cdns и активные игровые домены.
pub fn collect_override_domains(cfg: &crate::config::AppConfig, extra_cdns: &[String]) -> Vec<String> {
    if !extra_cdns.is_empty() {
        sync_cached_auto_cdns_for_sources(extra_cdns, &cfg.force_domains);
    }
    let cached = get_cached_auto_cdns_for_force_domains(&cfg.force_domains);

    let mut all = cfg.force_domains.clone();
    if !cfg.force_domains.is_empty() {
        let static_cdns = crate::cdn_discovery::expand_bundles(&cfg.force_domains);
        for cdn in static_cdns {
            if !all.contains(&cdn) {
                all.push(cdn);
            }
        }
        for cdn in cached {
            if !all.contains(&cdn) {
                all.push(cdn);
            }
        }
    }
    for cdn in extra_cdns {
        if !all.contains(cdn) {
            all.push(cdn.clone());
        }
    }
    if cfg.gaming.enabled {
        let exclude_discord = cfg.zapret.enabled && cfg.zapret.hybrid_discord;
        let game_domains = crate::routing::get_gaming_domains_filtered(&cfg.gaming, exclude_discord);
        for g in game_domains {
            if !all.contains(&g) {
                all.push(g);
            }
        }
    }

    // Исключаем домены, которые пользователь явно направил в DIRECT
    if !cfg.direct_domains.is_empty() {
        all.retain(|d| {
            let clean = crate::config::normalize_domain(d);
            !cfg.direct_domains.iter().any(|dir| {
                let clean_dir = crate::config::normalize_domain(dir);
                !clean_dir.is_empty() && (clean == clean_dir || clean.ends_with(&format!(".{clean_dir}")))
            })
        });
    }

    // Исключаем домены, которые Zapret ведет в DIRECT (баг 13 ревизия)
    if cfg.zapret.enabled {
        all.retain(|d| !crate::routing::is_zapret_direct_domain(d, &cfg.zapret));
    }

    all
}

/// Баг 13: Единая точка синхронизации geo_override из AppConfig.
/// Никакие внешние модули не должны напрямую передавать неполные срезы доменов.
pub async fn sync_from_config(cfg: &crate::config::AppConfig, extra_cdns: &[String]) -> Result<usize, String> {
    let all = collect_override_domains(cfg, extra_cdns);
    sync_geo_override(&all).await
}

async fn write_override_file_streaming(
    src_path: &str,
    dst_path: &str,
    v4_ips: &BTreeSet<Ipv4Addr>,
    v6_ips: &BTreeSet<Ipv6Addr>,
) -> Result<(), String> {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};

    let dst_file = tokio::fs::File::create(dst_path)
        .await
        .map_err(|e| format!("Не удалось создать {dst_path}: {e}"))?;
    let mut writer = tokio::io::BufWriter::new(dst_file);

    let mut had_block = false;

    match tokio::fs::File::open(src_path).await {
        Ok(src_file) => {
            let reader = tokio::io::BufReader::new(src_file);
            let mut lines = reader.lines();
            let mut in_block = false;

            while let Some(line) = lines.next_line().await.map_err(|e| format!("Ошибка чтения {src_path}: {e}"))? {
                let t = line.trim();
                if t == MARKER_BEGIN {
                    in_block = true;
                    had_block = true;
                    write_ips_block(&mut writer, v4_ips, v6_ips).await.map_err(|e| format!("Ошибка записи {dst_path}: {e}"))?;
                    continue;
                }
                if t == MARKER_END {
                    in_block = false;
                    continue;
                }
                if in_block {
                    continue;
                }
                writer.write_all(format!("{line}\n").as_bytes()).await.map_err(|e| format!("Ошибка записи {dst_path}: {e}"))?;
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Файл ещё не существует — допустимо создать с нуля
        }
        Err(e) => {
            return Err(format!("Ошибка открытия {src_path}: {e}. Операция отменена во избежание потери правил"));
        }
    }

    if !had_block {
        write_ips_block(&mut writer, v4_ips, v6_ips).await.map_err(|e| format!("Ошибка записи {dst_path}: {e}"))?;
    }

    writer.flush().await.map_err(|e| format!("Ошибка сброса буфера {dst_path}: {e}"))?;
    Ok(())
}

async fn write_ips_block(
    w: &mut tokio::io::BufWriter<tokio::fs::File>,
    v4_ips: &BTreeSet<Ipv4Addr>,
    v6_ips: &BTreeSet<Ipv6Addr>,
) -> Result<(), std::io::Error> {
    use tokio::io::AsyncWriteExt;
    if !v4_ips.is_empty() || !v6_ips.is_empty() {
        w.write_all(format!("{MARKER_BEGIN}\n").as_bytes()).await?;
        for ip in v4_ips {
            w.write_all(format!("{ip}\n").as_bytes()).await?;
        }
        for ip in v6_ips {
            w.write_all(format!("{ip}\n").as_bytes()).await?;
        }
        w.write_all(format!("{MARKER_END}\n").as_bytes()).await?;
    }
    Ok(())
}

async fn run_ipset(args: &[&str]) -> Result<(), String> {
    match tokio::process::Command::new("ipset").args(args).output().await {
        Ok(out) => {
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                crate::log_d!("[OVERRIDE] ipset {:?} завершился с кодом {:?}: {}", args, out.status.code(), err.trim());
                Err(format!("ipset {:?} статус {:?}: {}", args, out.status.code(), err.trim()))
            } else {
                Ok(())
            }
        }
        Err(e) => {
            crate::log_w!("[OVERRIDE] Ошибка вызова ipset {:?}: {}", args, e);
            Err(format!("Ошибка вызова ipset {:?}: {e}", args))
        }
    }
}

async fn run_ipset_restore(script: &str) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;
    let mut child = tokio::process::Command::new("ipset")
        .args(["restore", "-exist"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("Ошибка вызова ipset restore: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(script.as_bytes()).await.map_err(|e| format!("ipset restore stdin: {e}"))?;
    }
    let out = child.wait_with_output().await.map_err(|e| format!("ipset restore: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!("ipset restore статус {:?}: {}", out.status.code(), String::from_utf8_lossy(&out.stderr).trim()))
    }
}

async fn sync_ipset_family(set_name: &str, family: &str) -> Result<(), String> {
    let tmp = format!("{set_name}_{}_tmp", std::process::id());

    // Уничтожаем возможный старый временный ipset от прерванного swap (ошибка ожидаема, если не существовал)
    let _ = run_ipset(&["destroy", &tmp]).await;

    let entries: Vec<String> = match tokio::fs::read_to_string(OVERRIDE_FILE).await {
        Ok(text) => text.lines().map(|l| l.to_string()).collect(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("Не удалось прочитать {OVERRIDE_FILE}: {e}")),
    };
    let script = build_ipset_restore_script(&tmp, family, &entries);
    if let Err(e) = run_ipset_restore(&script).await {
        let _ = run_ipset(&["destroy", &tmp]).await;
        return Err(format!("Ошибка заполнения staging ipset {tmp}: {e}"));
    }

    run_ipset(&["create", set_name, "hash:net", "family", family, "-exist"]).await
        .map_err(|e| format!("Ошибка создания рабочего ipset {set_name}: {e}"))?;

    let swap_res = run_ipset(&["swap", set_name, &tmp]).await;
    let _ = run_ipset(&["destroy", &tmp]).await;

    if let Err(e) = swap_res {
        crate::log_w!("[OVERRIDE] ipset swap {set_name} failed: {e}");
        return Err(format!("ipset swap {set_name}: {e}"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "override_sync_tests.rs"]
mod tests;
