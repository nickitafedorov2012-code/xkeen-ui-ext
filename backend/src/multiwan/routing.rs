#[cfg(unix)]
use std::collections::HashSet;

#[cfg(unix)]
use crate::multiwan::discovery::{BASE_ROUTE_TABLE, MAX_WAN_SLOTS};
use crate::multiwan::discovery::DiscoveredWan;

pub const MULTIWAN_MARK_MASK: u32 = 0xf000_0000;
pub const LAN_DIRECT_MARK: u32 = 0x9000_0000;
pub const ECMP_ROUTE_TABLE: u32 = 189;
pub const FWMARK_RULE_PREF_BASE: u32 = 60;
pub const ECMP_RULE_PREF: u32 = 68;
pub const SRC_RULE_PREF_BASE: u32 = 70;
pub const SINGLE_IP_SET: &str = "xkeen_mw_single_ip";
pub const PROXY_SERVERS_SET: &str = "xkeen_mw_proxies";
pub const DEFAULT_MANAGEMENT_PORTS: &[u16] = &[22, 80, 222, 1000, 1001];
pub const MULTIWAN_NAT_COMMENT: &str = "xkeen-route-multiwan";
pub const CHAIN_OUT: &str = "XKEEN_MULTIWAN_OUT";
pub const CHAIN_PRE: &str = "XKEEN_MULTIWAN_PRE";
pub const NDM_NETFILTER_HOOK: &str = "/opt/etc/ndm/netfilter.d/070-xkeen-multiwan.sh";
pub const NDM_IFSTATE_HOOK: &str = "/opt/etc/ndm/ifstatechanged.d/070-xkeen-multiwan.sh";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWanRoute {
    pub wan: DiscoveredWan,
    pub weight: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingPlan {
    pub active_wans: Vec<ActiveWanRoute>,
    pub lan_cidrs: Vec<String>,
    pub single_ip_targets: Vec<String>,
    pub proxy_server_targets: Vec<String>,
    pub use_statistic: bool,
    pub use_ipset: bool,
    pub management_ports: Vec<u16>,
}

pub fn mark_hex(mark: u32) -> String {
    format!("0x{mark:08x}")
}

pub fn mark_masked(mark: u32) -> String {
    format!("0x{mark:08x}/0x{MULTIWAN_MARK_MASK:08x}")
}

/// Строит детерминированный план маршрутизации (совместимая 4-аргументная обёртка).
pub fn build_routing_plan(
    active_wans: Vec<ActiveWanRoute>,
    lan_cidrs: Vec<String>,
    single_ip_targets: Vec<String>,
    use_statistic: bool,
) -> RoutingPlan {
    build_routing_plan_full(
        active_wans,
        lan_cidrs,
        single_ip_targets,
        Vec::new(),
        use_statistic,
        true,
        DEFAULT_MANAGEMENT_PORTS.to_vec(),
    )
}

/// Строит полный детерминированный план маршрутизации с прокси-ipset и портами управления.
pub fn build_routing_plan_full(
    active_wans: Vec<ActiveWanRoute>,
    mut lan_cidrs: Vec<String>,
    mut single_ip_targets: Vec<String>,
    mut proxy_server_targets: Vec<String>,
    use_statistic: bool,
    use_ipset: bool,
    mut management_ports: Vec<u16>,
) -> RoutingPlan {
    let mut filtered: Vec<ActiveWanRoute> = active_wans
        .into_iter()
        .filter(|w| w.weight > 0 && !w.wan.gateway.is_empty() && !w.wan.ipv4.is_empty())
        .collect();
    filtered.sort_by_key(|w| w.wan.slot);
    if lan_cidrs.is_empty() {
        lan_cidrs.push("192.168.1.0/24".to_string());
    }
    single_ip_targets.sort();
    single_ip_targets.dedup();
    proxy_server_targets.sort();
    proxy_server_targets.dedup();
    for &p in DEFAULT_MANAGEMENT_PORTS {
        if !management_ports.contains(&p) {
            management_ports.push(p);
        }
    }
    management_ports.sort_unstable();
    management_ports.dedup();
    RoutingPlan {
        active_wans: filtered,
        lan_cidrs,
        single_ip_targets,
        proxy_server_targets,
        use_statistic,
        use_ipset,
        management_ports,
    }
}

pub fn build_iptables_mangle_commands(plan: &RoutingPlan) -> Vec<Vec<String>> {
    let mut cmds: Vec<Vec<String>> = Vec::new();
    let mask_hex = mark_hex(MULTIWAN_MARK_MASK);
    let zero_masked = format!("0x0/{mask_hex}");

    // Сброс и создание цепочек
    cmds.push(vec!["-t".into(), "mangle".into(), "-N".into(), CHAIN_OUT.into()]);
    cmds.push(vec!["-t".into(), "mangle".into(), "-F".into(), CHAIN_OUT.into()]);
    cmds.push(vec!["-t".into(), "mangle".into(), "-N".into(), CHAIN_PRE.into()]);
    cmds.push(vec!["-t".into(), "mangle".into(), "-F".into(), CHAIN_PRE.into()]);

    if plan.active_wans.is_empty() {
        return cmds;
    }

    // --- XKEEN_MULTIWAN_OUT (mangle OUTPUT) ---
    // 1. Сохраняем явный SO_MARK от csqtt-client / canary-проб (верхний ниббл != 0)
    cmds.push(vec![
        "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
        "-m".into(), "mark".into(), "!".into(), "--mark".into(), zero_masked.clone(),
        "-j".into(), "RETURN".into(),
    ]);
    // 2. Не трогаем служебные метки csq0 (222), TPROXY/Mihomo (0x111, 0x115) и DSCP 0x3e
    cmds.push(vec![
        "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
        "-m".into(), "mark".into(), "--mark".into(), "222".into(),
        "-j".into(), "RETURN".into(),
    ]);
    for sys_mark in ["0x111/0x111", "0x115/0x115"] {
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
            "-m".into(), "mark".into(), "--mark".into(), sys_mark.into(),
            "-j".into(), "RETURN".into(),
        ]);
    }
    cmds.push(vec![
        "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
        "-m".into(), "dscp".into(), "--dscp".into(), "0x3e".into(),
        "-j".into(), "RETURN".into(),
    ]);
    for dev in ["lo", "csq0"] {
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
            "-o".into(), dev.into(), "-j".into(), "RETURN".into(),
        ]);
    }
    // 3. Неприкосновенность портов управления роутером и веб-панели (SSH/HTTP/XKeen)
    for &port in &plan.management_ports {
        let p_str = port.to_string();
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
            "-p".into(), "tcp".into(), "--sport".into(), p_str.clone(),
            "-j".into(), "RETURN".into(),
        ]);
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
            "-p".into(), "tcp".into(), "--dport".into(), p_str,
            "-j".into(), "RETURN".into(),
        ]);
    }
    // 4. Пропускаем только реальные локальные/мультикаст подсети (БЕЗ 192.168.0.0/16!)
    for cidr in ["127.0.0.0/8", "169.254.0.0/16", "224.0.0.0/4", "255.255.255.255/32"] {
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
            "-d".into(), cidr.into(), "-j".into(), "RETURN".into(),
        ]);
    }
    for cidr in &plan.lan_cidrs {
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
            "-d".into(), cidr.clone(), "-j".into(), "RETURN".into(),
        ]);
    }
    // 5. Восстанавливаем только верхний ниббл (0xf0000000) из conntrack
    cmds.push(vec![
        "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
        "-j".into(), "CONNMARK".into(), "--restore-mark".into(),
        "--nfmask".into(), mask_hex.clone(), "--ctmask".into(), mask_hex.clone(),
    ]);
    cmds.push(vec![
        "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
        "-m".into(), "mark".into(), "!".into(), "--mark".into(), zero_masked.clone(),
        "-j".into(), "RETURN".into(),
    ]);

    let primary = plan
        .active_wans
        .iter()
        .max_by_key(|w| w.weight)
        .unwrap_or(&plan.active_wans[0]);

    // 6. Защита от бана за 2 IP на платных подписках: пиним dst IP из ipset к primary WAN
    if !plan.single_ip_targets.is_empty() {
        if plan.use_ipset {
            cmds.push(vec![
                "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
                "-m".into(), "set".into(), "--match-set".into(), SINGLE_IP_SET.into(), "dst".into(),
                "-j".into(), "MARK".into(), "--set-xmark".into(), mark_masked(primary.wan.fwmark),
            ]);
        } else {
            for ip in plan.single_ip_targets.iter().take(32) {
                cmds.push(vec![
                    "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
                    "-d".into(), ip.clone(),
                    "-j".into(), "MARK".into(), "--set-xmark".into(), mark_masked(primary.wan.fwmark),
                ]);
            }
        }
    }

    // 7. Балансировка новых прокси-соединений (xkeen_mw_proxies) по весам;
    //    прямой трафик (Mihomo DIRECT, Zapret) уходит ниже в sticky ECMP-таблицу 189.
    if plan.active_wans.len() == 1 {
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
            "-m".into(), "mark".into(), "--mark".into(), zero_masked.clone(),
            "-j".into(), "MARK".into(), "--set-xmark".into(), mark_masked(plan.active_wans[0].wan.fwmark),
        ]);
    } else {
        if plan.use_statistic && plan.use_ipset && !plan.proxy_server_targets.is_empty() {
            let mut remaining: u32 = plan.active_wans.iter().map(|w| u32::from(w.weight)).sum();
            for (idx, item) in plan.active_wans.iter().enumerate() {
                if idx + 1 == plan.active_wans.len() || remaining <= u32::from(item.weight) {
                    cmds.push(vec![
                        "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
                        "-m".into(), "mark".into(), "--mark".into(), zero_masked.clone(),
                        "-m".into(), "set".into(), "--match-set".into(), PROXY_SERVERS_SET.into(), "dst".into(),
                        "-j".into(), "MARK".into(), "--set-xmark".into(), mark_masked(item.wan.fwmark),
                    ]);
                    break;
                }
                let prob = (f64::from(item.weight) / f64::from(remaining)).clamp(0.01, 0.99);
                remaining = remaining.saturating_sub(u32::from(item.weight));
                cmds.push(vec![
                    "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
                    "-m".into(), "mark".into(), "--mark".into(), zero_masked.clone(),
                    "-m".into(), "set".into(), "--match-set".into(), PROXY_SERVERS_SET.into(), "dst".into(),
                    "-m".into(), "statistic".into(), "--mode".into(), "random".into(),
                    "--probability".into(), format!("{prob:.4}"),
                    "-j".into(), "MARK".into(), "--set-xmark".into(), mark_masked(item.wan.fwmark),
                ]);
            }
        }
        // Весь неразмеченный прямой трафик из OUTPUT (Mihomo DIRECT, Zapret) -> L3-sticky ECMP таблица 189
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
            "-m".into(), "mark".into(), "--mark".into(), zero_masked.clone(),
            "-j".into(), "MARK".into(), "--set-xmark".into(), mark_masked(LAN_DIRECT_MARK),
        ]);
    }

    // 8. Сохраняем только верхний ниббл в conntrack
    cmds.push(vec![
        "-t".into(), "mangle".into(), "-A".into(), CHAIN_OUT.into(),
        "-j".into(), "CONNMARK".into(), "--save-mark".into(),
        "--nfmask".into(), mask_hex.clone(), "--ctmask".into(), mask_hex,
    ]);

    // --- XKEEN_MULTIWAN_PRE (mangle PREROUTING — ТОЛЬКО packet MARK, БЕЗ CONNMARK!) ---
    cmds.push(vec![
        "-t".into(), "mangle".into(), "-A".into(), CHAIN_PRE.into(),
        "!".into(), "-i".into(), "br+".into(), "-j".into(), "RETURN".into(),
    ]);
    for cidr in ["127.0.0.0/8", "169.254.0.0/16", "224.0.0.0/4", "255.255.255.255/32"] {
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_PRE.into(),
            "-d".into(), cidr.into(), "-j".into(), "RETURN".into(),
        ]);
    }
    for cidr in &plan.lan_cidrs {
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_PRE.into(),
            "-d".into(), cidr.clone(), "-j".into(), "RETURN".into(),
        ]);
    }
    for sys_mark in ["222", "0x111/0x111", "0x115/0x115"] {
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_PRE.into(),
            "-m".into(), "mark".into(), "--mark".into(), sys_mark.into(),
            "-j".into(), "RETURN".into(),
        ]);
    }
    cmds.push(vec![
        "-t".into(), "mangle".into(), "-A".into(), CHAIN_PRE.into(),
        "-m".into(), "dscp".into(), "--dscp".into(), "0x3e".into(),
        "-j".into(), "RETURN".into(),
    ]);
    for &port in &plan.management_ports {
        let p_str = port.to_string();
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_PRE.into(),
            "-p".into(), "tcp".into(), "--sport".into(), p_str.clone(),
            "-j".into(), "RETURN".into(),
        ]);
        cmds.push(vec![
            "-t".into(), "mangle".into(), "-A".into(), CHAIN_PRE.into(),
            "-p".into(), "tcp".into(), "--dport".into(), p_str,
            "-j".into(), "RETURN".into(),
        ]);
    }
    cmds.push(vec![
        "-t".into(), "mangle".into(), "-A".into(), CHAIN_PRE.into(),
        "-m".into(), "mark".into(), "--mark".into(), zero_masked,
        "-j".into(), "MARK".into(), "--set-xmark".into(), mark_masked(LAN_DIRECT_MARK),
    ]);

    cmds
}

