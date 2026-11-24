use std::time::{Duration, Instant};

use crate::config::MultiWanWeightMode;
use crate::multiwan::{
    CpuAutoScaleGuard,
    discovery::{
        WanKind, assemble_discovered_wans, assemble_discovered_wans_full, detect_gateway_collision,
        is_excluded_linux_iface,
    },
    extract_proxy_servers_from_yaml,
    format_wan_marks_spec,
    health::{
        ChannelHealthEntry, QUARANTINE_DURATION, WanChannelState, WeightedChannelInput,
        compute_effective_weights, parse_proc_net_dev,
    },
    routing::{
        ActiveWanRoute, CHAIN_PRE, LAN_DIRECT_MARK, MULTIWAN_MARK_MASK, PROXY_SERVERS_SET,
        build_iptables_mangle_commands, build_routing_plan, build_routing_plan_full,
        check_routing_integrity, mark_hex,
    },
};

#[test]
fn discovery_detects_dual_wisp_and_usb_modem_and_excludes_lan_and_tunnels() {
    let rci_ifaces = serde_json::json!({
        "Bridge0": { "id": "Bridge0", "type": "Bridge", "address": "192.168.1.1", "state": "up" },
        "WifiMaster0/WifiStation0": { "id": "WifiMaster0/WifiStation0", "type": "WifiStation", "description": "Modem-1", "ssid": "LTE-1", "address": "192.168.10.22", "state": "up", "link": "up" },
        "WifiMaster1/WifiStation0": { "id": "WifiMaster1/WifiStation0", "type": "WifiStation", "description": "Modem-2", "ssid": "LTE-2", "address": "192.168.2.52", "state": "up", "link": "up" },
        "UsbQmi0": { "id": "UsbQmi0", "type": "UsbQmi", "description": "Quectel 4G", "address": "10.42.0.7", "state": "up", "link": "up" },
        "Wireguard0": { "id": "Wireguard0", "type": "Wireguard", "address": "10.9.0.2", "state": "up" }
    });
    let ip_addr_out = "\
1: lo: <LOOPBACK,UP> mtu 65536
    inet 127.0.0.1/8 scope host lo
2: br0: <BROADCAST,MULTICAST,UP> mtu 1500
    inet 192.168.1.1/24 brd 192.168.1.255 scope global br0
3: apcli0: <BROADCAST,MULTICAST,UP> mtu 1500
    inet 192.168.10.22/24 brd 192.168.10.255 scope global apcli0
4: apcli1: <BROADCAST,MULTICAST,UP> mtu 1500
    inet 192.168.2.52/24 brd 192.168.2.255 scope global apcli1
5: wwan0: <POINTOPOINT,UP> mtu 1500
    inet 10.42.0.7/24 scope global wwan0
6: csq0: <POINTOPOINT,UP> mtu 1400
    inet 10.10.0.2/32 scope global csq0
7: tun0: <POINTOPOINT,UP> mtu 1500
    inet 198.18.0.1/16 scope global tun0
";
    let ip_route_out = "\
default via 192.168.10.1 dev apcli0 table 16390 proto static
default via 192.168.2.1 dev apcli1 table 16389 proto static
default via 10.42.0.1 dev wwan0 table 42 proto static
default via 10.10.0.1 dev csq0 table 222 proto static
";

    let (wans, lan_cidrs) = assemble_discovered_wans(
        Some(&rci_ifaces),
        None,
        ip_addr_out,
        ip_route_out,
    );

    assert_eq!(lan_cidrs, vec!["192.168.1.0/24".to_string()]);
    assert_eq!(wans.len(), 3);
    assert!(wans.iter().any(|w| w.linux_iface == "apcli0" && w.kind == WanKind::Wisp && w.gateway == "192.168.10.1"));
    assert!(wans.iter().any(|w| w.linux_iface == "apcli1" && w.kind == WanKind::Wisp && w.gateway == "192.168.2.1"));
    assert!(wans.iter().any(|w| w.linux_iface == "wwan0" && w.kind == WanKind::Modem && w.gateway == "10.42.0.1"));
    assert!(!wans.iter().any(|w| w.linux_iface == "br0" || w.linux_iface == "csq0" || w.linux_iface == "tun0" || w.linux_iface == "lo"));
    assert!(is_excluded_linux_iface("br0"));
    assert!(is_excluded_linux_iface("csq0"));
    assert!(is_excluded_linux_iface("nwg0"));
    assert!(!is_excluded_linux_iface("apcli0"));
    assert!(!is_excluded_linux_iface("cdc_br0"));
}

