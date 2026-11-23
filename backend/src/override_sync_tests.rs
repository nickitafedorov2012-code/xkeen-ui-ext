use super::*;

/// Асинхронно резолвит список доменов во все уникальные IPv4 и IPv6 адреса параллельно.
#[cfg(test)]
pub async fn resolve_domains(domains: &[String]) -> (BTreeSet<Ipv4Addr>, BTreeSet<Ipv6Addr>) {
    let (v4, v6, _) = resolve_domains_counted(domains).await;
    (v4, v6)
}

/// То же, что `resolve_domains`, плюс число доменов, для которых получен хотя бы один пригодный адрес.
#[cfg(test)]
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

#[cfg(test)]
pub fn set_override_cache_path_override(path: Option<std::path::PathBuf>) {
    let mut guard = OVERRIDE_CACHE_PATH_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = path;
}

/// Сбой DNS во время синхронизации (баг 97): если разрешилось меньше половины доменов,
/// старый блок geo_override сохраняется, иначе force-домены молча уходят в DIRECT.
#[cfg(test)]
pub fn keep_previous_override(requested: usize, resolved: usize) -> bool {
    requested > 0 && resolved * 2 < requested
}

/// Обновляет блок в файле ru_exclude_override.lst, сохраняя любые ручные записи пользователя.
#[cfg(test)]
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

#[cfg(test)]
static AUTO_CDNS_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub fn set_auto_cdns_cache_path_override(path: Option<std::path::PathBuf>) {
    let mut guard = AUTO_CDNS_CACHE_PATH_OVERRIDE.lock().unwrap_or_else(|e| e.into_inner());
    *guard = path;
}

#[cfg(test)]
pub fn clear_cached_auto_cdns_for_test() {
    if let Ok(mut cache) = CACHED_AUTO_CDNS.write() {
        cache.clear();
    }
}




#[test]
fn regression_bug_13_cached_cdns_and_zapret_direct_filtered() {
    let _test_guard = AUTO_CDNS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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

    // Проверяем исключение доменов Zapret DIRECT (YouTube, Discord, GitHub, пользовательские custom_entries)
    cfg.zapret.hybrid_discord = true;
    cfg.zapret.bypass_github = true;
    cfg.zapret.custom_entries = vec![crate::config::ZapretCustomEntry {
        domain: "mysku.club".into(),
        enabled: true,
        cdns: vec!["img.mysku-st.ru".into()],
    }];
    cfg.direct_domains = vec!["bank.ru".into()];
    cfg.force_domains.push("googlevideo.com".into());
    cfg.force_domains.push("discord.gg".into());
    cfg.force_domains.push("api.github.com".into());
    cfg.force_domains.push("mysku.club".into());
    cfg.force_domains.push("img.mysku-st.ru".into());
    cfg.force_domains.push("sub.bank.ru".into());
    let domains_with_zapret = collect_override_domains(&cfg, &[]);
    assert!(
        !domains_with_zapret.contains(&"googlevideo.com".to_string()),
        "Zapret DIRECT домены не должны попадать в geo_override"
    );
    assert!(!domains_with_zapret.contains(&"discord.gg".to_string()));
    assert!(!domains_with_zapret.contains(&"api.github.com".to_string()));
    assert!(!domains_with_zapret.contains(&"mysku.club".to_string()));
    assert!(!domains_with_zapret.contains(&"img.mysku-st.ru".to_string()));
    assert!(!domains_with_zapret.contains(&"sub.bank.ru".to_string()));

    // Когда пользователь очищает force_domains, старые кэшированные CDN не должны оставаться в geo_override
    cfg.force_domains.clear();
    let empty_force_domains = collect_override_domains(&cfg, &[]);
    assert!(
        empty_force_domains.is_empty(),
        "При пустом force_domains устаревший кэш CDN не должен утекать в geo_override: {:?}",
        empty_force_domains
    );
}

