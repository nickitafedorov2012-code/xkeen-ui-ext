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
        } else if bytes[i] == b'+' {
            out.push(b' ');
            i += 1;
            continue;
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

/// Парсинг ссылки VLESS: `vless://uuid@host:port?params#name`
pub fn parse_vless_uri(uri: &str) -> Option<Value> {
    let after_scheme = uri.strip_prefix("vless://")?;
    let (main_part, fragment) = match after_scheme.split_once('#') {
        Some((m, f)) => (m, percent_decode(f)),
        None => (after_scheme, String::new()),
    };

    let (auth_host, query_str) = match main_part.split_once('?') {
        Some((ah, q)) => (ah, q),
        None => (main_part, ""),
    };

    let (uuid, host_port) = auth_host.split_once('@')?;
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
    map.insert("uuid".into(), Value::String(uuid.to_string()));
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

    let network = params.get("type").or_else(|| params.get("net")).map(String::as_str).unwrap_or("tcp");
    if network == "ws" {
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
    } else if network == "grpc" {
        map.insert("network".into(), Value::String("grpc".into()));
        let mut grpc_opts = serde_json::Map::new();
        if let Some(sn) = params.get("servicename") {
            grpc_opts.insert("grpc-service-name".into(), Value::String(sn.clone()));
        }
        map.insert("grpc-opts".into(), Value::Object(grpc_opts));
    }

    Some(Value::Object(map))
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
    map.insert("password".into(), Value::String(password.to_string()));
    map.insert("udp".into(), Value::Bool(true));

    if let Some(sni) = params.get("sni") {
        map.insert("sni".into(), Value::String(sni.clone()));
    }
    if let Some(insecure) = params.get("insecure") {
        map.insert("skip-cert-verify".into(), Value::Bool(insecure == "1" || insecure == "true"));
    }

    Some(Value::Object(map))
}

/// Парсинг ссылки Trojan: `trojan://password@host:port?sni=...#name`
pub fn parse_trojan_uri(uri: &str) -> Option<Value> {
    let after_scheme = uri.strip_prefix("trojan://")?;
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
        format!("Trojan {server}:{port}")
    };

    let mut map = serde_json::Map::new();
    map.insert("name".into(), Value::String(server_name));
    map.insert("type".into(), Value::String("trojan".into()));
    map.insert("server".into(), Value::String(server));
    map.insert("port".into(), json!(port));
    map.insert("password".into(), Value::String(password.to_string()));
    map.insert("udp".into(), Value::Bool(true));

    if let Some(sni) = params.get("sni") {
        map.insert("sni".into(), Value::String(sni.clone()));
    }

    Some(Value::Object(map))
}