#[test]
fn health_hysteresis_quarantine_and_standby_promotion() {
    let mut entry = ChannelHealthEntry::default();
    let t0 = Instant::now();

    // 1 fail does not drop Online
    entry.record_probe_outcome(false, None, 100, t0);
    assert_eq!(entry.state, WanChannelState::Online);

    // 2 consecutive fails -> Offline
    entry.record_probe_outcome(false, None, 100, t0 + Duration::from_secs(4));
    assert_eq!(entry.state, WanChannelState::Offline);

    // 2 consecutive successes -> Online
    entry.record_probe_outcome(true, Some(30), 0, t0 + Duration::from_secs(8));
    assert_eq!(entry.state, WanChannelState::Offline);
    entry.record_probe_outcome(true, Some(30), 0, t0 + Duration::from_secs(12));
    assert_eq!(entry.state, WanChannelState::Online);

    // 2 more drops within 60s -> 3 total drops -> Quarantine + flapping
    entry.record_probe_outcome(false, None, 100, t0 + Duration::from_secs(16));
    entry.record_probe_outcome(false, None, 100, t0 + Duration::from_secs(20));
    entry.record_probe_outcome(true, Some(25), 0, t0 + Duration::from_secs(22));
    entry.record_probe_outcome(true, Some(25), 0, t0 + Duration::from_secs(24));
    entry.record_probe_outcome(false, None, 100, t0 + Duration::from_secs(28));
    entry.record_probe_outcome(false, None, 100, t0 + Duration::from_secs(32));
    assert_eq!(entry.state, WanChannelState::Quarantine);
    assert!(entry.flapping);

    // After quarantine duration, recovers through Offline -> 2 ok -> Online
    let after_q = t0 + Duration::from_secs(32) + QUARANTINE_DURATION + Duration::from_secs(1);
    entry.record_probe_outcome(true, Some(20), 0, after_q);
    assert_eq!(entry.state, WanChannelState::Offline);
    entry.record_probe_outcome(true, Some(20), 0, after_q + Duration::from_secs(4));
    assert_eq!(entry.state, WanChannelState::Online);
}

#[test]
fn manual_standby_promotes_when_primary_offline() {
    let (wans, _) = assemble_discovered_wans(
        None,
        None,
        "3: apcli0: <UP>\n    inet 192.168.10.22/24 brd 192.168.10.255 scope global apcli0\n4: apcli1: <UP>\n    inet 192.168.2.52/24 brd 192.168.2.255 scope global apcli1\n",
        "default via 192.168.10.1 dev apcli0 table 16390\ndefault via 192.168.2.1 dev apcli1 table 16389\n",
    );
    assert_eq!(wans.len(), 2);

    let mut h_off = ChannelHealthEntry::default().snapshot();
    h_off.state = WanChannelState::Offline;
    let h_on = ChannelHealthEntry::default().snapshot();

    // Primary offline, Standby (manual_weight=0) online -> Standby promoted to 100%
    let inputs = vec![
        WeightedChannelInput { wan: &wans[0], health: &h_off, enabled: true, manual_weight: 100 },
        WeightedChannelInput { wan: &wans[1], health: &h_on, enabled: true, manual_weight: 0 },
    ];
    let weights = compute_effective_weights(&inputs, MultiWanWeightMode::Manual);
    assert_eq!(weights[0].effective_weight, 0);
    assert_eq!(weights[1].effective_weight, 100);
    assert_eq!(weights[1].display_state, WanChannelState::Online);
}