#[cfg(unix)]
async fn run_cmd(cmd: &str, args: &[&str]) -> bool {
    tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(unix)]
async fn run_cmd_str(cmd: &str, args: &[String]) -> bool {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    run_cmd(cmd, &refs).await
}

#[cfg(unix)]
pub async fn probe_xt_statistic() -> bool {
    // Проверяем поддержку xt_statistic в ядре без побочных эффектов
    let _ = crate::system::run_iptables(&["-t", "mangle", "-N", "XKEEN_MW_PROBE"]);
    let ok = crate::system::run_iptables_ok(&[
        "-t", "mangle", "-A", "XKEEN_MW_PROBE",
        "-m", "statistic", "--mode", "random", "--probability", "0.5",
        "-j", "RETURN",
    ]);
    let _ = crate::system::run_iptables(&["-t", "mangle", "-F", "XKEEN_MW_PROBE"]);
    let _ = crate::system::run_iptables(&["-t", "mangle", "-X", "XKEEN_MW_PROBE"]);
    ok
}

#[cfg(not(unix))]
pub async fn probe_xt_statistic() -> bool {
    true
}

#[cfg(unix)]
pub async fn probe_ipset_available() -> bool {
    run_cmd("ipset", &["list", "-n"]).await
}

#[cfg(not(unix))]
pub async fn probe_ipset_available() -> bool {
    true
}

