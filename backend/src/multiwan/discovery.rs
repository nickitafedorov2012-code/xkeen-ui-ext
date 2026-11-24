use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::config::AppConfig;

pub const BASE_ROUTE_TABLE: u32 = 180;
pub const MAX_WAN_SLOTS: usize = 8;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WanKind {
    Modem,
    Wisp,
    Ethernet,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredWan {
    pub rci_id: String,
    pub name: String,
    pub kind: WanKind,
    pub linux_iface: String,
    pub ipv4: String,
    pub gateway: String,
    pub keenetic_table: Option<u32>,
    pub slot: u8,
    pub table_id: u32,
    pub fwmark: u32,
}

#[derive(Debug, Clone)]
pub struct LinuxAddr {
    pub iface: String,
    pub ipv4: String,
}

#[derive(Debug, Clone)]
pub struct LinuxRoute {
    pub iface: String,
    pub gateway: String,
    pub table: Option<u32>,
}

#[derive(Debug, Clone)]
struct RciIfaceInfo {
    rci_id: String,
    name: String,
    ipv4: String,
    gateway: Option<String>,
    linux_hint: Option<String>,
    up: bool,
}

pub fn is_excluded_linux_iface(iface: &str) -> bool {
    let s = iface.trim();
    if s.is_empty() || s == "lo" || s == "csq0" {
        return true;
    }
    const PREFIXES: &[&str] = &[
        "br", "Bridge", "tun", "tap", "nwg", "wg", "ovpn", "ezcfg", "dummy", "gre", "sit",
        "xfrm", "ip6tnl", "teql", "erspan", "veth", "docker", "ra0", "rax", "rai",
    ];
    // Примечание: apcli* / wlan* — это WISP-клиенты (не AP), их НЕ исключаем!
    // ra0/rax0/rai0 — точки доступа Wi-Fi, но если они не в br0 и имеют внешний IP+gw —
    // лучше исключать только явные bridge/tunnel префиксы; для ra* проверяем только br-членство.
    if s.starts_with("ra0") || s.starts_with("rax") || s.starts_with("rai") {
        // Если на устройстве ra* используется как AP, он не будет иметь default gateway в ip route.
        return false;
    }
    PREFIXES.iter().any(|p| s.starts_with(p))
}

pub fn is_excluded_rci_iface(id: &str, rci_type: &str) -> bool {
    let id_l = id.to_ascii_lowercase();
    let ty_l = rci_type.to_ascii_lowercase();
    if ty_l == "bridge"
        || ty_l == "loopback"
        || ty_l == "accesspoint"
        || ty_l.contains("vpn")
        || ty_l.contains("tunnel")
    {
        return true;
    }
    const BAD_PREFIXES: &[&str] = &[
        "bridge", "loopback", "openvpn", "wireguard", "pptp", "l2tp", "sstp", "ipip", "gre",
        "eoip", "nwg",
    ];
    BAD_PREFIXES.iter().any(|p| id_l.starts_with(p))
}

fn classify_wan_kind(rci_id: &str, rci_type: &str, linux_iface: &str, desc: &str) -> WanKind {
    let combined = format!("{rci_id} {rci_type} {linux_iface} {desc}").to_ascii_lowercase();
    if combined.contains("wifistation")
        || combined.contains("wisp")
        || linux_iface.starts_with("apcli")
        || linux_iface.starts_with("wlan")
    {
        return WanKind::Wisp;
    }
    if combined.contains("usb")
        || combined.contains("modem")
        || combined.contains("lte")
        || combined.contains("qmi")
        || combined.contains("cdc")
        || combined.contains("ncm")
        || combined.contains("rndis")
        || linux_iface.starts_with("cdc_")
        || linux_iface.starts_with("wwan")
        || linux_iface.starts_with("ppp")
    {
        return WanKind::Modem;
    }
    WanKind::Ethernet
}

/// Вычисляет сетевой CIDR (например `192.168.1.0/24`) из `192.168.1.1/24`.
pub fn ip_prefix_to_network_cidr(ip_cidr: &str) -> Option<String> {
    let (ip_str, pref_str) = ip_cidr.trim().split_once('/')?;
    let ip: std::net::Ipv4Addr = ip_str.parse().ok()?;
    let prefix: u32 = pref_str.parse().ok()?;
    if prefix > 32 {
        return None;
    }
    let mask = if prefix == 0 {
        0u32
    } else {
        !0u32 << (32 - prefix)
    };
    let net = std::net::Ipv4Addr::from(u32::from(ip) & mask);
    Some(format!("{net}/{prefix}"))
}

/// Парсит `ip -4 addr show` -> `(список адресов, список LAN-подсетей из br*/Bridge*)`.
pub fn parse_ip_addr_output(raw: &str) -> (Vec<LinuxAddr>, Vec<String>) {
    let mut addrs = Vec::new();
    let mut lan_cidrs = Vec::new();
    let mut cur_iface = String::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(first_ch) = line.chars().next() {
            if first_ch.is_ascii_digit() {
                // Строка вида: "12: apcli0: <BROADCAST,MULTICAST,UP,LOWER_UP> ..."
                let after_colon = line.split_once(':').map(|x| x.1).unwrap_or("");
                let iface_part = after_colon
                    .split_once(':')
                    .map(|x| x.0)
                    .unwrap_or("")
                    .trim();
                cur_iface = iface_part
                    .split('@')
                    .next()
                    .unwrap_or(iface_part)
                    .to_string();
                continue;
            }
        }
        if let Some(rest) = trimmed.strip_prefix("inet ") {
            let cidr = rest.split_whitespace().next().unwrap_or("");
            if let Some((ip_str, _)) = cidr.split_once('/') {
                if !cur_iface.is_empty() {
                    if cur_iface.starts_with("br") || cur_iface.starts_with("Bridge") {
                        if let Some(net) = ip_prefix_to_network_cidr(cidr) {
                            if !lan_cidrs.contains(&net) {
                                lan_cidrs.push(net);
                            }
                        }
                    } else {
                        addrs.push(LinuxAddr {
                            iface: cur_iface.clone(),
                            ipv4: ip_str.to_string(),
                        });
                    }
                }
            }
        }
    }

    (addrs, lan_cidrs)
}

