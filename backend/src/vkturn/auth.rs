use std::time::Duration;
use crate::config::VkTurnConfig;
use super::*;

pub fn parse_call_link(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    let hash_re = regex_lite::Regex::new(r"^[A-Za-z0-9_\-]{8,128}$").unwrap();
    if s.to_ascii_lowercase().contains("telemost.yandex") {
        return Err("Ядро csqtt работает только со звонками ВКонтакте — ссылка Телемоста не подойдёт".into());
    }
    let link_re = regex_lite::Regex::new(r"^(?:https://)?(?:m\.)?vk\.(?:com|ru)/call/join/([^?#/\s]+)/?(?:[?#][^\s]*)?$").unwrap();
    let hash = if let Some(c) = link_re.captures(s) {
        c[1].to_string()
    } else if hash_re.is_match(s) {
        s.to_string()
    } else {
        return Err("Нужна ссылка на звонок ВКонтакте вида https://vk.com/call/join/…".into());
    };
    if !hash_re.is_match(&hash) {
        return Err("Нужна ссылка на звонок ВКонтакте вида https://vk.com/call/join/…".into());
    }
    Ok(hash)
}

/// Каноничный вид ссылки для хранения: https://vk.com/call/join/<хеш>
pub fn normalize_call_link(raw: &str) -> Result<String, String> {
    parse_call_link(raw).map(|h| format!("https://vk.com/call/join/{h}"))
}

/// Новая ссылка становится основной; запасные остаются в прежнем порядке, без дублей, не больше MAX_LINKS.
/// Ссылки сравниваются по хешу звонка.
pub fn with_primary_link(links: &[String], primary: &str) -> Result<Vec<String>, String> {
    let primary = normalize_call_link(primary)?;
    let mut out = vec![primary];
    for l in links {
        if let Ok(n) = normalize_call_link(l) {
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out.truncate(MAX_LINKS);
    Ok(out)
}

/// "host:port": IPv4 или домен, порт 1..65535
pub fn validate_peer(peer: &str) -> Result<(), String> {
    let re = regex_lite::Regex::new(r"^([A-Za-z0-9\-\.]{1,253}):([0-9]{1,5})$").unwrap();
    let caps = re.captures(peer.trim()).ok_or("Адрес сервера должен быть вида host:port")?;
    let port: u32 = caps[2].parse().map_err(|_| "Неверный порт")?;
    if port == 0 || port > 65535 {
        return Err("Порт вне диапазона 1–65535".into());
    }
    Ok(())
}

/// Разбор ссылки профиля csqtt (как в focsq: csqtt://connect?v=2&host=...&peer=46000&password=... или csqtt://pass@host:port)
pub fn decrypt_csqtt_link(raw: &str) -> Option<String> {
    use base64::Engine;
    use sha2::Digest;
    let payload = raw.trim().strip_prefix("csqtt://")?;
    if payload.len() > 16384 { return None; }
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(payload))
        .ok()?;
    if bytes.len() < 28 { return None; }
    let nonce: [u8; 12] = bytes[..12].try_into().ok()?;
    let digest = sha2::Sha256::digest(b"csqtt-chiper-key");
    let key = ring::aead::UnboundKey::new(&ring::aead::AES_256_GCM, &digest).ok()?;
    let key = ring::aead::LessSafeKey::new(key);
    let mut encrypted = bytes[12..].to_vec();
    let plain = key.open_in_place(ring::aead::Nonce::assume_unique_for_key(nonce), ring::aead::Aad::empty(), &mut encrypted).ok()?;
    let decoded = std::str::from_utf8(plain).ok()?.trim();
    if decoded.starts_with("csqtt://") {
        Some(decoded.to_string())
    } else if decoded.starts_with("connect?") || decoded.contains('@') {
        Some(format!("csqtt://{decoded}"))
    } else if decoded.contains("host=") {
        Some(format!("csqtt://connect?{decoded}"))
    } else {
        None
    }
}

fn decode_csqtt_component(value: &str) -> Option<String> {
    let bytes = value.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            result.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result).ok()
}

