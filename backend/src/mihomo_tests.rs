use super::*;
use serde_json::json;

#[test]
fn regression_bug_8_gaming_switch_closes_only_gaming_device_sockets() {
    let val = json!({ "connections": [
        { "id": "a", "metadata": { "sourceIP": "192.168.1.50" } },
        { "id": "b", "metadata": { "sourceIP": "192.168.1.60" } },
        { "id": "c", "metadata": { "sourceIP": "" } },
        { "id": "d", "metadata": { "sourceIP": "192.168.1.50" } }
    ]});
    assert_eq!(connection_ids_from_sources(&val, &["192.168.1.50".to_string()]), vec!["a", "d"]);
    assert!(connection_ids_from_sources(&val, &[]).is_empty(), "без устройств ничего не закрываем по IP");

    let mut g = crate::config::GamingConfig::default();
    let dev = |ip: &str, enabled: bool| crate::config::GamingDevice { mac: format!("m{ip}"), ip: ip.into(), enabled, ..Default::default() };
    g.devices = vec![dev("192.168.1.50", true), dev("192.168.1.60", false), dev("", true)];
    assert_eq!(gaming_device_ips(&g), vec!["192.168.1.50"], "только включённые с IP");
    g.devices = vec![dev("192.168.1.60", false)];
    assert_eq!(gaming_device_ips(&g), vec!["192.168.1.60"], "нет включённых — единственное известное");
}


fn map(entries: Vec<(&str, Value)>) -> BTreeMap<String, Value> {
    entries.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

#[test]
fn resolve_leaf_through_chain() {
    let p = map(vec![
        ("PROXY", json!({"type": "select", "now": "Fallback"})),
        ("Fallback", json!({"type": "fallback", "now": "DE Germany"})),
        ("DE Germany", json!({"type": "vless", "server": "de1"})),
    ]);
    assert_eq!(resolve_active_leaf(&p), "DE Germany");
}

#[test]
fn resolve_leaf_empty_when_no_groups() {
    let p = map(vec![("X", json!({"type": "vless"}))]);
    assert_eq!(resolve_active_leaf(&p), "");
}

#[test]
fn resolve_leaf_cyclic_group() {
    let p = map(vec![
        ("PROXY", json!({"type": "selector", "now": "Fallback"})),
        ("Fallback", json!({"type": "fallback", "now": "PROXY"})),
    ]);
    assert_eq!(resolve_active_leaf(&p), "Fallback");
}

#[test]
fn resolve_leaf_deep_chain() {
    let p = map(vec![
        ("PROXY", json!({"type": "selector", "now": "G1"})),
        ("G1", json!({"type": "selector", "now": "G2"})),
        ("G2", json!({"type": "fallback", "now": "G3"})),
        ("G3", json!({"type": "urltest", "now": "FinalServer"})),
        ("FinalServer", json!({"type": "vless", "server": "1.2.3.4"})),
    ]);
    assert_eq!(resolve_active_leaf(&p), "FinalServer");
}

#[test]
fn resolve_leaf_empty_map() {
    let p = BTreeMap::new();
    assert_eq!(resolve_active_leaf(&p), "");
}

#[test]
fn ip_from_group_name_variants() {
    assert_eq!(ip_from_group_name("Big PC 192_168_2_118").as_deref(), Some("192.168.2.118"));
    assert_eq!(ip_from_group_name("DEV_aa_bb_cc_dd_ee_ff").as_deref(), Some("aa:bb:cc:dd:ee:ff"));
    assert_eq!(ip_from_group_name("NoIpHere"), None);
}

#[test]
fn ip_groups_from_rules_parses_src_ip_only() {
    let rules = json!({"rules": [
        {"type": "SrcIPCIDR", "payload": "192.168.2.118/32", "proxy": "Big PC 192_168_2_118"},
        {"type": "GeoIP", "payload": "RU", "proxy": "DIRECT"}
    ]});
    let m = ip_groups_from_rules(&rules);
    assert_eq!(m.get("192.168.2.118").map(|s| s.as_str()), Some("Big PC 192_168_2_118"));
    assert_eq!(m.len(), 1);
}

#[test]
fn urlencoding_encodes_non_unreserved() {
    assert_eq!(urlencoding_lite("a b/c.d-e~f_g"), "a%20b%2Fc.d-e~f_g");
    // UTF-8 multibyte characters (Cyrillic & emoji)
    assert_eq!(
        urlencoding_lite("🇺🇸 США"),
        "%F0%9F%87%BA%F0%9F%87%B8%20%D0%A1%D0%A8%D0%90"
    );
}

#[test]
fn detect_server_country_prevents_false_positives() {
    assert_eq!(detect_server_country("🇺🇸 USA Denver"), "US");
    assert_eq!(detect_server_country("Node-default"), "GLOBAL");
    assert_eq!(detect_server_country("Provider-demo"), "GLOBAL");
    assert_eq!(detect_server_country("Sweden"), "GLOBAL");
    assert_eq!(detect_server_country("Download-Node"), "GLOBAL");
    assert_eq!(detect_server_country("Only-NL-Proxy"), "GLOBAL");
    assert_eq!(detect_server_country("🇩🇪 Germany Frankfurt"), "DE");
    assert_eq!(detect_server_country("🇳🇱 Netherlands Amsterdam"), "NL");
    assert_eq!(detect_server_country("🇨🇦 Canada"), "CA");
    assert_eq!(detect_server_country("🇷🇺 Россия"), "RU");
}

#[test]
fn eval_flow_status_works_correctly() {
    assert_eq!(eval_flow_status("🇺🇸 USA Denver", "USA Denver"), "ok");
    assert_eq!(eval_flow_status("🇨🇦 Canada Fast", "Canada Fast"), "ok");
    assert_eq!(eval_flow_status("🇷🇺 Россия", "Россия"), "blocked");
    assert_eq!(eval_flow_status("🇩🇪 Germany", "Germany"), "ok");
}

#[test]
fn mojibake_cp1251_repaired() {
    // "Финляндия" в UTF-8, прочитанном как cp1251
    let src = "Финляндия";
    let bytes = src.as_bytes().to_vec();
    // кодируем байты как cp1251-символы (имитация чтения UTF-8 как cp1251)
    let mojibake: String = bytes
        .into_iter()
        .map(|b| match b {
            0xC0..=0xFF => char::from_u32(0x0410 + (b as u32 - 0xC0)).unwrap(),
            b => b as char,
        })
        .collect();
    let fixed = fix_mojibake(&mojibake).expect("должен починиться");
    assert_eq!(fixed, src);
}

#[test]
fn mojibake_cp866_pseudographics_repaired() {
    // Реальный случай с роутера: «🇫🇮 Финляндия [⚡ Стабильный ]», прочитанный
    // как cp866 (байты ≥0xB0 стали псевдографикой: «╨д╨╕╨╜…», «тЪб» = ⚡).
    let mojibake = "\u{0401}\u{042F}\u{0417}\u{043B}\u{0401}\u{042F}\u{0417}\u{043E} \
\u{2568}\u{0434}\u{2568}\u{2555}\u{2568}\u{255C}\u{2568}\u{2557}\u{2564}\u{041F}\u{2568}\u{255C}\u{2568}\u{2524}\u{2568}\u{2555}\u{2564}\u{041F} \
[\u{0442}\u{042A}\u{0431} \u{2568}\u{0431}\u{2564}\u{0412}\u{2568}\u{2591}\u{2568}\u{2592}\u{2568}\u{2555}\u{2568}\u{2557}\u{2564}\u{041C}\u{2568}\u{255C}\u{2564}\u{041B}\u{2568}\u{2563} ]";
    let dn = display_name(mojibake);
    assert!(dn.contains("Финляндия"), "DN={dn}");
    assert!(dn.contains("Стабильный"), "DN={dn}");
}

#[test]
fn mojibake_normal_cyrillic_untouched() {
    assert_eq!(fix_mojibake("Финляндия"), None);
    assert_eq!(fix_mojibake("DE Germany"), None);
}

#[test]
fn clean_name_strips_flags_and_emoji() {
    assert_eq!(clean_display_name("🇫🇮 Финляндия [⚡ Стабильный ]").starts_with("Финляндия"), true);
    assert_eq!(clean_display_name("⚡ Fastest"), "Fastest");
    assert_eq!(clean_display_name("plain"), "plain");
}

#[test]
fn display_name_repairs_and_cleans() {
    // mojibake + флаг: чистим и чиним
    let bytes = "Финляндия".as_bytes().to_vec();
    let mojibake: String = bytes
        .into_iter()
        .map(|b| match b {
            0xC0..=0xFF => char::from_u32(0x0410 + (b as u32 - 0xC0)).unwrap(),
            b => b as char,
        })
        .collect();
    assert_eq!(display_name(&mojibake), "Финляндия");
    // флаг-эмодзи перед mojibake: чиним mojibake, сохраняя флаг
    let with_flag = format!("\u{1F1EB}\u{1F1EE} {mojibake}");
    assert_eq!(display_name(&with_flag), "\u{1F1EB}\u{1F1EE} Финляндия");
    // NBSP (байт 0xA0 в mojibake) не должен рвать слова: "🇷🇺 Россия" с 0xA0 внутри
    let mut ru_bytes = "Россия".as_bytes().to_vec();
    let ru_mojibake: String = ru_bytes
        .drain(..)
        .map(|b| match b {
            0xC0..=0xFF => char::from_u32(0x0410 + (b as u32 - 0xC0)).unwrap(),
            b => char::from_u32(b as u32).unwrap(),
        })
        .collect();
    let ru_with_flag = format!("\u{1F1F7}\u{1F1FA} {ru_mojibake}");
    assert_eq!(display_name(&ru_with_flag), "\u{1F1F7}\u{1F1FA} Россия");
    // нормальное имя с флагом сохраняет флаг полностью
    assert_eq!(display_name("🇩🇪 Германия"), "🇩🇪 Германия");
}

#[test]
fn test_evaluate_selector_auto_heal_subscription_provider_protection() {
    // Узел из подписки (proxy-provider) ObsV-ne_dir: он есть в all группы, но его нет в корневом словаре proxies
    let now = "ObsV-ne_dir";
    let all = ["Fallback", "Fastest", "ObsV-ne_dir", "🇩🇪 Германия"];
    let proxies_has_key = |k: &str| k == "Fallback" || k == "Fastest" || k == "PROXY";

    // Проверяем, что для subscription-узла НЕ происходит ложного срабатывания auto-heal
    let heal = evaluate_selector_auto_heal(now, &all, proxies_has_key);
    assert_eq!(heal, None, "Узел из подписки, присутствующий в all, не должен сбрасываться в Fallback!");

    // Проверяем, что реально отсутствующий/удаленный узел переключается на Fallback
    let missing_now = "Deleted-Node-123";
    let heal_missing = evaluate_selector_auto_heal(missing_now, &all, proxies_has_key);
    assert_eq!(heal_missing, Some("Fallback"));

    // Если Fallback отсутствует в all, переключается на Fastest
    let all_no_fallback = ["Fastest", "ObsV-ne_dir"];
    let heal_no_fallback = evaluate_selector_auto_heal(missing_now, &all_no_fallback, proxies_has_key);
    assert_eq!(heal_no_fallback, Some("Fastest"));
}

#[test]
fn regression_bug_123b_auto_heal_never_sends_bs_group_to_vps() {
    let cfg = AppConfig::default();
    let proxies_has_key = |k: &str| k == "Fallback" || k == "Fastest" || k == "PROXY";

    // 1. Группа БС с отсутствующим текущим выбором и наличием Fallback/Fastest + БС-узла:
    // НИКОГДА не переключается на Fallback/Fastest/VPS — только на БС-узел (или None).
    let all_with_bs = ["Fallback", "Fastest", "DE-VPS-1", "MSK [БС]"];
    let res_bs = evaluate_selector_auto_heal_for_group(
        crate::routing::WHITELIST_GROUP_NAME,
        "Deleted-BS-Node",
        &all_with_bs,
        proxies_has_key,
        &cfg,
    );
    assert_eq!(res_bs, Some("MSK [БС]"));

    // 2. Если в группе БС нет ни одного БС-узла, ничего не трогаем (None), не отправляем в Fallback
    let all_only_vps = ["Fallback", "Fastest", "DE-VPS-1"];
    let res_no_bs = evaluate_selector_auto_heal_for_group(
        crate::routing::WHITELIST_GROUP_NAME,
        "Deleted-BS-Node",
        &all_only_vps,
        proxies_has_key,
        &cfg,
    );
    assert_eq!(res_no_bs, None);

    // 3. Игровая группа не переводится на Fallback/Fastest
    let res_gaming = evaluate_selector_auto_heal_for_group(
        crate::routing::GAMING_GROUP_NAME,
        "Deleted-Game-Node",
        &all_only_vps,
        proxies_has_key,
        &cfg,
    );
    assert_eq!(res_gaming, None);

    // 4. Пустой all (провайдеры ещё не загружены после reload) -> None
    let empty: [&str; 0] = [];
    assert_eq!(
        evaluate_selector_auto_heal_for_group("PROXY", "Any", &empty, proxies_has_key, &cfg),
        None
    );

    // 5. Обычная группа PROXY по-прежнему лечится через Fallback
    assert_eq!(
        evaluate_selector_auto_heal_for_group("PROXY", "Deleted-Node", &all_only_vps, proxies_has_key, &cfg),
        Some("Fallback")
    );
}

#[test]
fn test_disabled_providers_filtering_logic() {
    let mut cfg = AppConfig::default();
    cfg.disabled_providers = vec!["sub1".to_string(), "old_provider".to_string()];

    // Проверяем проверку принадлежности к отключенной подписке
    assert!(cfg.disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case("sub1")));
    assert!(cfg.disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case("OLD_PROVIDER")));
    assert!(!cfg.disabled_providers.iter().any(|dp| dp.eq_ignore_ascii_case("active_sub")));
}

