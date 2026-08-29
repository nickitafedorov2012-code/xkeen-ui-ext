//! Клиент Mihomo (Clash) REST API: серверы, переключение, пинг, reload конфига.

use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::config::AppConfig;

#[derive(Clone, Debug)]
pub struct Server {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub host: String,
    pub port: u16,
    pub is_active: bool,
    pub is_priority: bool,
    pub ping_ms: i64,
}

fn auth_header(secret: &str) -> Option<(&'static str, String)> {
    if secret.is_empty() {
        None
    } else {
        Some(("Authorization", format!("Bearer {secret}")))
    }
}

pub async fn m_get(http: &reqwest::Client, cfg: &AppConfig, path: &str) -> Result<Value, String> {
    let url = format!("{}{}", cfg.mihomo_url(), path);
    let mut req = http.get(&url).timeout(std::time::Duration::from_secs(5));
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| format!("Mihomo {path}: {e}"))?;
    let status = resp.status();
    let body: Value = resp.json().await.map_err(|e| format!("Mihomo {path}: {e}"))?;
    if !status.is_success() {
        return Err(format!("Mihomo {path}: статус {status}"));
    }
    Ok(body)
}

pub async fn m_put(http: &reqwest::Client, cfg: &AppConfig, path: &str, body: Value, timeout: u64) -> Result<(), String> {
    let url = format!("{}{}", cfg.mihomo_url(), path);
    let mut req = http
        .put(&url)
        .timeout(std::time::Duration::from_secs(timeout))
        .json(&body);
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| format!("Mihomo {path}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Mihomo {path}: статус {}", resp.status()));
    }
    Ok(())
}

/// Mihomo отвечает на /proxies объектом {"proxies": {...}} — достаём карту.
pub async fn get_proxies(http: &reqwest::Client, cfg: &AppConfig) -> Result<BTreeMap<String, Value>, String> {
    let v = m_get(http, cfg, "/proxies").await?;
    let mut out = BTreeMap::new();
    if let Some(map) = v.get("proxies").and_then(|p| p.as_object()) {
        for (k, val) in map {
            out.insert(k.clone(), val.clone());
        }
    }
    Ok(out)
}