fn repair_glued_csqtt_query(query: &str) -> Option<String> {
    let re = regex_lite::Regex::new(r"(?i)^v=2host=([a-z0-9.-]+)&?peer=([0-9]+)&?password=([^&;]+)$").ok()?;
    let glued_key_re = regex_lite::Regex::new(r"(?i)(?:v|host|peer|password|client|hashes|token|workers|power)=").ok()?;
    let caps = re.captures(query)?;
    if glued_key_re.is_match(&caps[3]) {
        return None;
    }
    Some(format!("v=2&host={}&peer={}&password={}", &caps[1], &caps[2], &caps[3]))
}

pub fn parse_csqtt_link(raw: &str) -> Option<(String, String)> {
    let s = raw.trim();
    if !s.starts_with("csqtt://") {
        return None;
    }
    if !s.starts_with("csqtt://connect?") && !s[8..].contains('@') {
        let decoded = decrypt_csqtt_link(s)?;
        return parse_csqtt_link(&decoded);
    }
    if s.starts_with("csqtt://connect?") {
        let raw_query = s.trim_start_matches("csqtt://connect?");
        let repaired = repair_glued_csqtt_query(raw_query);
        let query = repaired.as_deref().unwrap_or(raw_query);
        let mut fields = std::collections::HashMap::new();
        for pair in query.split('&') {
            let (key, value) = pair.split_once('=')?;
            let key = decode_csqtt_component(key)?;
            if !matches!(key.as_str(), "v" | "host" | "peer" | "password") || fields.contains_key(&key) {
                return None;
            }
            fields.insert(key, decode_csqtt_component(value)?);
        }
        if fields.get("v")?.as_str() != "2" { return None; }
        let host = fields.get("host")?;
        let password = fields.get("password")?;
        let port = fields.get("peer")?.parse::<u16>().ok().filter(|p| *p > 0)?;
        let addr = format!("{host}:{port}");
        validate_peer(&addr).ok()?;
        validate_password(password).ok()?;
        return Some((addr, password.clone()));
    }
    let rest = s.trim_start_matches("csqtt://");
    if let Some((pw, addr)) = rest.split_once('@') {
        if !pw.is_empty() && !addr.is_empty() {
            validate_peer(addr).ok()?;
            let password = decode_csqtt_component(pw)?;
            validate_password(&password).ok()?;
            return Some((addr.to_string(), password));
        }
    }
    None
}

#[derive(Debug, PartialEq)]
pub struct CsqttProfile {
    pub peer: String,
    pub password: String,
    pub links: Vec<String>,
    pub token: Option<String>,
    pub power: Option<u8>,
}

