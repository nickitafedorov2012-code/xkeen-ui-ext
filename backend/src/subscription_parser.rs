//! Модуль поддержки мобильных подписок V2Ray / Base64 (VLESS, Hysteria2, Trojan, VMess, Shadowsocks)
//! с точным сохранением названий серверов и эмодзи из фрагментов ссылок (#...).

use serde_json::{json, Value};
use std::collections::BTreeMap;

/// Декодирование URL-encoded (percent-encoded) строки, включая эмодзи и спецсимволы UTF-8.
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(val) = u8::from_str_radix(std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""), 16) {
                out.push(val);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Декодирование Base64 (стандартного и URL-safe, с паддингом '=' или без)
pub fn base64_decode(input: &str) -> Result<Vec<u8>, String> {
    let clean: String = input.chars().filter(|c| !c.is_whitespace()).collect();
    if clean.is_empty() {
        return Ok(Vec::new());
    }

    let mut buf = Vec::with_capacity(clean.len());
    for c in clean.chars() {
        if c == '=' {
            break;
        }
        let val = match c {
            'A'..='Z' => c as u8 - b'A',
            'a'..='z' => c as u8 - b'a' + 26,
            '0'..='9' => c as u8 - b'0' + 52,
            '+' | '-' => 62,
            '/' | '_' => 63,
            _ => continue,
        };
        buf.push(val);
    }

    let mut out = Vec::with_capacity(buf.len() * 3 / 4);
    for chunk in buf.chunks(4) {
        match chunk.len() {
            4 => {
                out.push((chunk[0] << 2) | (chunk[1] >> 4));
                out.push((chunk[1] << 4) | (chunk[2] >> 2));
                out.push((chunk[2] << 6) | chunk[3]);
            }
            3 => {
                out.push((chunk[0] << 2) | (chunk[1] >> 4));
                out.push((chunk[1] << 4) | (chunk[2] >> 2));
            }
            2 => {
                out.push((chunk[0] << 2) | (chunk[1] >> 4));
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Проверка, является ли контент V2Ray/Base64 подпиской (а не Clash YAML)
pub fn is_v2ray_or_base64_subscription(raw: &str) -> bool {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return false;
    }
    if trimmed.starts_with("proxies:") || trimmed.starts_with("proxy-providers:") || trimmed.starts_with("port:") {
        return false;
    }
    if trimmed.starts_with('[') && trimmed.contains("\"remarks\"") && trimmed.contains("\"outbounds\"") {
        return true;
    }
    if trimmed.contains("vless://")
        || trimmed.contains("hy2://")
        || trimmed.contains("hysteria2://")
        || trimmed.contains("trojan://")
        || trimmed.contains("ss://")
        || trimmed.contains("vmess://")
    {
        return true;
    }
    if let Ok(bytes) = base64_decode(trimmed) {
        if let Ok(text) = std::str::from_utf8(&bytes) {
            let t = text.trim();
            if (t.starts_with('[') && t.contains("\"remarks\"") && t.contains("\"outbounds\""))
                || t.contains("vless://")
                || t.contains("hy2://")
                || t.contains("hysteria2://")
                || t.contains("trojan://")
                || t.contains("ss://")
                || t.contains("vmess://")
            {
                return true;
            }
        }
    }
    false
}

/// Извлечение параметров query-строки (key=val)
fn parse_query_params(query: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for part in query.split('&') {
        if let Some((k, v)) = part.split_once('=') {
            map.insert(k.trim().to_lowercase(), percent_decode(v.trim()));
        }
    }
    map
}

/// Парсинг хоста и порта из строки вида `host:port` или `[ipv6]:port`
fn parse_host_port(host_port: &str) -> (String, u16) {
    let hp = host_port.trim();
    if hp.starts_with('[') {
        if let Some(end_bracket) = hp.find(']') {
            let host = hp[1..end_bracket].to_string();
            let port = hp[end_bracket + 1..]
                .trim_start_matches(':')
                .parse::<u16>()
                .unwrap_or(443);
            return (host, port);
        }
    }
    if let Some((h, p)) = hp.rsplit_once(':') {
        let port = p.parse::<u16>().unwrap_or(443);
        (h.to_string(), port)
    } else {
        (hp.to_string(), 443)
    }
}

/// Парсинг ссылки VLESS с возвратом причины отклонения (баг 79).
pub fn parse_vless_uri_checked(uri: &str) -> Result<Value, String> {
    let after_scheme = uri
        .strip_prefix("vless://")
        .ok_or_else(|| "некорректная схема vless://".to_string())?;
    let (main_part, fragment) = match after_scheme.split_once('#') {
        Some((m, f)) => (m, percent_decode(f)),
        None => (after_scheme, String::new()),
    };

    let (auth_host, query_str) = match main_part.split_once('?') {
        Some((ah, q)) => (ah, q),
        None => (main_part, ""),
    };

    let (uuid, host_port) = auth_host
        .split_once('@')
        .ok_or_else(|| "отсутствует uuid@host в vless://".to_string())?;
    let (server, port) = parse_host_port(host_port);
    let params = parse_query_params(query_str);

    let server_name = if !fragment.is_empty() {
        fragment
    } else {
        format!("VLESS {server}:{port}")
    };

    let mut map = serde_json::Map::new();
    map.insert("name".into(), Value::String(server_name));
    map.insert("type".into(), Value::String("vless".into()));
    map.insert("server".into(), Value::String(server));
    map.insert("port".into(), json!(port));
    map.insert("uuid".into(), Value::String(percent_decode(uuid)));
    map.insert("udp".into(), Value::Bool(true));

    if let Some(flow) = params.get("flow").filter(|f| !f.is_empty()) {
        map.insert("flow".into(), Value::String(flow.clone()));
    }

    let security = params.get("security").map(String::as_str).unwrap_or("");
    if security == "reality" {
        map.insert("tls".into(), Value::Bool(true));
        if let Some(sni) = params.get("sni").or_else(|| params.get("servernames")) {
            map.insert("servername".into(), Value::String(sni.clone()));
        }
        if let Some(fp) = params.get("fp") {
            map.insert("client-fingerprint".into(), Value::String(fp.clone()));
        }
        let mut reality_opts = serde_json::Map::new();
        if let Some(pbk) = params.get("pbk") {
            reality_opts.insert("public-key".into(), Value::String(pbk.clone()));
        }
        if let Some(sid) = params.get("sid") {
            reality_opts.insert("short-id".into(), Value::String(sid.clone()));
        }
        map.insert("reality-opts".into(), Value::Object(reality_opts));
    } else if security == "tls" {
        map.insert("tls".into(), Value::Bool(true));
        if let Some(sni) = params.get("sni").or_else(|| params.get("servernames")) {
            map.insert("servername".into(), Value::String(sni.clone()));
        }
        if let Some(fp) = params.get("fp") {
            map.insert("client-fingerprint".into(), Value::String(fp.clone()));
        }
    }

    let network = params
        .get("type")
        .or_else(|| params.get("net"))
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "tcp".to_string());
    match network.as_str() {
        "" | "tcp" | "raw" => {}
        "ws" | "httpupgrade" => {
            map.insert("network".into(), Value::String("ws".into()));
            let mut ws_opts = serde_json::Map::new();
            if network == "httpupgrade" {
                ws_opts.insert("v2ray-http-upgrade".into(), Value::Bool(true));
            }
            if let Some(path) = params.get("path") {
                ws_opts.insert("path".into(), Value::String(path.clone()));
            }
            if let Some(host) = params.get("host") {
                let mut headers = serde_json::Map::new();
                headers.insert("Host".into(), Value::String(host.clone()));
                ws_opts.insert("headers".into(), Value::Object(headers));
            }
            map.insert("ws-opts".into(), Value::Object(ws_opts));
        }
        "grpc" => {
            map.insert("network".into(), Value::String("grpc".into()));
            let mut grpc_opts = serde_json::Map::new();
            if let Some(sn) = params.get("servicename") {
                grpc_opts.insert("grpc-service-name".into(), Value::String(sn.clone()));
            }
            map.insert("grpc-opts".into(), Value::Object(grpc_opts));
        }
        "xhttp" | "splithttp" => {
            map.insert("network".into(), Value::String("xhttp".into()));
            let mut xopts = serde_json::Map::new();
            if let Some(mode) = params.get("mode").filter(|v| !v.is_empty()) {
                xopts.insert("mode".into(), Value::String(mode.clone()));
            }
            if let Some(path) = params.get("path").filter(|v| !v.is_empty()) {
                xopts.insert("path".into(), Value::String(path.clone()));
            }
            if let Some(host) = params.get("host").filter(|v| !v.is_empty()) {
                xopts.insert("host".into(), Value::String(host.clone()));
            }
            map.insert("xhttp-opts".into(), Value::Object(xopts));
        }
        "h2" => {
            map.insert("network".into(), Value::String("h2".into()));
            let mut h2_opts = serde_json::Map::new();
            if let Some(path) = params.get("path").filter(|v| !v.is_empty()) {
                h2_opts.insert("path".into(), Value::String(path.clone()));
            }
            if let Some(host) = params.get("host").filter(|v| !v.is_empty()) {
                h2_opts.insert("host".into(), json!([host.clone()]));
            }
            map.insert("h2-opts".into(), Value::Object(h2_opts));
        }
        "http" => {
            map.insert("network".into(), Value::String("http".into()));
            let mut http_opts = serde_json::Map::new();
            if let Some(path) = params.get("path").filter(|v| !v.is_empty()) {
                http_opts.insert("path".into(), json!([path.clone()]));
            }
            if let Some(host) = params.get("host").filter(|v| !v.is_empty()) {
                let mut headers = serde_json::Map::new();
                headers.insert("Host".into(), json!([host.clone()]));
                http_opts.insert("headers".into(), Value::Object(headers));
            }
            map.insert("http-opts".into(), Value::Object(http_opts));
        }
        other => {
            return Err(format!("неподдерживаемый транспорт VLESS: {other}"));
        }
    }

    Ok(Value::Object(map))
}

/// Парсинг ссылки VLESS: `vless://uuid@host:port?params#name`
pub fn parse_vless_uri(uri: &str) -> Option<Value> {
    parse_vless_uri_checked(uri).ok()
}

/// Парсинг ссылки Hysteria2: `hy2://password@host:port?sni=...#name` или `hysteria2://...`
pub fn parse_hysteria2_uri(uri: &str) -> Option<Value> {
    let after_scheme = if let Some(s) = uri.strip_prefix("hy2://") {
        s
    } else {
        uri.strip_prefix("hysteria2://")?
    };

    let (main_part, fragment) = match after_scheme.split_once('#') {
        Some((m, f)) => (m, percent_decode(f)),
        None => (after_scheme, String::new()),
    };

    let (auth_host, query_str) = match main_part.split_once('?') {
        Some((ah, q)) => (ah, q),
        None => (main_part, ""),
    };

    let (password, host_port) = auth_host.split_once('@')?;
    let (server, port) = parse_host_port(host_port);
    let params = parse_query_params(query_str);

    let server_name = if !fragment.is_empty() {
        fragment
    } else {
        format!("Hysteria2 {server}:{port}")
    };

    let mut map = serde_json::Map::new();
    map.insert("name".into(), Value::String(server_name));
    map.insert("type".into(), Value::String("hysteria2".into()));
    map.insert("server".into(), Value::String(server));
    map.insert("port".into(), json!(port));
    map.insert("password".into(), Value::String(percent_decode(password)));
    map.insert("udp".into(), Value::Bool(true));

    if let Some(sni) = params.get("sni") {
        map.insert("sni".into(), Value::String(sni.clone()));
    }
    if let Some(insecure) = params.get("insecure") {
        map.insert("skip-cert-verify".into(), Value::Bool(insecure == "1" || insecure == "true"));
    }
    if let Some(obfs) = params.get("obfs").filter(|v| !v.is_empty()) {
        map.insert("obfs".into(), Value::String(obfs.clone()));
    }
    if let Some(obfs_pw) = params.get("obfs-password").filter(|v| !v.is_empty()) {
        map.insert("obfs-password".into(), Value::String(obfs_pw.clone()));
    }
    if let Some(mport) = params.get("mport").filter(|v| !v.is_empty()) {
        map.insert("ports".into(), Value::String(mport.clone()));
    }

    Some(Value::Object(map))
}

/// Парсинг ссылки Trojan с возвратом причины отклонения (баг 79).
pub fn parse_trojan_uri_checked(uri: &str) -> Result<Value, String> {
    let after_scheme = uri
        .strip_prefix("trojan://")
        .ok_or_else(|| "некорректная схема trojan://".to_string())?;
    let (main_part, fragment) = match after_scheme.split_once('#') {
        Some((m, f)) => (m, percent_decode(f)),
        None => (after_scheme, String::new()),
    };

    let (auth_host, query_str) = match main_part.split_once('?') {
        Some((ah, q)) => (ah, q),
        None => (main_part, ""),
    };

    let (password, host_port) = auth_host
        .split_once('@')
        .ok_or_else(|| "отсутствует password@host в trojan://".to_string())?;
    let (server, port) = parse_host_port(host_port);
    let params = parse_query_params(query_str);

    let server_name = if !fragment.is_empty() {
        fragment
    } else {
        format!("Trojan {server}:{port}")
    };

    let mut map = serde_json::Map::new();
    map.insert("name".into(), Value::String(server_name));
    map.insert("type".into(), Value::String("trojan".into()));
    map.insert("server".into(), Value::String(server));
    map.insert("port".into(), json!(port));
    map.insert("password".into(), Value::String(percent_decode(password)));
    map.insert("udp".into(), Value::Bool(true));

    if let Some(sni) = params.get("sni") {
        map.insert("sni".into(), Value::String(sni.clone()));
    }
    if let Some(fp) = params.get("fp").filter(|v| !v.is_empty()) {
        map.insert("client-fingerprint".into(), Value::String(fp.clone()));
    }
    if let Some(insecure) = params.get("allowinsecure").or_else(|| params.get("insecure")) {
        map.insert("skip-cert-verify".into(), Value::Bool(insecure == "1" || insecure == "true"));
    }

    let network = params
        .get("type")
        .or_else(|| params.get("net"))
        .map(|s| s.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "tcp".to_string());
    match network.as_str() {
        "" | "tcp" | "raw" => {}
        "ws" => {
            map.insert("network".into(), Value::String("ws".into()));
            let mut ws_opts = serde_json::Map::new();
            if let Some(path) = params.get("path") {
                ws_opts.insert("path".into(), Value::String(path.clone()));
            }
            if let Some(host) = params.get("host") {
                let mut headers = serde_json::Map::new();
                headers.insert("Host".into(), Value::String(host.clone()));
                ws_opts.insert("headers".into(), Value::Object(headers));
            }
            map.insert("ws-opts".into(), Value::Object(ws_opts));
        }
        "grpc" => {
            map.insert("network".into(), Value::String("grpc".into()));
            let mut grpc_opts = serde_json::Map::new();
            if let Some(sn) = params.get("servicename") {
                grpc_opts.insert("grpc-service-name".into(), Value::String(sn.clone()));
            }
            map.insert("grpc-opts".into(), Value::Object(grpc_opts));
        }
        other => {
            return Err(format!("неподдерживаемый транспорт Trojan: {other}"));
        }
    }

    Ok(Value::Object(map))
}

/// Парсинг ссылки Trojan: `trojan://password@host:port?sni=...#name`
pub fn parse_trojan_uri(uri: &str) -> Option<Value> {
    parse_trojan_uri_checked(uri).ok()
}

/// Парсинг ссылки Shadowsocks с возвратом причины отклонения (баг 79).
pub fn parse_ss_uri_checked(uri: &str) -> Result<Value, String> {
    let after_scheme = uri
        .strip_prefix("ss://")
        .ok_or_else(|| "некорректная схема ss://".to_string())?;
    let (main_part, fragment) = match after_scheme.split_once('#') {
        Some((m, f)) => (m, percent_decode(f)),
        None => (after_scheme, String::new()),
    };

    let (auth_host, query_str) = match main_part.split_once('?') {
        Some((ah, q)) => (ah, q),
        None => (main_part, ""),
    };
    let params = parse_query_params(query_str);

    // 1) SIP002: ss://BASE64(method:password)@host:port или plain SIP002 / SS-2022 ss://method:key@host:port
    let (server, port, cipher, password) = if let Some((user_info, host_port)) = auth_host.split_once('@') {
        let user_decoded_plain = percent_decode(user_info);
        let (cipher, pass) = if let Some((c, p)) = user_decoded_plain.split_once(':') {
            (c.to_string(), p.to_string())
        } else {
            let decoded = base64_decode(user_info)
                .ok()
                .and_then(|b| String::from_utf8(b).ok())
                .ok_or_else(|| "некорректный userinfo в ss://".to_string())?;
            let (c, p) = decoded
                .split_once(':')
                .ok_or_else(|| "отсутствует cipher:password в ss://".to_string())?;
            (c.to_string(), p.to_string())
        };
        let (s, p) = parse_host_port(host_port);
        (s, p, cipher, pass)
    } else {
        // 2) Legacy: ss://BASE64(method:password@host:port)
        let decoded = base64_decode(auth_host)
            .ok()
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or_else(|| "некорректный base64 в ss://".to_string())?;
        let (cipher_pass, host_port) = decoded
            .split_once('@')
            .ok_or_else(|| "отсутствует @host в legacy ss://".to_string())?;
        let (cipher, pass) = cipher_pass
            .split_once(':')
            .ok_or_else(|| "отсутствует cipher:password в legacy ss://".to_string())?;
        let (s, p) = parse_host_port(host_port);
        (s, p, cipher.to_string(), pass.to_string())
    };

    let server_name = if !fragment.is_empty() {
        fragment
    } else {
        format!("Shadowsocks {server}:{port}")
    };

    let mut map = serde_json::Map::new();
    map.insert("name".into(), Value::String(server_name));
    map.insert("type".into(), Value::String("ss".into()));
    map.insert("server".into(), Value::String(server));
    map.insert("port".into(), json!(port));
    map.insert("cipher".into(), Value::String(cipher));
    map.insert("password".into(), Value::String(password));
    map.insert("udp".into(), Value::Bool(true));

    if let Some(plugin_raw) = params.get("plugin").filter(|v| !v.is_empty()) {
        let mut parts = plugin_raw.split(';');
        let plugin_name = parts.next().unwrap_or("").trim();
        match plugin_name {
            "obfs-local" | "simple-obfs" | "obfs" => {
                map.insert("plugin".into(), Value::String("obfs".into()));
                let mut popts = serde_json::Map::new();
                for kv in parts {
                    if let Some((k, v)) = kv.split_once('=') {
                        match k.trim() {
                            "obfs" => {
                                popts.insert("mode".into(), Value::String(v.trim().to_string()));
                            }
                            "obfs-host" => {
                                popts.insert("host".into(), Value::String(v.trim().to_string()));
                            }
                            _ => {}
                        }
                    }
                }
                map.insert("plugin-opts".into(), Value::Object(popts));
            }
            other => {
                return Err(format!("неподдерживаемый плагин Shadowsocks: {other}"));
            }
        }
    }

    Ok(Value::Object(map))
}

/// Парсинг ссылки Shadowsocks: `ss://BASE64@host:port#name` или `ss://BASE64#name`
pub fn parse_ss_uri(uri: &str) -> Option<Value> {
    parse_ss_uri_checked(uri).ok()
}

/// Парсинг ссылки VMess: `vmess://BASE64(JSON)`
pub fn parse_vmess_uri(uri: &str) -> Option<Value> {
    let b64 = uri.strip_prefix("vmess://")?;
    let decoded_bytes = base64_decode(b64).ok()?;
    let json_val: Value = serde_json::from_slice(&decoded_bytes).ok()?;

    let name = json_val.get("ps").and_then(|v| v.as_str()).unwrap_or("VMess Server").trim().to_string();
    let server = json_val.get("add").and_then(|v| v.as_str())?.trim().to_string();
    let port_num = if let Some(p) = json_val.get("port") {
        if let Some(n) = p.as_u64() {
            n as u16
        } else if let Some(s) = p.as_str() {
            s.parse::<u16>().unwrap_or(443)
        } else {
            443
        }
    } else {
        443
    };
    let uuid = json_val.get("id").and_then(|v| v.as_str())?.trim().to_string();
    let alter_id = json_val.get("aid").and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))).unwrap_or(0);
    let cipher = json_val.get("scy").and_then(|v| v.as_str()).unwrap_or("auto").to_string();
    let net = json_val.get("net").and_then(|v| v.as_str()).unwrap_or("tcp");
    let tls = json_val.get("tls").and_then(|v| v.as_str()).map(|s| s == "tls").unwrap_or(false);
    let sni = json_val.get("sni").and_then(|v| v.as_str()).or_else(|| json_val.get("host").and_then(|v| v.as_str())).unwrap_or("");

    let mut map = serde_json::Map::new();
    map.insert("name".into(), Value::String(name));
    map.insert("type".into(), Value::String("vmess".into()));
    map.insert("server".into(), Value::String(server));
    map.insert("port".into(), json!(port_num));
    map.insert("uuid".into(), Value::String(uuid));
    map.insert("alterId".into(), json!(alter_id));
    map.insert("cipher".into(), Value::String(cipher));
    map.insert("udp".into(), Value::Bool(true));

    if tls {
        map.insert("tls".into(), Value::Bool(true));
        if !sni.is_empty() {
            map.insert("servername".into(), Value::String(sni.to_string()));
        }
    }

    if net == "ws" {
        map.insert("network".into(), Value::String("ws".into()));
        let mut ws_opts = serde_json::Map::new();
        if let Some(path) = json_val.get("path").and_then(|v| v.as_str()) {
            if !path.is_empty() {
                ws_opts.insert("path".into(), Value::String(path.to_string()));
            }
        }
        if let Some(host) = json_val.get("host").and_then(|v| v.as_str()) {
            if !host.is_empty() {
                let mut headers = serde_json::Map::new();
                headers.insert("Host".into(), Value::String(host.to_string()));
                ws_opts.insert("headers".into(), Value::Object(headers));
            }
        }
        map.insert("ws-opts".into(), Value::Object(ws_opts));
    } else if net == "grpc" {
        map.insert("network".into(), Value::String("grpc".into()));
        if let Some(path) = json_val.get("path").and_then(|v| v.as_str()) {
            let mut grpc_opts = serde_json::Map::new();
            grpc_opts.insert("grpc-service-name".into(), Value::String(path.to_string()));
            map.insert("grpc-opts".into(), Value::Object(grpc_opts));
        }
    }

    Some(Value::Object(map))
}

