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
    let mut set = tokio::task::JoinSet::new();

    for (idx, d) in domains.iter().enumerate() {
        let clean = d.trim().trim_start_matches("www.").to_string();
        if clean.is_empty() {
            continue;
        }
        for host in [clean.clone(), format!("www.{clean}")] {
            set.spawn(async move {
                let mut v4_res = Vec::new();
                let mut v6_res = Vec::new();
                let addr = format!("{host}:443");
                let res = tokio::time::timeout(Duration::from_millis(1500), tokio::net::lookup_host(addr)).await;
                if let Ok(Ok(iter)) = res {
                    for sa in iter {
                        match sa.ip() {
                            IpAddr::V4(ip) => {
                                if is_override_eligible_v4(&ip) {
                                    v4_res.push(ip);
                                }
                            }
                            IpAddr::V6(ip) => {
                                if is_override_eligible_v6(&ip) {
                                    v6_res.push(ip);
                                }
                            }
                        }
                    }
                }
                (idx, v4_res, v6_res)
            });
        }
    }

    let mut v4 = BTreeSet::new();
    let mut v6 = BTreeSet::new();
    let mut resolved = std::collections::HashSet::new();

    while let Some(res) = set.join_next().await {
        if let Ok((idx, v4_list, v6_list)) = res {
            if !v4_list.is_empty() || !v6_list.is_empty() {
                resolved.insert(idx);
            }
            v4.extend(v4_list);
            v6.extend(v6_list);
        }
    }

    (v4, v6, resolved.len())
}

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

/// Синхронизирует IP-адреса force_domains с ru_exclude_override.lst и живыми ipset ядра.
pub async fn sync_geo_override(domains: &[String]) -> Result<usize, String> {
    if !Path::new(OVERRIDE_DIR).exists() {
        return Ok(0);
    }

    let _lock = IPSET_SYNC_LOCK.lock().await;

    let requested = domains.iter().filter(|d| !d.trim().is_empty()).count();
    let (v4, v6, resolved) = resolve_domains_counted(domains).await;
    let total_ips = v4.len() + v6.len();
    if keep_previous_override(requested, resolved) {
        crate::log_w!(
            "[OVERRIDE] Разрешилось {resolved} из {requested} доменов — похоже на сбой DNS, geo_override не трогаем"
        );
        return Err(format!(
            "DNS не ответил для {} из {requested} доменов: geo_override сохранён без изменений",
            requested - resolved
        ));
    }

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

    crate::log_i!("[OVERRIDE] Синхронизировано {total_ips} IP для {} доменов", domains.len());
    Ok(total_ips)
}

/// Баг 13: Единый сбор всех доменов для geo_override из полного конфига.
/// Объединяет force_domains, статические CDN-бандлы, обнаруженные auto_cdns и активные игровые домены.
pub fn collect_override_domains(cfg: &crate::config::AppConfig, extra_cdns: &[String]) -> Vec<String> {
    let mut all = cfg.force_domains.clone();
    if !cfg.force_domains.is_empty() {
        let static_cdns = crate::cdn_discovery::expand_bundles(&cfg.force_domains);
        for cdn in static_cdns {
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
    fn regression_bug_13_geo_override_has_single_source_of_truth() {
        // Баг 13: geo_override перезаписывали шесть мест, каждое своим набором доменов: сохранение доменов
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
}