#[test]
fn test_collect_auto_ping_ids_dedup_and_groups() {
    let mut cfg = AppConfig::default();
    cfg.failover.priority_chain = vec![
        "🇩🇪 Обход [Silver] - Германия".to_string(),
        "Fastest".to_string(),
        "  ".to_string(),
        String::new(),
        "🇩🇪 Обход [Silver] - Германия".to_string(),
    ];
    cfg.failover.priority_server = "Fallback".to_string();

    let ids = collect_auto_ping_ids(&cfg);
    // Дедуп цепочки, пустые отброшены, порядок сохранён
    assert_eq!(ids[0], "🇩🇪 Обход [Silver] - Германия");
    assert_eq!(ids[1], "Fastest");
    // priority_server добавлен, авто-группы добавлены, дублей нет
    assert!(ids.contains(&"Fallback".to_string()));
    assert!(ids.contains(&"PROXY".to_string()));
    assert_eq!(ids.len(), ids.iter().collect::<std::collections::HashSet<_>>().len());
    assert!(ids.len() <= AUTO_PING_MAX_IDS);
}

#[test]
fn test_collect_auto_ping_ids_caps_at_limit() {
    let mut cfg = AppConfig::default();
    cfg.failover.priority_chain = (0..50).map(|i| format!("node-{i}")).collect();
    cfg.failover.priority_server = String::new();

    let ids = collect_auto_ping_ids(&cfg);
    // Анти-регрессия PING-AUTO: фоновый замер не должен раздуваться до всех нод
    assert_eq!(ids.len(), AUTO_PING_MAX_IDS);
    assert_eq!(ids[0], "node-0");
}

#[test]
fn test_ping_cache_store_and_read_fresh() {
    // Анти-регрессия PING-AUTO: замеры обязаны попадать в кэш и читаться свежими,
    // иначе GET /api/servers отдаёт протухшую 5-минутную историю healthcheck-а mihomo
    let mut measured = BTreeMap::new();
    measured.insert("🇩🇪 Обход [Cooper] - Германия".to_string(), 87);
    measured.insert("dead-node".to_string(), -1);
    measured.insert("zero-node".to_string(), 0);
    store_ping_cache(&measured);

    assert_eq!(cached_ping("🇩🇪 Обход [Cooper] - Германия"), Some(87));
    // Провальные замеры (-1/0) кэш не затирают
    assert_eq!(cached_ping("dead-node"), None);
    assert_eq!(cached_ping("zero-node"), None);
    assert_eq!(cached_ping("never-measured"), None);
}

#[test]
fn test_ping_cache_replaces_with_newer_measurement() {
    let mut first = BTreeMap::new();
    first.insert("node-x".to_string(), 300);
    store_ping_cache(&first);
    assert_eq!(cached_ping("node-x"), Some(300));

    let mut second = BTreeMap::new();
    second.insert("node-x".to_string(), 95);
    store_ping_cache(&second);
    // Свежий успешный замер перезаписывает старый
    assert_eq!(cached_ping("node-x"), Some(95));

    // Неудачный повтор не стирает последний живой замер
    let mut failed = BTreeMap::new();
    failed.insert("node-x".to_string(), 0);
    store_ping_cache(&failed);
    assert_eq!(cached_ping("node-x"), Some(95));
}

#[test]
fn regression_ping_cache_evicts_stale_positive_on_dead_minus_one() {
    let mut alive = BTreeMap::new();
    alive.insert("node-die-test".to_string(), 120);
    store_ping_cache(&alive);
    assert_eq!(cached_ping("node-die-test"), Some(120));

    // При подтверждённом -1 старый положительный пинг должен быть удалён из кэша
    let mut dead = BTreeMap::new();
    dead.insert("node-die-test".to_string(), -1);
    store_ping_cache(&dead);
    assert_eq!(
        cached_ping("node-die-test"),
        None,
        "confirmed -1 must evict stale positive ping from PING_CACHE"
    );
}

#[test]
fn test_ip_groups_from_rules_rejects_dst_ip_cidr() {
    let rules_json = serde_json::json!({
        "rules": [
            {
                "type": "SRC-IP-CIDR",
                "payload": "192.168.1.55/32",
                "proxy": "GamingGroup"
            },
            {
                "type": "IP-CIDR",
                "payload": "8.8.8.0/24",
                "proxy": "DIRECT"
            },
            {
                "type": "DST-IP-CIDR",
                "payload": "1.1.1.0/24",
                "proxy": "PROXY"
            }
        ]
    });

    let map = ip_groups_from_rules(&rules_json);
    assert_eq!(map.get("192.168.1.55").map(|s| s.as_str()), Some("GamingGroup"));
    assert_eq!(map.get("8.8.8.0"), None, "IP-CIDR must NOT be mapped as a LAN device!");
    assert_eq!(map.get("1.1.1.0"), None, "DST-IP-CIDR must NOT be mapped as a LAN device!");
}