/// Парсинг отдельной URI-ссылки в объект узла Mihomo с причиной отклонения (баг 79).
pub fn parse_v2ray_line_checked(line: &str) -> Result<Value, String> {
    let trimmed = line.trim();
    if trimmed.starts_with("vless://") {
        parse_vless_uri_checked(trimmed)
    } else if trimmed.starts_with("hy2://") || trimmed.starts_with("hysteria2://") {
        parse_hysteria2_uri(trimmed).ok_or_else(|| "некорректная ссылка hysteria2://".to_string())
    } else if trimmed.starts_with("trojan://") {
        parse_trojan_uri_checked(trimmed)
    } else if trimmed.starts_with("ss://") {
        parse_ss_uri_checked(trimmed)
    } else if trimmed.starts_with("vmess://") {
        parse_vmess_uri(trimmed).ok_or_else(|| "некорректная ссылка vmess://".to_string())
    } else {
        Err("неподдерживаемый протокол/схема".to_string())
    }
}

/// Парсинг отдельной URI-ссылки в объект узла Mihomo
pub fn parse_v2ray_line(line: &str) -> Option<Value> {
    parse_v2ray_line_checked(line).ok()
}

///// Парсинг одного профиля из Xray/V2Ray JSON-массива подписки
pub fn parse_xray_json_profile(profile: &Value) -> Option<Value> {
    let remarks = profile.get("remarks").and_then(|r| r.as_str())?.trim();
    if remarks.is_empty() {
        return None;
    }
    let outbounds = profile.get("outbounds").and_then(|o| o.as_array())?;
    // Ищем первый исходящий узел, который не является direct/freedom/blackhole
    let out = outbounds.iter().find(|o| {
        let proto = o.get("protocol").and_then(|p| p.as_str()).unwrap_or("");
        proto != "freedom" && proto != "blackhole" && proto != "direct" && proto != "block"
    })?;

    let proto = out.get("protocol").and_then(|p| p.as_str())?;
    let settings = out.get("settings").unwrap_or(&Value::Null);
    let stream_settings = out.get("streamSettings").unwrap_or(&Value::Null);

    let mut map = serde_json::Map::new();
    map.insert("name".into(), Value::String(remarks.to_string()));

    match proto {
        "vless" => {
            let vnext = settings.get("vnext").and_then(|v| v.as_array())?.first()?;
            let server = vnext.get("address").and_then(|a| a.as_str())?;
            let port = vnext.get("port").and_then(|p| p.as_u64())?;
            let user = vnext.get("users").and_then(|u| u.as_array())?.first()?;
            let uuid = user.get("id").and_then(|i| i.as_str())?;
            let flow = user.get("flow").and_then(|f| f.as_str()).unwrap_or("");

            map.insert("type".into(), Value::String("vless".into()));
            map.insert("server".into(), Value::String(server.to_string()));
            map.insert("port".into(), json!(port));
            map.insert("uuid".into(), Value::String(percent_decode(uuid)));
            map.insert("udp".into(), Value::Bool(true));
            if !flow.is_empty() {
                map.insert("flow".into(), Value::String(flow.to_string()));
            }

            let network = stream_settings.get("network").and_then(|n| n.as_str()).unwrap_or("tcp");
            if network != "tcp" {
                map.insert("network".into(), Value::String(network.to_string()));
            }

            let security = stream_settings.get("security").and_then(|s| s.as_str()).unwrap_or("");
            if security == "reality" {
                map.insert("tls".into(), Value::Bool(true));
                if let Some(reality) = stream_settings.get("realitySettings").and_then(|r| r.as_object()) {
                    if let Some(sn) = reality.get("serverName").and_then(|s| s.as_str()) {
                        map.insert("servername".into(), Value::String(sn.to_string()));
                    }
                    if let Some(fp) = reality.get("fingerprint").and_then(|f| f.as_str()) {
                        map.insert("client-fingerprint".into(), Value::String(fp.to_string()));
                    }
                    let mut ropts = serde_json::Map::new();
                    if let Some(pbk) = reality.get("publicKey").and_then(|p| p.as_str()) {
                        ropts.insert("public-key".into(), Value::String(pbk.to_string()));
                    }
                    if let Some(sid) = reality.get("shortId").and_then(|s| s.as_str()) {
                        ropts.insert("short-id".into(), Value::String(sid.to_string()));
                    }
                    map.insert("reality-opts".into(), Value::Object(ropts));
                }
            } else if security == "tls" {
                map.insert("tls".into(), Value::Bool(true));
                if let Some(tls) = stream_settings.get("tlsSettings").and_then(|t| t.as_object()) {
                    if let Some(sn) = tls.get("serverName").and_then(|s| s.as_str()) {
                        map.insert("servername".into(), Value::String(sn.to_string()));
                    }
                    if let Some(fp) = tls.get("fingerprint").and_then(|f| f.as_str()) {
                        map.insert("client-fingerprint".into(), Value::String(fp.to_string()));
                    }
                    if let Some(alpn) = tls.get("alpn").and_then(|a| a.as_array()) {
                        map.insert("alpn".into(), Value::Array(alpn.clone()));
                    }
                }
            }

            if network == "xhttp" {
                if let Some(xh) = stream_settings.get("xhttpSettings").and_then(|x| x.as_object()) {
                    let mut xopts = serde_json::Map::new();
                    if let Some(mode) = xh.get("mode").and_then(|m| m.as_str()) {
                        xopts.insert("mode".into(), Value::String(mode.to_string()));
                    }
                    if let Some(path) = xh.get("path").and_then(|p| p.as_str()) {
                        xopts.insert("path".into(), Value::String(path.to_string()));
                    }
                    if let Some(host) = xh.get("host").and_then(|h| h.as_str()) {
                        xopts.insert("host".into(), Value::String(host.to_string()));
                    }
                    map.insert("xhttp-opts".into(), Value::Object(xopts));
                }
            } else if network == "grpc" {
                if let Some(grpc) = stream_settings.get("grpcSettings").and_then(|g| g.as_object()) {
                    let mut gopts = serde_json::Map::new();
                    if let Some(sn) = grpc.get("serviceName").and_then(|s| s.as_str()) {
                        gopts.insert("grpc-service-name".into(), Value::String(sn.to_string()));
                    }
                    map.insert("grpc-opts".into(), Value::Object(gopts));
                }
            } else if network == "ws" {
                if let Some(ws) = stream_settings.get("wsSettings").and_then(|w| w.as_object()) {
                    let mut wopts = serde_json::Map::new();
                    if let Some(path) = ws.get("path").and_then(|p| p.as_str()) {
                        wopts.insert("path".into(), Value::String(path.to_string()));
                    }
                    if let Some(headers) = ws.get("headers").and_then(|h| h.as_object()) {
                        wopts.insert("headers".into(), Value::Object(headers.clone()));
                    }
                    map.insert("ws-opts".into(), Value::Object(wopts));
                }
            }
            Some(Value::Object(map))
        }
        "hysteria" | "hysteria2" => {
            let server = settings.get("address").and_then(|a| a.as_str())?;
            let port = settings.get("port").and_then(|p| p.as_u64())?;
            let auth = stream_settings
                .get("hysteriaSettings")
                .and_then(|h| h.get("auth"))
                .and_then(|a| a.as_str())
                .unwrap_or("");
            let sni = stream_settings
                .get("tlsSettings")
                .and_then(|t| t.get("serverName"))
                .and_then(|s| s.as_str())
                .unwrap_or(server);

            map.insert("type".into(), Value::String("hysteria2".into()));
            map.insert("server".into(), Value::String(server.to_string()));
            map.insert("port".into(), json!(port));
            map.insert("password".into(), Value::String(auth.to_string()));
            map.insert("sni".into(), Value::String(sni.to_string()));
            map.insert("alpn".into(), json!(["h3"]));
            map.insert("skip-cert-verify".into(), Value::Bool(false));
            Some(Value::Object(map))
        }
        "trojan" => {
            let srv = settings.get("servers").and_then(|s| s.as_array())?.first()?;
            let server = srv.get("address").and_then(|a| a.as_str())?;
            let port = srv.get("port").and_then(|p| p.as_u64())?;
            let pass = srv.get("password").and_then(|p| p.as_str())?;

            map.insert("type".into(), Value::String("trojan".into()));
            map.insert("server".into(), Value::String(server.to_string()));
            map.insert("port".into(), json!(port));
            map.insert("password".into(), Value::String(pass.to_string()));
            map.insert("udp".into(), Value::Bool(true));
            Some(Value::Object(map))
        }
        _ => None,
    }
}

