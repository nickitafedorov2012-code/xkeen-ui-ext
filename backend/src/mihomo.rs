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
/// Mihomo отвечает на /providers/proxies объектом {"providers": {...}} — разворачиваем в карту
/// «имя прокси → данные». Провайдеры с vehicleType "Compatible" (служебные DIRECT/REJECT) пропускаем.
pub async fn get_provider_proxies(http: &reqwest::Client, cfg: &AppConfig) -> Result<BTreeMap<String, Value>, String> {
    let v = m_get(http, cfg, "/providers/proxies").await?;
    let mut out = BTreeMap::new();
    if let Some(map) = v.get("providers").and_then(|p| p.as_object()) {
        for (_pname, prov) in map {
            if prov.get("vehicleType").and_then(|t| t.as_str()) == Some("Compatible") {
                continue;
            }
            if let Some(list) = prov.get("proxies").and_then(|p| p.as_array()) {
                for p in list {
                    if let Some(name) = p.get("name").and_then(|n| n.as_str()) {
                        out.insert(name.to_string(), p.clone());
                    }
                }
            }
        }
    }
    Ok(out)
}

/// Имена провайдеров, которых нужно принудительно обновлять (vehicleType HTTP/File).
pub async fn updatable_provider_names(http: &reqwest::Client, cfg: &AppConfig) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(v) = m_get(http, cfg, "/providers/proxies").await {
        if let Some(map) = v.get("providers").and_then(|p| p.as_object()) {
            for (pname, prov) in map {
                if let Some(t) = prov.get("vehicleType").and_then(|t| t.as_str()) {
                    if t == "HTTP" || t == "File" {
                        out.push(pname.clone());
                    }
                }
            }
        }
    }
    out
}

/// Принудительное обновление провайдера: exclude-filter применяется при загрузке,
/// поэтому после правки config.yaml провайдера нужно перечитать (PUT, пустой ответ = ок).
pub async fn force_update_provider(http: &reqwest::Client, cfg: &AppConfig, name: &str) -> Result<(), String> {
    let enc = urlencoding_lite(name);
    let url = format!("{}{}", cfg.mihomo_url(), format_args!("/providers/proxies/{enc}"));
    let mut req = http
        .put(&url)
        .timeout(std::time::Duration::from_secs(60))
        .header("Content-Length", "0");
    if let Some((k, v)) = auth_header(&cfg.mihomo.secret) {
        req = req.header(k, v);
    }
    let resp = req.send().await.map_err(|e| format!("Mihomo update {name}: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Mihomo update {name}: статус {}", resp.status()));
    }
    Ok(())
}

/// Принудительное обновление всех провайдеров (после правки exclude-filter).
pub async fn force_update_all_providers(http: &reqwest::Client, cfg: &AppConfig) -> usize {
    let mut ok = 0usize;
    for name in updatable_provider_names(http, cfg).await {
        if force_update_provider(http, cfg, &name).await.is_ok() {
            ok += 1;
        }
    }
    ok
}

/// Рекурсивное разрешение активного «листа» из цепочек selector/fallback/urltest.