#[test]
fn test_last_delay_respects_alive_flag_and_filters_zero() {
    // 1. Alive = false: must return -1 even with old positive delay in history
    let dead_proxy = serde_json::json!({
        "alive": false,
        "history": [
            { "time": "2026-10-16T10:00:00Z", "delay": 75 }
        ]
    });
    assert_eq!(last_delay(&dead_proxy), -1);

    // 2. Alive = true, positive delay: returns delay
    let live_proxy = serde_json::json!({
        "alive": true,
        "history": [
            { "time": "2026-10-16T10:00:00Z", "delay": 75 },
            { "time": "2026-10-16T10:05:00Z", "delay": 84 }
        ]
    });
    assert_eq!(last_delay(&live_proxy), 84);

    // 3. Delay = 0 (failed check): returns -1
    let zero_delay_proxy = serde_json::json!({
        "alive": true,
        "history": [
            { "time": "2026-10-16T10:00:00Z", "delay": 0 }
        ]
    });
    assert_eq!(last_delay(&zero_delay_proxy), -1);

    // 4. Empty history: returns -1
    let no_hist_proxy = serde_json::json!({
        "alive": true,
        "history": []
    });
    assert_eq!(last_delay(&no_hist_proxy), -1);
}

#[test]
fn test_v1_9_30_eval_flow_status_allows_european_and_non_ru_nodes() {
    assert_eq!(eval_flow_status("DE", "Germany"), "ok");
    assert_eq!(eval_flow_status("NL", "Netherlands"), "ok");
    assert_eq!(eval_flow_status("FI", "Finland"), "ok");
    assert_eq!(eval_flow_status("GB", "UK"), "ok");
    assert_eq!(eval_flow_status("US", "USA"), "ok");
    assert_eq!(eval_flow_status("CA", "Canada"), "ok");
    assert_eq!(eval_flow_status("RU", "Russia"), "blocked");
    assert_eq!(eval_flow_status("BY", "Belarus"), "blocked");
}

#[test]
fn test_v1_9_30_failover_pool_delay_fallback_to_leaf_node() {
    let mut proxies = std::collections::BTreeMap::new();
    // Leaf node with valid delay
    let leaf_node = serde_json::json!({
        "name": "Finland-Fast",
        "type": "vless",
        "alive": true,
        "history": [{ "time": "2026-10-22T21:00:00Z", "delay": 42 }]
    });
    proxies.insert("Finland-Fast".to_string(), leaf_node);

    // Fallback pool with delay: 0 and now: "Finland-Fast"
    let pool = serde_json::json!({
        "name": "Fallback",
        "type": "fallback",
        "now": "Finland-Fast",
        "alive": true,
        "history": [{ "time": "2026-10-22T21:00:00Z", "delay": 0 }]
    });
    proxies.insert("Fallback".to_string(), pool);

    let p = proxies.get("Fallback").unwrap();
    let now_target = p.get("now").and_then(|n| n.as_str()).unwrap_or("");
    let group_delay = last_delay(p);
    let effective_delay = if group_delay > 0 {
        group_delay
    } else if !now_target.is_empty() && now_target != "—" {
        proxies.get(now_target).map(last_delay).unwrap_or(0)
    } else {
        0
    };
    assert_eq!(effective_delay, 42, "Lazy pool delay must resolve to active leaf node delay");
}

#[tokio::test]
async fn test_ping_server_l7_split_primary_timeout_secondary_fast() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    if req.contains("cloudflare") {
                        // Primary endpoint times out / hangs
                        tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;
                        let resp = "HTTP/1.1 200 OK\r\nContent-Length: 14\r\nContent-Type: application/json\r\n\r\n{\"delay\": -1}";
                        let _ = socket.write_all(resp.as_bytes()).await;
                    } else {
                        // Secondary endpoint responds fast
                        tokio::time::sleep(tokio::time::Duration::from_millis(30)).await;
                        let resp = "HTTP/1.1 200 OK\r\nContent-Length: 13\r\nContent-Type: application/json\r\n\r\n{\"delay\": 75}";
                        let _ = socket.write_all(resp.as_bytes()).await;
                    }
                });
            } else {
                break;
            }
        }
    });

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".to_string();
    cfg.mihomo.port = port;
    let client = reqwest::Client::new();

    let start = std::time::Instant::now();
    let ping = ping_server_l7_split(&client, &cfg, "test_server", 1000).await;
    let elapsed = start.elapsed();

    assert_eq!(ping, 75, "Expected secondary response 75ms");
    assert!(elapsed < tokio::time::Duration::from_millis(600), "Must return fast secondary response without waiting for primary timeout, elapsed: {:?}", elapsed);
}

#[tokio::test]
async fn test_ping_server_l7_split_secondary_fails_fast_primary_succeeds() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    tokio::spawn(async move {
        loop {
            if let Ok((mut socket, _)) = listener.accept().await {
                tokio::spawn(async move {
                    let mut buf = [0u8; 1024];
                    let n = socket.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]);
                    if req.contains("cloudflare") {
                        // Primary succeeds in 60ms
                        tokio::time::sleep(tokio::time::Duration::from_millis(60)).await;
                        let resp = "HTTP/1.1 200 OK\r\nContent-Length: 13\r\nContent-Type: application/json\r\n\r\n{\"delay\": 85}";
                        let _ = socket.write_all(resp.as_bytes()).await;
                    } else {
                        // Secondary fails fast in 10ms
                        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                        let resp = "HTTP/1.1 500 Internal Server Error\r\nContent-Length: 0\r\n\r\n";
                        let _ = socket.write_all(resp.as_bytes()).await;
                    }
                });
            } else {
                break;
            }
        }
    });

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".to_string();
    cfg.mihomo.port = port;
    let client = reqwest::Client::new();

    let ping = ping_server_l7_split(&client, &cfg, "test_server", 1000).await;
    assert_eq!(ping, 85, "Expected primary response 85ms when secondary failed fast");
}

#[tokio::test]
async fn test_vk_call_server_catalog_includes_file_provider_node() {
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut cfg = AppConfig::default();
    cfg.vkturn.enabled = true;
    let name = crate::vkturn::PROXY_NAME;
    let proxies = BTreeMap::from([
        ("PROXY".into(), json!({ "type": "Selector", "now": name })),
        ("DIRECT".into(), json!({ "type": "Direct" })),
    ]);
    let providers = BTreeMap::from([
        (name.into(), json!({
            "name": name, "type": "Direct", "_provider": "vkturn",
            "history": [{ "delay": 73 }]
        })),
        ("other-direct".into(), json!({ "type": "Direct" })),
    ]);
    set_proxies_cache_for_test(proxies, std::time::Instant::now());
    set_provider_proxies_cache_for_test(providers, std::time::Instant::now());
    let result = get_servers(&reqwest::Client::new(), &cfg, &[name.into()]).await;
    clear_proxies_cache_for_test();
    let servers = result.unwrap();
    let vk: Vec<_> = servers.iter().filter(|s| s.id == name).collect();
    assert_eq!(vk.len(), 1, "VK file-provider node must appear once in the server catalog");
    assert_eq!(vk[0].protocol, "VK-CALL");
    assert_eq!(vk[0].provider.as_deref(), Some("vkturn"));
    assert_eq!(vk[0].ping_ms, 73);
    assert!(vk[0].is_active);
    assert!(vk[0].is_priority);
    assert!(!vk[0].is_pool);
    assert!(!servers.iter().any(|s| s.id == "DIRECT" || s.id == "other-direct"));
}

#[tokio::test]
async fn test_vk_call_server_catalog_deduplicates_and_hides_disabled_tunnel() {
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let name = crate::vkturn::PROXY_NAME;
    let proxy = json!({ "name": name, "type": "Direct", "history": [] });
    for enabled in [true, false] {
        let mut cfg = AppConfig::default();
        cfg.vkturn.enabled = enabled;
        set_proxies_cache_for_test(BTreeMap::from([(name.into(), proxy.clone())]), std::time::Instant::now());
        set_provider_proxies_cache_for_test(BTreeMap::from([(name.into(), proxy.clone())]), std::time::Instant::now());
        let result = get_servers(&reqwest::Client::new(), &cfg, &[]).await;
        clear_proxies_cache_for_test();
        let servers = result.unwrap();
        assert_eq!(servers.iter().filter(|s| s.id == name).count(), usize::from(enabled));
        if enabled {
            assert_eq!(servers[0].ping_ms, -1, "an unmeasured tunnel must not get a fabricated ping");
            assert!(!servers[0].is_active);
        }
    }
}

#[tokio::test]
async fn regression_vk_call_delay_uses_active_selector_for_file_provider() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let response = if req.contains("/group/PROXY/delay") {
                    let body = "{\"📞 VK-туннель [БС]\":108}";
                    format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len())
                } else {
                    "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n".to_string()
                };
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });
    let node = crate::vkturn::PROXY_NAME;
    set_proxies_cache_for_test(BTreeMap::from([(
        "PROXY".into(),
        json!({ "type": "Selector", "now": node, "all": [node] }),
    )]), std::time::Instant::now());
    set_provider_proxies_cache_for_test(BTreeMap::new(), std::time::Instant::now());
    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    let delay = ping_server_url_expected_range(&reqwest::Client::new(), &cfg, node, 5000, Some("http://example.test/204"), None).await;
    clear_proxies_cache_for_test();
    assert_eq!(delay, 108, "active file-provider proxy must be tested through its selector group");
}

fn sample_proxies() -> BTreeMap<String, Value> {
    let mut m = BTreeMap::new();
    m.insert("PROXY".to_string(), json!({ "type": "Selector", "now": "DE-01" }));
    m.insert("DE-01".to_string(), json!({ "type": "Vless", "server": "1.2.3.4" }));
    m
}