/// Преобразование сырого контента подписки (Base64, Xray JSON или список URI) в YAML провайдера для Mihomo
/// YAML-скаляр в двойных кавычках с экранированием (баг 77): `\`, `"`, переводы строк и управляющие символы.
pub fn yaml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c == '\u{7f}' => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Простые безопасные значения (vless, aes-256-gcm, h2) выводятся без кавычек, остальные — через yaml_quote.
fn yaml_scalar(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    let reserved = matches!(lower.as_str(), "true" | "false" | "yes" | "no" | "on" | "off" | "null" | "~" | "y" | "n");
    let plain = !s.is_empty()
        && !reserved
        && s.chars().next().is_some_and(|c| c.is_ascii_alphabetic())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.');
    if plain { s.to_string() } else { yaml_quote(s) }
}

fn yaml_key(k: &str) -> String {
    if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') {
        k.to_string()
    } else {
        yaml_quote(k)
    }
}

fn emit_yaml_entry(out: &mut String, indent: usize, key: &str, v: &Value) {
    let pad = " ".repeat(indent);
    let k = yaml_key(key);
    match v {
        Value::Null => {}
        Value::Bool(b) => out.push_str(&format!("{pad}{k}: {b}\n")),
        Value::Number(n) => out.push_str(&format!("{pad}{k}: {n}\n")),
        // Перечисления (type/network/cipher) — без кавычек, как в прежнем выводе; прочие строки — в кавычках
        Value::String(s) if matches!(key, "type" | "network" | "cipher") => {
            out.push_str(&format!("{pad}{k}: {}\n", yaml_scalar(s)))
        }
        Value::String(s) => out.push_str(&format!("{pad}{k}: {}\n", yaml_quote(s))),
        Value::Array(arr) => {
            if arr.is_empty() {
                out.push_str(&format!("{pad}{k}: []\n"));
                return;
            }
            out.push_str(&format!("{pad}{k}:\n"));
            for item in arr {
                match item {
                    Value::String(s) => out.push_str(&format!("{pad}  - {}\n", yaml_scalar(s))),
                    Value::Number(n) => out.push_str(&format!("{pad}  - {n}\n")),
                    Value::Bool(b) => out.push_str(&format!("{pad}  - {b}\n")),
                    other => out.push_str(&format!("{pad}  - {}\n", yaml_quote(&other.to_string()))),
                }
            }
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str(&format!("{pad}{k}: {{}}\n"));
                return;
            }
            out.push_str(&format!("{pad}{k}:\n"));
            for (ck, cv) in map {
                emit_yaml_entry(out, indent + 2, ck, cv);
            }
        }
    }
}