/// Парсинг ссылки Shadowsocks: `ss://BASE64@host:port#name` или `ss://BASE64#name`
pub fn parse_ss_uri(uri: &str) -> Option<Value> {
    let after_scheme = uri.strip_prefix("ss://")?;
    let (main_part, fragment) = match after_scheme.split_once('#') {
        Some((m, f)) => (m, percent_decode(f)),
        None => (after_scheme, String::new()),
    };

    let (auth_host, _query_str) = match main_part.split_once('?') {
        Some((ah, q)) => (ah, q),
        None => (main_part, ""),
    };

    // 1) SIP002: ss://BASE64(method:password)@host:port
    let (server, port, cipher, password) = if let Some((user_info, host_port)) = auth_host.split_once('@') {
        let decoded = String::from_utf8(base64_decode(user_info).ok()?).ok()?;
        let (cipher, pass) = decoded.split_once(':')?;
        let (s, p) = parse_host_port(host_port);
        (s, p, cipher.to_string(), pass.to_string())
    } else {
        // 2) Legacy: ss://BASE64(method:password@host:port)
        let decoded = String::from_utf8(base64_decode(auth_host).ok()?).ok()?;
        let (cipher_pass, host_port) = decoded.split_once('@')?;
        let (cipher, pass) = cipher_pass.split_once(':')?;
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

    Some(Value::Object(map))
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

/// Парсинг отдельной URI-ссылки в объект узла Mihomo
pub fn parse_v2ray_line(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.starts_with("vless://") {
        parse_vless_uri(trimmed)
    } else if trimmed.starts_with("hy2://") || trimmed.starts_with("hysteria2://") {
        parse_hysteria2_uri(trimmed)
    } else if trimmed.starts_with("trojan://") {
        parse_trojan_uri(trimmed)
    } else if trimmed.starts_with("ss://") {
        parse_ss_uri(trimmed)
    } else if trimmed.starts_with("vmess://") {
        parse_vmess_uri(trimmed)
    } else {
        None
    }
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
            map.insert("uuid".into(), Value::String(uuid.to_string()));
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
pub fn convert_subscription_to_mihomo_yaml(raw_content: &str) -> Result<(String, usize), String> {
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
    let trimmed = decoded_text.trim();

    // 1. Попытка распарсить как Xray/V2Ray JSON-массив подписки
    if trimmed.starts_with('[') {
        if let Ok(profiles) = serde_json::from_str::<Vec<Value>>(trimmed) {
            for p in &profiles {
                if let Some(proxy_val) = parse_xray_json_profile(p) {
                    proxies.push(proxy_val);
                }
            }
        }
    }

    // 2. Если не JSON, парсим построчно (VLESS/Hysteria2/Trojan URI ссылки)
    if proxies.is_empty() {
        for line in decoded_text.lines() {
            let trimmed_line = line.trim();
            if trimmed_line.is_empty() || trimmed_line.starts_with('#') || trimmed_line.starts_with("//") {
                continue;
            }
            if let Some(proxy_val) = parse_v2ray_line(trimmed_line) {
                proxies.push(proxy_val);
            }
        }
    }

    if proxies.is_empty() {
        return Err("Не удалось распарсить ни одного сервера из подписки V2Ray/Base64/JSON".into());
    }

    let count = proxies.len();
    let mut yaml = String::from("proxies:\n");
    for p in proxies {
        let obj = p.as_object().ok_or("Некорректный узел")?;
        let name = obj.get("name").and_then(|v| v.as_str()).unwrap_or("Server");
        let ptype = obj.get("type").and_then(|v| v.as_str()).unwrap_or("vless");
        let server = obj.get("server").and_then(|v| v.as_str()).unwrap_or("");
        let port = obj.get("port").and_then(|v| v.as_u64()).unwrap_or(443);

        yaml.push_str(&format!("  - name: \"{}\"\n", name.replace('"', "\\\"")));
        yaml.push_str(&format!("    type: {ptype}\n"));
        yaml.push_str(&format!("    server: \"{server}\"\n"));
        yaml.push_str(&format!("    port: {port}\n"));

        if let Some(uuid) = obj.get("uuid").and_then(|v| v.as_str()) {
            yaml.push_str(&format!("    uuid: \"{uuid}\"\n"));
        }
        if let Some(password) = obj.get("password").and_then(|v| v.as_str()) {
            yaml.push_str(&format!("    password: \"{password}\"\n"));
        }
        yaml.push_str("    udp: true\n");

        if let Some(tls) = obj.get("tls").and_then(|v| v.as_bool()) {
            yaml.push_str(&format!("    tls: {tls}\n"));
        }
        if let Some(sni) = obj.get("servername").or_else(|| obj.get("sni")).and_then(|v| v.as_str()) {
            if ptype == "hysteria2" {
                yaml.push_str(&format!("    sni: \"{sni}\"\n"));
            } else {
                yaml.push_str(&format!("    servername: \"{sni}\"\n"));
            }
        }
        if let Some(fp) = obj.get("client-fingerprint").and_then(|v| v.as_str()) {
            yaml.push_str(&format!("    client-fingerprint: \"{fp}\"\n"));
        }
        if let Some(flow) = obj.get("flow").and_then(|v| v.as_str()) {
            yaml.push_str(&format!("    flow: \"{flow}\"\n"));
        }
        if let Some(skip) = obj.get("skip-cert-verify").and_then(|v| v.as_bool()) {
            yaml.push_str(&format!("    skip-cert-verify: {skip}\n"));
        }
        if let Some(alpn) = obj.get("alpn").and_then(|v| v.as_array()) {
            yaml.push_str("    alpn:\n");
            for a in alpn {
                if let Some(s) = a.as_str() {
                    yaml.push_str(&format!("      - {s}\n"));
                }
            }
        }

        if let Some(reality) = obj.get("reality-opts").and_then(|v| v.as_object()) {
            yaml.push_str("    reality-opts:\n");
            if let Some(pbk) = reality.get("public-key").and_then(|v| v.as_str()) {
                yaml.push_str(&format!("      public-key: \"{pbk}\"\n"));
            }
            if let Some(sid) = reality.get("short-id").and_then(|v| v.as_str()) {
                yaml.push_str(&format!("      short-id: \"{sid}\"\n"));
            }
        }

        if let Some(network) = obj.get("network").and_then(|v| v.as_str()) {
            yaml.push_str(&format!("    network: {network}\n"));
            if network == "ws" {
                if let Some(ws) = obj.get("ws-opts").and_then(|v| v.as_object()) {
                    yaml.push_str("    ws-opts:\n");
                    if let Some(path) = ws.get("path").and_then(|v| v.as_str()) {
                        yaml.push_str(&format!("      path: \"{path}\"\n"));
                    }
                    if let Some(headers) = ws.get("headers").and_then(|v| v.as_object()) {
                        yaml.push_str("      headers:\n");
                        for (k, v) in headers {
                            if let Some(val) = v.as_str() {
                                yaml.push_str(&format!("        {k}: \"{val}\"\n"));
                            }
                        }
                    }
                }
            } else if network == "grpc" {
                if let Some(grpc) = obj.get("grpc-opts").and_then(|g| g.as_object()) {
                    yaml.push_str("    grpc-opts:\n");
                    if let Some(sn) = grpc.get("grpc-service-name").and_then(|v| v.as_str()) {
                        yaml.push_str(&format!("      grpc-service-name: \"{sn}\"\n"));
                    }
                }
            } else if network == "xhttp" {
                if let Some(xh) = obj.get("xhttp-opts").and_then(|x| x.as_object()) {
                    yaml.push_str("    xhttp-opts:\n");
                    if let Some(mode) = xh.get("mode").and_then(|m| m.as_str()) {
                        yaml.push_str(&format!("      mode: \"{mode}\"\n"));
                    }
                    if let Some(path) = xh.get("path").and_then(|p| p.as_str()) {
                        yaml.push_str(&format!("      path: \"{path}\"\n"));
                    }
                    if let Some(host) = xh.get("host").and_then(|h| h.as_str()) {
                        yaml.push_str(&format!("      host: \"{host}\"\n"));
                    }
                }
            }
        }
    }

    Ok((yaml, count))
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
}