#[test]
fn test_proxies_cache_hit_within_ttl() {
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let data = sample_proxies();
    set_proxies_cache_for_test(data.clone(), std::time::Instant::now());

    let cached = get_cached_proxies_if_fresh();
    assert!(cached.is_some(), "Cache must be fresh within 3 seconds");
    let cached_map = cached.unwrap();
    assert_eq!(cached_map.len(), 2);
    assert_eq!(cached_map.get("PROXY").unwrap().get("now").unwrap().as_str(), Some("DE-01"));

    clear_proxies_cache_for_test();
}

#[test]
fn test_proxies_cache_expiry_after_ttl() {
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let data = sample_proxies();
    let expired_time = std::time::Instant::now() - std::time::Duration::from_secs(PROXIES_CACHE_TTL_SECS + 1);
    set_proxies_cache_for_test(data, expired_time);

    let cached = get_cached_proxies_if_fresh();
    assert!(cached.is_none(), "Cache must expire after 3 seconds TTL");

    clear_proxies_cache_for_test();
}

#[test]
fn test_proxies_cache_manual_invalidation() {
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let data = sample_proxies();
    set_proxies_cache_for_test(data, std::time::Instant::now());
    assert!(get_cached_proxies_if_fresh().is_some());

    invalidate_proxies_cache();
    assert!(get_cached_proxies_if_fresh().is_none(), "Cache must be cleared after invalidation");

    clear_proxies_cache_for_test();
}

#[test]
fn test_proxies_cache_stale_fallback_window() {
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let data = sample_proxies();

    // 1. Entry is 10 seconds old: expired for fresh read, but valid for resilient fallback (< 30s)
    let stale_time = std::time::Instant::now() - std::time::Duration::from_secs(10);
    set_proxies_cache_for_test(data.clone(), stale_time);

    assert!(get_cached_proxies_if_fresh().is_none(), "10s old entry must not be returned as fresh");
    let fallback = get_cached_proxies_if_recent();
    assert!(fallback.is_some(), "10s old entry must be available for resilient fallback");
    assert!(fallback.unwrap().1 >= std::time::Duration::from_secs(10));

    // 2. Entry is 35 seconds old: exceeded MAX_STALE (30s)
    let dead_time = std::time::Instant::now() - std::time::Duration::from_secs(PROXIES_CACHE_MAX_STALE_SECS + 5);
    set_proxies_cache_for_test(data, dead_time);

    assert!(get_cached_proxies_if_recent().is_none(), "35s old entry must exceed maximum stale window");

    clear_proxies_cache_for_test();
}