pub fn convert_subscription_to_mihomo_yaml(raw_content: &str) -> Result<(String, usize), String> {
    let (yaml, count, rejected) = convert_subscription_to_mihomo_yaml_with_report(raw_content)?;
    for (node_hint, reason) in &rejected {
        crate::log_w!("[SUB-PARSER] Узел отклонён ({reason}): {node_hint}");
    }
    Ok((yaml, count))
}

/// Преобразование подписки в YAML с возвратом списка отклонённых узлов `(строка/имя, причина)` (баг 79).
pub fn convert_subscription_to_mihomo_yaml_with_report(
    raw_content: &str,
) -> Result<(String, usize, Vec<(String, String)>), String> {
    let decoded_text = if let Ok(bytes) = base64_decode(raw_content.trim()) {
        if let Ok(text) = String::from_utf8(bytes) {
            let t = text.trim();
            if t.contains("://") || (t.starts_with('[') && t.contains("\"remarks\"")) {
                text
            } else {
                raw_content.to_string()
            }
        } else {
            raw_content.to_string()
        }
    } else {
        raw_content.to_string()
    };

    let mut proxies = Vec::new();
    let mut rejected: Vec<(String, String)> = Vec::new();
    let trimmed = decoded_text.trim();

    // 1. Попытка распарсить как Xray/V2Ray JSON-массив подписки
    if trimmed.starts_with('[') {
        if let Ok(profiles) = serde_json::from_str::<Vec<Value>>(trimmed) {
            for p in &profiles {
                if let Some(proxy_val) = parse_xray_json_profile(p) {
                    proxies.push(proxy_val);
                } else {
                    let hint = p
                        .get("remarks")
                        .and_then(|r| r.as_str())
                        .unwrap_or("xray-json-profile")
                        .to_string();
                    rejected.push((hint, "неподдерживаемый или неполный Xray JSON-профиль".to_string()));
                }
            }
        }
    }

    // 2. Если не JSON, парсим построчно (VLESS/Hysteria2/Trojan/SS/VMess URI ссылки)
    if proxies.is_empty() {
        for line in decoded_text.lines() {
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() || trimmed_line.starts_with('#') || trimmed_line.starts_with("//") {
                continue;
            }
            match parse_v2ray_line_checked(trimmed_line) {
                Ok(proxy_val) => proxies.push(proxy_val),
                Err(reason) => {
                    let label = trimmed_line
                        .split_once('#')
                        .map(|(_, f)| percent_decode(f))
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| trimmed_line.chars().take(48).collect());
                    rejected.push((label, reason));
                }
            }
        }
    }

    if proxies.is_empty() {
        return Err("Не удалось распарсить ни одного сервера из подписки V2Ray/Base64/JSON".into());
    }

    let count = proxies.len();
    // Баги 76, 77, 79: узел выводится целиком (cipher, alterId, транспорты и прочие поля парсера),
    // а каждая строка — экранированным YAML-скаляром.
    let mut yaml = String::from("proxies:\n");
    for p in proxies {
        let obj = p.as_object().ok_or("Некорректный узел")?;
        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("Server");
        yaml.push_str(&format!("  - name: {}\n", yaml_quote(name)));
        let mut keys: Vec<&String> = obj.keys().filter(|k| k.as_str() != "name").collect();
        let rank = |k: &str| match k {
            "type" => 0,
            "server" => 1,
            "port" => 2,
            _ => 3,
        };
        keys.sort_by(|x, y| rank(x).cmp(&rank(y)).then_with(|| x.cmp(y)));
        for k in keys {
            emit_yaml_entry(&mut yaml, 4, k, &obj[k.as_str()]);
        }
        if !obj.contains_key("udp") {
            yaml.push_str("    udp: true\n");
        }
    }

    Ok((yaml, count, rejected))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_percent_decode_emojis_and_spaces() {
        let encoded = "%F0%9F%87%A9%F0%9F%87%AA%20%D0%9E%D0%B1%D1%85%D0%BE%D0%B4%20%5BPlatinum%5D%20-%20%D0%93%D0%B5%D1%80%D0%BC%D0%B0%D0%BD%D0%B8%D1%8F";
        let decoded = percent_decode(encoded);
        assert_eq!(decoded, "🇩🇪 Обход [Platinum] - Германия");
    }

    #[test]
    fn test_parse_vless_reality_with_exact_name() {
        let uri = "vless://a1b2c3d4-e5f6-7890-abcd-ef1234567890@185.120.45.67:443?security=reality&sni=google.com&fp=chrome&pbk=test_pubkey&sid=1234abcd&flow=xtls-rprx-vision#%F0%9F%87%A9%F0%9F%87%AA%20%D0%9E%D0%B1%D1%85%D0%BE%D0%B4%20%5BPlatinum%5D%20-%20%D0%93%D0%B5%D1%80%D0%BC%D0%B0%D0%BD%D0%B8%D1%8F";
        let res = parse_vless_uri(uri).expect("Must parse VLESS URI");
        assert_eq!(res["name"], "🇩🇪 Обход [Platinum] - Германия");
        assert_eq!(res["type"], "vless");
        assert_eq!(res["server"], "185.120.45.67");
        assert_eq!(res["port"], 443);
        assert_eq!(res["uuid"], "a1b2c3d4-e5f6-7890-abcd-ef1234567890");
        assert_eq!(res["tls"], true);
        assert_eq!(res["servername"], "google.com");
        assert_eq!(res["reality-opts"]["public-key"], "test_pubkey");
        assert_eq!(res["reality-opts"]["short-id"], "1234abcd");
    }

    #[test]
    fn test_parse_hysteria2_with_emoji() {
        let uri = "hy2://secret123@95.163.220.10:8443?sni=fast.cdn.net&insecure=1#%F0%9F%87%B5%F0%9F%87%B1%20%E2%9A%A1%D0%9E%D0%B1%D1%85%D0%BE%D0%B4%20%5BGold%5D%20-%20%D0%9F%D0%BE%D0%BB%D1%8C%D1%88%D0%B0";
        let res = parse_hysteria2_uri(uri).expect("Must parse Hysteria2 URI");
        assert_eq!(res["name"], "🇵🇱 ⚡Обход [Gold] - Польша");
        assert_eq!(res["type"], "hysteria2");
        assert_eq!(res["server"], "95.163.220.10");
        assert_eq!(res["port"], 8443);
        assert_eq!(res["password"], "secret123");
        assert_eq!(res["sni"], "fast.cdn.net");
        assert_eq!(res["skip-cert-verify"], true);
    }

    #[test]
    fn test_convert_base64_subscription_to_mihomo_yaml() {
        let vless = "vless://uuid-1@1.2.3.4:443?security=reality&sni=cdn.com#%F0%9F%87%A9%F0%9F%87%AA%20DE";
        let hy2 = "hy2://pass-2@5.6.7.8:8443#%F0%9F%87%B3%F0%9F%87%B1%20NL";
        let combined = format!("{vless}\n{hy2}\n");
        let b64 = crate::adguard::base64_encode(combined.as_bytes());

        let (yaml, count) = convert_subscription_to_mihomo_yaml(&b64).expect("Conversion must succeed");
        assert_eq!(count, 2);
        assert!(yaml.contains("🇩🇪 DE"));
        assert!(yaml.contains("🇳🇱 NL"));
        assert!(yaml.contains("type: vless"));
        assert!(yaml.contains("type: hysteria2"));
    }

    #[test]
    fn test_parse_xray_json_subscription_array() {
        let json_data = r#"[
            {
                "remarks": "🇵🇱 ⚡Обход  [Gold] - Польша",
                "outbounds": [
                    {
                        "tag": "proxy",
                        "protocol": "vless",
                        "settings": {
                            "vnext": [{
                                "address": "gateway.console.pc-club.app",
                                "port": 443,
                                "users": [{"id": "a0d4c6fb-0057-4ff5-ac6a-80694a4b5c41", "encryption": "none", "flow": ""}]
                            }]
                        },
                        "streamSettings": {
                            "network": "xhttp",
                            "xhttpSettings": {
                                "mode": "packet-up",
                                "host": "gateway.console.pc-club.app",
                                "path": "/gateway/v1/ws/24e3db84acda637a"
                            },
                            "security": "tls",
                            "tlsSettings": {
                                "serverName": "gateway.console.pc-club.app",
                                "fingerprint": "chrome",
                                "alpn": ["h2"]
                            }
                        }
                    },
                    {"tag": "direct", "protocol": "freedom"}
                ]
            },
            {
                "remarks": "🇷🇺 ⚡ Обход [Cooper] - Россия",
                "outbounds": [
                    {
                        "tag": "proxy",
                        "protocol": "hysteria",
                        "settings": {
                            "address": "eu88.vaultberry.cloud",
                            "port": 8448,
                            "version": 2
                        },
                        "streamSettings": {
                            "network": "hysteria",
                            "hysteriaSettings": {
                                "version": 2,
                                "auth": "secret-auth-key-123"
                            },
                            "security": "tls",
                            "tlsSettings": {
                                "serverName": "eu88.vaultberry.cloud",
                                "fingerprint": "firefox",
                                "alpn": ["h3"]
                            }
                        }
                    },
                    {"tag": "direct", "protocol": "freedom"}
                ]
            }
        ]"#;

        assert!(is_v2ray_or_base64_subscription(json_data));
        let (yaml, count) = convert_subscription_to_mihomo_yaml(json_data).expect("Must convert JSON subscription array");
        assert_eq!(count, 2);
        assert!(yaml.contains("🇵🇱 ⚡Обход  [Gold] - Польша"));
        assert!(yaml.contains("type: vless"));
        assert!(yaml.contains("network: xhttp"));
        assert!(yaml.contains("mode: \"packet-up\""));
        assert!(yaml.contains("path: \"/gateway/v1/ws/24e3db84acda637a\""));

        assert!(yaml.contains("🇷🇺 ⚡ Обход [Cooper] - Россия"));
        assert!(yaml.contains("type: hysteria2"));
        assert!(yaml.contains("server: \"eu88.vaultberry.cloud\""));
        assert!(yaml.contains("port: 8448"));
        assert!(yaml.contains("password: \"secret-auth-key-123\""));
        assert!(yaml.contains("sni: \"eu88.vaultberry.cloud\""));
    }

    #[test]
    fn test_parse_real_geodema_subscription() {
        if let Ok(data) = std::fs::read_to_string("../geodema2_sub.json") {
            let (yaml, count) = convert_subscription_to_mihomo_yaml(&data).expect("Must parse real JSON");
            assert_eq!(count, 13, "Real geodema subscription must parse exactly 13 servers");
            assert!(yaml.contains("🇪🇺Авто Обход | Самый быстрый"));
            assert!(yaml.contains("🇵🇱 ⚡Обход  [Gold] - Польша"));
            assert!(yaml.contains("🇷🇺 Обход [Cooper] - Россия"));
            assert!(yaml.contains("Silver — Германия"));
        }
    }

    #[test]
    fn test_parse_real_main_geodema_subscription() {
        if let Ok(data) = std::fs::read_to_string("../sub_geodema.json") {
            let (yaml, count) = convert_subscription_to_mihomo_yaml(&data).expect("Must parse real main JSON");
            assert!(count > 0, "Real main geodema subscription must parse servers");
            assert!(yaml.contains("proxies:"));
        }
    }

    #[test]
    fn test_parse_ss_sip002() {
        // chacha20-ietf-poly1305:mypassword -> base64 = Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTpteXBhc3N3b3Jk
        let uri = "ss://Y2hhY2hhMjAtaWV0Zi1wb2x5MTMwNTpteXBhc3N3b3Jk@198.51.100.1:8388#%F0%9F%87%A9%F0%9F%87%AA%20Shadowsocks%20Node";
        let val = parse_v2ray_line(uri).expect("Must parse SIP002 ss URI");
        assert_eq!(val.get("type").and_then(|v| v.as_str()), Some("ss"));
        assert_eq!(val.get("server").and_then(|v| v.as_str()), Some("198.51.100.1"));
        assert_eq!(val.get("port").and_then(|v| v.as_u64()), Some(8388));
        assert_eq!(val.get("cipher").and_then(|v| v.as_str()), Some("chacha20-ietf-poly1305"));
        assert_eq!(val.get("password").and_then(|v| v.as_str()), Some("mypassword"));
        assert_eq!(val.get("name").and_then(|v| v.as_str()), Some("🇩🇪 Shadowsocks Node"));
    }

    #[test]
    fn test_parse_ss_legacy() {
        // aes-256-gcm:pass123@1.2.3.4:9000 -> base64 = YWVzLTI1Ni1nY206cGFzczEyM0AxLjIuMy40OjkwMDA=
        let uri = "ss://YWVzLTI1Ni1nY206cGFzczEyM0AxLjIuMy40OjkwMDA=#Legacy%20SS";
        let val = parse_v2ray_line(uri).expect("Must parse legacy ss URI");
        assert_eq!(val.get("type").and_then(|v| v.as_str()), Some("ss"));
        assert_eq!(val.get("server").and_then(|v| v.as_str()), Some("1.2.3.4"));
        assert_eq!(val.get("port").and_then(|v| v.as_u64()), Some(9000));
        assert_eq!(val.get("cipher").and_then(|v| v.as_str()), Some("aes-256-gcm"));
        assert_eq!(val.get("password").and_then(|v| v.as_str()), Some("pass123"));
        assert_eq!(val.get("name").and_then(|v| v.as_str()), Some("Legacy SS"));
    }

    #[test]
    fn test_parse_vmess_uri() {
        let vmess_json = r#"{"v":"2","ps":"🇳🇱 Netherlands VMess","add":"nl.example.com","port":443,"id":"b831381d-6324-4d53-ad4f-8cda48b30811","aid":0,"scy":"auto","net":"ws","tls":"tls","sni":"nl.example.com","path":"/vmess-path","host":"nl.example.com"}"#;
        // encode base64
        let b64 = {
            let mut buf = String::new();
            let bytes = vmess_json.as_bytes();
            for chunk in bytes.chunks(3) {
                let b0 = chunk[0];
                let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
                let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };
                let lookup = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
                buf.push(lookup[(b0 >> 2) as usize] as char);
                buf.push(lookup[(((b0 & 3) << 4) | (b1 >> 4)) as usize] as char);
                if chunk.len() > 1 {
                    buf.push(lookup[(((b1 & 15) << 2) | (b2 >> 6)) as usize] as char);
                } else {
                    buf.push('=');
                }
                if chunk.len() > 2 {
                    buf.push(lookup[(b2 & 63) as usize] as char);
                } else {
                    buf.push('=');
                }
            }
            buf
        };
        let uri = format!("vmess://{b64}");
        let val = parse_v2ray_line(&uri).expect("Must parse vmess URI");
        assert_eq!(val.get("type").and_then(|v| v.as_str()), Some("vmess"));
        assert_eq!(val.get("server").and_then(|v| v.as_str()), Some("nl.example.com"));
        assert_eq!(val.get("port").and_then(|v| v.as_u64()), Some(443));
        assert_eq!(val.get("uuid").and_then(|v| v.as_str()), Some("b831381d-6324-4d53-ad4f-8cda48b30811"));
        assert_eq!(val.get("name").and_then(|v| v.as_str()), Some("🇳🇱 Netherlands VMess"));
        assert_eq!(val.get("network").and_then(|v| v.as_str()), Some("ws"));
        assert_eq!(val.get("tls").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn regression_bug_76_ss_and_vmess_keep_cipher_and_alter_id() {
        // Баг 76: cipher (SS) и cipher/alterId (VMess) парсились, но не попадали в YAML — узлы нерабочие.
        let ss = format!("ss://{}@1.2.3.4:8388#SS-1", crate::adguard::base64_encode(b"aes-256-gcm:secret"));
        let vm_json = r#"{"v":"2","ps":"VM-1","add":"5.6.7.8","port":"443","id":"11111111-2222-3333-4444-555555555555","aid":"2","scy":"chacha20-poly1305","net":"tcp"}"#;
        let vm = format!("vmess://{}", crate::adguard::base64_encode(vm_json.as_bytes()));
        let raw = crate::adguard::base64_encode(format!("{ss}\n{vm}\n").as_bytes());
        let (yaml, count) = convert_subscription_to_mihomo_yaml(&raw).unwrap();
        assert_eq!(count, 2);
        assert!(yaml.contains("    cipher: aes-256-gcm\n"), "SS cipher:\n{yaml}");
        assert!(yaml.contains("    cipher: chacha20-poly1305\n"), "VMess cipher:\n{yaml}");
        assert!(yaml.contains("    alterId: 2\n"), "VMess alterId:\n{yaml}");
    }

    #[test]
    fn regression_bug_77_provider_yaml_scalars_are_escaped() {
        // Баг 77: YAML провайдера собирался format! без экранирования — пароль с `\` или `"` ломал провайдер,
        // имя с `\"` позволяло выйти из строки и внедрить ключи из чужой подписки.
        let ss = format!(
            "ss://{}@1.2.3.4:8388#{}",
            crate::adguard::base64_encode(b"aes-256-gcm:pa\"ss\\word"),
            "evil%5C%22%0A%20%20%20%20type%3A%20direct"
        );
        let raw = crate::adguard::base64_encode(format!("{ss}\n").as_bytes());
        let (yaml, _) = convert_subscription_to_mihomo_yaml(&raw).unwrap();
        assert!(yaml.contains(r#"    password: "pa\"ss\\word""#), "пароль экранирован:\n{yaml}");
        let type_lines = yaml.lines().filter(|l| l.trim_start().starts_with("type:")).count();
        assert_eq!(type_lines, 1, "имя из подписки не может добавить ключ type:\n{yaml}");
        assert!(yaml.contains(r#"  - name: "evil\\\"\n    type: direct""#), "переводы строк и кавычки в имени экранированы:\n{yaml}");
    }

    #[test]
    fn regression_bug_78b_userinfo_percent_decoded_plus_kept() {
        let tr = parse_trojan_uri("trojan://p%40ss+1@h:443#n%201").unwrap();
        assert_eq!(tr["password"], "p@ss+1", "Trojan password must be percent-decoded and keep '+'");
        assert_eq!(tr["name"], "n 1");

        let vl = parse_vless_uri("vless://u%2Bx@h:443?type=ws&path=%2Fa+b#n").unwrap();
        assert_eq!(vl["uuid"], "u+x", "VLESS uuid must be percent-decoded");
        assert_eq!(vl["ws-opts"]["path"], "/a+b", "query path must keep '+' (RFC 3986)");

        let hy = parse_hysteria2_uri("hysteria2://p%21w+2@h:443#n").unwrap();
        assert_eq!(hy["password"], "p!w+2", "Hy2 password must be percent-decoded and keep '+'");

        let params = parse_query_params("path=%2Fws+v1&host=a+b.example.com");
        assert_eq!(params.get("path").map(String::as_str), Some("/ws+v1"));
        assert_eq!(params.get("host").map(String::as_str), Some("a+b.example.com"));
    }

    #[test]
    fn regression_bug_79b_transports_not_silently_dropped() {
        // 79.1: VLESS xhttp, httpupgrade, h2, and unknown transport rejection
        let vl_xhttp = parse_vless_uri("vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443?type=xhttp&path=%2Fx&host=cdn.example.com&mode=auto#XHTTP").unwrap();
        assert_eq!(vl_xhttp["network"], "xhttp");
        assert_eq!(vl_xhttp["xhttp-opts"]["path"], "/x");
        assert_eq!(vl_xhttp["xhttp-opts"]["host"], "cdn.example.com");

        let vl_hup = parse_vless_uri("vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443?type=httpupgrade&path=%2Fup&host=h.example.com#HUP").unwrap();
        assert_eq!(vl_hup["network"], "ws");
        assert_eq!(vl_hup["ws-opts"]["v2ray-http-upgrade"], true);
        assert_eq!(vl_hup["ws-opts"]["path"], "/up");

        let vl_h2 = parse_vless_uri("vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443?type=h2&path=%2Fh2&host=h2.example.com#H2").unwrap();
        assert_eq!(vl_h2["network"], "h2");
        assert_eq!(vl_h2["h2-opts"]["path"], "/h2");

        // Неизвестный транспорт не должен молча превращаться в tcp
        assert!(parse_vless_uri("vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443?type=kcp#KCP").is_none());

        // 79.2: Trojan ws + grpc
        let tr_ws = parse_trojan_uri("trojan://secret@1.2.3.4:443?type=ws&path=%2Ftr&host=tr.example.com#TRWS").unwrap();
        assert_eq!(tr_ws["network"], "ws");
        assert_eq!(tr_ws["ws-opts"]["path"], "/tr");
        assert_eq!(tr_ws["ws-opts"]["headers"]["Host"], "tr.example.com");

        let tr_grpc = parse_trojan_uri("trojan://secret@1.2.3.4:443?type=grpc&serviceName=trgrpc#TRGRPC").unwrap();
        assert_eq!(tr_grpc["network"], "grpc");
        assert_eq!(tr_grpc["grpc-opts"]["grpc-service-name"], "trgrpc");

        // 79.3: Hy2 obfs, obfs-password, mport
        let hy = parse_hysteria2_uri("hy2://pass@1.2.3.4:443?obfs=salamander&obfs-password=obfspw&mport=20000-30000&sni=hy.example.com#HY2").unwrap();
        assert_eq!(hy["obfs"], "salamander");
        assert_eq!(hy["obfs-password"], "obfspw");
        assert_eq!(hy["ports"], "20000-30000");

        // 79.4: SS plain SIP002 / SS-2022 + plugin rejection
        let ss_plain = parse_ss_uri("ss://2022-blake3-aes-128-gcm:plainKey%3D%3D@1.2.3.4:8388#SS2022").unwrap();
        assert_eq!(ss_plain["cipher"], "2022-blake3-aes-128-gcm");
        assert_eq!(ss_plain["password"], "plainKey==");

        // Неподдерживаемый plugin отвергается с причиной, а не молча теряется
        let ss_unknown_plugin = "ss://2022-blake3-aes-128-gcm:key@1.2.3.4:8388?plugin=shadow-tls%3Bv3#SSBAD";
        assert!(parse_ss_uri(ss_unknown_plugin).is_none());

        // 79.5: Отчёт об отклонённых нодах с понятной причиной
        let sub = format!(
            "vless://11111111-1111-1111-1111-111111111111@1.2.3.4:443?type=xhttp&path=%2Fx#OK\n{ss_unknown_plugin}\n"
        );
        let (_yaml, count, rejected) = convert_subscription_to_mihomo_yaml_with_report(&sub).unwrap();
        assert_eq!(count, 1);
        assert_eq!(rejected.len(), 1);
        assert!(!rejected[0].1.is_empty(), "rejected node must include reason");
    }
}