#[cfg(unix)]
async fn ensure_jump_rule(chain: &str, target: &str) {
    let check = ["-t", "mangle", "-C", chain, "-j", target];
    if !crate::system::run_iptables_ok(&check) {
        let _ = crate::system::run_iptables(&["-t", "mangle", "-A", chain, "-j", target]);
    }
}

#[cfg(unix)]
async fn remove_jump_rule(chain: &str, target: &str) {
    for _ in 0..8 {
        if !crate::system::run_iptables_ok(&["-t", "mangle", "-D", chain, "-j", target]) {
            break;
        }
    }
}

#[cfg(unix)]
async fn sync_ipset(set_name: &str, ips: &[String]) {
    if ips.is_empty() {
        let _ = run_cmd("ipset", &["destroy", set_name]).await;
        return;
    }
    let _ = run_cmd("ipset", &["create", set_name, "hash:ip", "-exist"]).await;
    let _ = run_cmd("ipset", &["flush", set_name]).await;
    let mut seen = HashSet::new();
    for ip in ips {
        if seen.insert(ip) {
            let _ = run_cmd("ipset", &["add", set_name, ip, "-exist"]).await;
        }
    }
}

#[cfg(unix)]
async fn write_ndm_hooks(port: u16) {
    let script = format_ndm_hook_script(port);
    for path in [NDM_NETFILTER_HOOK, NDM_IFSTATE_HOOK] {
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = tokio::fs::create_dir_all(parent).await;
        }
        if tokio::fs::write(path, &script).await.is_ok() {
            use std::os::unix::fs::PermissionsExt;
            let _ = tokio::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).await;
        }
    }
}