#[test]
fn test_proxies_cache_concurrent_thread_safety() {
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let handles: Vec<_> = (0..8)
        .map(|i| {
            std::thread::spawn(move || {
                let mut m = BTreeMap::new();
                m.insert(format!("NODE_{i}"), json!({ "id": i }));
                set_proxies_cache_for_test(m, std::time::Instant::now());
                let _ = get_cached_proxies_if_fresh();
                if i % 2 == 0 {
                    invalidate_proxies_cache();
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("Concurrent cache thread must not panic or deadlock");
    }

    clear_proxies_cache_for_test();
}

#[test]
fn test_provider_proxies_cache_lifecycle() {
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let mut data = BTreeMap::new();
    data.insert("ProvServer1".to_string(), json!({ "name": "ProvServer1", "type": "vless" }));

    // 1. Fresh hit within TTL
    set_provider_proxies_cache_for_test(data.clone(), std::time::Instant::now());
    let cached = get_cached_provider_proxies_if_fresh();
    assert!(cached.is_some(), "Provider proxies cache must be fresh within 3s");
    assert_eq!(cached.unwrap().len(), 1);

    // 2. Invalidation
    invalidate_proxies_cache();
    assert!(get_cached_provider_proxies_if_fresh().is_none(), "Provider proxies cache must be invalidated");

    // 3. Expiry after TTL
    let expired_time = std::time::Instant::now() - std::time::Duration::from_secs(PROXIES_CACHE_TTL_SECS + 1);
    set_provider_proxies_cache_for_test(data.clone(), expired_time);
    assert!(get_cached_provider_proxies_if_fresh().is_none(), "Provider proxies cache must expire after TTL");

    // 4. Stale fallback
    let fallback = get_cached_provider_proxies_if_recent();
    assert!(fallback.is_some(), "Provider proxies cache must have recent stale fallback");
    assert_eq!(fallback.unwrap().0.len(), 1);

    clear_proxies_cache_for_test();
}

#[test]
fn test_subscription_userinfo_parsing_and_injection() {
    let raw_hdr = "upload=1024; download=2048; total=10737418240; expire=1735689600";
    let parsed = parse_subscription_userinfo_str(raw_hdr).expect("Userinfo must parse");
    assert_eq!(parsed.upload, 1024);
    assert_eq!(parsed.download, 2048);
    assert_eq!(parsed.total, 10737418240);
    assert_eq!(parsed.expire, 1735689600);

    let yaml = "proxies:\n  - name: test\n    type: vless\n";
    let injected = inject_or_replace_userinfo_in_yaml(yaml, &parsed);
    assert!(injected.starts_with("# subscription-userinfo: upload=1024; download=2048; total=10737418240; expire=1735689600\n"));
    assert!(injected.contains("proxies:\n  - name: test"));

    // Replacing existing userinfo line
    let mut parsed2 = parsed.clone();
    parsed2.download = 4096;
    let replaced = inject_or_replace_userinfo_in_yaml(&injected, &parsed2);
    assert!(replaced.starts_with("# subscription-userinfo: upload=1024; download=4096; total=10737418240; expire=1735689600\n"));
    // Ensure not duplicated
    let count = replaced.matches("subscription-userinfo").count();
    assert_eq!(count, 1);

    // Body extraction
    let extracted = extract_subscription_userinfo_from_body(&replaced).expect("Must extract from body");
    assert_eq!(extracted.download, 4096);
}

#[test]
fn regression_bug_122_switch_server_keeps_whitelist_and_user_groups() {
    // Баг 122: switch_server переключал на цель ВСЕ selector-группы, включая 🛡️ Whitelist и
    // пользовательские группы (YouTube/Netflix), намеренно стоящие на других нодах.
    let mut proxies = std::collections::BTreeMap::new();
    let sel = |now: &str| serde_json::json!({"type": "Selector", "now": now, "all": ["node-a", "node-b", "node-c"]});
    proxies.insert("PROXY".to_string(), sel("node-a"));
    proxies.insert("Telegram".to_string(), sel("node-a")); // следует за основной группой
    proxies.insert("YouTube".to_string(), sel("node-c")); // намеренно на другой ноде
    proxies.insert("🛡️ Whitelist".to_string(), sel("node-b"));
    proxies.insert("🎮 Gaming".to_string(), sel("node-a"));
    let groups = select_groups_to_switch(&proxies, "node-a");
    assert_eq!(groups, vec!["PROXY".to_string(), "Telegram".to_string()]);
}

#[test]
fn regression_bug_54b_flow_switch_does_not_reset_lan() {
    // Баг 54: переключение маршрута Google AI не должно вызывать close_all_connections
    let mihomo_src = include_str!("mihomo.rs");
    let api_src = include_str!("api/servers/mod.rs");

    // Проверяем тело функции switch_flow_server в mihomo.rs
    let switch_fn = mihomo_src.split("pub async fn switch_flow_server")
        .nth(1)
        .expect("switch_flow_server function must exist");
    let switch_body = switch_fn.split("pub async fn ping_group").next().unwrap_or(switch_fn);
    assert!(!switch_body.contains("close_all_connections"), "switch_flow_server не должен вызывать close_all_connections");

    // Проверяем тело функции repair_flow в api.rs
    let repair_fn = api_src.split("pub async fn repair_flow")
        .nth(1)
        .expect("repair_flow function must exist");
    let repair_body = repair_fn.split("pub struct FlowAutoSelectReq").next().unwrap_or(repair_fn);
    assert!(!repair_body.contains("close_all_connections"), "repair_flow не должен вызывать close_all_connections");

    // Проверяем тело функции auto_select_flow_server в api.rs
    let autoselect_fn = api_src.split("pub async fn auto_select_flow_server")
        .nth(1)
        .expect("auto_select_flow_server function must exist");
    let autoselect_body = autoselect_fn.split("pub async fn get_flow_extension").next().unwrap_or(autoselect_fn);
    assert!(!autoselect_body.contains("close_all_connections"), "auto_select_flow_server не должен вызывать close_all_connections");
}

#[tokio::test]
async fn regression_vkturn_25_providers_filter_unified_node_and_targeted_ping() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());

    let group_delay_hits = std::sync::Arc::new(AtomicUsize::new(0));
    let healthcheck_hits = std::sync::Arc::new(AtomicUsize::new(0));
    let hc_hits_srv = healthcheck_hits.clone();
    let gd_hits_srv = group_delay_hits.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let hc_hits = hc_hits_srv.clone();
            let gd_hits = gd_hits_srv.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let body;
                let status;
                if first_line.contains("/providers/proxies/vkturn/healthcheck") {
                    hc_hits.fetch_add(1, Ordering::SeqCst);
                    status = "204 No Content";
                    body = String::new();
                } else if first_line.contains("/providers/proxies/vkturn") {
                    status = "200 OK";
                    body = json!({
                        "name": "vkturn",
                        "vehicleType": "File",
                        "proxies": [{
                            "name": crate::vkturn::PROXY_NAME,
                            "type": "Direct",
                            "history": [{ "delay": 95 }]
                        }]
                    }).to_string();
                } else if first_line.starts_with("GET /providers/proxies ") {
                    status = "200 OK";
                    body = json!({
                        "providers": {
                            "vkturn": {
                                "name": "vkturn",
                                "vehicleType": "File",
                                "proxies": [{ "name": crate::vkturn::PROXY_NAME }]
                            },
                            "sub_main": {
                                "name": "sub_main",
                                "vehicleType": "HTTP",
                                "proxies": [{ "name": "DE-01" }]
                            }
                        }
                    }).to_string();
                } else if first_line.contains("/group/PROXY/delay") {
                    gd_hits.fetch_add(1, Ordering::SeqCst);
                    status = "200 OK";
                    body = "{\"📞 VK-туннель [БС]\":108}".to_string();
                } else {
                    status = "404 Not Found";
                    body = String::new();
                }
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    cfg.vkturn.enabled = true;
    cfg.vkturn.peer = "203.0.113.10:46000".into();

    // Item 6: vkturn must not appear in get_providers_info or updatable_provider_names
    let provs = get_providers_info(&reqwest::Client::new(), &cfg).await.unwrap();
    assert!(!provs.iter().any(|p| p.id == crate::vkturn::PROVIDER_ID), "vkturn must be hidden from providers");
    assert!(provs.iter().any(|p| p.id == "sub_main"));
    let updatable = updatable_provider_names(&reqwest::Client::new(), &cfg).await;
    assert!(!updatable.iter().any(|p| p == crate::vkturn::PROVIDER_ID));
    assert!(updatable.iter().any(|p| p == "sub_main"));

    // Items 7 & 16: unified name (without old "Сервер звонков ВК") and parsed peer host/port
    let vk_name = crate::vkturn::PROXY_NAME;
    set_proxies_cache_for_test(BTreeMap::from([
        ("PROXY".into(), json!({ "type": "Selector", "now": vk_name, "all": [vk_name] })),
    ]), std::time::Instant::now());
    set_provider_proxies_cache_for_test(BTreeMap::new(), std::time::Instant::now());
    let servers = get_servers(&reqwest::Client::new(), &cfg, &[]).await.unwrap();
    let vk = servers.iter().find(|s| s.id == vk_name).expect("vkturn server present");
    assert_eq!(vk.name, display_name(vk_name));
    assert_eq!(vk.host, "203.0.113.10");
    assert_eq!(vk.port, 46000);

    // Item 21: targeted vkturn healthcheck instead of pinging entire PROXY group
    let delay = ping_server_url_expected_range(
        &reqwest::Client::new(),
        &cfg,
        vk_name,
        5000,
        Some("http://example.test/204"),
        None,
    ).await;
    clear_proxies_cache_for_test();
    assert_eq!(delay, 95);
    assert!(healthcheck_hits.load(Ordering::SeqCst) >= 1, "targeted vkturn provider healthcheck must be used");
    assert_eq!(group_delay_hits.load(Ordering::SeqCst), 0, "must not ping entire PROXY selector when provider healthcheck succeeds");
}

#[test]
fn regression_bug_55b_country_by_not_english_word_and_bs_candidates() {
    // 55.2: "by" как английское слово НЕ даёт Беларусь; флаг / заглавный токен BY / Belarus -> BY
    assert_ne!(detect_country("Server by Hetzner"), Some("BY"));
    assert_ne!(detect_country("Fast relay by user"), Some("BY"));
    assert_eq!(detect_country("🇧🇾 Minsk"), Some("BY"));
    assert_eq!(detect_country("DE-BY-1"), Some("BY"));
    assert_eq!(detect_country("[BY] Belarus Relay"), Some("BY"));
    assert_eq!(detect_country("Belarus-01"), Some("BY"));
    assert_eq!(detect_country("🇩🇪 Germany by Hetzner"), Some("DE"));

    // 55.1: при активном БС кандидаты берутся из БС-группы/Flow, а не из PROXY
    let cfg = AppConfig::default();
    let groups = vec![
        "PROXY".to_string(),
        "Fastest".to_string(),
        crate::routing::WHITELIST_GROUP_NAME.to_string(),
        "🤖 Google AI".to_string(),
    ];
    let bs_cands = flow_ping_candidates(&cfg, &groups, true);
    assert!(
        bs_cands.contains(&crate::routing::WHITELIST_GROUP_NAME.to_string()),
        "when bs_active=true, whitelist group must be in candidates: {bs_cands:?}"
    );
    assert!(
        !bs_cands.iter().any(|g| g == "PROXY" || g == "Fastest"),
        "when bs_active=true, PROXY/Fastest must NOT be pinged: {bs_cands:?}"
    );

    let normal_cands = flow_ping_candidates(&cfg, &groups, false);
    assert_eq!(
        normal_cands.first().map(String::as_str),
        Some("🤖 Google AI"),
        "when bs_active=false, Flow group comes first: {normal_cands:?}"
    );
    assert!(
        normal_cands.contains(&"PROXY".to_string()),
        "when bs_active=false, PROXY fallback is allowed: {normal_cands:?}"
    );
}

#[tokio::test]
async fn regression_v1_16_15_desired_tunnel_server_and_reload_selector_restore() {
    use axum::{extract::Request, http::StatusCode, response::IntoResponse, Json, Router};
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    let _cache_guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let mut cfg = AppConfig::default();
    cfg.failover.priority_server = "🇩🇪 Обход".into();
    cfg.failover.priority_chain = vec!["🇫🇮 Finland".into()];
    cfg.failover.bs_priority_chain = vec!["📞 VK-туннель [БС]".into()];
    cfg.whitelist.enabled = true;
    cfg.whitelist.follow_failover = false;
    cfg.whitelist.target_server = Some("📞 VK-туннель [БС]".into());

    // 1. desired_tunnel_server_for_cfg: concrete pre_reload wins over cfg; meta-groups ignored
    assert_eq!(
        desired_tunnel_server_for_cfg(&cfg, Some("🇫🇮 Finland")).as_deref(),
        Some("🇫🇮 Finland")
    );
    assert_eq!(
        desired_tunnel_server_for_cfg(&cfg, Some("Fallback")).as_deref(),
        Some("📞 VK-туннель [БС]")
    );
    assert_eq!(
        desired_tunnel_server_for_cfg(&cfg, Some("Fastest")).as_deref(),
        Some("📞 VK-туннель [БС]")
    );
    assert_eq!(
        desired_tunnel_server_for_cfg(&cfg, None).as_deref(),
        Some("📞 VK-туннель [БС]")
    );
    cfg.whitelist.enabled = false;
    assert_eq!(
        desired_tunnel_server_for_cfg(&cfg, None).as_deref(),
        Some("🇩🇪 Обход")
    );
    cfg.whitelist.enabled = true;

    // 2. Runtime tracking (LAST_SELECTED_TUNNEL_SERVER) ignores meta-groups
    record_selected_tunnel_server("Fallback");
    assert_eq!(last_selected_tunnel_server(), None);
    record_selected_tunnel_server("📞 VK-туннель [БС]");
    assert_eq!(last_selected_tunnel_server().as_deref(), Some("📞 VK-туннель [БС]"));

    // 3. reload_config preserves active server across PUT /configs?force=true even if YAML resets selectors
    //    and syncs 🛡️ Whitelist even when follow_failover == false and target_server was stale.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = listener.local_addr().unwrap().port();
    cfg.whitelist.target_server = Some("🇩🇪 Обход".into());

    let reloaded = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reloaded_srv = reloaded.clone();
    let calls: Arc<Mutex<Vec<(axum::http::Method, String, Option<String>)>>> =
        Arc::new(Mutex::new(Vec::new()));
    let calls_srv = calls.clone();

    let app = Router::new().fallback(move |req: Request| {
        let reloaded_srv = reloaded_srv.clone();
        let calls_srv = calls_srv.clone();
        async move {
            let method = req.method().clone();
            let path = req.uri().path().to_string();
            let body_bytes = axum::body::to_bytes(req.into_body(), 4096)
                .await
                .unwrap_or_default();
            let body_name = serde_json::from_slice::<serde_json::Value>(&body_bytes)
                .ok()
                .and_then(|v| v.get("name").and_then(|n| n.as_str()).map(str::to_string));
            calls_srv.lock().unwrap().push((method.clone(), path.clone(), body_name));

            match (method, path.as_str()) {
                (axum::http::Method::GET, "/proxies") => {
                    let (proxy_now, wl_now) = if reloaded_srv.load(std::sync::atomic::Ordering::SeqCst) {
                        ("🇩🇪 Обход", "🇩🇪 Обход")
                    } else {
                        ("📞 VK-туннель [БС]", "🇩🇪 Обход")
                    };
                    Json(json!({
                        "proxies": {
                            "PROXY": { "type": "Selector", "now": proxy_now, "all": ["🇩🇪 Обход", "📞 VK-туннель [БС]", "Fallback"] },
                            "🛡️ Whitelist": { "type": "Selector", "now": wl_now, "all": ["🇩🇪 Обход", "📞 VK-туннель [БС]", "DIRECT"] },
                            "🇩🇪 Обход": { "type": "Vless" },
                            "📞 VK-туннель [БС]": { "type": "Vless" },
                        }
                    }))
                    .into_response()
                }
                (axum::http::Method::PUT, "/configs") => {
                    reloaded_srv.store(true, std::sync::atomic::Ordering::SeqCst);
                    StatusCode::NO_CONTENT.into_response()
                }
                _ => StatusCode::NO_CONTENT.into_response(),
            }
        }
    });
    let mock = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let http = reqwest::Client::new();

    reload_config(&http, &cfg).await.expect("reload_config must succeed");
    mock.abort();
    clear_proxies_cache_for_test();

    let wl_path = format!("/proxies/{}", urlencoding_lite(crate::routing::WHITELIST_GROUP_NAME));
    let recorded = calls.lock().unwrap();
    assert!(
        recorded.iter().any(|(m, p, n)| {
            *m == axum::http::Method::PUT && p == "/proxies/PROXY" && n.as_deref() == Some("📞 VK-туннель [БС]")
        }),
        "PROXY selector must be restored to pre-reload active server: {recorded:?}"
    );
    assert!(
        recorded.iter().any(|(m, p, n)| {
            *m == axum::http::Method::PUT && *p == wl_path && n.as_deref() == Some("📞 VK-туннель [БС]")
        }),
        "🛡️ Whitelist selector must be restored to active tunnel server too: {recorded:?}"
    );
}

#[tokio::test]
async fn regression_provider_node_ping_uses_provider_healthcheck_without_group_delay_storm() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();

    let direct_proxy_delay_hits = Arc::new(AtomicUsize::new(0));
    let group_delay_hits = Arc::new(AtomicUsize::new(0));
    let provider_node_hc_hits = Arc::new(AtomicUsize::new(0));

    let d_hits = direct_proxy_delay_hits.clone();
    let g_hits = group_delay_hits.clone();
    let p_hits = provider_node_hc_hits.clone();

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let d_hits = d_hits.clone();
            let g_hits = g_hits.clone();
            let p_hits = p_hits.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let (status, body) = if first_line.contains("/providers/proxies/geodema2/geodema-de-1/healthcheck") {
                    p_hits.fetch_add(1, Ordering::SeqCst);
                    ("200 OK", r#"{"delay":142}"#.to_string())
                } else if first_line.contains("/providers/proxies/geodema2/geodema-dead-2/healthcheck") {
                    p_hits.fetch_add(1, Ordering::SeqCst);
                    ("504 Gateway Timeout", r#"{"message":"timeout"}"#.to_string())
                } else if first_line.contains("/group/PROXY/delay") {
                    g_hits.fetch_add(1, Ordering::SeqCst);
                    ("200 OK", r#"{"geodema-de-1":999}"#.to_string())
                } else if first_line.contains("/proxies/geodema-de-1/delay")
                    || first_line.contains("/proxies/geodema-dead-2/delay")
                {
                    d_hits.fetch_add(1, Ordering::SeqCst);
                    ("404 Not Found", String::new())
                } else {
                    ("404 Not Found", String::new())
                };
                let resp = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    set_proxies_cache_for_test(
        BTreeMap::from([(
            "PROXY".into(),
            json!({
                "type": "Selector",
                "now": "other-active-node",
                "all": ["other-active-node", "geodema-de-1", "geodema-dead-2"]
            }),
        )]),
        std::time::Instant::now(),
    );
    set_provider_proxies_cache_for_test(
        BTreeMap::from([
            (
                "geodema-de-1".into(),
                json!({ "name": "geodema-de-1", "_provider": "geodema2", "_providers": ["geodema2"] }),
            ),
            (
                "geodema-dead-2".into(),
                json!({ "name": "geodema-dead-2", "_provider": "geodema2", "_providers": ["geodema2"] }),
            ),
        ]),
        std::time::Instant::now(),
    );

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;

    let client = reqwest::Client::new();
    let alive_delay = ping_server_url_expected_range(
        &client,
        &cfg,
        "geodema-de-1",
        3000,
        Some("http://example.test/204"),
        None,
    )
    .await;
    let dead_delay = ping_server_url_expected_range(
        &client,
        &cfg,
        "geodema-dead-2",
        3000,
        Some("http://example.test/204"),
        None,
    )
    .await;
    clear_proxies_cache_for_test();

    assert_eq!(alive_delay, 142, "alive provider node must return delay from /providers/proxies/{{provider}}/{{node}}/healthcheck");
    assert_eq!(dead_delay, -1, "dead provider node must return -1 without group delay storm");
    assert_eq!(provider_node_hc_hits.load(Ordering::SeqCst), 2, "must call per-node provider healthcheck twice");
    assert_eq!(group_delay_hits.load(Ordering::SeqCst), 0, "must NEVER trigger /group/PROXY/delay when PROXY.now != server_id");
    assert_eq!(direct_proxy_delay_hits.load(Ordering::SeqCst), 0, "must not hit 404 /proxies/{{id}}/delay for known provider nodes");
}

#[tokio::test]
async fn regression_get_provider_proxies_singleflight_and_invalidation() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let hits = Arc::new(AtomicUsize::new(0));
    let hits_srv = hits.clone();

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let hits_srv = hits_srv.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = socket.read(&mut buf).await;
                hits_srv.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(30)).await;
                let body = r#"{"providers":{"sub1":{"vehicleType":"HTTP","proxies":[{"name":"DE-01"}]}}}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    let client = reqwest::Client::new();

    // Запускаем 12 параллельных запросов при холодном кэше — на сервер должен уйти только 1 запрос
    let results: Vec<_> = stream::iter(0..12)
        .map(|_| get_provider_proxies(&client, &cfg))
        .buffer_unordered(12)
        .collect()
        .await;
    for res in results {
        let map = res.expect("get_provider_proxies must succeed");
        assert!(map.contains_key("DE-01"));
    }
    assert_eq!(hits.load(Ordering::SeqCst), 1, "12 concurrent cold-cache calls must coalesce into 1 HTTP request");

    // После инвалидации кэша следующий вызов должен снова сходить на сервер
    invalidate_proxies_cache();
    let map2 = get_provider_proxies(&client, &cfg).await.expect("get_provider_proxies after invalidation");
    assert!(map2.contains_key("DE-01"));
    assert_eq!(hits.load(Ordering::SeqCst), 2, "invalidated cache must trigger a fresh fetch");
    clear_proxies_cache_for_test();
}

#[tokio::test]
async fn regression_auto_ping_skips_when_manual_ping_active_or_recent() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();
    reset_manual_ping_state_for_test();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let ping_hits = Arc::new(AtomicUsize::new(0));
    let ping_hits_srv = ping_hits.clone();

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let ping_hits_srv = ping_hits_srv.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                if req.contains("/proxies/node-1/delay") {
                    ping_hits_srv.fetch_add(1, Ordering::SeqCst);
                }
                let body = r#"{"delay":55}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(resp.as_bytes()).await;
                let _ = socket.shutdown().await;
            });
        }
    });

    set_proxies_cache_for_test(
        BTreeMap::from([("node-1".into(), json!({ "type": "Vless", "server": "1.2.3.4" }))]),
        std::time::Instant::now(),
    );
    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    cfg.failover.priority_chain = vec!["node-1".into()];

    let client = reqwest::Client::new();

    // 1. Во время активного ручного пинга фоновый тик должен пропускаться
    let manual_guard = begin_manual_ping();
    assert!(should_skip_auto_ping(), "must skip auto-ping while manual ping is active");
    assert!(!run_auto_ping_tick(&client, &cfg).await, "auto-ping tick must be skipped during manual ping");
    assert_eq!(ping_hits.load(Ordering::SeqCst), 0);

    // 2. Сразу после завершения ручного пинга (в окне cooldown) тоже пропускается
    drop(manual_guard);
    assert!(should_skip_auto_ping(), "must skip auto-ping during post-manual cooldown");
    assert!(!run_auto_ping_tick(&client, &cfg).await, "auto-ping tick must be skipped during cooldown");
    assert_eq!(ping_hits.load(Ordering::SeqCst), 0);

    // 3. Когда ручного пинга давно не было — тик выполняется
    reset_manual_ping_state_for_test();
    assert!(!should_skip_auto_ping());
    assert!(run_auto_ping_tick(&client, &cfg).await, "auto-ping tick must run when no manual ping is active/recent");
    assert_eq!(ping_hits.load(Ordering::SeqCst), 1);

    clear_proxies_cache_for_test();
    reset_manual_ping_state_for_test();
}