/// Рекурсивное разрешение активного «листа» из цепочек selector/fallback/urltest.
pub fn resolve_active_leaf(proxies: &BTreeMap<String, Value>) -> String {
    let mut initial = ["PROXY", "GLOBAL", "Proxy", "auto", "Fallback"]
        .iter()
        .find_map(|g| {
            proxies.get(*g).and_then(|p| p.get("now")).and_then(|n| n.as_str()).map(|s| s.to_string()).filter(|s| !s.is_empty())
        })
        .unwrap_or_default();
    // Универсальный fallback: нет привычных имён — берём первый selector.
    if initial.is_empty() {
        for (_, p) in proxies.iter() {
            let typ = p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
            if typ == "selector" {
                if let Some(now) = p.get("now").and_then(|n| n.as_str()) {
                    if !now.is_empty() {
                        initial = now.to_string();
                        break;
                    }
                }
            }
        }
    }
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

// --- Отображение имён: mojibake-ремонт и чистка эмодзи ---

/// Попытка обратной перекодировки: строка была UTF-8, но байты были прочитаны как
/// cp1251/cp1252/cp866 и сохранены как есть. Возвращаем байты исходного UTF-8.
fn decode_as(source: &str, table: fn(char) -> Option<u8>) -> Option<String> {
    let mut bytes = Vec::with_capacity(source.len());
    for c in source.chars() {
        if c.is_ascii() {
            bytes.push(c as u8);
        } else {
            bytes.push(table(c)?);
        }
    }
    String::from_utf8(bytes).ok()
}

fn cp1251_byte(c: char) -> Option<u8> {
    let u = c as u32;
    match u {
        0x0410..=0x044F => Some((u - 0x0410 + 0xC0) as u8),
        0x0401 => Some(0xA8),
        0x0451 => Some(0xB8),
        _ => None,
    }
}

fn cp1252_byte(c: char) -> Option<u8> {
    let u = c as u32;
    match u {
        0x20AC => Some(0x80), 0x201A => Some(0x82), 0x0192 => Some(0x83),
        0x201E => Some(0x84), 0x2026 => Some(0x85), 0x2020 => Some(0x86),
        0x2021 => Some(0x87), 0x02C6 => Some(0x88), 0x2030 => Some(0x89),
        0x0160 => Some(0x8A), 0x2039 => Some(0x8B), 0x0152 => Some(0x8C),
        0x017D => Some(0x8E), 0x2018 => Some(0x91), 0x2019 => Some(0x92),
        0x201C => Some(0x93), 0x201D => Some(0x94), 0x2022 => Some(0x95),
        0x2013 => Some(0x96), 0x2014 => Some(0x97), 0x02DC => Some(0x98),
        0x2122 => Some(0x99), 0x0161 => Some(0x9A), 0x203A => Some(0x9B),
        0x0153 => Some(0x9C), 0x017E => Some(0x9E), 0x0178 => Some(0x9F),
        // C1-управляющие (байты 0x80-0x9F, не имеющие представления в cp1252) — passthrough
        0x0080..=0x009F => Some(u as u8),
        0x00A0..=0x00FF => Some(u as u8),
        _ => None,
    }
}

fn cp866_byte(c: char) -> Option<u8> {
    let u = c as u32;
    match u {
        0x0410..=0x043F => Some((u - 0x0410 + 0x80) as u8),
        0x0440..=0x044F => Some((u - 0x0440 + 0xE0) as u8),
        0x0401 => Some(0xF0),
        0x0451 => Some(0xF1),
        _ => None,
    }
}

fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| matches!(c as u32, 0x0400..=0x04FF))
}

/// Ремонт mojibake: если строка — результат чтения UTF-8 как cp1251/cp1252/cp866
/// (однократно или дважды), возвращаем исходный читаемый текст.
/// Реальный mojibake смешивает символы из разных таблиц (байты ≥0xC0 читаются как
/// кириллица, остальные — как Latin-1/спецсимволы), поэтому пробуем все по очереди.
pub fn fix_mojibake(s: &str) -> Option<String> {
    let fixed = decode_as(s, |c| cp1251_byte(c).or_else(|| cp1252_byte(c)).or_else(|| cp866_byte(c)))?;
    if has_cyrillic(&fixed) && fixed != s {
        // Рекурсия на случай двойного перекодирования.
        if let Some(double) = fix_mojibake(&fixed) {
            return Some(double);
        }
        return Some(fixed);
    }
    None
}

/// Чистое имя для отображения: без ведущих эмодзи/флагов/fe0f, обрезка пробелов.
/// id/переключение всегда по оригинальному имени (mihomo требует точное имя).
pub fn clean_display_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .skip_while(|c| {
            let u = *c as u32;
            c.is_whitespace()
                || u == 0xFE0F
                || u == 0x200D
                || u == 0x20E3
                || (0x1F000..=0x1FAFF).contains(&u)
                || (0x2600..=0x27BF).contains(&u)
                || (0x2B00..=0x2BFF).contains(&u)
        })
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        name.trim().to_string()
    } else {
        trimmed.to_string()
    }
}

/// Имя для показа в панели: сначала ремонт mojibake, затем чистка эмодзи.
pub fn display_name(name: &str) -> String {
    let repaired = fix_mojibake_smart(name);
    clean_display_name(&repaired)
}