#[test]
fn regression_bug_13_geo_override_has_single_source_of_truth() {
    // стирало игровые домены и найденные CDN, тумблер игрового режима — CDN, старт без force-доменов
    // не синхронизировал вовсе.
    for (name, src) in [
        ("api/mod.rs", include_str!("api/mod.rs")),
        ("api/backups.rs", include_str!("api/backups.rs")),
        ("api/clash_proxy.rs", include_str!("api/clash_proxy.rs")),
        ("api/dns_adguard.rs", include_str!("api/dns_adguard.rs")),
        ("api/domains.rs", include_str!("api/domains.rs")),
        ("api/routing/mod.rs", include_str!("api/routing/mod.rs")),
        ("api/routing/devices.rs", include_str!("api/routing/devices.rs")),
        ("api/routing/gaming.rs", include_str!("api/routing/gaming.rs")),
        ("api/routing/whitelist.rs", include_str!("api/routing/whitelist.rs")),
        ("api/servers/mod.rs", include_str!("api/servers/mod.rs")),
        ("api/servers/subscriptions.rs", include_str!("api/servers/subscriptions.rs")),
        ("api/status.rs", include_str!("api/status.rs")),
        ("api/system_ssh.rs", include_str!("api/system_ssh.rs")),
        ("api/vkturn.rs", include_str!("api/vkturn.rs")),
        ("api/zapret/mod.rs", include_str!("api/zapret/mod.rs")),
        ("api/zapret/firewall.rs", include_str!("api/zapret/firewall.rs")),
        ("api/zapret/presets.rs", include_str!("api/zapret/presets.rs")),
        ("api/zapret/diagnostics.rs", include_str!("api/zapret/diagnostics.rs")),
        ("bot.rs", include_str!("bot.rs")),
        ("failover/history.rs", include_str!("failover/history.rs")),
        ("failover/mod.rs", include_str!("failover/mod.rs")),
        ("failover/probe.rs", include_str!("failover/probe.rs")),
        ("failover/quarantine.rs", include_str!("failover/quarantine.rs")),
        ("failover/scoring.rs", include_str!("failover/scoring.rs")),
        ("failover/switcher.rs", include_str!("failover/switcher.rs")),
        ("failover/types.rs", include_str!("failover/types.rs")),
        ("main.rs", include_str!("main.rs")),
        ("routing/dns_redirect.rs", include_str!("routing/dns_redirect.rs")),
        ("routing/ipset.rs", include_str!("routing/ipset.rs")),
        ("routing/iptables.rs", include_str!("routing/iptables.rs")),
        ("routing/mod.rs", include_str!("routing/mod.rs")),
        ("routing/policy_routing.rs", include_str!("routing/policy_routing.rs")),
        ("routing/validator.rs", include_str!("routing/validator.rs")),
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

#[test]
fn test_override_cache_path_override_roundtrip() {
    let custom = std::env::temp_dir().join(format!("xkr-cache-override-{}.json", std::process::id()));
    set_override_cache_path_override(Some(custom.clone()));
    assert_eq!(get_override_cache_path(), custom);
    set_override_cache_path_override(None);
    assert_eq!(
        get_override_cache_path(),
        std::path::PathBuf::from(DEFAULT_OVERRIDE_CACHE_FILE)
    );
}

#[test]
fn regression_bug_13b_cdn_cache_survives_restart() {
    let _test_guard = AUTO_CDNS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = std::env::temp_dir().join(format!("xkr-bug13b-{}", crate::auth::random_hex(8)));
    std::fs::create_dir_all(&dir).unwrap();
    let cache_file = dir.join("auto_cdns.json");

    set_auto_cdns_cache_path_override(Some(cache_file.clone()));
    clear_cached_auto_cdns_for_test();

    // 13.1: sync_cached_auto_cdns сохраняет найденные CDN и в память, и на диск
    sync_cached_auto_cdns(&["cdn.example-video.net".to_string(), "googlevideo.com".to_string()]);
    assert!(cache_file.exists(), "файл кэша auto_cdns должен быть создан на диске");

    // Имитация перезапуска панели: очищаем память, загружаем с диска
    clear_cached_auto_cdns_for_test();
    assert!(get_cached_auto_cdns().is_empty());

    let loaded = load_cached_auto_cdns();
    assert!(loaded.contains(&"cdn.example-video.net".to_string()));

    // 13.2 & 13.3: collect_override_domains(cfg, &[]) возвращает CDN из кэша, но фильтрует Zapret DIRECT (googlevideo.com)
    let mut cfg = crate::config::AppConfig::default();
    cfg.force_domains = vec!["example-video.net".to_string()];
    cfg.zapret.enabled = true;
    cfg.zapret.youtube_turbo = true;
    let collected = collect_override_domains(&cfg, &[]);
    assert!(collected.contains(&"cdn.example-video.net".to_string()), "CDN из дискового кэша обязан сохраняться при sync_from_config(cfg, &[])");
    assert!(!collected.contains(&"googlevideo.com".to_string()), "домены Zapret DIRECT должны отфильтровываться");

    clear_cached_auto_cdns_for_test();
    set_auto_cdns_cache_path_override(None);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn regression_r53_removing_force_domain_drops_its_auto_cdns() {
    let _test_guard = AUTO_CDNS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_cached_auto_cdns_for_test();

    let mut cfg_a = crate::config::AppConfig::default();
    cfg_a.force_domains = vec!["alpha.example".to_string()];
    let domains_a = collect_override_domains(&cfg_a, &["fastly-alpha-cdn.net".to_string()]);
    assert!(domains_a.contains(&"fastly-alpha-cdn.net".to_string()));

    // Пользователь удалил alpha.example и оставил только beta.example
    let mut cfg_b = crate::config::AppConfig::default();
    cfg_b.force_domains = vec!["beta.example".to_string()];
    let domains_b = collect_override_domains(&cfg_b, &["edge-beta-cdn.net".to_string()]);
    assert!(domains_b.contains(&"edge-beta-cdn.net".to_string()));
    assert!(
        !domains_b.contains(&"fastly-alpha-cdn.net".to_string()),
        "сиротский CDN удалённого force_domain не должен оставаться в override"
    );

    clear_cached_auto_cdns_for_test();
}

#[test]
fn regression_r53_ab_to_b_drops_alpha_only_cdns() {
    let _test_guard = AUTO_CDNS_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_cached_auto_cdns_for_test();

    let mut by_source = std::collections::BTreeMap::new();
    by_source.insert(
        "alpha.example".to_string(),
        ["cdn-alpha-only.net".to_string()].into_iter().collect(),
    );
    by_source.insert(
        "beta.example".to_string(),
        ["cdn-beta-only.net".to_string()].into_iter().collect(),
    );
    sync_cached_auto_cdns_by_source(&by_source);

    let mut cfg_ab = crate::config::AppConfig::default();
    cfg_ab.force_domains = vec!["alpha.example".to_string(), "beta.example".to_string()];
    let domains_ab = collect_override_domains(&cfg_ab, &[]);
    assert!(domains_ab.contains(&"cdn-alpha-only.net".to_string()));
    assert!(domains_ab.contains(&"cdn-beta-only.net".to_string()));

    // Пользователь удалил alpha.example, остался только beta.example
    let mut cfg_b = crate::config::AppConfig::default();
    cfg_b.force_domains = vec!["beta.example".to_string()];
    let domains_b = collect_override_domains(&cfg_b, &[]);
    assert!(domains_b.contains(&"cdn-beta-only.net".to_string()));
    assert!(
        !domains_b.contains(&"cdn-alpha-only.net".to_string()),
        "R53: CDN, найденный только для alpha.example, не должен оставаться при наличии beta.example"
    );

    clear_cached_auto_cdns_for_test();
}
