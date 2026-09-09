//! Синхронизация принудительно проксируемых доменов (force_domains)
//! с ipset geo_override ядра Linux и файлом /opt/etc/xkeen/ipset/ru_exclude_override.lst.

use std::collections::BTreeSet;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::path::Path;

const OVERRIDE_FILE: &str = "/opt/etc/xkeen/ipset/ru_exclude_override.lst";
const OVERRIDE_DIR: &str = "/opt/etc/xkeen/ipset";
pub const MARKER_BEGIN: &str = "# --- XKEEN-ROUTE-OVERRIDE-BEGIN ---";
pub const MARKER_END: &str = "# --- XKEEN-ROUTE-OVERRIDE-END ---";

/// Асинхронно резолвит список доменов во все уникальные IPv4 и IPv6 адреса.
pub async fn resolve_domains(domains: &[String]) -> (BTreeSet<Ipv4Addr>, BTreeSet<Ipv6Addr>) {
    let mut v4 = BTreeSet::new();
    let mut v6 = BTreeSet::new();

    for d in domains {
        let clean = d.trim().trim_start_matches("www.");
        if clean.is_empty() {
            continue;
        }
        for host in [clean.to_string(), format!("www.{clean}")] {
            let addr = format!("{host}:443");
            if let Ok(iter) = tokio::net::lookup_host(addr).await {
                for sa in iter {
                    match sa.ip() {
                        IpAddr::V4(ip) => {
                            if !ip.is_loopback() && !ip.is_unspecified() {
                                v4.insert(ip);
                            }
                        }
                        IpAddr::V6(ip) => {
                            if !ip.is_loopback() && !ip.is_unspecified() {
                                v6.insert(ip);
                            }
                        }
                    }
                }
            }
        }
    }
    (v4, v6)
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

/// Синхронизирует IP-адреса force_domains с ru_exclude_override.lst и живыми ipset ядра.
pub async fn sync_geo_override(domains: &[String]) -> Result<usize, String> {
    if !Path::new(OVERRIDE_DIR).exists() {
        return Ok(0);
    }

    let (v4, v6) = resolve_domains(domains).await;
    let total_ips = v4.len() + v6.len();

    // 1. Обновляем файл ru_exclude_override.lst
    let existing = tokio::fs::read_to_string(OVERRIDE_FILE)
        .await
        .unwrap_or_default();
    let new_content = update_override_file_content(&existing, &v4, &v6);

    let tmp_file = format!("{OVERRIDE_FILE}.tmp");
    if let Err(e) = tokio::fs::write(&tmp_file, &new_content).await {
        return Err(format!("Не удалось записать {tmp_file}: {e}"));
    }
    if let Err(e) = tokio::fs::rename(&tmp_file, OVERRIDE_FILE).await {
        return Err(format!("Не удалось переименовать {tmp_file}: {e}"));
    }

    // 2. Атомарно обновляем ipset ядра geo_override (IPv4)
    sync_ipset_family("geo_override", "inet").await?;

    // 3. Атомарно обновляем ipset ядра geo_override6 (IPv6)
    if !v6.is_empty() {
        let _ = sync_ipset_family("geo_override6", "inet6").await;
    }

    crate::log_i!("[OVERRIDE] Синхронизировано {total_ips} IP для {} доменов", domains.len());
    Ok(total_ips)
}

async fn sync_ipset_family(set_name: &str, family: &str) -> Result<(), String> {
    let tmp = format!("{set_name}_tmp");

    _ = tokio::process::Command::new("ipset")
        .args(["create", &tmp, "hash:net", "family", family, "-exist"])
        .output()
        .await;

    _ = tokio::process::Command::new("ipset")
        .args(["flush", &tmp])
        .output()
        .await;

    if let Ok(content) = tokio::fs::read_to_string(OVERRIDE_FILE).await {
        for line in content.lines() {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let is_v6 = t.contains(':');
            if (family == "inet" && !is_v6) || (family == "inet6" && is_v6) {
                _ = tokio::process::Command::new("ipset")
                    .args(["add", &tmp, t, "-exist"])
                    .output()
                    .await;
            }
        }
    }

    _ = tokio::process::Command::new("ipset")
        .args(["create", set_name, "hash:net", "family", family, "-exist"])
        .output()
        .await;

    let swap = tokio::process::Command::new("ipset")
        .args(["swap", set_name, &tmp])
        .output()
        .await;

    _ = tokio::process::Command::new("ipset")
        .args(["destroy", &tmp])
        .output()
        .await;

    if let Err(e) = swap {
        return Err(format!("ipset swap {set_name}: {e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