pub(crate) fn format_ndm_hook_script(port: u16) -> String {
    format!(
        "#!/bin/sh\ncurl -fsS -X POST -H 'Origin: http://127.0.0.1:{port}' http://127.0.0.1:{port}/api/multiwan/reconcile >/dev/null 2>&1 || true\n"
    )
}

#[cfg(unix)]
async fn remove_ndm_hooks() {
    for path in [NDM_NETFILTER_HOOK, NDM_IFSTATE_HOOK] {
        let _ = tokio::fs::remove_file(path).await;
    }
}

#[cfg(unix)]
pub async fn flush_conntrack_for_dropped_wan(wan_ip: &str, fwmark: u32) {
    let conntrack_bin = if std::path::Path::new("/opt/sbin/conntrack").exists() {
        "/opt/sbin/conntrack"
    } else {
        "conntrack"
    };
    let _ = run_cmd(conntrack_bin, &["-D", "--reply-dst", wan_ip]).await;
    let mark_arg = mark_masked(fwmark);
    let _ = run_cmd(conntrack_bin, &["-D", "-m", &mark_arg]).await;
}

#[cfg(not(unix))]
pub async fn flush_conntrack_for_dropped_wan(_wan_ip: &str, _fwmark: u32) {}

#[cfg(unix)]
async fn apply_wan_table_and_rules(item: &ActiveWanRoute) -> Result<(), String> {
    let w = &item.wan;
    let sysctl_key = format!("net.ipv4.conf.{}.rp_filter=2", w.linux_iface);
    let _ = run_cmd("sysctl", &["-w", &sysctl_key]).await;

    let tbl = w.table_id.to_string();
    let _ = run_cmd(
        "ip",
        &["route", "replace", &w.gateway, "dev", &w.linux_iface, "scope", "link", "table", &tbl],
    )
    .await;
    let _ = run_cmd(
        "ip",
        &[
            "route", "replace", "default", "via", &w.gateway, "dev", &w.linux_iface,
            "src", &w.ipv4, "table", &tbl,
        ],
    )
    .await;

    // fwmark-маршрутизация (pref 60..67) должна срабатывать ДО source-policy (pref 70..77)
    let legacy_src_pref = (50 + u32::from(w.slot)).to_string();
    let _ = run_cmd("ip", &["rule", "del", "pref", &legacy_src_pref]).await;
    let src_pref = (SRC_RULE_PREF_BASE + u32::from(w.slot)).to_string();
    let src_cidr = format!("{}/32", w.ipv4);
    let _ = run_cmd("ip", &["rule", "del", "pref", &src_pref]).await;
    let _ = run_cmd(
        "ip",
        &["rule", "add", "pref", &src_pref, "from", &src_cidr, "lookup", &tbl],
    )
    .await;

    let pref = (FWMARK_RULE_PREF_BASE + u32::from(w.slot)).to_string();
    let _ = run_cmd("ip", &["rule", "del", "pref", &pref]).await;
    let mark_str = mark_masked(w.fwmark);
    if !run_cmd(
        "ip",
        &["rule", "add", "pref", &pref, "fwmark", &mark_str, "lookup", &tbl],
    )
    .await
    {
        return Err(format!("Не удалось добавить ip rule для {} ({mark_str} -> {tbl})", w.linux_iface));
    }

    let nat_check = [
        "-t", "nat", "-C", "POSTROUTING", "-o", &w.linux_iface,
        "-m", "comment", "--comment", MULTIWAN_NAT_COMMENT, "-j", "MASQUERADE",
    ];
    if !crate::system::run_iptables_ok(&nat_check) {
        let _ = crate::system::run_iptables(&[
            "-t", "nat", "-A", "POSTROUTING", "-o", &w.linux_iface,
            "-m", "comment", "--comment", MULTIWAN_NAT_COMMENT, "-j", "MASQUERADE",
        ]);
    }
    Ok(())
}