#[test]
fn routing_plan_uses_top_nibble_mask_no_connmark_in_prerouting_and_no_blanket_192_168_skip() {
    let (wans, lan_cidrs) = assemble_discovered_wans(
        None,
        None,
        "2: br0: <UP>\n    inet 192.168.1.1/24 brd 192.168.1.255 scope global br0\n3: apcli0: <UP>\n    inet 192.168.10.22/24 scope global apcli0\n4: apcli1: <UP>\n    inet 192.168.2.52/24 scope global apcli1\n",
        "default via 192.168.10.1 dev apcli0 table 16390\ndefault via 192.168.2.1 dev apcli1 table 16389\n",
    );
    let routes = vec![
        ActiveWanRoute { wan: wans[0].clone(), weight: 60 },
        ActiveWanRoute { wan: wans[1].clone(), weight: 40 },
    ];
    let spec = format_wan_marks_spec(&routes);
    assert!(spec.contains("0x10000000:60@192.168.10.22"));
    assert!(spec.contains("0x20000000:40@192.168.2.52"));

    let plan = build_routing_plan(routes, lan_cidrs, vec!["1.2.3.4".into()], true);
    let cmds = build_iptables_mangle_commands(&plan);
    let joined: Vec<String> = cmds.iter().map(|c| c.join(" ")).collect();
    let full = joined.join("\n");

    let mask = mark_hex(MULTIWAN_MARK_MASK);
    assert!(full.contains(&format!("--nfmask {mask} --ctmask {mask}")));
    assert!(full.contains("-d 192.168.1.0/24 -j RETURN"));
    assert!(!full.contains("192.168.0.0/16"), "Запрещено пропускать всю 192.168.0.0/16 из-за WISP-шлюзов");

    // XKEEN_MULTIWAN_PRE ни в коем случае не должен трогать CONNMARK (конфликт с _NDM_HOTSPOT_PREROUTING_MANGL 0xffffaaa)
    for line in &joined {
        if line.contains(CHAIN_PRE) {
            assert!(!line.contains("CONNMARK"), "PREROUTING не должен менять CONNMARK: {line}");
        }
    }

    // parse_proc_net_dev sanity
    let parsed = parse_proc_net_dev("Inter-|\n face |\n apcli0: 1000 1 0 0 0 0 0 0 2000 1\n");
    assert_eq!(parsed["apcli0"].rx_bytes, 1000);
    assert_eq!(parsed["apcli0"].tx_bytes, 2000);
}

#[test]
fn ndm_reset_integrity_detects_missing_jump_rules_and_routes() {
    let (wans, lan_cidrs) = assemble_discovered_wans(
        None,
        None,
        "3: apcli0: <UP>\n    inet 192.168.10.22/24 scope global apcli0\n4: apcli1: <UP>\n    inet 192.168.2.52/24 scope global apcli1\n",
        "default via 192.168.10.1 dev apcli0 table 16390\ndefault via 192.168.2.1 dev apcli1 table 16389\n",
    );
    let routes = vec![
        ActiveWanRoute { wan: wans[0].clone(), weight: 50 },
        ActiveWanRoute { wan: wans[1].clone(), weight: 50 },
    ];
    let plan = build_routing_plan(routes, lan_cidrs, Vec::new(), true);

    let ok_rules = "\
50: from 192.168.10.22 lookup 180
51: from 192.168.2.52 lookup 181
60: from all fwmark 0x10000000/0xf0000000 lookup 180
61: from all fwmark 0x20000000/0xf0000000 lookup 181
68: from all fwmark 0x90000000/0xf0000000 lookup 189
";
    let ok_routes = "\
default via 192.168.10.1 dev apcli0 table 180
default via 192.168.2.1 dev apcli1 table 181
default table 189
";

    let intact = check_routing_integrity(&plan, true, true, true, true, ok_rules, ok_routes);
    assert!(intact.is_intact());

    let missing_jump = check_routing_integrity(&plan, false, true, true, true, ok_rules, ok_routes);
    assert!(!missing_jump.is_intact());
    assert!(!missing_jump.out_jump_ok);

    let missing_rule = check_routing_integrity(
        &plan,
        true,
        true,
        true,
        true,
        "60: from all fwmark 0x10000000/0xf0000000 lookup 180\n68: from all fwmark 0x90000000/0xf0000000 lookup 189\n",
        ok_routes,
    );
    assert!(!missing_rule.is_intact());
    assert_eq!(missing_rule.missing_wan_slots, vec![1]);

    let missing_ecmp = check_routing_integrity(
        &plan,
        true,
        true,
        true,
        true,
        ok_rules,
        "default via 192.168.10.1 dev apcli0 table 180\ndefault via 192.168.2.1 dev apcli1 table 181\n",
    );
    assert!(!missing_ecmp.is_intact());
    assert!(missing_ecmp.missing_ecmp);
}