/// Парсит `ip -4 route show table all` -> default маршруты с `via <gw> dev <iface>`.
pub fn parse_ip_route_all_output(raw: &str) -> Vec<LinuxRoute> {
    let mut out = Vec::new();
    for line in raw.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("default ") {
            continue;
        }
        let tokens: Vec<&str> = trimmed.split_whitespace().collect();
        let mut gateway = None;
        let mut iface = None;
        let mut table = None;
        let mut i = 1;
        while i < tokens.len() {
            match tokens[i] {
                "via" if i + 1 < tokens.len() => {
                    gateway = Some(tokens[i + 1].to_string());
                    i += 2;
                }
                "dev" if i + 1 < tokens.len() => {
                    iface = Some(tokens[i + 1].to_string());
                    i += 2;
                }
                "table" if i + 1 < tokens.len() => {
                    let t = tokens[i + 1];
                    table = if t == "main" {
                        Some(254)
                    } else {
                        t.parse::<u32>().ok()
                    };
                    i += 2;
                }
                _ => i += 1,
            }
        }
        if let (Some(gw), Some(dev)) = (gateway, iface) {
            out.push(LinuxRoute {
                iface: dev,
                gateway: gw,
                table,
            });
        }
    }
    out
}

fn parse_rci_interfaces(rci_iface_json: Option<&Value>, rci_route_json: Option<&Value>) -> Vec<RciIfaceInfo> {
    let mut gw_by_rci_id: HashMap<String, String> = HashMap::new();
    if let Some(Value::Array(routes)) = rci_route_json {
        for r in routes {
            let dest = r.get("destination").and_then(|v| v.as_str()).unwrap_or("");
            if dest == "0.0.0.0/0" || dest == "default" {
                let gw = r.get("gateway").and_then(|v| v.as_str()).unwrap_or("").trim();
                let iface = r.get("interface").and_then(|v| v.as_str()).unwrap_or("").trim();
                if !gw.is_empty() && gw != "0.0.0.0" && !iface.is_empty() {
                    gw_by_rci_id.entry(iface.to_string()).or_insert_with(|| gw.to_string());
                }
            }
        }
    }

    let mut items: Vec<(&str, &Value)> = Vec::new();
    if let Some(val) = rci_iface_json {
        match val {
            Value::Object(map) => {
                for (k, v) in map {
                    items.push((k.as_str(), v));
                }
            }
            Value::Array(arr) => {
                for v in arr {
                    let id = v.get("id").and_then(|x| x.as_str()).unwrap_or("");
                    if !id.is_empty() {
                        items.push((id, v));
                    }
                }
            }
            _ => {}
        }
    }

    let mut out = Vec::new();
    for (fallback_id, obj) in items {
        let rci_id = obj
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or(fallback_id)
            .trim()
            .to_string();
        if rci_id.is_empty() {
            continue;
        }
        let rci_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if is_excluded_rci_iface(&rci_id, rci_type) {
            continue;
        }
        let ipv4 = obj
            .get("address")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        if ipv4.is_empty() || ipv4 == "0.0.0.0" {
            continue;
        }
        let desc = obj
            .get("description")
            .and_then(|v| v.as_str())
            .or_else(|| obj.get("interface-name").and_then(|v| v.as_str()))
            .unwrap_or("")
            .trim();
        let ssid = obj.get("ssid").and_then(|v| v.as_str()).unwrap_or("").trim();
        let name = if !desc.is_empty() && !ssid.is_empty() && !desc.contains(ssid) {
            format!("{desc} ({ssid})")
        } else if !desc.is_empty() {
            desc.to_string()
        } else if !ssid.is_empty() {
            format!("WISP {ssid}")
        } else {
            rci_id.clone()
        };
        let linux_hint = obj
            .get("system-name")
            .or_else(|| obj.get("linux-name"))
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let link_up = obj
            .get("link")
            .and_then(|v| v.as_str())
            .map(|s| s.eq_ignore_ascii_case("up"))
            .unwrap_or(true);
        let state_up = obj
            .get("state")
            .and_then(|v| v.as_str())
            .map(|s| s.eq_ignore_ascii_case("up"))
            .unwrap_or(true);
        let gw = obj
            .get("via")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && *s != "0.0.0.0")
            .map(str::to_string)
            .or_else(|| gw_by_rci_id.get(&rci_id).cloned());
        out.push(RciIfaceInfo {
            rci_id,
            name,
            ipv4,
            gateway: gw,
            linux_hint,
            up: link_up && state_up,
        });
    }
    out
}