#[tokio::test]
async fn regression_switch_server_uses_whitelist_group_even_when_cfg_whitelist_disabled() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let wl_put_hits = std::sync::Arc::new(AtomicUsize::new(0));
    let wl_hits_srv = wl_put_hits.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let wl_hits = wl_hits_srv.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let (status, body) = if first_line.starts_with("GET /proxies") {
                    let json_body = serde_json::json!({
                        "proxies": {
                            "PROXY": { "type": "Selector", "now": "DIRECT", "all": ["DIRECT"] },
                            crate::routing::WHITELIST_GROUP_NAME: {
                                "type": "Selector",
                                "now": "DIRECT",
                                "all": ["DIRECT", "wl-node-1"]
                            },
                            "wl-node-1": { "type": "Vless", "server": "1.2.3.4" }
                        }
                    }).to_string();
                    ("200 OK", json_body)
                } else if first_line.starts_with("PUT /proxies/") && first_line.contains("Whitelist") {
                    wl_hits.fetch_add(1, Ordering::SeqCst);
                    ("204 No Content", String::new())
                } else if first_line.starts_with("DELETE /connections") {
                    ("204 No Content", String::new())
                } else {
                    ("400 Bad Request", "{\"message\":\"not found\"}".to_string())
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

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    cfg.whitelist.enabled = false;

    let client = reqwest::Client::new();
    let res = switch_server(&client, &cfg, "wl-node-1").await;
    assert!(res.is_ok(), "switch_server must succeed via Whitelist group even if cfg.whitelist.enabled=false: {res:?}");
    assert_eq!(wl_put_hits.load(Ordering::SeqCst), 1, "must switch 🛡️ Whitelist group");
    clear_proxies_cache_for_test();
}