/// Рекурсивное разрешение активного «листа» из цепочек selector/fallback/urltest.
pub fn resolve_active_leaf(proxies: &BTreeMap<String, Value>) -> String {
    let initial = ["PROXY", "GLOBAL", "Proxy", "auto", "Fallback"]
        .iter()
        .find_map(|g| {
            proxies.get(*g).and_then(|p| p.get("now")).and_then(|n| n.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
    if initial.is_empty() {
        return String::new();
    }

    let mut cur = initial;
    let mut visited = std::collections::HashSet::new();
    while let Some(sub) = proxies.get(&cur) {
        if visited.contains(&cur) {
            break;
        }
        visited.insert(cur.clone());
        let typ = sub.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        if typ != "fallback" && typ != "urltest" && typ != "selector" {
            break;
        }
        match sub.get("now").and_then(|n| n.as_str()) {
            Some(next) if !next.is_empty() && next != &cur => cur = next.to_string(),
            _ => break,
        }
    }
    cur
}

fn last_delay(p: &Value) -> i64 {
    p.get("history")
        .and_then(|h| h.as_array())
        .and_then(|a| a.last())
        .and_then(|e| e.get("delay"))
        .and_then(|d| d.as_i64())
        .unwrap_or(-1)
}

/// Список серверов с синтетическими Fastest/Fallback и активным листом.
pub async fn get_servers(
    http: &reqwest::Client,
    cfg: &AppConfig,
    priority_server: &str,
) -> Result<Vec<Server>, String> {
    let proxies = get_proxies(http, cfg).await?;
    let active = resolve_active_leaf(&proxies);
    let proxy_now = proxies
        .get("PROXY")
        .and_then(|p| p.get("now"))
        .and_then(|n| n.as_str())
        .unwrap_or("")
        .to_string();

    let mut servers: Vec<Server> = Vec::new();
    let mut seen_names: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (id, label, icon) in [("Fastest", "URL-TEST", "⚡"), ("Fallback", "FALLBACK", "🛡️")] {
        if let Some(p) = proxies.get(id) {
            servers.push(Server {
                id: id.into(),
                name: format!("{icon} {id} (Авто)"),
                protocol: label.into(),
                host: format!("Авто: {}", p.get("now").and_then(|n| n.as_str()).unwrap_or("—")),
                port: 0,
                is_active: proxy_now == id,
                is_priority: priority_server == id,
                ping_ms: last_delay(p),
            });
        }
    }

    for (name, p) in &proxies {
        let typ = p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        if matches!(typ.as_str(), "direct" | "reject" | "reject-drop" | "pass" | "compatible" | "selector" | "urltest" | "fallback" | "relay" | "load") {
            continue;
        }
        if seen_names.contains(name) {
            continue;
        }
        seen_names.insert(name.clone());
        servers.push(Server {
            id: name.clone(),
            name: name.clone(),
            protocol: typ.to_uppercase(),
            host: p.get("server").and_then(|s| s.as_str()).unwrap_or(name).to_string(),
            port: p.get("port").and_then(|s| s.as_u64()).unwrap_or(0) as u16,
            is_active: *name == active,
            is_priority: *name == priority_server,
            ping_ms: last_delay(p),
        });
    }

    servers.sort_by(|a, b| b.is_active.cmp(&a.is_active).then(b.ping_ms.cmp(&a.ping_ms)));
    Ok(servers)
}

/// Карта IP → группа из SRC-IP правил Mihomo.
pub fn ip_groups_from_rules(rules: &Value) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(list) = rules.get("rules").and_then(|r| r.as_array()) {
        for r in list {
            let ptype = r.get("type").and_then(|t| t.as_str()).unwrap_or("").to_uppercase();
            let payload = r.get("payload").and_then(|p| p.as_str()).unwrap_or("").to_string();
            let target = r.get("proxy").and_then(|p| p.as_str()).unwrap_or("").to_string();
            if (ptype.contains("SRC-IP") || payload.contains('/')) && !target.is_empty() {
                let ip = payload.split('/').next().unwrap_or("").trim().to_string();
                if !ip.is_empty() {
                    out.insert(ip, target);
                }
            }
        }
    }
    out
}

/// 'Big PC 192_168_2_118' → '192.168.2.118' (формат AUTO-DEVICE групп).
pub fn ip_from_group_name(gname: &str) -> Option<String> {
    let mut token = gname.trim().split(' ').next_back()?;
    if let Some(stripped) = token.strip_prefix("DEV_") {
        token = stripped;
    }
    let cand = token.replace('_', ".");
    let parts: Vec<&str> = cand.split('.').collect();
    if parts.len() == 4 && parts.iter().all(|p| p.parse::<u8>().is_ok()) {
        return Some(cand);
    }
    if token.contains('_') {
        return Some(token.replace('_', ":"));
    }
    None
}

/// Live-состояние: какой сервер реально используется каждым устройством.
pub async fn live_device_servers(http: &reqwest::Client, cfg: &AppConfig) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    let proxies = match get_proxies(http, cfg).await {
        Ok(p) => p,
        Err(_) => return out,
    };
    let rules = m_get(http, cfg, "/rules").await.unwrap_or(Value::Null);

    // 1. Из SRC-IP правил: ip → группа → now
    for (ip, group) in ip_groups_from_rules(&rules) {
        if let Some(now) = proxies.get(&group).and_then(|g| g.get("now")).and_then(|n| n.as_str()) {
            out.insert(ip, now.to_string());
        }
    }

    // 2. Из групп с IP в имени ('Big PC 192_168_2_118')
    for (gname, pdata) in &proxies {
        let Some(now) = pdata.get("now").and_then(|n| n.as_str()) else { continue };
        if let Some(ip) = ip_from_group_name(gname) {
            out.entry(ip).or_insert_with(|| now.to_string());
        }
    }
    out
}

/// Переключение активного сервера во всех группах маршрутизации.
pub async fn switch_server(http: &reqwest::Client, cfg: &AppConfig, server_id: &str) -> Result<String, String> {
    let target = match server_id.to_lowercase().as_str() {
        "fastest" => "Fastest".to_string(),
        "fallback" => "Fallback".to_string(),
        _ => server_id.trim().to_string(),
    };
    let groups = ["PROXY", "GLOBAL", "Proxy", "YouTube", "Discord", "Telegram", "Steam", "Twitch", "User List"];
    let mut switched = false;
    for g in groups {
        if m_put(http, cfg, &format!("/proxies/{g}"), json!({ "name": target }), 3).await.is_ok() {
            switched = true;
        }
    }
    if switched {
        Ok(format!("Активный сервер переключен на '{target}'"))
    } else {
        Err("Не удалось изменить активный сервер в Mihomo".into())
    }
}

/// Пинг сервера через delay-API Mihomo (мс; -1 = недоступен).
pub async fn ping_server(http: &reqwest::Client, cfg: &AppConfig, server_id: &str, timeout_ms: u64) -> i64 {
    let enc = urlencoding_lite(server_id);
    let url = format!(
        "{}/proxies/{enc}/delay?timeout={timeout_ms}&url={}",
        cfg.mihomo_url(),
        "http%3A%2F%2Fwww.gstatic.com%2Fgenerate_204"
    );
    let mut req = http.get(&url).timeout(std::time::Duration::from_millis(timeout_ms + 1500));
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    if let Ok(resp) = req.send().await {
        if let Ok(v) = resp.json::<Value>().await {
            if let Some(d) = v.get("delay").and_then(|d| d.as_i64()) {
                if d > 0 {
                    return d;
                }
            }
        }
    }
    -1
}

/// Параллельный пинг списка серверов.
pub async fn ping_all(http: &reqwest::Client, cfg: &AppConfig, ids: &[String], timeout_ms: u64) -> BTreeMap<String, i64> {
    let mut handles = Vec::new();
    for id in ids {
        let http = http.clone();
        let cfg = cfg.clone();
        let id = id.clone();
        handles.push(tokio::spawn(async move {
            let ms = ping_server(&http, &cfg, &id, timeout_ms).await;
            (id, ms)
        }));
    }
    let mut out = BTreeMap::new();
    for h in handles {
        if let Ok((id, ms)) = h.await {
            out.insert(id, ms);
        }
    }
    out
}

/// Reload конфига Mihomo (после правки config.yaml).
pub async fn reload_config(http: &reqwest::Client, cfg: &AppConfig) -> Result<(), String> {
    m_put(http, cfg, "/configs?force=true", json!({ "path": "/opt/etc/mihomo/config.yaml" }), 15).await
}

/// Выбор сервера в конкретной группе (для AUTO-DEVICE групп после reload).
pub async fn switch_group(http: &reqwest::Client, cfg: &AppConfig, group: &str, server: &str) -> Result<(), String> {
    let enc = urlencoding_lite(group);
    m_put(http, cfg, &format!("/proxies/{enc}"), json!({ "name": server }), 5).await
}


/// Минимальный percent-encoding для имён прокси в URL.
fn urlencoding_lite(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    }
}