fn parse_rci_policy_tables(rci_policy_json: Option<&Value>) -> HashMap<String, u32> {
    let mut out = HashMap::new();
    let Some(val) = rci_policy_json else {
        return out;
    };
    let items: Vec<&Value> = match val {
        Value::Object(map) => map.values().collect(),
        Value::Array(arr) => arr.iter().collect(),
        _ => Vec::new(),
    };
    for pol in items {
        let table_id = pol
            .get("table")
            .or_else(|| pol.get("table-id"))
            .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
            .map(|n| n as u32);
        let Some(tbl) = table_id else {
            continue;
        };
        if let Some(ifaces) = pol.get("interface").and_then(|v| v.as_array()) {
            for item in ifaces {
                if let Some(id) = item
                    .get("id")
                    .or_else(|| item.get("interface"))
                    .and_then(|v| v.as_str())
                {
                    out.entry(id.trim().to_string()).or_insert(tbl);
                }
            }
        }
    }
    out
}

fn parse_proc_net_dev_ifaces(raw: &str) -> HashSet<String> {
    raw.lines()
        .skip(2)
        .filter_map(|line| line.split_once(':').map(|(iface, _)| iface.trim().to_string()))
        .filter(|s| !s.is_empty())
        .collect()
}

pub fn ipv4_slash24(ip_str: &str) -> Option<String> {
    let ip: std::net::Ipv4Addr = ip_str.trim().parse().ok()?;
    let o = ip.octets();
    Some(format!("{}.{}.{}.0/24", o[0], o[1], o[2]))
}