#[tokio::test]
async fn regression_switch_server_preserves_whitelist_isolation_for_vps() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let wl_put_hits = std::sync::Arc::new(AtomicUsize::new(0));
    let proxy_put_hits = std::sync::Arc::new(AtomicUsize::new(0));
    let wl_srv = wl_put_hits.clone();
    let proxy_srv = proxy_put_hits.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let wl_hits = wl_srv.clone();
            let p_hits = proxy_srv.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]);
                let first_line = req.lines().next().unwrap_or("");
                let (status, body) = if first_line.starts_with("GET /proxies") {
                    let json_body = serde_json::json!({
                        "proxies": {
                            "PROXY": { "type": "Selector", "now": "DIRECT", "all": ["DIRECT", "vps-node-1"] },
                            crate::routing::WHITELIST_GROUP_NAME: {
                                "type": "Selector",
                                "now": "DIRECT",
                                "all": ["DIRECT", "vps-node-1"]
                            },
                            "vps-node-1": { "type": "Vless", "server": "5.6.7.8" }
                        }
                    }).to_string();
                    ("200 OK", json_body)
                } else if first_line.starts_with("PUT /proxies/") && first_line.contains("Whitelist") {
                    wl_hits.fetch_add(1, Ordering::SeqCst);
                    ("204 No Content", String::new())
                } else if first_line.starts_with("PUT /proxies/PROXY") {
                    p_hits.fetch_add(1, Ordering::SeqCst);
                    ("204 No Content", String::new())
                } else if first_line.starts_with("DELETE /connections") {
                    ("204 No Content", String::new())
                } else {
                    ("400 Bad Request", "{\"message\":\"not found\"}".to_string())
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

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    cfg.whitelist.enabled = false;

    let client = reqwest::Client::new();
    let res = switch_server(&client, &cfg, "vps-node-1").await;
    assert!(res.is_ok(), "switch_server must succeed via PROXY: {res:?}");
    assert_eq!(proxy_put_hits.load(Ordering::SeqCst), 1, "must switch PROXY group");
    assert_eq!(wl_put_hits.load(Ordering::SeqCst), 0, "AUDIT-004: must NOT touch 🛡️ Whitelist when cfg.whitelist.enabled=false for VPS");
    clear_proxies_cache_for_test();
}

#[tokio::test]
async fn regression_ping_all_url_resolves_virtual_free_sub_pools_and_selectors() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let bad_virtual_hits = Arc::new(AtomicUsize::new(0));
    let bad_hits_srv = bad_virtual_hits.clone();

    tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { return };
            let bad_hits_srv = bad_hits_srv.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req_str = String::from_utf8_lossy(&buf[..n]);
                let first_line = req_str.lines().next().unwrap_or("");
                let (status, body) = if first_line.starts_with("GET /proxies/free_wl/delay")
                    || first_line.starts_with("GET /proxies/free_vpn/delay")
                {
                    bad_hits_srv.fetch_add(1, Ordering::SeqCst);
                    ("404 Not Found", "{\"message\":\"not found\"}".to_string())
                } else if first_line.starts_with("GET /proxies ") {
                    let json_body = serde_json::json!({
                        "proxies": {
                            "PROXY": { "type": "Selector", "now": "wl-leaf-1", "all": ["wl-leaf-1"] },
                            crate::routing::WHITELIST_GROUP_NAME: {
                                "type": "Selector",
                                "now": "wl-leaf-1",
                                "all": ["wl-leaf-1"]
                            },
                            "wl-leaf-1": { "type": "Vless", "server": "1.1.1.1", "history": [] }
                        }
                    }).to_string();
                    ("200 OK", json_body)
                } else if first_line.starts_with("GET /providers/proxies/free_wl/wl-leaf-1/healthcheck") {
                    ("200 OK", "{\"delay\":145}".to_string())
                } else if first_line.starts_with("GET /providers/proxies/free_vpn/vpn-leaf-1/healthcheck") {
                    ("200 OK", "{\"delay\":210}".to_string())
                } else if first_line.starts_with("GET /providers/proxies ") {
                    let json_body = serde_json::json!({
                        "providers": {
                            "free_wl": {
                                "vehicleType": "HTTP",
                                "proxies": [{ "name": "wl-leaf-1", "alive": true, "history": [] }]
                            },
                            "free_vpn": {
                                "vehicleType": "HTTP",
                                "proxies": [{ "name": "vpn-leaf-1", "alive": true, "history": [] }]
                            }
                        }
                    }).to_string();
                    ("200 OK", json_body)
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

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    let client = reqwest::Client::new();

    let ids = vec![
        "free_wl".to_string(),
        "free_vpn".to_string(),
        "PROXY".to_string(),
        crate::routing::WHITELIST_GROUP_NAME.to_string(),
    ];
    let res = ping_all_url(&client, &cfg, &ids, 2500, None).await;
    assert_eq!(bad_virtual_hits.load(Ordering::SeqCst), 0, "virtual pool ids must not be pinged via /proxies/{{id}}/delay");
    assert_eq!(res.get("free_wl"), Some(&145));
    assert_eq!(res.get("free_vpn"), Some(&210));
    assert_eq!(res.get("PROXY"), Some(&145));
    assert_eq!(res.get(crate::routing::WHITELIST_GROUP_NAME), Some(&145));
    clear_proxies_cache_for_test();
}