#[test]
fn proxy_ipset_balances_proxies_while_direct_uses_sticky_ecmp_and_protects_mgmt_ports() {
    let (wans, lan_cidrs) = assemble_discovered_wans(
        None,
        None,
        "3: apcli0: <UP>\n    inet 192.168.10.22/24 scope global apcli0\n4: apcli1: <UP>\n    inet 192.168.2.52/24 scope global apcli1\n",
        "default via 192.168.10.1 dev apcli0 table 16390\ndefault via 192.168.2.1 dev apcli1 table 16389\n",
    );
    let routes = vec![
        ActiveWanRoute { wan: wans[0].clone(), weight: 60 },
        ActiveWanRoute { wan: wans[1].clone(), weight: 40 },
    ];
    let plan = build_routing_plan_full(
        routes,
        lan_cidrs,
        vec!["45.12.34.56".into()],
        vec!["45.12.34.56".into(), "89.10.20.30".into()],
        true,
        true,
        vec![22, 80, 222, 1000, 1001],
    );
    let cmds = build_iptables_mangle_commands(&plan);
    let full = cmds.iter().map(|c| c.join(" ")).collect::<Vec<_>>().join("\n");

    // 1. statistic --mode random вешается ТОЛЬКО с -m set --match-set xkeen_mw_proxies dst
    let stat_lines: Vec<&str> = full.lines().filter(|l| l.contains("--mode random")).collect();
    assert!(!stat_lines.is_empty());
    for l in stat_lines {
        assert!(l.contains(&format!("--match-set {PROXY_SERVERS_SET} dst")), "{l}");
    }

    // 2. Остальной неразмеченный прямой трафик из OUTPUT уходит в ECMP 189 (0x90000000)
    assert!(full.contains(&mark_hex(LAN_DIRECT_MARK)));

    // 3. Порты управления 22, 80, 222, 1000, 1001 и DSCP 0x3e + метки 222/0x111/0x115 защищены
    for port in [22, 80, 222, 1000, 1001] {
        assert!(full.contains(&format!("--sport {port} -j RETURN")));
        assert!(full.contains(&format!("--dport {port} -j RETURN")));
    }
    assert!(full.contains("--dscp 0x3e -j RETURN"));
    assert!(full.contains("--mark 222 -j RETURN"));
    assert!(full.contains("--mark 0x111/0x111 -j RETURN"));
    assert!(full.contains("--mark 0x115/0x115 -j RETURN"));
}

#[test]
fn extract_proxy_servers_handles_flow_and_block_yaml() {
    let yaml = r#"
mixed-port: 7892
proxies:
  - { name: "DE-1", type: vless, server: 95.164.10.20, port: 443 }
  - name: "NL-Paid"
    type: trojan
    server: "nl-edge.example.org"
    port: 443
  - name: "Local"
    type: socks5
    server: 192.168.1.10
    port: 1080
proxy-groups:
  - name: PROXY
    type: select
"#;
    let servers = extract_proxy_servers_from_yaml(yaml);
    assert_eq!(servers, vec!["95.164.10.20".to_string(), "nl-edge.example.org".to_string()]);
}