/// Полный профиль для сохранения: поля, влияющие на подключение, не отбрасываются.
pub fn parse_csqtt_profile(raw: &str) -> Option<CsqttProfile> {
    let s = raw.trim();
    let payload = s.strip_prefix("csqtt://")?;
    if !payload.starts_with("connect?") {
        if payload.contains('@') {
            let (peer, password) = parse_csqtt_link(s)?;
            return Some(CsqttProfile { peer, password, links: vec![], token: None, power: None });
        }
        return parse_csqtt_profile(&decrypt_csqtt_link(s)?);
    }
    let raw_query = payload.strip_prefix("connect?")?.replace("&amp;", "&");
    let query = repair_glued_csqtt_query(&raw_query).unwrap_or(raw_query);
    let mut fields = std::collections::HashMap::new();
    for part in query.split(['&', ';']) {
        if part.is_empty() { continue; }
        let (raw_key, value) = part.split_once('=')?;
        let key = decode_csqtt_component(raw_key)?;
        if !matches!(key.as_str(), "v" | "host" | "peer" | "password" | "hashes" | "token" | "power" | "workers" | "client")
            || fields.insert(key, value.to_string()).is_some() { return None; }
    }
    let field = |key: &str| fields.get(key).and_then(|v| decode_csqtt_component(v));
    if field("v")?.as_str() != "2" || fields.contains_key("power") && fields.contains_key("workers") { return None; }
    let host = field("host")?;
    if host.eq_ignore_ascii_case("localhost") || matches!(host.as_str(), "127.0.0.1" | "0.0.0.0") { return None; }
    let port = field("peer")?.parse::<u16>().ok().filter(|p| *p > 0)?;
    let peer = format!("{host}:{port}");
    validate_peer(&peer).ok()?;
    let password = field("password")?;
    if password.chars().any(char::is_whitespace) { return None; }
    validate_password(&password).ok()?;
    let mut links = Vec::new();
    if let Some(raw_hashes) = fields.get("hashes") {
        for raw_hash in raw_hashes.split(['+', ',']) {
            let value = decode_csqtt_component(raw_hash)?;
            let hash = parse_call_link(&value).ok()?;
            if hash.len() < 16 || links.len() >= MAX_LINKS || links.iter().any(|h| h == &hash) { return None; }
            links.push(hash);
        }
    }
    let token = if fields.contains_key("token") {
        let value = field("token")?;
        if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_whitespace) { return None; }
        Some(value)
    } else { None };
    if token.is_some() && !links.is_empty() { return None; }
    let power = if fields.contains_key("power") || fields.contains_key("workers") {
        let raw = field(if fields.contains_key("power") { "power" } else { "workers" })?;
        let value = raw.parse::<u32>().ok().filter(|n| *n > 0)?;
        Some(value.clamp(10, 100) as u8)
    } else { None };
    Some(CsqttProfile { peer, password, links, token, power })
}

/// Пользователь может ввести только IP/домен — PEER-порт сервера csqtt по умолчанию подставляется сам.
/// Также поддерживаются ссылки csqtt:// (извлекается хост:порт).
pub fn normalize_peer(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(String::new());
    }
    if let Some((addr, _)) = parse_csqtt_link(s) {
        validate_peer(&addr)?;
        return Ok(addr);
    }
    let with_port = if s.contains(':') { s.to_string() } else { format!("{s}:{DEFAULT_SERVER_PORT}") };
    validate_peer(&with_port)?;
    Ok(with_port)
}

/// Пароль передаётся клиенту переменной окружения, из него выводится ключ шифрования туннеля.
pub fn validate_password(pw: &str) -> Result<(), String> {
    if pw.is_empty() {
        return Err("Укажите пароль подключения к серверу csqtt".into());
    }
    if pw.chars().count() > 128 {
        return Err("Пароль подключения слишком длинный (больше 128 символов)".into());
    }
    if pw.chars().any(|c| c.is_control()) {
        return Err("Пароль подключения не должен содержать переводы строк и управляющие символы".into());
    }
    Ok(())
}