#[cfg(unix)]
async fn apply_ecmp_table_and_rule(plan: &RoutingPlan) {
    let ecmp_tbl = ECMP_ROUTE_TABLE.to_string();
    let mut ecmp_args: Vec<String> = vec![
        "route".into(), "replace".into(), "default".into(),
        "scope".into(), "global".into(), "table".into(), ecmp_tbl.clone(),
    ];
    for item in &plan.active_wans {
        ecmp_args.push("nexthop".into());
        ecmp_args.push("via".into());
        ecmp_args.push(item.wan.gateway.clone());
        ecmp_args.push("dev".into());
        ecmp_args.push(item.wan.linux_iface.clone());
        ecmp_args.push("weight".into());
        ecmp_args.push(item.weight.max(1).to_string());
    }
    let _ = run_cmd_str("ip", &ecmp_args).await;
    let _ = run_cmd("ip", &["rule", "del", "pref", "68"]).await;
    let lan_mark = mark_masked(LAN_DIRECT_MARK);
    let _ = run_cmd(
        "ip",
        &["rule", "add", "pref", "68", "fwmark", &lan_mark, "lookup", &ecmp_tbl],
    )
    .await;
}

#[cfg(unix)]
pub async fn apply_routing_plan(plan: &RoutingPlan, panel_port: u16) -> Result<(), String> {
    if plan.active_wans.is_empty() {
        teardown_routing().await;
        return Ok(());
    }

    // 1. sysctl (L3 sticky = 0; L4 = 1 только при аварийном отсутствии xt_statistic)
    let hash_policy = if plan.use_statistic {
        "net.ipv4.fib_multipath_hash_policy=0"
    } else {
        "net.ipv4.fib_multipath_hash_policy=1"
    };
    let _ = run_cmd("sysctl", &["-w", hash_policy]).await;

    let mut active_slots: HashSet<u8> = HashSet::new();
    for item in &plan.active_wans {
        active_slots.insert(item.wan.slot);
        apply_wan_table_and_rules(item).await?;
    }

    // Очищаем неактивные слоты 0..7
    for slot in 0..MAX_WAN_SLOTS as u8 {
        if !active_slots.contains(&slot) {
            let legacy_src_pref = (50 + u32::from(slot)).to_string();
            let src_pref = (SRC_RULE_PREF_BASE + u32::from(slot)).to_string();
            let pref = (FWMARK_RULE_PREF_BASE + u32::from(slot)).to_string();
            let tbl = (BASE_ROUTE_TABLE + u32::from(slot)).to_string();
            let _ = run_cmd("ip", &["rule", "del", "pref", &legacy_src_pref]).await;
            let _ = run_cmd("ip", &["rule", "del", "pref", &src_pref]).await;
            let _ = run_cmd("ip", &["rule", "del", "pref", &pref]).await;
            let _ = run_cmd("ip", &["route", "flush", "table", &tbl]).await;
        }
    }

    // 2. ECMP таблица 189 для прямого/транзитного трафика
    apply_ecmp_table_and_rule(plan).await;

    // 3. ipset для защиты платных подписок и балансировки прокси-узлов
    if plan.use_ipset {
        sync_ipset(SINGLE_IP_SET, &plan.single_ip_targets).await;
        sync_ipset(PROXY_SERVERS_SET, &plan.proxy_server_targets).await;
    }

    // 4. Цепочки mangle + jump-правила
    for args in build_iptables_mangle_commands(plan) {
        let refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let _ = crate::system::run_iptables(&refs);
    }
    ensure_jump_rule("OUTPUT", CHAIN_OUT).await;
    ensure_jump_rule("PREROUTING", CHAIN_PRE).await;

    // 5. NDM-хуки
    write_ndm_hooks(panel_port).await;
    Ok(())
}