/// Проверяет совпадение шлюза или `/24`-подсети у двух и более WAN (например, два модема Huawei HiLink `192.168.8.1/24`).
pub fn detect_gateway_collision(wans: &[DiscoveredWan]) -> Option<String> {
    if wans.len() < 2 {
        return None;
    }
    let mut by_gw: HashMap<&str, Vec<&DiscoveredWan>> = HashMap::new();
    let mut by_net: HashMap<String, Vec<&DiscoveredWan>> = HashMap::new();
    for w in wans {
        if !w.gateway.is_empty() {
            by_gw.entry(w.gateway.as_str()).or_default().push(w);
        }
        if let Some(net) = ipv4_slash24(&w.ipv4) {
            by_net.entry(net).or_default().push(w);
        }
    }

    let mut conflicts = Vec::new();
    for (gw, list) in by_gw {
        if list.len() >= 2 {
            let names: Vec<String> = list
                .iter()
                .map(|w| format!("{} ({})", w.name, w.linux_iface))
                .collect();
            conflicts.push(format!("совпадает шлюз {gw} у каналов {}", names.join(", ")));
        }
    }
    for (net, list) in by_net {
        if list.len() >= 2 {
            let names: Vec<String> = list
                .iter()
                .map(|w| format!("{} ({})", w.name, w.linux_iface))
                .collect();
            conflicts.push(format!("совпадает подсеть {net} у каналов {}", names.join(", ")));
        }
    }
    if conflicts.is_empty() {
        None
    } else {
        Some(format!(
            "Обнаружена коллизия адресов модемов: {}. Маршруты изолированы по интерфейсам (dev), но для максимальной стабильности рекомендуется сменить подсеть одного из модемов.",
            conflicts.join("; ")
        ))
    }
}

/// Чистая функция объединения RCI-метаданных и системного сетевого стека Linux в список WAN.
pub fn assemble_discovered_wans(
    rci_iface_json: Option<&Value>,
    rci_route_json: Option<&Value>,
    ip_addr_out: &str,
    ip_route_out: &str,
) -> (Vec<DiscoveredWan>, Vec<String>) {
    assemble_discovered_wans_full(
        rci_iface_json,
        rci_route_json,
        None,
        None,
        ip_addr_out,
        ip_route_out,
    )
}