/// Умный ремонт mojibake. Порядок важен:
/// 1) вся строка целиком (байт 0xA0 в mojibake — это NBSP, поэтому split_whitespace
///    рвёт слова — делить нельзя, пока строка не декодируется целиком);
/// 2) срезаем ведущие эмодзи-флаги (они ломают декодирование) и пробуем снова;
/// 3) запасной вариант — по словам (только по обычному пробелу).
pub fn fix_mojibake_smart(s: &str) -> String {
    if let Some(fixed) = fix_mojibake(s) {
        return fixed;
    }
    let stripped = clean_display_name(s);
    if let Some(fixed) = fix_mojibake(&stripped) {
        return fixed;
    }
    let words: Vec<String> = stripped
        .split(' ')
        .map(|w| fix_mojibake(w).unwrap_or_else(|| w.to_string()))
        .collect();
    let joined = words.join(" ");
    if joined == s {
        s.to_string()
    } else {
        joined
    }
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
    // Активная группа: привычные имена, иначе первый selector (универсальность).
    let proxy_group: Option<String> = ["PROXY", "GLOBAL", "Proxy"]
        .iter()
        .find(|g| proxies.contains_key(**g))
        .map(|g| g.to_string())
        .or_else(|| {
            proxies
                .iter()
                .find(|(_, p)| p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase() == "selector")
                .map(|(name, _)| name.clone())
        });
    let proxy_now = proxy_group
        .as_deref()
        .and_then(|g| proxies.get(g))
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
        if matches!(typ.as_str(), "direct" | "reject" | "reject-drop" | "rejectdrop" | "pass" | "passrule" | "compatible" | "selector" | "urltest" | "fallback" | "relay" | "load") {
            continue;
        }
        if seen_names.contains(name) {
            continue;
        }
        seen_names.insert(name.clone());
        servers.push(Server {
            id: name.clone(),
            name: display_name(name),
            protocol: typ.to_uppercase(),
            host: p.get("server").and_then(|s| s.as_str()).unwrap_or(name).to_string(),
            port: p.get("port").and_then(|s| s.as_u64()).unwrap_or(0) as u16,
            is_active: *name == active,
            is_priority: *name == priority_server,
            ping_ms: last_delay(p),
        });
    }

    // Провайдерные серверы: GET /proxies их НЕ содержит — мержим /providers/proxies.
    if let Ok(providers) = get_provider_proxies(http, cfg).await {
        for (name, p) in &providers {
            let typ = p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
            if matches!(typ.as_str(), "direct" | "reject" | "reject-drop" | "rejectdrop" | "pass" | "passrule" | "compatible" | "selector" | "urltest" | "fallback" | "relay" | "load") {
                continue;
            }
            if seen_names.contains(name) {
                continue;
            }
            seen_names.insert(name.clone());
            servers.push(Server {
                id: name.clone(),
                name: display_name(name),
                protocol: typ.to_uppercase(),
                host: p.get("server").and_then(|s| s.as_str()).unwrap_or(name).to_string(),
                port: p.get("port").and_then(|s| s.as_u64()).unwrap_or(0) as u16,
                is_active: *name == active,
                is_priority: *name == priority_server,
                ping_ms: last_delay(p),
            });
        }
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

/// Переключение активного сервера во всех основных select-группах.
/// Универсально: группы обнаруживаются динамически (selector без IP в имени,
/// кроме Fastest/Fallback и AUTO-DEVICE групп устройств).
pub async fn switch_server(http: &reqwest::Client, cfg: &AppConfig, server_id: &str) -> Result<String, String> {
    let target = match server_id.to_lowercase().as_str() {
        "fastest" => "Fastest".to_string(),
        "fallback" => "Fallback".to_string(),
        _ => server_id.trim().to_string(),
    };
    let proxies = get_proxies(http, cfg).await?;
    let mut groups: Vec<String> = Vec::new();
    for (name, p) in &proxies {
        let typ = p.get("type").and_then(|t| t.as_str()).unwrap_or("").to_lowercase();
        if typ != "selector" {
            continue;
        }
        // исключаем авто-группы и группы устройств (в имени есть IP/MAC)
        if name == "Fastest" || name == "Fallback" || ip_from_group_name(name).is_some() {
            continue;
        }
        groups.push(name.clone());
    }
    // Известные общие имена — в начало (если существуют).
    for pref in ["PROXY", "GLOBAL", "Proxy"] {
        if let Some(pos) = groups.iter().position(|g| g == pref) {
            let g = groups.remove(pos);
            groups.insert(0, g);
        }
    }
    if groups.is_empty() {
        return Err("Не найдено ни одной select-группы для переключения".into());
    }
    let mut switched = 0usize;
    let mut last_err = String::new();
    for g in &groups {
        match m_put(http, cfg, &format!("/proxies/{}", urlencoding_lite(g)), json!({ "name": target }), 3).await {
            Ok(_) => switched += 1,
            Err(e) => last_err = e,
        }
    }
    if switched > 0 {
        Ok(format!("Активный сервер переключен на '{target}' (групп: {switched})"))
    } else {
        Err(format!("Не удалось изменить активный сервер в Mihomo: {last_err}"))
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

/// Reload конфига Mihomo (после правки config.yaml). Путь — из настроек.
pub async fn reload_config(http: &reqwest::Client, cfg: &AppConfig) -> Result<(), String> {
    m_put(http, cfg, "/configs?force=true", json!({ "path": cfg.mihomo.config_path }), 15).await
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
        // флаг-эмодзи перед mojibake: вся строка не декодируется — срезаем флаг, чиним остаток
        let with_flag = format!("\u{1F1EB}\u{1F1EE} {mojibake}");
        assert_eq!(display_name(&with_flag), "Финляндия");
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
        assert_eq!(display_name(&ru_with_flag), "Россия");
        // нормальное имя с флагом не ломается
        assert_eq!(display_name("🇩🇪 Германия"), "Германия");
    }
}