#[cfg(not(unix))]
pub async fn apply_routing_plan(_plan: &RoutingPlan, _panel_port: u16) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
pub async fn teardown_routing() {
    remove_jump_rule("OUTPUT", CHAIN_OUT).await;
    remove_jump_rule("PREROUTING", CHAIN_PRE).await;
    for chain in [CHAIN_OUT, CHAIN_PRE] {
        let _ = crate::system::run_iptables(&["-t", "mangle", "-F", chain]);
        let _ = crate::system::run_iptables(&["-t", "mangle", "-X", chain]);
    }

    // Удаляем все NAT MASQUERADE правила с комментарием MULTIWAN_NAT_COMMENT
    if let Ok(out) = tokio::process::Command::new("iptables-save")
        .args(["-t", "nat"])
        .output()
        .await
    {
        if out.status.success() {
            let stdout = String::from_utf8_lossy(&out.stdout);
            for line in stdout.lines() {
                if line.contains(MULTIWAN_NAT_COMMENT) && line.starts_with("-A POSTROUTING ") {
                    let del_rule = line.replacen("-A ", "-D ", 1);
                    let parts: Vec<String> = del_rule
                        .split_whitespace()
                        .map(|s| s.trim_matches('"').to_string())
                        .collect();
                    let mut args = vec!["-t".to_string(), "nat".to_string()];
                    args.extend(parts);
                    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                    let _ = crate::system::run_iptables(&refs);
                }
            }
        }
    }

    for slot in 0..MAX_WAN_SLOTS as u8 {
        let legacy_src_pref = (50 + u32::from(slot)).to_string();
        let src_pref = (SRC_RULE_PREF_BASE + u32::from(slot)).to_string();
        let pref = (FWMARK_RULE_PREF_BASE + u32::from(slot)).to_string();
        let tbl = (BASE_ROUTE_TABLE + u32::from(slot)).to_string();
        let _ = run_cmd("ip", &["rule", "del", "pref", &legacy_src_pref]).await;
        let _ = run_cmd("ip", &["rule", "del", "pref", &src_pref]).await;
        let _ = run_cmd("ip", &["rule", "del", "pref", &pref]).await;
        let _ = run_cmd("ip", &["route", "flush", "table", &tbl]).await;
    }
    let _ = run_cmd("ip", &["rule", "del", "pref", "68"]).await;
    let _ = run_cmd("ip", &["route", "flush", "table", &ECMP_ROUTE_TABLE.to_string()]).await;
    let _ = run_cmd("ipset", &["destroy", SINGLE_IP_SET]).await;
    let _ = run_cmd("ipset", &["destroy", PROXY_SERVERS_SET]).await;
    remove_ndm_hooks().await;
}