/// Чтение счетчиков (rx_bytes, tx_bytes) виртуального интерфейса туннеля (csq0).
/// Читается напрямую из системного /proc/net/dev (пассивно, 0% CPU).
pub fn read_tun_interface_bytes(iface_name: &str) -> (u64, u64) {
    #[cfg(target_os = "linux")]
    if let Ok(content) = std::fs::read_to_string("/proc/net/dev") {
        for line in content.lines().skip(2) {
            if let Some((iface_part, stats_part)) = line.split_once(':') {
                if iface_part.trim() == iface_name {
                    let mut iter = stats_part.split_whitespace();
                    let rx_val = iter.next().and_then(|s| s.parse::<u64>().ok());
                    let tx_val = iter.nth(7).and_then(|s| s.parse::<u64>().ok());
                    return (rx_val.unwrap_or(0), tx_val.unwrap_or(0));
                }
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    let _ = iface_name;
    (0, 0)
}

/// Число потоков для `-n`: поддерживает точные потоки (3-100) или дефолтные 10 на ссылку (TRN-WRK-01).
pub fn effective_workers(cfg: &VkTurnConfig) -> u32 {
    let wanted = if cfg.streams == 0 {
        let links_count = call_hashes(cfg).len().max(1) as u32;
        (links_count * WORKERS_PER_GROUP).clamp(WORKERS_PER_GROUP, DEFAULT_WORKERS)
    } else {
        cfg.streams as u32
    };
    wanted.clamp(3, MAX_WORKERS)
}

/// Хеши звонков для `-vk` (без дублей, в порядке приоритета).
pub fn call_hashes(cfg: &VkTurnConfig) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for l in &cfg.links {
        if let Ok(h) = parse_call_link(l) {
            if !out.contains(&h) {
                out.push(h);
            }
        }
    }
    out.truncate(MAX_LINKS);
    out
}

/// The saved account remains available, but manual mode uses only the supplied call hashes.
pub fn selected_vk_token(cfg: &VkTurnConfig) -> Option<&str> {
    let token = match cfg.auth_mode.as_str() {
        "manual" => None,
        "profile_token" => cfg.profile_token.as_deref(),
        _ => cfg.vk_token.as_deref(),
    };
    token.filter(|value| !value.trim().is_empty())
}

/// Маскирование токена VK в логах и API.
pub fn mask_vk_token(token: &str) -> String {
    let t = token.trim();
    if t.is_empty() {
        return String::new();
    }
    if t.starts_with("vk1.a.") {
        "vk1.a.***".to_string()
    } else {
        "[REDACTED]".to_string()
    }
}

/// Разбор редирект-хеша авторизации VK (#access_token=...&user_id=...) или прямого токена.
pub fn parse_vk_redirect_hash(fragment_or_url: &str) -> Result<(String, Option<u64>), String> {
    let input = fragment_or_url.trim();
    if input.is_empty() {
        return Err("Пустая строка авторизации".into());
    }

    let mut token: Option<String> = None;
    let mut user_id: Option<u64> = None;

    if let Some(pos) = input.find("access_token=") {
        let rest = &input[pos + "access_token=".len()..];
        let tok = rest.split('&').next().unwrap_or("").trim();
        if !tok.is_empty() {
            token = Some(tok.to_string());
        }
    }

    if let Some(pos) = input.find("user_id=") {
        let rest = &input[pos + "user_id=".len()..];
        let uid_str = rest.split('&').next().unwrap_or("").trim();
        if let Ok(uid) = uid_str.parse::<u64>() {
            user_id = Some(uid);
        }
    }

    if let Some(tok) = token {
        return Ok((tok, user_id));
    }

    // Запасной вариант: пользователь вставил чистый токен (например vk1.a....)
    if input.starts_with("vk1.a.") || (!input.contains('=') && input.len() > 20) {
        return Ok((input.to_string(), None));
    }

    Err("Не удалось найти access_token в предоставленной ссылке или строке".into())
}

/// Формирование строки инициализации csqtt-client в формате VK_JS_BOOTSTRAP:<base64-json>\n
pub fn format_vk_bootstrap_line(token: &str, user_id: u64) -> String {
    let payload = serde_json::json!({
        "token": token.trim(),
        "access_token": token.trim(),
        "user_id": user_id,
    });
    let raw = serde_json::to_string(&payload).unwrap_or_default();
    let b64 = crate::adguard::base64_encode(raw.as_bytes());
    format!("VK_JS_BOOTSTRAP:{b64}\n")
}

/// Запрос профиля пользователя через официальный VK API (users.get v=5.199).
#[derive(Debug, PartialEq, Eq)]
pub enum VkProfileError {
    Rejected,
    Unavailable,
}

pub async fn fetch_vk_user_profile(
    http: &reqwest::Client,
    token: &str,
    fallback_id: Option<u64>,
) -> Result<(u64, String), VkProfileError> {
    fetch_vk_user_profile_at(http, "https://api.vk.ru", token, fallback_id).await
}

pub async fn fetch_vk_user_profile_at(
    http: &reqwest::Client,
    base_url: &str,
    token: &str,
    fallback_id: Option<u64>,
) -> Result<(u64, String), VkProfileError> {
    let base = base_url.trim_end_matches('/');
    // 1. Проверяем валидность аккаунта через account.getAppPermissions
    let perm_url = format!("{base}/method/account.getAppPermissions");
    if let Ok(resp) = http
        .get(&perm_url)
        .query(&[("access_token", token.trim()), ("v", "5.199")])
        .timeout(Duration::from_secs(5))
        .send()
        .await
    {
        if let Ok(json) = resp.json::<serde_json::Value>().await {
            if let Some(err) = json.get("error") {
                let code = err.get("error_code").and_then(|c| c.as_i64()).unwrap_or(0);
                let msg = err.get("error_msg").and_then(|m| m.as_str()).unwrap_or("Неизвестная ошибка VK");
                if is_token_invalid_code(code) || msg.contains("blocked") || msg.contains("banned") {
                    return Err(VkProfileError::Rejected);
                }
            }
        }
    }

    let url = format!("{base}/method/users.get");
    let resp = http
        .get(&url)
        .query(&[("access_token", token.trim()), ("v", "5.199")])
        .timeout(Duration::from_secs(6))
        .send()
        .await
        .map_err(|_| VkProfileError::Unavailable)?;

    if !resp.status().is_success() { return Err(VkProfileError::Unavailable); }

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| VkProfileError::Unavailable)?;

    if let Some(err) = json.get("error") {
        let code = err.get("error_code").and_then(|c| c.as_i64()).unwrap_or(0);
        let msg = err.get("error_msg").and_then(|m| m.as_str()).unwrap_or("");
        return Err(if is_token_invalid_code(code) || msg.contains("blocked") || msg.contains("banned") {
            VkProfileError::Rejected
        } else {
            VkProfileError::Unavailable
        });
    }

    if let Some(user) = json.get("response").and_then(|r| r.as_array()).and_then(|a| a.first()) {
        let uid = user.get("id").and_then(|i| i.as_u64()).or(fallback_id).unwrap_or(0);
        let first = user.get("first_name").and_then(|s| s.as_str()).unwrap_or("");
        let last = user.get("last_name").and_then(|s| s.as_str()).unwrap_or("");
        let full_name = format!("{first} {last}").trim().to_string();
        let name = if full_name.is_empty() {
            format!("Пользователь VK {}", uid)
        } else {
            full_name
        };
        Ok((uid, name))
    } else {
        Err(VkProfileError::Unavailable)
    }
}

/// Расчет необходимого числа звонков по числу воркеров (по архитектуре focsq: 1 звонок на 27 потоков, от 1 до MAX_LINKS).
pub fn call_count_for_workers(workers: u32) -> usize {
    for n in 1..=MAX_LINKS {
        if workers <= (n as u32) * 27 {
            return n;
        }
    }
    MAX_LINKS
}

pub const TOKEN_INVALID_CODES: &[i64] = &[4, 5, 27, 28];

pub fn is_token_invalid_code(code: i64) -> bool {
    TOKEN_INVALID_CODES.contains(&code)
}

#[derive(Debug, Clone)]
pub enum VkCallResult {
    Started { call_id: String, hash: String },
    TokenInvalid { code: i64, message: String },
    TransientError(String),
}

pub async fn start_vk_call_single(http: &reqwest::Client, token: &str) -> VkCallResult {
    let url = "https://api.vk.ru/method/calls.start";
    let resp = match http
        .post(url)
        .header("Authorization", format!("Bearer {}", token.trim()))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[("v", "5.199")])
        .timeout(Duration::from_secs(8))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return VkCallResult::TransientError(format!("Сетевая ошибка calls.start: {e}")),
    };

    let json: serde_json::Value = match resp.json().await {
        Ok(j) => j,
        Err(e) => return VkCallResult::TransientError(format!("Ошибка парсинга JSON calls.start: {e}")),
    };

    if let Some(err) = json.get("error") {
        let code = err.get("error_code").and_then(|c| c.as_i64()).unwrap_or(0);
        let msg = err.get("error_msg").and_then(|m| m.as_str()).unwrap_or("Ошибка VK").to_string();
        if is_token_invalid_code(code) {
            return VkCallResult::TokenInvalid { code, message: msg };
        }
        return VkCallResult::TransientError(format!("VK API calls.start код {code}: {msg}"));
    }

    let Some(response) = json.get("response") else {
        return VkCallResult::TransientError("нет response в ответе calls.start".into());
    };

    let call_id = response.get("call_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
    let join_link = response.get("join_link").and_then(|v| v.as_str()).unwrap_or("");
    let ok_join_link = response.get("ok_join_link").and_then(|v| v.as_str()).unwrap_or("");

    let hash = if !ok_join_link.trim().is_empty() && !ok_join_link.contains('/') {
        ok_join_link.trim().to_string()
    } else if !join_link.trim().is_empty() {
        match parse_call_link(join_link) {
            Ok(h) => h,
            Err(e) => return VkCallResult::TransientError(e),
        }
    } else {
        match parse_call_link(ok_join_link) {
            Ok(h) => h,
            Err(e) => return VkCallResult::TransientError(e),
        }
    };

    if call_id.is_empty() || hash.is_empty() {
        return VkCallResult::TransientError("Пустой call_id или hash в calls.start".into());
    }

    VkCallResult::Started { call_id, hash }
}

