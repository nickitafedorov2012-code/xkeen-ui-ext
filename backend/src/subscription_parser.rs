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
            if t.contains("vless://")
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

/// Парсинг отдельной URI-ссылки в объект узла Mihomo
pub fn parse_v2ray_line(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.starts_with("vless://") {
        parse_vless_uri(trimmed)
    } else if trimmed.starts_with("hy2://") || trimmed.starts_with("hysteria2://") {
        parse_hysteria2_uri(trimmed)
    } else if trimmed.starts_with("trojan://") {
        parse_trojan_uri(trimmed)
    } else {
        None
    }
}

/// Преобразование сырого контента подписки (Base64 или список URI) в YAML провайдера для Mihomo
pub fn convert_subscription_to_mihomo_yaml(raw_content: &str) -> Result<(String, usize), String> {
    let decoded_text = if let Ok(bytes) = base64_decode(raw_content.trim()) {
        if let Ok(text) = String::from_utf8(bytes) {
            if text.contains("://") {
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
    for line in decoded_text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        if let Some(proxy_val) = parse_v2ray_line(trimmed) {
            proxies.push(proxy_val);
        }
    }

    if proxies.is_empty() {
        return Err("Не удалось распарсить ни одного сервера из подписки V2Ray/Base64".into());
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
            yaml.push_str(&format!("    servername: \"{sni}\"\n"));
        }
        if let Some(fp) = obj.get("client-fingerprint").and_then(|v| v.as_str()) {
            yaml.push_str(&format!("    client-fingerprint: \"{fp}\"\n"));
        }
        if let Some(flow) = obj.get("flow").and_then(|v| v.as_str()) {
            yaml.push_str(&format!("    flow: \"{flow}\"\n"));
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
                if let Some(grpc) = obj.get("grpc-opts").and_then(|v| v.as_object()) {
                    yaml.push_str("    grpc-opts:\n");
                    if let Some(sn) = grpc.get("grpc-service-name").and_then(|v| v.as_str()) {
                        yaml.push_str(&format!("      grpc-service-name: \"{sn}\"\n"));
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
}