#[cfg(not(unix))]
pub async fn teardown_routing() {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutingIntegrity {
    pub out_jump_ok: bool,
    pub pre_jump_ok: bool,
    pub out_chain_ok: bool,
    pub pre_chain_ok: bool,
    pub missing_wan_slots: Vec<u8>,
    pub missing_ecmp: bool,
}

impl RoutingIntegrity {
    pub fn is_intact(&self) -> bool {
        self.out_jump_ok
            && self.pre_jump_ok
            && self.out_chain_ok
            && self.pre_chain_ok
            && self.missing_wan_slots.is_empty()
            && !self.missing_ecmp
    }
}

/// Чистая проверка целостности маршрутизации (обнаруживает сброс netfilter/ip-rule со стороны ndm).
pub fn check_routing_integrity(
    plan: &RoutingPlan,
    out_jump_ok: bool,
    pre_jump_ok: bool,
    out_chain_ok: bool,
    pre_chain_ok: bool,
    ip_rule_out: &str,
    ip_route_all_out: &str,
) -> RoutingIntegrity {
    if plan.active_wans.is_empty() {
        return RoutingIntegrity {
            out_jump_ok: true,
            pre_jump_ok: true,
            out_chain_ok: true,
            pre_chain_ok: true,
            missing_wan_slots: Vec::new(),
            missing_ecmp: false,
        };
    }

    let mut missing_wan_slots = Vec::new();
    for item in &plan.active_wans {
        let tbl_token = format!("lookup {}", item.wan.table_id);
        let tbl_route_token = format!("table {}", item.wan.table_id);
        let mark_pref = format!("{}:", 60 + u32::from(item.wan.slot));
        let has_rule = ip_rule_out
            .lines()
            .any(|l| l.contains(&tbl_token) && l.trim_start().starts_with(&mark_pref));
        let has_route = ip_route_all_out
            .lines()
            .any(|l| l.trim_start().starts_with("default ") && l.contains(&tbl_route_token));
        if !has_rule || !has_route {
            missing_wan_slots.push(item.wan.slot);
        }
    }

    let ecmp_lookup = format!("lookup {ECMP_ROUTE_TABLE}");
    let ecmp_route_tbl = format!("table {ECMP_ROUTE_TABLE}");
    let has_ecmp_rule = ip_rule_out.lines().any(|l| l.contains(&ecmp_lookup));
    let has_ecmp_route = ip_route_all_out
        .lines()
        .any(|l| l.trim_start().starts_with("default ") && l.contains(&ecmp_route_tbl));

    RoutingIntegrity {
        out_jump_ok,
        pre_jump_ok,
        out_chain_ok,
        pre_chain_ok,
        missing_wan_slots,
        missing_ecmp: !has_ecmp_rule || !has_ecmp_route,
    }
}

#[cfg(unix)]
async fn run_cmd_stdout_local(cmd: &str, args: &[&str]) -> String {
    tokio::process::Command::new(cmd)
        .args(args)
        .output()
        .await
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(unix)]
pub async fn inspect_routing_integrity(plan: &RoutingPlan) -> RoutingIntegrity {
    if plan.active_wans.is_empty() {
        return check_routing_integrity(plan, true, true, true, true, "", "");
    }
    let out_jump_ok = crate::system::run_iptables_ok(&["-t", "mangle", "-C", "OUTPUT", "-j", CHAIN_OUT]);
    let pre_jump_ok = crate::system::run_iptables_ok(&["-t", "mangle", "-C", "PREROUTING", "-j", CHAIN_PRE]);
    let out_chain_ok = crate::system::run_iptables_ok(&["-t", "mangle", "-L", CHAIN_OUT, "-n"]);
    let pre_chain_ok = crate::system::run_iptables_ok(&["-t", "mangle", "-L", CHAIN_PRE, "-n"]);
    let (ip_rule_out, ip_route_all_out) = tokio::join!(
        run_cmd_stdout_local("ip", &["rule", "show"]),
        run_cmd_stdout_local("ip", &["-4", "route", "show", "table", "all"]),
    );
    check_routing_integrity(
        plan,
        out_jump_ok,
        pre_jump_ok,
        out_chain_ok,
        pre_chain_ok,
        &ip_rule_out,
        &ip_route_all_out,
    )
}

#[cfg(not(unix))]
pub async fn inspect_routing_integrity(plan: &RoutingPlan) -> RoutingIntegrity {
    check_routing_integrity(plan, true, true, true, true, "", "")
}

pub async fn is_routing_intact(plan: &RoutingPlan) -> bool {
    inspect_routing_integrity(plan).await.is_intact()
}

/// Идемпотентный саморемонт после сброса `ndm` без `iptables -F` живых цепочек (сохраняет счётчики и соединения).
#[cfg(unix)]
pub async fn repair_routing_in_place(
    plan: &RoutingPlan,
    integrity: &RoutingIntegrity,
    panel_port: u16,
) -> Result<(), String> {
    if plan.active_wans.is_empty() || integrity.is_intact() {
        return Ok(());
    }
    for &slot in &integrity.missing_wan_slots {
        if let Some(item) = plan.active_wans.iter().find(|w| w.wan.slot == slot) {
            apply_wan_table_and_rules(item).await?;
        }
    }
    if integrity.missing_ecmp {
        apply_ecmp_table_and_rule(plan).await;
    }
    if !integrity.out_chain_ok || !integrity.pre_chain_ok {
        for args in build_iptables_mangle_commands(plan) {
            let is_out = args.iter().any(|a| a == CHAIN_OUT);
            let is_pre = args.iter().any(|a| a == CHAIN_PRE);
            if (!integrity.out_chain_ok && is_out) || (!integrity.pre_chain_ok && is_pre) {
                let refs: Vec<&str> = args.iter().map(String::as_str).collect();
                let _ = crate::system::run_iptables(&refs);
            }
        }
    }
    if !integrity.out_jump_ok {
        ensure_jump_rule("OUTPUT", CHAIN_OUT).await;
    }
    if !integrity.pre_jump_ok {
        ensure_jump_rule("PREROUTING", CHAIN_PRE).await;
    }
    write_ndm_hooks(panel_port).await;
    Ok(())
}

#[cfg(not(unix))]
pub async fn repair_routing_in_place(
    _plan: &RoutingPlan,
    _integrity: &RoutingIntegrity,
    _panel_port: u16,
) -> Result<(), String> {
    Ok(())
}