#[test]
fn hilink_dual_192_168_8_1_collision_preserves_both_wans_and_sets_warning() {
    let rci_ifaces = serde_json::json!({
        "UsbLte0": { "id": "UsbLte0", "type": "UsbLte", "description": "Huawei-1", "address": "192.168.8.100", "system-name": "eth2", "state": "up", "link": "up" },
        "UsbLte1": { "id": "UsbLte1", "type": "UsbLte", "description": "Huawei-2", "address": "192.168.8.100", "system-name": "eth3", "state": "up", "link": "up" }
    });
    let rci_policy = serde_json::json!([
        { "id": "Policy0", "table": 16401, "interface": [{ "id": "UsbLte0" }] },
        { "id": "Policy1", "table": 16402, "interface": [{ "id": "UsbLte1" }] }
    ]);
    let ip_addr_out = "\
3: eth2: <UP>\n    inet 192.168.8.100/24 scope global eth2
4: eth3: <UP>\n    inet 192.168.8.100/24 scope global eth3
";
    let ip_route_out = "\
default via 192.168.8.1 dev eth2 table 16401
default via 192.168.8.1 dev eth3 table 16402
";
    let proc_net_dev = "Inter-|\n face |\n  eth2: 10 0 0 0 0 0 0 0 10 0\n  eth3: 20 0 0 0 0 0 0 0 20 0\n";

    let (wans, _) = assemble_discovered_wans_full(
        Some(&rci_ifaces),
        None,
        Some(&rci_policy),
        Some(proc_net_dev),
        ip_addr_out,
        ip_route_out,
    );

    assert_eq!(wans.len(), 2, "Оба HiLink-модема с 192.168.8.100 должны сохраниться");
    assert_eq!(wans[0].linux_iface, "eth2");
    assert_eq!(wans[1].linux_iface, "eth3");
    assert_eq!(wans[0].keenetic_table, Some(16401));
    assert_eq!(wans[1].keenetic_table, Some(16402));

    let warn = detect_gateway_collision(&wans).expect("Должно быть предупреждение о коллизии");
    assert!(warn.contains("192.168.8.1"));
    assert!(warn.contains("192.168.8.0/24"));
}

#[test]
fn cpu_autoscale_guard_scales_to_50_60_and_steps_down_only_after_30s_above_90pct() {
    let mut guard = CpuAutoScaleGuard::default();
    let t0 = Instant::now();

    // 2 активных WAN при CPU 40% -> потолок 50
    assert_eq!(guard.evaluate(2, 40.0, 30, t0), 50);
    // 3 активных WAN при CPU 40% -> плавный шаг до 58, затем 60
    assert_eq!(guard.evaluate(3, 40.0, 30, t0 + Duration::from_secs(12)), 58);
    assert_eq!(guard.evaluate(3, 40.0, 30, t0 + Duration::from_secs(24)), 60);

    // Кратковременный пик CPU 95% (0..25с) НЕ сбрасывает потоки
    assert_eq!(guard.evaluate(3, 95.0, 30, t0 + Duration::from_secs(30)), 60);
    assert_eq!(guard.evaluate(3, 95.0, 30, t0 + Duration::from_secs(55)), 60);

    // После >30с непрерывного CPU > 90% -> плавно снижает по -8
    assert_eq!(guard.evaluate(3, 95.0, 30, t0 + Duration::from_secs(61)), 52);
    // Слишком рано (через 4с) не снижает ещё раз
    assert_eq!(guard.evaluate(3, 95.0, 30, t0 + Duration::from_secs(65)), 52);
    // Через 12с снижает до 44
    assert_eq!(guard.evaluate(3, 95.0, 30, t0 + Duration::from_secs(73)), 44);

    // В полосе гистерезиса 85..=90% удерживает 44
    assert_eq!(guard.evaluate(3, 87.0, 30, t0 + Duration::from_secs(85)), 44);
    // При снижении CPU < 85% плавно восстанавливает +8
    assert_eq!(guard.evaluate(3, 70.0, 30, t0 + Duration::from_secs(86)), 52);
}

#[test]
fn ndm_hook_script_includes_csrf_origin_header() {
    let script = crate::multiwan::routing::format_ndm_hook_script(1001);
    assert!(script.contains("Origin: http://127.0.0.1:1001"));
    assert!(script.contains("http://127.0.0.1:1001/api/multiwan/reconcile"));
}

#[test]
fn ip_rule_priorities_place_fwmark_and_ecmp_above_source_routing() {
    use crate::multiwan::routing::{ECMP_RULE_PREF, FWMARK_RULE_PREF_BASE, SRC_RULE_PREF_BASE};
    assert!(FWMARK_RULE_PREF_BASE + 7 < ECMP_RULE_PREF);
    assert!(ECMP_RULE_PREF < SRC_RULE_PREF_BASE);
}