/// Полная сборка списка WAN с учётом `/rci/show/ip/policy` и `/proc/net/dev`.
pub fn assemble_discovered_wans_full(
    rci_iface_json: Option<&Value>,
    rci_route_json: Option<&Value>,
    rci_policy_json: Option<&Value>,
    proc_net_dev: Option<&str>,
    ip_addr_out: &str,
    ip_route_out: &str,
) -> (Vec<DiscoveredWan>, Vec<String>) {
    let (linux_addrs, lan_cidrs) = parse_ip_addr_output(ip_addr_out);
    let linux_routes = parse_ip_route_all_output(ip_route_out);
    let rci_ifaces = parse_rci_interfaces(rci_iface_json, rci_route_json);
    let policy_tables = parse_rci_policy_tables(rci_policy_json);
    let netdev_ifaces: Option<HashSet<String>> = proc_net_dev
        .filter(|s| !s.trim().is_empty())
        .map(parse_proc_net_dev_ifaces);

    let mut addr_by_iface: HashMap<String, &LinuxAddr> = HashMap::new();
    let mut addrs_by_ip: HashMap<String, Vec<&LinuxAddr>> = HashMap::new();
    for a in &linux_addrs {
        if is_excluded_linux_iface(&a.iface) {
            continue;
        }
        if let Some(ref nd) = netdev_ifaces {
            if !nd.contains(&a.iface) {
                continue;
            }
        }
        addr_by_iface.insert(a.iface.clone(), a);
        addrs_by_ip.entry(a.ipv4.clone()).or_default().push(a);
    }

    let mut route_by_iface: HashMap<String, &LinuxRoute> = HashMap::new();
    for r in &linux_routes {
        if is_excluded_linux_iface(&r.iface) {
            continue;
        }
        // Предпочитаем выделенную таблицу KeeneticOS (не 254 main), либо первую найденную
        route_by_iface
            .entry(r.iface.clone())
            .and_modify(|existing| {
                if existing.table == Some(254) && r.table.is_some_and(|t| t != 254) {
                    *existing = r;
                }
            })
            .or_insert(r);
    }

    let mut by_linux_iface: BTreeMap<String, DiscoveredWan> = BTreeMap::new();
    let mut claimed_ifaces: HashSet<String> = HashSet::new();

    for rci in rci_ifaces {
        if !rci.up {
            continue;
        }
        let linux_addr = rci
            .linux_hint
            .as_deref()
            .and_then(|h| addr_by_iface.get(h).copied())
            .filter(|a| !claimed_ifaces.contains(&a.iface))
            .or_else(|| {
                addrs_by_ip
                    .get(&rci.ipv4)
                    .and_then(|list| list.iter().copied().find(|a| !claimed_ifaces.contains(&a.iface)))
            });
        let Some(addr) = linux_addr else {
            continue;
        };
        if is_excluded_linux_iface(&addr.iface) {
            continue;
        }
        let linux_route = route_by_iface.get(&addr.iface).copied();
        let gateway = linux_route
            .map(|r| r.gateway.clone())
            .or(rci.gateway)
            .unwrap_or_default();
        if gateway.is_empty() {
            continue;
        }
        claimed_ifaces.insert(addr.iface.clone());
        let keenetic_table = linux_route
            .and_then(|r| r.table)
            .or_else(|| policy_tables.get(&rci.rci_id).copied());
        let kind = classify_wan_kind(&rci.rci_id, "", &addr.iface, &rci.name);
        by_linux_iface.insert(
            addr.iface.clone(),
            DiscoveredWan {
                rci_id: rci.rci_id,
                name: rci.name,
                kind,
                linux_iface: addr.iface.clone(),
                ipv4: addr.ipv4.clone(),
                gateway,
                keenetic_table,
                slot: 0,
                table_id: 0,
                fwmark: 0,
            },
        );
    }

    // Fallback: любые неисключённые Linux-интерфейсы с IP и default gateway (на случай если RCI недоступен)
    for route in &linux_routes {
        if is_excluded_linux_iface(&route.iface) || by_linux_iface.contains_key(&route.iface) {
            continue;
        }
        let Some(addr) = addr_by_iface.get(&route.iface) else {
            continue;
        };
        let kind = classify_wan_kind(&route.iface, "", &route.iface, "");
        by_linux_iface.insert(
            route.iface.clone(),
            DiscoveredWan {
                rci_id: route.iface.clone(),
                name: format!("WAN {}", route.iface),
                kind,
                linux_iface: route.iface.clone(),
                ipv4: addr.ipv4.clone(),
                gateway: route.gateway.clone(),
                keenetic_table: route.table,
                slot: 0,
                table_id: 0,
                fwmark: 0,
            },
        );
    }

    let mut out: Vec<DiscoveredWan> = by_linux_iface.into_values().take(MAX_WAN_SLOTS).collect();
    for (idx, wan) in out.iter_mut().enumerate() {
        let slot = idx as u8;
        wan.slot = slot;
        wan.table_id = BASE_ROUTE_TABLE + u32::from(slot);
        wan.fwmark = (u32::from(slot) + 1) << 28;
    }

    (out, lan_cidrs)
}

async fn run_cmd_stdout(cmd: &str, args: &[&str]) -> String {
    #[cfg(unix)]
    {
        if let Ok(out) = tokio::process::Command::new(cmd).args(args).output().await {
            return String::from_utf8_lossy(&out.stdout).into_owned();
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (cmd, args);
    }
    String::new()
}

async fn read_proc_net_dev_discovery() -> String {
    #[cfg(target_os = "linux")]
    {
        tokio::fs::read_to_string("/proc/net/dev")
            .await
            .unwrap_or_default()
    }
    #[cfg(not(target_os = "linux"))]
    {
        String::new()
    }
}

pub async fn discover_wans(http: &reqwest::Client, cfg: &AppConfig) -> (Vec<DiscoveredWan>, Vec<String>) {
    let (rci_iface_res, rci_route_res, rci_policy_res, proc_net_dev, ip_addr_out, ip_route_out) = tokio::join!(
        crate::rci::get_raw_json(http, cfg, "/rci/show/interface"),
        crate::rci::get_raw_json(http, cfg, "/rci/show/ip/route"),
        crate::rci::get_raw_json(http, cfg, "/rci/show/ip/policy"),
        read_proc_net_dev_discovery(),
        run_cmd_stdout("ip", &["-4", "addr", "show"]),
        run_cmd_stdout("ip", &["-4", "route", "show", "table", "all"]),
    );

    assemble_discovered_wans_full(
        rci_iface_res.ok().as_ref(),
        rci_route_res.ok().as_ref(),
        rci_policy_res.ok().as_ref(),
        Some(&proc_net_dev),
        &ip_addr_out,
        &ip_route_out,
    )
}
