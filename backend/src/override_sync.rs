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

/// Асинхронно резолвит список доменов во все уникальные IPv4 и IPv6 адреса параллельно.
pub async fn resolve_domains(domains: &[String]) -> (BTreeSet<Ipv4Addr>, BTreeSet<Ipv6Addr>) {
    let mut set = tokio::task::JoinSet::new();

    for d in domains {
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
                                if !ip.is_loopback() && !ip.is_unspecified() {
                                    v4_res.push(ip);
                                }
                            }
                            IpAddr::V6(ip) => {
                                if !ip.is_loopback() && !ip.is_unspecified() {
                                    v6_res.push(ip);
                                }
                            }
                        }
                    }
                }
                (v4_res, v6_res)
            });
        }
    }

    let mut v4 = BTreeSet::new();
    let mut v6 = BTreeSet::new();

    while let Some(res) = set.join_next().await {
        if let Ok((v4_list, v6_list)) = res {
            v4.extend(v4_list);
            v6.extend(v6_list);
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

    // 1. Потоково обновляем файл ru_exclude_override.lst без буферизации всего содержимого в RAM
    let tmp_file = format!("{OVERRIDE_FILE}.tmp");
    write_override_file_streaming(OVERRIDE_FILE, &tmp_file, &v4, &v6).await?;
    if let Err(e) = tokio::fs::rename(&tmp_file, OVERRIDE_FILE).await {
        let _ = tokio::fs::remove_file(&tmp_file).await;
        return Err(format!("Не удалось переименовать {tmp_file}: {e}"));
    }

    // 2. Атомарно обновляем ipset ядра geo_override (IPv4)
    sync_ipset_family("geo_override", "inet").await?;

    // 3. Атомарно обновляем ipset ядра geo_override6 (IPv6)
    if !v6.is_empty() {
        if let Err(e) = sync_ipset_family("geo_override6", "inet6").await {
            crate::log_w!("[OVERRIDE] Ошибка синхронизации IPv6 ipset: {e}");
        }
    }

    crate::log_i!("[OVERRIDE] Синхронизировано {total_ips} IP для {} доменов", domains.len());
    Ok(total_ips)
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

    if let Ok(src_file) = tokio::fs::File::open(src_path).await {
        let reader = tokio::io::BufReader::new(src_file);
        let mut lines = reader.lines();
        let mut in_block = false;

        while let Ok(Some(line)) = lines.next_line().await {
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

async fn sync_ipset_family(set_name: &str, family: &str) -> Result<(), String> {
    let tmp = format!("{set_name}_tmp");

    // Уничтожаем возможный старый временный ipset от прерванного swap (ошибка ожидаема, если не существовал)
    let _ = run_ipset(&["destroy", &tmp]).await;

    if let Err(e) = run_ipset(&["create", &tmp, "hash:net", "family", family, "-exist"]).await {
        crate::log_w!("[OVERRIDE] ipset create {tmp} failed: {e}");
    }

    let _ = run_ipset(&["flush", &tmp]).await;

    if let Ok(file) = tokio::fs::File::open(OVERRIDE_FILE).await {
        use tokio::io::AsyncBufReadExt;
        let reader = tokio::io::BufReader::new(file);
        let mut lines = reader.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let t = line.trim();
            if t.is_empty() || t.starts_with('#') {
                continue;
            }
            let is_v6 = t.contains(':');
            if (family == "inet" && !is_v6) || (family == "inet6" && is_v6) {
                let _ = run_ipset(&["add", &tmp, t, "-exist"]).await;
            }
        }
    }

    let _ = run_ipset(&["create", set_name, "hash:net", "family", family, "-exist"]).await;

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
}