/// Запуск легального звонка через официальный VK API calls.start (с до 3 повторных попыток, как в focsq).
/// Возвращает Ok((call_id, hash)) или Err((is_token_invalid, error_message))
pub async fn start_vk_call(http: &reqwest::Client, token: &str) -> Result<(String, String), (bool, String)> {
    let mut last_err = "Не удалось выполнить запрос calls.start".to_string();
    for attempt in 0..3 {
        if attempt > 0 {
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
        match start_vk_call_single(http, token).await {
            VkCallResult::Started { call_id, hash } => return Ok((call_id, hash)),
            VkCallResult::TokenInvalid { code, message } => {
                return Err((true, format!("Токен ВКонтакте недействителен (код {code}: {message})")));
            }
            VkCallResult::TransientError(e) => {
                last_err = e;
            }
        }
    }
    Err((false, last_err))
}

pub async fn finish_vk_call(http: &reqwest::Client, token: &str, call_id: &str) {
    if token.trim().is_empty() || call_id.trim().is_empty() {
        return;
    }
    let url = "https://api.vk.ru/method/calls.forceFinish";
    let _ = http
        .post(url)
        .header("Authorization", format!("Bearer {}", token.trim()))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .form(&[("v", "5.199"), ("call_id", call_id)])
        .timeout(Duration::from_secs(5))
        .send()
        .await;
}

/// Параллельное завершение всех звонков (как в focsq / finishAll)
pub async fn finish_vk_calls_batch(http: &reqwest::Client, token: &str, call_ids: &[String]) {
    if token.trim().is_empty() || call_ids.is_empty() {
        return;
    }
    let mut tasks = Vec::new();
    for cid in call_ids {
        let h = http.clone();
        let tok = token.trim().to_string();
        let id = cid.clone();
        tasks.push(tokio::spawn(async move {
            finish_vk_call(&h, &tok, &id).await;
        }));
    }
    for t in tasks {
        let _ = t.await;
    }
}

