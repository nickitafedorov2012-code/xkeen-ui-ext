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

/// Асинхронно резолвит список доменов во все уникальные IPv4 и IPv6 адреса параллельно.
pub async fn resolve_domains(domains: &[String]) -> (BTreeSet<Ipv4Addr>, BTreeSet<Ipv6Addr>) {
    let (v4, v6, _) = resolve_domains_counted(domains).await;
    (v4, v6)
}

/// То же, что `resolve_domains`, плюс число доменов, для которых получен хотя бы один пригодный адрес.
pub async fn resolve_domains_counted(domains: &[String]) -> (BTreeSet<Ipv4Addr>, BTreeSet<Ipv6Addr>, usize) {
    let (map, count) = resolve_domains_map(domains).await;
    let mut v4 = BTreeSet::new();
    let mut v6 = BTreeSet::new();
    for (_d, ips) in map {
        for ip in ips {
            match ip {
                IpAddr::V4(addr) => { v4.insert(addr); }
                IpAddr::V6(addr) => { v6.insert(addr); }
            }
        }
    }
    (v4, v6, count)
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

#[cfg(test)]
pub fn set_override_cache_path_override(path: Option<std::path::PathBuf>) {
    let mut guard = OVERRIDE_CACHE_PATH_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = path;
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

/// Сбой DNS во время синхронизации (баг 97): если разрешилось меньше половины доменов,
/// старый блок geo_override сохраняется, иначе force-домены молча уходят в DIRECT.
pub fn keep_previous_override(requested: usize, resolved: usize) -> bool {
    requested > 0 && resolved * 2 < requested
}

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

/// Обновляет блок в файле ru_exclude_override.lst, сохраняя любые ручные записи пользователя.
pub fn update_override_file_content(
    existing: &str,
    v4_ips: &BTreeSet<Ipv4Addr>,
    v6_ips: &BTreeSet<Ipv6Addr>,
) -> String {
    let mut lines_before = Vec::new();
    let mut lines_after = Vec::new();
    let mut in_block = false;
    let mut had_block = false;

    for line in existing.lines() {
        let t = line.trim();
        if t == MARKER_BEGIN {
            in_block = true;
            had_block = true;
            continue;
        }
        if t == MARKER_END {
            in_block = false;
            continue;
        }
        if in_block {
            continue;
        }
        if had_block {
            lines_after.push(line);
        } else {
            lines_before.push(line);
        }
    }

    let mut out = Vec::new();
    for l in lines_before {
        out.push(l.to_string());
    }

    if !v4_ips.is_empty() || !v6_ips.is_empty() {
        out.push(MARKER_BEGIN.to_string());
        for ip in v4_ips {
            out.push(ip.to_string());
        }
        for ip in v6_ips {
            out.push(ip.to_string());
        }
        out.push(MARKER_END.to_string());
    }

    for l in lines_after {
        out.push(l.to_string());
    }

    let joined = out.join("\n");
    if joined.is_empty() {
        String::new()
    } else {
        joined + "\n"
    }
}

static IPSET_SYNC_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static CACHED_AUTO_CDNS: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

/// Обновление глобального кэша найденных CDN доменов (баг 13)
pub fn sync_cached_auto_cdns(cdns: &[String]) {
    if let Ok(mut cache) = CACHED_AUTO_CDNS.write() {
        for c in cdns {
            let clean = c.trim().to_lowercase();
            if !clean.is_empty() && !cache.contains(&clean) {
                cache.push(clean);
            }
        }
    }
}

pub fn get_cached_auto_cdns() -> Vec<String> {
    CACHED_AUTO_CDNS.read().map(|c| c.clone()).unwrap_or_default()
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
        sync_cached_auto_cdns(extra_cdns);
    }
    let cached = get_cached_auto_cdns();

    let mut all = cfg.force_domains.clone();
    if !cfg.force_domains.is_empty() {
        let static_cdns = crate::cdn_discovery::expand_bundles(&cfg.force_domains);
        for cdn in static_cdns {
            if !all.contains(&cdn) {
                all.push(cdn);
            }
        }
    }
    for cdn in cached {
        if !all.contains(&cdn) {
            all.push(cdn);
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
mod tests {
    use super::*;

    #[test]
    fn regression_bug_13_cached_cdns_and_zapret_direct_filtered() {
        // Баг 13:
        // 1. Найденные сканированием CDN должны сохраняться в кэше и не стираться
        //    при последующем вызове sync_from_config(cfg, &[]).
        // 2. Домены, которые Zapret ведет в DIRECT, не должны попадать в geo_override.
        let mut cfg = crate::config::AppConfig::default();
        cfg.force_domains = vec!["instagram.com".into()];
        cfg.zapret.enabled = true;
        cfg.zapret.hybrid_youtube = true;

        // Эмулируем, что сканирование обнаружило CDN
        let scanned_cdn = vec!["cdn.instagram.com".into()];
        sync_cached_auto_cdns(&scanned_cdn);

        // Последующий вызов с пустым списком (&[]) НЕ должен потерять найденный CDN!
        let domains = collect_override_domains(&cfg, &[]);
        assert!(
            domains.contains(&"cdn.instagram.com".to_string()),
            "Найденные сканированием CDN обязаны сохраняться и входить в geo_override"
        );

        // Проверяем исключение доменов Zapret DIRECT (например, googlevideo.com)
        cfg.force_domains.push("googlevideo.com".into());
        let domains_with_zapret = collect_override_domains(&cfg, &[]);
        assert!(
            !domains_with_zapret.contains(&"googlevideo.com".to_string()),
            "Zapret DIRECT домены не должны попадать в geo_override"
        );
    }

    #[test]
    fn regression_bug_13_geo_override_has_single_source_of_truth() {
        // стирало игровые домены и найденные CDN, тумблер игрового режима — CDN, старт без force-доменов
        // не синхронизировал вовсе.
        for (name, src) in [
            ("api.rs", include_str!("api.rs")),
            ("bot.rs", include_str!("bot.rs")),
            ("main.rs", include_str!("main.rs")),
            ("watchdog.rs", include_str!("watchdog.rs")),
        ] {
            assert!(!src.contains("sync_geo_override(&"), "{name}: geo_override собирается одной функцией из полного конфига");
        }
    }


    #[test]
    fn test_update_override_content() {
        let existing = "# Custom user comment\n1.2.3.4\n";
        let mut v4 = BTreeSet::new();
        v4.insert("178.248.237.68".parse().unwrap());
        let v6 = BTreeSet::new();

        let updated = update_override_file_content(existing, &v4, &v6);
        assert!(updated.contains("# Custom user comment"));
        assert!(updated.contains("1.2.3.4"));
        assert!(updated.contains(MARKER_BEGIN));
        assert!(updated.contains("178.248.237.68"));
        assert!(updated.contains(MARKER_END));

        // Повторное обновление заменяет только блок
        let mut v4_new = BTreeSet::new();
        v4_new.insert("77.246.157.212".parse().unwrap());
        let updated2 = update_override_file_content(&updated, &v4_new, &v6);
        assert!(updated2.contains("1.2.3.4"));
        assert!(!updated2.contains("178.248.237.68"));
        assert!(updated2.contains("77.246.157.212"));

        // Очистка при пустом списке
        let cleared = update_override_file_content(&updated2, &BTreeSet::new(), &BTreeSet::new());
        assert!(cleared.contains("1.2.3.4"));
        assert!(!cleared.contains(MARKER_BEGIN));
        assert!(!cleared.contains("77.246.157.212"));
    }

    #[tokio::test]
    async fn test_streaming_override_file() {
        let temp_src = std::env::temp_dir().join(format!("test_src_{}.lst", std::process::id()));
        let temp_dst = std::env::temp_dir().join(format!("test_dst_{}.lst", std::process::id()));

        tokio::fs::write(&temp_src, "# Header line\n10.0.0.1\n").await.unwrap();

        let mut v4 = BTreeSet::new();
        v4.insert("1.1.1.1".parse().unwrap());
        let v6 = BTreeSet::new();

        write_override_file_streaming(
            temp_src.to_str().unwrap(),
            temp_dst.to_str().unwrap(),
            &v4,
            &v6,
        ).await.unwrap();

        let read_back = tokio::fs::read_to_string(&temp_dst).await.unwrap();
        assert!(read_back.contains("# Header line"));
        assert!(read_back.contains("10.0.0.1"));
        assert!(read_back.contains(MARKER_BEGIN));
        assert!(read_back.contains("1.1.1.1"));
        assert!(read_back.contains(MARKER_END));

        let _ = tokio::fs::remove_file(&temp_src).await;
        let _ = tokio::fs::remove_file(&temp_dst).await;
    }

    #[tokio::test]
    async fn regression_bug_97b_fake_ip_and_private_addresses_not_added() {
        // Баг 97: fake-ip Mihomo (198.18.0.0/15) и частные адреса попадали в geo_override.
        let input: Vec<String> = [
            "198.18.0.5", "198.19.255.1", "10.1.2.3", "192.168.1.10", "172.16.5.5",
            "100.64.0.1", "169.254.1.1", "8.8.8.8", "fc00::1", "fe80::1", "2001:4860:4860::8888",
        ].iter().map(|s| s.to_string()).collect();
        let (v4, v6) = resolve_domains(&input).await;
        let v4: Vec<String> = v4.iter().map(|i| i.to_string()).collect();
        let v6: Vec<String> = v6.iter().map(|i| i.to_string()).collect();
        assert_eq!(v4, vec!["8.8.8.8".to_string()], "в geo_override допустимы только публичные IPv4");
        assert_eq!(v6, vec!["2001:4860:4860::8888".to_string()], "в geo_override допустимы только публичные IPv6");
    }

    #[test]
    fn regression_bug_97a_dns_outage_keeps_previous_override() {
        // Баг 97: DNS не ответил → пустой блок → swap на пустой ipset → force-домены уходят в DIRECT.
        assert!(keep_previous_override(10, 0), "ни один домен не разрешился — блок нельзя стирать");
        assert!(keep_previous_override(10, 4), "разрешилось меньше половины — похоже на сбой DNS");
        assert!(!keep_previous_override(10, 5));
        assert!(!keep_previous_override(10, 10));
        assert!(!keep_previous_override(0, 0), "пользователь удалил все домены — блок очищается");
    }

    #[test]
    fn regression_bug_97c_ipset_filled_by_single_restore_batch() {
        // Баг 97: каждый IP добавлялся отдельным процессом `ipset add`.
        let entries: Vec<String> = ["1.2.3.4", "5.6.0.0/16", "2001:db8::1", "мусор; reboot", "", "# comment", "9.9.9.9/33"]
            .iter().map(|s| s.to_string()).collect();
        let v4 = build_ipset_restore_script("geo_override_tmp", "inet", &entries);
        assert_eq!(
            v4,
            "create geo_override_tmp hash:net family inet -exist
flush geo_override_tmp
add geo_override_tmp 1.2.3.4 -exist
add geo_override_tmp 5.6.0.0/16 -exist
"
        );
        let v6 = build_ipset_restore_script("geo_override6_tmp", "inet6", &entries);
        assert_eq!(
            v6,
            "create geo_override6_tmp hash:net family inet6 -exist
flush geo_override6_tmp
add geo_override6_tmp 2001:db8::1 -exist
"
        );
    }

    #[test]
    fn regression_bug_97d_partial_dns_keeps_last_good_ips() {
        use std::collections::HashMap;
        use std::net::IpAddr;

        let requested: Vec<String> = (1..=10).map(|i| format!("site{i}.example.com")).collect();
        let mut cache: HashMap<String, Vec<IpAddr>> = HashMap::new();
        for i in 1..=10 {
            cache.insert(
                format!("site{i}.example.com"),
                vec![format!("1.1.1.{i}").parse().unwrap()],
            );
        }

        // Частичный ответ DNS: разрешились только домены 1..=5 (и один из них вернул fake-ip + публичный)
        let mut resolved_now: HashMap<String, Vec<IpAddr>> = HashMap::new();
        for i in 1..=5 {
            let mut ips: Vec<IpAddr> = vec![format!("2.2.2.{i}").parse().unwrap()];
            if i == 1 {
                ips.push("198.18.0.5".parse().unwrap());
                ips.push("10.0.0.1".parse().unwrap());
                ips.push("fc00::1".parse().unwrap());
            }
            resolved_now.insert(format!("site{i}.example.com"), ips);
        }

        let merged = merge_with_last_good(&requested, &resolved_now, &mut cache);

        // Для разрешённых сейчас (1..=5) — новые публичные IP; для неразрешённых (6..=10) — кэшированные
        for i in 1..=5 {
            let new_ip: IpAddr = format!("2.2.2.{i}").parse().unwrap();
            assert!(merged.contains(&new_ip), "должен содержать новый IP {new_ip}");
        }
        for i in 6..=10 {
            let cached_ip: IpAddr = format!("1.1.1.{i}").parse().unwrap();
            assert!(merged.contains(&cached_ip), "должен сохранить кэшированный IP {cached_ip}");
        }
        assert!(!merged.contains(&"198.18.0.5".parse::<IpAddr>().unwrap()), "fake-ip 198.18.0.5 не должен попадать");
        assert!(!merged.contains(&"10.0.0.1".parse::<IpAddr>().unwrap()), "частный IP не должен попадать");
        assert!(!merged.contains(&"fc00::1".parse::<IpAddr>().unwrap()), "ULA IPv6 не должен попадать");
        assert_eq!(merged.len(), 10, "в итоге ровно 10 публичных IP для всех 10 доменов");

        // Кэш обновился только для разрешённых доменов 1..=5, а 6..=10 сохранили прежние IP
        assert_eq!(cache.get("site1.example.com").unwrap(), &vec!["2.2.2.1".parse::<IpAddr>().unwrap()]);
        assert_eq!(cache.get("site6.example.com").unwrap(), &vec!["1.1.1.6".parse::<IpAddr>().unwrap()]);

        // Проверка персистентного кэша на диске: сохранение и повторная загрузка переживают рестарт
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("xkr-bug97-cache-{}-{}", std::process::id(), nonce));
        std::fs::create_dir_all(&dir).unwrap();
        let cache_file = dir.join("override_cache.json");
        save_override_cache(&cache_file, &cache).unwrap();
        let loaded = load_override_cache(&cache_file);
        assert_eq!(loaded.len(), 10);
        assert_eq!(loaded.get("site6.example.com").unwrap(), &vec!["1.1.1.6".parse::<IpAddr>().unwrap()]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