#[tokio::test]
async fn regression_l7_stable_rejects_ru_relay_blocking_youtube() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
                let (status, body) = if first_line.contains("/ru-relay/healthcheck") {
                    if first_line.contains("youtube.com") {
                        ("504 Gateway Timeout", r#"{"message":"context deadline exceeded"}"#.to_string())
                    } else {
                        ("200 OK", r#"{"delay":120}"#.to_string())
                    }
                } else if first_line.contains("/eu-node/healthcheck") {
                    ("200 OK", r#"{"delay":90}"#.to_string())
                } else {
                    ("404 Not Found", r#"{"message":"not found"}"#.to_string())
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

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    let client = reqwest::Client::new();

    let ru_res = probe_service_provider_node_l7_stable(&client, &cfg, "__free_sub_probe", "ru-relay", 2000).await;
    assert!(ru_res.is_err(), "RU relay blocking youtube.com must be rejected by Stability Gate");
    let err_msg = ru_res.unwrap_err();
    assert!(err_msg.contains("youtube.com/generate_204"), "unexpected error: {err_msg}");

    let eu_res = probe_service_provider_node_l7_stable(&client, &cfg, "__free_sub_probe", "eu-node", 2000).await;
    assert_eq!(eu_res, Ok(90));
    srv.abort();
}

#[tokio::test]
async fn regression_l7_stable_verifies_payload_throughput_and_rejects_throttled_node() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
                let (status, body) = if first_line.contains("/throttled-node/healthcheck") {
                    if first_line.contains("speed.cloudflare.com") {
                        ("504 Gateway Timeout", r#"{"message":"context deadline exceeded"}"#.to_string())
                    } else {
                        ("200 OK", r#"{"delay":150}"#.to_string())
                    }
                } else if first_line.contains("/cf-403-node/healthcheck") {
                    if first_line.contains("speed.cloudflare.com") {
                        ("503 Service Unavailable", r#"{"message":"status code 403"}"#.to_string())
                    } else {
                        ("200 OK", r#"{"delay":110}"#.to_string())
                    }
                } else if first_line.contains("/fast-node/healthcheck") {
                    if first_line.contains("speed.cloudflare.com") {
                        assert!(first_line.contains("expected=200"), "line={first_line}");
                        assert!(first_line.contains("timeout=3000"), "line={first_line}");
                    }
                    ("200 OK", r#"{"delay":120}"#.to_string())
                } else {
                    ("404 Not Found", r#"{"message":"not found"}"#.to_string())
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

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    let client = reqwest::Client::new();

    let throttled = probe_service_provider_node_l7_stable(&client, &cfg, "__free_sub_probe", "throttled-node", 5000).await;
    assert!(throttled.is_err(), "Зашейпленный узел с таймаутом на 50 КБ должен быть отклонён");
    assert!(throttled.unwrap_err().contains("speed.cloudflare.com"));

    let cf_403 = probe_service_provider_node_l7_stable(&client, &cfg, "__free_sub_probe", "cf-403-node", 5000).await;
    assert_eq!(cf_403, Ok(110), "При HTTP 403 от speed.cloudflare.com должен сработать fallback на cdn-cgi/trace");

    let fast = probe_service_provider_node_l7_stable(&client, &cfg, "__free_sub_probe", "fast-node", 5000).await;
    assert_eq!(fast, Ok(120));
    srv.abort();
}

#[test]
fn regression_l7_stable_penalizes_slow_throughput_node_for_auto_upgrade() {
    // Перегруженный узел: низкий базовый пинг 100 мс, но 50 КБ скачиваются за 500 мс (~125 КБ/с)
    let slow_score = compute_l7_stable_score(100, 500, 120, true);
    // Быстрый узел: базовый пинг 120 мс, но 50 КБ скачиваются за 160 мс (~1.25 МБ/с)
    let fast_score = compute_l7_stable_score(120, 160, 140, true);
    assert!(
        slow_score >= 800 && fast_score <= 250,
        "Медленный узел должен получать ощутимый штраф за payload: slow={slow_score}, fast={fast_score}"
    );
    assert!(
        crate::free_sub::should_upgrade_active_node(slow_score, false, fast_score, false),
        "Фоновый добор должен автоматически переключить активный узел с медленного ({slow_score}) на быстрый ({fast_score})"
    );
}

#[tokio::test]
async fn regression_l7_scored_keeps_working_nodes_when_cloudflare_speed_times_out() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
                let (status, body) = if first_line.contains("/cf-timeout-node/healthcheck") {
                    if first_line.contains("speed.cloudflare.com") {
                        ("504 Gateway Timeout", r#"{"message":"context deadline exceeded"}"#.to_string())
                    } else {
                        ("200 OK", r#"{"delay":150}"#.to_string())
                    }
                } else if first_line.contains("/dead-tg-node/healthcheck") {
                    if first_line.contains("telegram.org") {
                        ("504 Gateway Timeout", r#"{"message":"context deadline exceeded"}"#.to_string())
                    } else {
                        ("200 OK", r#"{"delay":100}"#.to_string())
                    }
                } else if first_line.contains("/ru-relay/healthcheck") {
                    if first_line.contains("youtube.com") {
                        ("504 Gateway Timeout", r#"{"message":"context deadline exceeded"}"#.to_string())
                    } else {
                        ("200 OK", r#"{"delay":90}"#.to_string())
                    }
                } else if first_line.contains("/fast-node/healthcheck") {
                    ("200 OK", r#"{"delay":120}"#.to_string())
                } else {
                    ("404 Not Found", r#"{"message":"not found"}"#.to_string())
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

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    let client = reqwest::Client::new();

    let ru = probe_service_provider_node_l7_scored(&client, &cfg, "__free_sub_probe", "ru-relay", 3000).await;
    assert!(ru.is_err(), "RU-релей без доступа к youtube.com должен отклоняться");

    let cf_timeout = probe_service_provider_node_l7_scored(&client, &cfg, "__free_sub_probe", "cf-timeout-node", 3000)
        .await
        .expect("Рабочий узел при таймауте speed.cloudflare.com не должен выбивать пул в 0");
    let fast = probe_service_provider_node_l7_scored(&client, &cfg, "__free_sub_probe", "fast-node", 3000)
        .await
        .expect("Быстрый узел должен проходить");

    assert!(
        cf_timeout > fast + 1000,
        "Узел без speed.cloudflare.com ({cf_timeout}) должен получать штраф и проигрывать быстрому ({fast})"
    );

    let dead_tg = probe_service_provider_node_l7_scored(&client, &cfg, "__free_sub_probe", "dead-tg-node", 3000)
        .await
        .expect("Узел с мёртвым Telegram должен оцениваться со штрафом");
    assert!(
        dead_tg > fast + 3000 && dead_tg > cf_timeout,
        "Узел с мёртвым Telegram ({dead_tg}) должен проигрывать узлам с живым Telegram (fast={fast}, cf_timeout={cf_timeout})"
    );
    srv.abort();
}

#[tokio::test]
async fn regression_switch_server_resolves_virtual_free_sub_pool_id_to_leaf_node() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _guard = PROXIES_CACHE_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    clear_proxies_cache_for_test();
    let put_targets = std::sync::Arc::new(tokio::sync::Mutex::new(Vec::<String>::new()));
    let put_clone = put_targets.clone();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let srv = tokio::spawn(async move {
        loop {
            let Ok((mut socket, _)) = listener.accept().await else { break };
            let pt = put_clone.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let n = socket.read(&mut buf).await.unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let first_line = req.lines().next().unwrap_or("");
                let (status, body) = if first_line.starts_with("PUT /proxies/") {
                    let body_part = req.split("\r\n\r\n").nth(1).unwrap_or("");
                    if let Ok(v) = serde_json::from_str::<Value>(body_part) {
                        if let Some(name) = v.get("name").and_then(|x| x.as_str()) {
                            pt.lock().await.push(name.to_string());
                            if name == "free_wl" {
                                return;
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
                                    { "name": "🇷🇺 VK — #4", "type": "VLESS", "alive": true }
                                ]
                            }
                        }
                    }).to_string();
                    ("200 OK", json_body)
                } else if first_line.contains("GET /proxies") {
                    let json_body = serde_json::json!({
                        "proxies": {
                            "PROXY": { "name": "PROXY", "type": "Selector", "now": "DIRECT", "all": ["DIRECT", "🇷🇺 VK — #4"] },
                            "🛡️ Whitelist": { "name": "🛡️ Whitelist", "type": "Selector", "now": "DIRECT", "all": ["DIRECT", "🇷🇺 VK — #4"] },
                            "🇷🇺 VK — #4": { "name": "🇷🇺 VK — #4", "type": "VLESS", "alive": true }
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

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    cfg.whitelist.enabled = true;
    let client = reqwest::Client::new();

    let res = switch_server(&client, &cfg, "free_wl").await;
    srv.abort();
    assert!(res.is_ok(), "switch_server('free_wl') должен разрешиться в листовой узел: {res:?}");
    let targets = put_targets.lock().await;
    assert!(!targets.is_empty());
    assert!(targets.iter().all(|t| t == "🇷🇺 VK — #4"), "targets={targets:?}");
    clear_proxies_cache_for_test();
}


#[test]
fn regression_l7_primary_timeout_skips_double_fallback_but_http_403_uses_fallback() {
    assert!(!should_fallback_l7_primary_error("timeout waiting for response"));
    assert!(!should_fallback_l7_primary_error("connection reset by peer"));
    assert!(should_fallback_l7_primary_error("HTTP 403 Forbidden"));
}

#[tokio::test]
async fn regression_l7_metrics_separates_rtt_from_score_and_falls_back_on_youtube_http_403() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
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
                let (status, body) = if first_line.contains("/dead-tg/healthcheck") {
                    if first_line.contains("telegram.org") {
                        ("504 Gateway Timeout", r#"{"message":"context deadline exceeded"}"#.to_string())
                    } else {
                        ("200 OK", r#"{"delay":120}"#.to_string())
                    }
                } else if first_line.contains("/yt-403-node/healthcheck") {
                    if first_line.contains("youtube.com") {
                        ("403 Forbidden", r#"{"message":"HTTP 403 Forbidden"}"#.to_string())
                    } else if first_line.contains("discord.com") {
                        ("200 OK", r#"{"delay":140}"#.to_string())
                    } else {
                        ("200 OK", r#"{"delay":100}"#.to_string())
                    }
                } else if first_line.contains("/ru-relay/healthcheck") {
                    if first_line.contains("youtube.com") {
                        ("504 Gateway Timeout", r#"{"message":"context deadline exceeded"}"#.to_string())
                    } else {
                        ("200 OK", r#"{"delay":90}"#.to_string())
                    }
                } else {
                    ("404 Not Found", r#"{"message":"not found"}"#.to_string())
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

    let mut cfg = AppConfig::default();
    cfg.mihomo.host = "127.0.0.1".into();
    cfg.mihomo.port = port;
    let client = reqwest::Client::new();

    let dead_tg = probe_service_provider_node_l7_metrics(&client, &cfg, "__free_sub_probe", "dead-tg", 3000)
        .await
        .expect("узел с мёртвым Telegram должен возвращать метрики со штрафом");
    assert!(dead_tg.rtt_ms <= 200, "чистый RTT должен быть реальным (<= 200), получено {}", dead_tg.rtt_ms);
    assert!(dead_tg.quality_score > 4000, "quality_score должен содержать штраф +4000 за Telegram, получено {}", dead_tg.quality_score);

    let yt_403 = probe_service_provider_node_l7_metrics(&client, &cfg, "__free_sub_probe", "yt-403-node", 3000)
        .await
        .expect("при HTTP 403 от YouTube должен сработать fallback на discord.com");
    assert_eq!(yt_403.rtt_ms, 120);
    assert!(yt_403.quality_score >= 520, "fallback должен добавлять штраф +400 (score={})", yt_403.quality_score);

    let ru = probe_service_provider_node_l7_metrics(&client, &cfg, "__free_sub_probe", "ru-relay", 3000).await;
    assert!(ru.is_err(), "RU-релей с 504 от YouTube должен отклоняться без fallback на Discord");

    srv.abort();
}

/// Тестовая блокировка для изоляции тестов кэша.
#[cfg(test)]
pub static PROXIES_CACHE_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Тестовый хелпер для очистки кэша между изолированными тестами.
#[cfg(test)]
pub fn clear_proxies_cache_for_test() {
    invalidate_proxies_cache();
    *LAST_SELECTED_TUNNEL_SERVER.write().unwrap_or_else(|e| e.into_inner()) = None;
}

/// Тестовый хелпер для внедрения данных в кэш без сетевых запросов к Mihomo.
#[cfg(test)]
pub fn set_proxies_cache_for_test(data: BTreeMap<String, Value>, cached_at: std::time::Instant) {
    let mut guard = PROXIES_CACHE.write().unwrap_or_else(|e| e.into_inner());
    *guard = Some(ProxiesCacheEntry { cached_at, endpoint: None, data });
}

#[cfg(test)]
pub fn set_provider_proxies_cache_for_test(data: BTreeMap<String, Value>, cached_at: std::time::Instant) {
    let mut guard = PROVIDER_PROXIES_CACHE.write().unwrap_or_else(|e| e.into_inner());
    *guard = Some(ProxiesCacheEntry { cached_at, endpoint: None, data });
}


#[cfg(test)]
pub fn get_cached_provider_proxies_if_fresh() -> Option<BTreeMap<String, Value>> {
    let guard = PROVIDER_PROXIES_CACHE.read().unwrap_or_else(|e| e.into_inner());
    if let Some(ref entry) = *guard {
        if entry.cached_at.elapsed() < std::time::Duration::from_secs(PROXIES_CACHE_TTL_SECS) {
            return Some(entry.data.clone());
        }
    }
    None
}


#[cfg(test)]
pub fn reset_manual_ping_state_for_test() {
    MANUAL_PING_IN_FLIGHT.store(0, std::sync::atomic::Ordering::SeqCst);
    *LAST_MANUAL_PING_AT.write().unwrap_or_else(|e| e.into_inner()) = None;
}
