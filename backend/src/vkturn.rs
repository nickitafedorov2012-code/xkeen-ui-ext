//! VK-туннель: обход белых списков через TURN-серверы ВК-звонков на ядре csqtt
//! (клиент из Afteroid/csqttr «CSQTT-Rebirth», PolyForm Noncommercial 1.0.0, © amurcanov;
//! сборка для роутера — `scripts/build-csqtt-core.sh`).
//! Панель сама скачивает клиент, создаёт для него TUN-интерфейс, запускает и стережёт процесс
//! и подключает туннель к Mihomo как сервер второго уровня (Tier 2, «БС»).
//!
//! Что проверено по исходникам csqttr (коммит CORE_UPSTREAM_COMMIT, 05.11.2026):
//! - флаги клиента: `-peer host:port -vk <хеши через запятую> -n <потоки, кратно 10> -listen 127.0.0.1:0
//!   -tun-uds <имя> -captcha-mode auto`; пароль — только в переменной окружения `CSQTT_PASSWORD`
//!   (в списке процессов его не видно);
//! - при `CSQTT_EVENTS=1` клиент печатает в stdout машинные события `__CSQTT_EVENT__|<ТИП>|<json>`:
//!   READY, STATS (active, bytes_up, bytes_down), CONFIG (`TUNCONF:<ip>:<dns>:…` — адрес туннеля от сервера),
//!   ERROR (code, message, fatal), TUN_LOST;
//! - трафик идёт через TUN: клиент слушает абстрактный unix-сокет `-tun-uds` и ждёт по нему дескриптор TUN
//!   (SCM_RIGHTS, байт 1 в ответ). На Android его создаёт VpnService, на роутере — панель (интерфейс `csq0`);
//! - Mihomo выводит в туннель трафик сервера «📞 VK-туннель [БС]» (`type: direct`, `interface-name: csq0`);
//! - ручная капча: клиент печатает `CAPTCHA_SOLVE|<mode>|<redirect_uri>|<session_token>` и ждёт
//!   `CAPTCHA_RESULT|<token>` в stdin. На роутере WebView нет — панель сразу отвечает ошибкой,
//!   и клиент продолжает встроенное автоматическое решение без ожидания таймаутов; `STOP` — мягкая остановка.

use std::collections::{BTreeSet, VecDeque};
use std::net::Ipv4Addr;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch, Mutex, RwLock};

use crate::config::VkTurnConfig;

/// Тег релиза xkeen-ui-ext, в котором опубликованы бинарники ядра (см. scripts/build-csqtt-core.sh).
pub const CORE_TAG: &str = "csqtt-core-0e90df2";
/// Репозиторий и коммит, из которых собрано ядро.
pub const CORE_UPSTREAM: &str = "Afteroid/csqttr";
pub const CORE_UPSTREAM_COMMIT: &str = "0e90df237583738741b50e86e4f51f87e96c94da";
pub const CORE_RELEASES: &str = "https://github.com/nickitafedorov2012-code/xkeen-ui-ext/releases/download";

// OAuth параметры официального приложения VK для звонков (CSQTT)
pub const VK_APP_ID: &str = "7793118";
pub const VK_OAUTH_SCOPE: &str = "1073737727";
pub const VK_REDIRECT_URI: &str = "https://oauth.vk.ru/blank.html";
pub const VK_AUTH_URL: &str = "https://oauth.vk.ru/authorize?client_id=7793118&scope=1073737727&redirect_uri=https%3A%2F%2Foauth.vk.ru%2Fblank.html&display=page&response_type=token&revoke=1&v=5.199";

pub const CORE_PATH: &str = "/opt/sbin/csqtt-client";
/// Ядро прошлой версии (WDTT): удаляется при установке нового, чтобы не занимать место.
pub const LEGACY_CORE_PATH: &str = "/opt/sbin/wdtt-client";
pub const WORK_DIR: &str = "/opt/var/lib/csqtt";
pub const PID_FILE: &str = "/opt/var/run/csqtt-client.pid";
pub const PROVIDER_ID: &str = "vkturn";
pub const PROXY_NAME: &str = "📞 VK-туннель [БС]";
/// PEER-порт сервера csqtt на VPS по умолчанию (deploy.sh: `CSQTT_PEER_PORT`, 46000).
pub const DEFAULT_SERVER_PORT: u16 = 46000;
/// TUN-интерфейс, через который Mihomo отправляет трафик в туннель.
pub const TUN_IFACE: &str = "csq0";
/// Имя абстрактного unix-сокета, по которому клиент ждёт дескриптор TUN.
pub const TUN_UDS_NAME: &str = "xkeen_route_csqtt_tun";
/// MTU для TUN-интерфейса поверх WebRTC/TURN (1280 = MAX_PACKET_PAYLOAD ядра csqtt, исключает 0xCF-фрагментацию).
pub const TUN_MTU: u16 = 1280;
/// Максимальный TCP MSS для соединений через csq0 (1280 - 60 байт запаса на IP/TCP заголовки и опции).
pub const TUN_TCP_MSS: u16 = 1220;
/// Отдельная таблица маршрутизации для сокетов, привязанных к csq0 (в пределах 1..252 для Busybox).
pub const ROUTE_TABLE: u32 = 222;
/// Подсети инфраструктуры ВК (AS47541) и TURN-серверов звонков. Трафик клиента туда идёт строго напрямую.
/// Конкретные адреса TURN, которые ВК выдал клиенту, панель дополнительно берёт из его лога.
pub const VK_NETWORK_CIDRS: &[&str] = &[
    "155.212.192.0/20",
    "87.240.128.0/18",
    "93.186.224.0/20",
    "95.142.192.0/20",
    "95.213.0.0/17",
    "185.32.248.0/22",
];
pub const VK_TURN_CIDRS: &[&str] = &["155.212.192.0/20"];

/// Хосты VK API из focsq (schedule_vk_endpoint_excludes), которые ядро посещает напрямую
pub const VK_API_HOSTS: &[&str] = &[
    "api.vk.me",
    "api.vk.com",
    "api.vk.ru",
    "login.vk.ru",
    "login.vk.com",
    "id.vk.ru",
    "id.vk.com",
    "oauth.vk.ru",
    "oauth.vk.com",
];
pub const RULES_BEGIN: &str = "# BEGIN xkeen-route vkturn";
pub const RULES_END: &str = "# END xkeen-route vkturn";
const LOG_TAIL: usize = 300;
/// csqtt делит потоки на группы по 10 (`WORKERS_PER_GROUP` в worker.rs), каждая группа берёт свой хеш звонка.
pub const WORKERS_PER_GROUP: u32 = 10;
/// Автоматическое значение для роутера. Ручной выбор по-прежнему поддерживает до 100 потоков.
pub const DEFAULT_WORKERS: u32 = 20;
/// `MAX_WORKERS` в main.rs клиента.
pub const MAX_WORKERS: u32 = 100;
/// Максимум ссылок: все используются одновременно (`MAX_VK_HASHES` в main.rs клиента).
pub const MAX_LINKS: usize = 6;
/// Сколько адресов TURN запоминаем для исключений (защита от разрастания правил).
const MAX_TURN_IPS: usize = 32;
const EVENT_PREFIX: &str = "__CSQTT_EVENT__|";
/// Диагностика этапов каждого потока клиента csqtt (stream_diagnostics.rs).
const STREAM_PREFIX: &str = "__CSQTT_STREAM__|";
/// Сколько тайм-аутов GETCONF подряд необходимо для сообщения о недоступности сервера.
const GETCONF_SILENT_THRESHOLD: u32 = 3;
const SERVER_SILENT_ON_PASSWORD: &str =
    "Сервер csqtt не ответил на запрос конфигурации: проверьте соединение и пароль подключения";
const CAPTCHA_SOLVE_PREFIX: &str = "CAPTCHA_SOLVE|";
/// Ответ клиенту на запрос WebView-капчи: на роутере WebView нет.
pub const CAPTCHA_NO_WEBVIEW_REPLY: &str = "CAPTCHA_RESULT|error:webview unavailable on router\n";

/// SHA-256 бинарников CORE_TAG (вывод scripts/build-csqtt-core.sh).
pub const CORE_SHA256: &[(&str, &str)] = &[
    ("csqtt-client-linux-arm64", "27cbeab0783758820c3973c7eda81b7ca83bcaed84b5579be5c0521509f9053b"),
];

/// Сопоставление с архитектурами от updater::detect_mihomo_arch().
/// Клиент csqtt написан на Rust; для MIPS-роутеров у Rust нет стабильной сборки с musl.
pub fn core_asset_for_arch(arch: &str) -> Option<&'static str> {
    let a = arch.to_ascii_lowercase();
    if a.contains("arm64") || a.contains("aarch64") {
        Some("csqtt-client-linux-arm64")
    } else {
        None
    }
}

// ---------- Ссылки на звонки, адрес сервера, пароль ----------

/// Строгая проверка ссылки (или голого хеша) на звонок ВК. Возвращает хеш звонка.
/// Хеш уходит аргументом процесса и в config.json — только безопасные символы.
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

/// Число потоков для `-n`: поддерживает точные потоки (3-100) или дефолтные 20.
pub fn effective_workers(cfg: &VkTurnConfig) -> u32 {
    let wanted = if cfg.streams == 0 { DEFAULT_WORKERS } else { cfg.streams as u32 };
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

/// Аргументы передаются МАССИВОМ, без shell — никакой подстановки команд. Пароля здесь нет:
/// он уходит в `CSQTT_PASSWORD` (см. client_env).
pub fn build_client_args_with_hashes(cfg: &VkTurnConfig, hashes: &[String]) -> Vec<String> {
    build_client_args_for_uds(cfg, hashes, &new_tun_uds_name())
}

fn new_tun_uds_name() -> String {
    format!("{TUN_UDS_NAME}_{}", crate::auth::random_hex(8))
}

fn build_client_args_for_uds(cfg: &VkTurnConfig, hashes: &[String], uds_name: &str) -> Vec<String> {
    vec![
        "-peer".into(),
        cfg.peer.trim().to_string(),
        "-vk".into(),
        hashes.join(","),
        "-n".into(),
        effective_workers(cfg).to_string(),
        "--exact-workers".into(),
        "-listen".into(),
        "127.0.0.1:0".into(),
        "-tun-uds".into(),
        uds_name.into(),
        "-captcha-mode".into(),
        "auto".into(),
        "-obfs".into(),
        "audio".into(),
    ]
}

pub fn build_client_args(cfg: &VkTurnConfig) -> Vec<String> {
    build_client_args_with_hashes(cfg, &call_hashes(cfg))
}

/// Переменные окружения клиента.
pub fn client_env(cfg: &VkTurnConfig) -> Vec<(&'static str, String)> {
    vec![("CSQTT_EVENTS", "1".into()), ("CSQTT_PASSWORD", cfg.password.clone())]
}

// ---------- Разбор вывода клиента и состояние ----------

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum VkTurnPhase {
    Disabled,
    NotInstalled,
    Starting,
    Authorizing,
    SolvingCaptcha,
    Connected,
    /// процесс жив, но активных соединений 0
    Degraded,
    CaptchaRequired,
    LinkInvalid,
    /// сервер csqtt отклонил пароль (неверный, истёк, деактивирован)
    AuthFailed,
    /// рукопожатие с сервером на VPS не проходит
    ServerUnreachable,
    Crashed,
    Backoff,
}

#[derive(Debug, Clone, Serialize)]
pub struct VkTurnRuntime {
    pub phase: VkTurnPhase,
    pub pid: Option<u32>,
    pub connected_streams: u32,
    pub bytes_up: u64,
    pub bytes_down: u64,
    pub restarts: u32,
    pub last_error: Option<String>,
    pub error_source: Option<&'static str>,
    pub error_reason: Option<&'static str>,
    /// Адрес туннеля, выданный сервером (TUNCONF).
    pub tunnel_ip: Option<Ipv4Addr>,
    /// DNS, который сервер предлагает внутри туннеля.
    pub tunnel_dns: Option<String>,
    /// Дескриптор TUN передан текущему процессу клиента.
    pub tun_attached: bool,
    pub tun_configured: bool,
    #[serde(skip)]
    run_generation: u64,
    /// Адреса TURN, которые ВК выдал клиенту — для исключений в iptables.
    #[serde(skip)]
    pub turn_ips: BTreeSet<Ipv4Addr>,
    #[serde(skip)]
    pub started_at: Option<Instant>,
    #[serde(skip)]
    pub zero_streams_since: Option<Instant>,
    /// Подряд оставшиеся без ответа запросы GETCONF при живом DTLS (признак неверного пароля).
    #[serde(skip)]
    pub getconf_silent: u32,
    pub log_tail: VecDeque<String>,
}

impl Default for VkTurnRuntime {
    fn default() -> Self {
        Self {
            phase: VkTurnPhase::Disabled,
            pid: None,
            connected_streams: 0,
            bytes_up: 0,
            bytes_down: 0,
            restarts: 0,
            last_error: None,
            error_source: None,
            error_reason: None,
            tunnel_ip: None,
            tunnel_dns: None,
            tun_attached: false,
            tun_configured: false,
            run_generation: 0,
            turn_ips: BTreeSet::new(),
            started_at: None,
            zero_streams_since: None,
            getconf_silent: 0,
            log_tail: VecDeque::with_capacity(LOG_TAIL),
        }
    }
}

impl VkTurnRuntime {
    /// Туннель готов принимать трафик Mihomo: сервер выдал адрес и TUN передан клиенту.
    pub fn tunnel_ready(&self) -> bool {
        self.tunnel_ip.is_some() && self.tun_attached && self.tun_configured
    }
}

/// Что надзору нужно сделать в ответ на строку вывода клиента.
#[derive(Debug, Clone, PartialEq)]
pub enum LineEffect {
    None,
    /// Клиент ждёт ответ на запрос WebView-капчи в stdin.
    CaptchaSolve,
    /// Сервер выдал адрес туннеля: настроить csq0 и передать клиенту дескриптор TUN.
    TunConfig { ip: Ipv4Addr, dns: Option<String> },
}

/// Состояния, которые объясняют паузу пользователю: перебор client_id не должен прятать их за «Подключаюсь…».
fn is_sticky(phase: VkTurnPhase) -> bool {
    matches!(
        phase,
        VkTurnPhase::CaptchaRequired | VkTurnPhase::LinkInvalid | VkTurnPhase::AuthFailed | VkTurnPhase::ServerUnreachable
    )
}

fn push_log(rt: &mut VkTurnRuntime, line: String) {
    if rt.log_tail.len() >= LOG_TAIL {
        rt.log_tail.pop_front();
    }
    rt.log_tail.push_back(line);
}

/// Сервер промолчал на GETCONF: тайм-аут не доказывает отказ в авторизации.
fn note_getconf_silent(rt: &mut VkTurnRuntime) {
    rt.getconf_silent = rt.getconf_silent.saturating_add(1);
    if rt.getconf_silent >= GETCONF_SILENT_THRESHOLD
        && rt.connected_streams == 0
        && !matches!(rt.phase, VkTurnPhase::CaptchaRequired | VkTurnPhase::LinkInvalid | VkTurnPhase::AuthFailed)
    {
        rt.phase = VkTurnPhase::ServerUnreachable;
        rt.last_error = Some(SERVER_SILENT_ON_PASSWORD.into());
        rt.error_source = Some("network");
        rt.error_reason = Some("server_timeout");
    }
}

/// `__CSQTT_STREAM__|{json}`: успешные этапы в журнал не пишем (это сотни строк JSON в минуту),
/// ошибки — коротко и по-человечески.
fn apply_stream_line(rt: &mut VkTurnRuntime, payload: &str) {
    let json: serde_json::Value = serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);
    let stage = json.get("stage").and_then(|v| v.as_str()).unwrap_or("");
    let status = json.get("status").and_then(|v| v.as_str()).unwrap_or("");
    let reason = json.get("reason").and_then(|v| v.as_str()).unwrap_or("");
    if stage == "getconf" {
        match status {
            "ok" => rt.getconf_silent = 0,
            "error" if reason.to_lowercase().contains("timeout") => note_getconf_silent(rt),
            _ => {}
        }
    }
    if status == "error" {
        let worker = json.get("worker").and_then(|v| v.as_i64()).unwrap_or(0);
        push_log(rt, redact_line(&format!("[ПОТОК #{worker}] {stage}: {reason}")));
    }
}

fn set_connected(rt: &mut VkTurnRuntime) {
    rt.getconf_silent = 0;
    rt.phase = VkTurnPhase::Connected;
    rt.zero_streams_since = None;
    rt.last_error = None;
    rt.error_source = None;
    rt.error_reason = None;
}

/// `TUNCONF:<ip>:<dns>:<…>` → адрес и DNS туннеля (protocol.rs сервера: `TUNCONF:{ip}:{dns}:0:stream-v1`).
pub fn parse_tunconf(config: &str) -> Option<(Ipv4Addr, Option<String>)> {
    let rest = config.trim().strip_prefix("TUNCONF:")?;
    let mut fields = rest.splitn(3, ':');
    let ip: Ipv4Addr = fields.next()?.trim().parse().ok()?;
    if ip.is_unspecified() || ip.is_loopback() || ip.is_multicast() {
        return None;
    }
    let dns = fields
        .next()
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty() && d.split(',').all(|x| x.trim().parse::<Ipv4Addr>().is_ok()));
    Some((ip, dns))
}

/// Чистая функция — покрыта тестами на строках из исходников csqtt.
pub fn apply_log_line(rt: &mut VkTurnRuntime, line: &str, now: Instant) -> LineEffect {
    let line = line.trim_end_matches('\r');
    let t = line.trim();
    if t.is_empty() {
        return LineEffect::None;
    }

    if let Some(rest) = t.strip_prefix(EVENT_PREFIX) {
        let (kind, payload) = rest.split_once('|').unwrap_or((rest, ""));
        let json: serde_json::Value = serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);
        return apply_event(rt, kind, &json, now);
    }
    if let Some(payload) = t.strip_prefix(STREAM_PREFIX) {
        apply_stream_line(rt, payload);
        return LineEffect::None;
    }

    if t.starts_with(CAPTCHA_SOLVE_PREFIX) {
        // В строке session_token и redirect_uri капчи — в лог не пишем.
        push_log(rt, "[КАПЧА] Клиент просит окно капчи — на роутере его нет, продолжаю автоматическое решение".into());
        if rt.connected_streams == 0 && !is_sticky(rt.phase) {
            rt.phase = VkTurnPhase::SolvingCaptcha;
        }
        return LineEffect::CaptchaSolve;
    }

    push_log(rt, redact_line(line));
    learn_turn_ips(rt, line);

    let no_streams = rt.connected_streams == 0;
    if line.contains("FATAL_AUTH") {
        rt.phase = VkTurnPhase::AuthFailed;
        rt.last_error = Some(auth_error_text(line).into());
        rt.error_source = Some("csqtt");
        rt.error_reason = Some(password_error_reason(line));
    } else if line.contains("User authorization failed")
        || line.contains("User was deleted or banned")
        || line.contains("API 5:")
    {
        rt.phase = VkTurnPhase::AuthFailed;
        rt.last_error = Some("Вход в аккаунт VK отклонён (аккаунт заблокирован или токен отозван). Пожалуйста, войдите с другого аккаунта.".into());
        rt.error_source = Some("vk");
        rt.error_reason = Some("authorization_rejected");
    } else if line.contains("WRAP_AUTH_TIMEOUT") {
        if no_streams && rt.phase != VkTurnPhase::AuthFailed {
            rt.phase = VkTurnPhase::ServerUnreachable;
            rt.error_source = Some("network");
            rt.error_reason = Some("server_timeout");
            rt.last_error = Some(
                "Сервер csqtt на VPS не отвечает: проверьте адрес, порт и пароль подключения (при неверном пароле сервер молчит)".into(),
            );
        }
    } else if is_call_unavailable_line(line) {
        if no_streams {
            rt.phase = VkTurnPhase::LinkInvalid;
        }
        rt.last_error = Some("Ссылка на звонок ВК не работает: звонок завершён или удалён".into());
        rt.error_source = Some("vk");
        rt.error_reason = Some("call_unavailable");
    } else if is_captcha_failed_line(line) {
        if no_streams {
            rt.phase = VkTurnPhase::CaptchaRequired;
        }
        rt.last_error = Some("ВК требует подтвердить «я не робот», автоматически пройти проверку не удалось".into());
        rt.error_source = Some("vk");
        rt.error_reason = Some("captcha_required");
    } else if line.contains("[КАПЧА]") || line.contains("[Captcha]") {
        if no_streams && !is_sticky(rt.phase) {
            rt.phase = VkTurnPhase::SolvingCaptcha;
        }
    } else if line.contains("[VKCalls]")
        || line.contains("[VK Auth]")
        || line.contains("Запрос кредов")
        || line.contains("[DTLS]")
        || line.contains("[HANDSHAKE]")
    {
        if no_streams && !is_sticky(rt.phase) && rt.phase != VkTurnPhase::SolvingCaptcha {
            rt.phase = VkTurnPhase::Authorizing;
        }
    } else if line.contains("[ФАТАЛ]") || line.contains("Нужен пароль") || line.contains("Нет хешей VK") {
        rt.phase = VkTurnPhase::Crashed;
        rt.last_error = Some(redact_line(line));
        rt.error_source = Some("csqtt");
        rt.error_reason = Some("client_error");
    }
    LineEffect::None
}

fn apply_event(rt: &mut VkTurnRuntime, kind: &str, json: &serde_json::Value, now: Instant) -> LineEffect {
    match kind {
        "READY" => {
            rt.connected_streams = rt.connected_streams.max(1);
            set_connected(rt);
        }
        "STATS" => {
            let active = json.get("active").and_then(|v| v.as_i64()).unwrap_or(0).max(0) as u32;
            if let Some(up) = json.get("bytes_up").and_then(|v| v.as_i64()) {
                rt.bytes_up = up.max(0) as u64;
            }
            if let Some(down) = json.get("bytes_down").and_then(|v| v.as_i64()) {
                rt.bytes_down = down.max(0) as u64;
            }
            rt.connected_streams = active;
            if active > 0 {
                set_connected(rt);
            } else {
                rt.zero_streams_since.get_or_insert(now);
                if rt.phase == VkTurnPhase::Connected {
                    rt.phase = VkTurnPhase::Degraded;
                }
            }
        }
        "CONFIG" => {
            if let Some(conf) = json.get("config").and_then(|v| v.as_str()) {
                if let Some((ip, dns)) = parse_tunconf(conf) {
                    if rt.tunnel_ip != Some(ip) {
                        push_log(rt, format!("[ТУННЕЛЬ] Сервер выдал адрес {ip}"));
                    }
                    return LineEffect::TunConfig { ip, dns };
                }
            }
        }
        "TUN_LOST" => {
            rt.tun_attached = false;
            push_log(rt, "[ТУННЕЛЬ] Клиент потерял TUN-интерфейс — повторяю передачу".into());
            if let Some(ip) = rt.tunnel_ip {
                return LineEffect::TunConfig { ip, dns: rt.tunnel_dns.clone() };
            }
        }
        "ERROR" => {
            let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("ошибка клиента");
            let code = json.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let fatal = json.get("fatal").and_then(|v| v.as_bool()).unwrap_or(false);
            push_log(rt, redact_line(&format!("[ОШИБКА] {msg}")));
            if code.contains("auth") || msg.contains("FATAL_AUTH") {
                rt.phase = VkTurnPhase::AuthFailed;
                rt.last_error = Some(auth_error_text(msg).into());
                rt.error_source = Some("csqtt");
                rt.error_reason = Some(password_error_reason(msg));
            } else {
                rt.last_error = Some(redact_line(msg));
                rt.error_source = Some("csqtt");
                rt.error_reason = Some("client_error");
                if fatal {
                    rt.phase = VkTurnPhase::Crashed;
                }
            }
        }
        _ => {}
    }
    LineEffect::None
}

/// Тексты отказов сервера csqtt (protocol.rs клиента: wrong_password / expired / deactivated).
fn auth_error_text(line: &str) -> &'static str {
    let l = line.to_lowercase();
    if l.contains("неверный пароль") || l.contains("invalid password") || l.contains("wrong_password") {
        "Сервер csqtt отклонил пароль подключения: пароль неверный"
    } else if l.contains("истёк") || l.contains("expired") {
        "Срок действия пароля подключения истёк — получите новый у владельца сервера"
    } else if l.contains("деактивирован") || l.contains("deactivated") {
        "Пароль подключения отключён на сервере csqtt"
    } else {
        "Сервер csqtt отказал в доступе"
    }
}

fn password_error_reason(line: &str) -> &'static str {
    let text = line.to_lowercase();
    if text.contains("expired") || text.contains("истёк") { "password_expired" }
    else if text.contains("deactivated") || text.contains("деактивирован") { "password_deactivated" }
    else if text.contains("wrong_password") || text.contains("invalid password") || text.contains("неверный пароль") { "password_invalid" }
    else { "authentication_rejected" }
}

/// Ошибки ВК «звонок недоступен».
fn is_call_unavailable_line(line: &str) -> bool {
    line.contains("VK returns error:")
        || line.contains("VK call is unavailable")
        || line.contains("non-retryable call error")
        || line.contains("хеш мёртв")
        || line.contains("Call not found")
}

/// Автоматическое решение капчи окончательно не удалось.
fn is_captcha_failed_line(line: &str) -> bool {
    line.contains("CAPTCHA_WAIT_REQUIRED")
        || line.contains("FATAL_CAPTCHA")
        || line.contains("[Captcha] Solve failed")
        || line.contains("[Captcha] Max attempts reached")
}

/// Адреса TURN из строк лога клиента: `turn_server.urls[0]=95.163.34.143:19302`, `[TURN] Подключение к …`,
/// `TURN: [95.163.34.143:19302 …]`, `Relay: 95.163.34.131:62836`.
fn learn_turn_ips(rt: &mut VkTurnRuntime, line: &str) {
    if !line.contains("TURN") && !line.contains("turn") && !line.contains("Relay") {
        return;
    }
    let re = regex_lite::Regex::new(r"(?:^|[^0-9.])(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}):\d{2,5}").unwrap();
    for c in re.captures_iter(line) {
        if let Ok(ip) = c[1].parse::<Ipv4Addr>() {
            let usable = !ip.is_loopback() && !ip.is_private() && !ip.is_unspecified() && !ip.is_link_local();
            if usable && rt.turn_ips.len() < MAX_TURN_IPS {
                rt.turn_ips.insert(ip);
            }
        }
    }
}

/// Ссылки на звонок и токены не пишем в лог панели целиком.
/// Также скрываются токены сессий ВК: клиент печатает redirect_uri капчи с session_token.
pub fn redact_line(line: &str) -> String {
    let links = regex_lite::Regex::new(r"(call/join/)([A-Za-z0-9_\-]{4})[A-Za-z0-9_\-]*").unwrap();
    let tokens = regex_lite::Regex::new(r#"((?:session_token|access_token|success_token|token)=)[^&\s\]"']+"#).unwrap();
    let hashes = regex_lite::Regex::new(r"((?:хеш|hash)[^:\n]{0,12}:\s*)([A-Za-z0-9_\-]{4})[A-Za-z0-9_\-]{4,}").unwrap();
    let out = links.replace_all(line, "$1$2…");
    let out = tokens.replace_all(&out, "${1}…");
    hashes.replace_all(&out, "$1$2…").to_string()
}

/// Сколько ждать без единого активного соединения, прежде чем перезапустить клиент.
pub fn stall_limit(phase: VkTurnPhase) -> Duration {
    match phase {
        VkTurnPhase::AuthFailed => Duration::from_secs(15),
        VkTurnPhase::LinkInvalid => Duration::from_secs(20),
        VkTurnPhase::CaptchaRequired => Duration::from_secs(45),
        VkTurnPhase::SolvingCaptcha => Duration::from_secs(300),
        _ => Duration::from_secs(120),
    }
}

// ---------- YAML для Mihomo ----------

fn is_ipv4(s: &str) -> bool {
    s.parse::<Ipv4Addr>().is_ok()
}

/// Содержимое file-провайдера `vkturn`: трафик узла уходит в TUN-интерфейс клиента csqtt.
/// Значений пользователя в нём нет — YAML-инъекция невозможна.
pub fn render_provider_yaml() -> String {
    format!("proxies:\n  - name: \"{PROXY_NAME}\"\n    type: direct\n    interface-name: {TUN_IFACE}\n    routing-mark: {ROUTE_TABLE}\n    ip-version: ipv4\n    udp: true\n    tfo: false\n")
}

/// Отключает принудительный `tfo: true` внутри секции `proxy-providers -> vkturn` существующего `config.yaml`,
/// не затрагивая остальные провайдеры и оставляя YAML без изменений, если правок не требуется.
pub fn normalize_vkturn_provider_in_yaml(yaml: &str) -> String {
    let eol = if yaml.contains("\r\n") { "\r\n" } else { "\n" };
    let had_trailing_eol = yaml.ends_with('\n');
    let mut in_providers = false;
    let mut vkturn_indent: Option<usize> = None;
    let mut changed = false;
    let mut out: Vec<String> = Vec::new();

    for line in yaml.lines() {
        let trimmed = line.trim();
        let indent = line.len() - line.trim_start_matches(' ').len();

        if indent == 0 && !trimmed.is_empty() && !trimmed.starts_with('#') {
            in_providers = trimmed == "proxy-providers:";
            vkturn_indent = None;
            out.push(line.to_string());
            continue;
        }

        if in_providers && !trimmed.is_empty() && !trimmed.starts_with('#') {
            if let Some(vi) = vkturn_indent {
                if indent <= vi {
                    vkturn_indent = None;
                }
            }
            if vkturn_indent.is_none() && trimmed == format!("{PROVIDER_ID}:") {
                vkturn_indent = Some(indent);
                out.push(line.to_string());
                continue;
            }
            if vkturn_indent.is_some() {
                let (before_comment, comment_part) = match line.find('#') {
                    Some(idx) => (&line[..idx], &line[idx..]),
                    None => (line, ""),
                };
                if before_comment.trim() == "tfo: true" {
                    let prefix = &before_comment[..before_comment.len() - before_comment.trim_start().len()];
                    let suffix_ws = &before_comment[before_comment.trim_end().len()..];
                    out.push(format!("{prefix}tfo: false{suffix_ws}{comment_part}"));
                    changed = true;
                    continue;
                }
            }
        }

        out.push(line.to_string());
    }

    if !changed {
        return yaml.to_string();
    }
    let mut joined = out.join(eol);
    if had_trailing_eol && !joined.ends_with(eol) {
        joined.push_str(eol);
    }
    joined
}

/// Правила DIRECT в начало `rules:`, чтобы трафик к TURN, VK API и к VPS никогда не ушёл в туннель или петлю.
pub fn rules_block(cfg: &VkTurnConfig) -> String {
    let mut lines = vec![format!("  {RULES_BEGIN}")];
    for c in VK_TURN_CIDRS {
        lines.push(format!("  - IP-CIDR,{c},DIRECT,no-resolve"));
    }
    for host in VK_API_HOSTS {
        lines.push(format!("  - DOMAIN,{host},DIRECT"));
    }
    if let Some((host, _)) = cfg.peer.trim().rsplit_once(':') {
        let clean_host = host.trim_matches(['[', ']']);
        if is_ipv4(clean_host) {
            lines.push(format!("  - IP-CIDR,{clean_host}/32,DIRECT,no-resolve"));
        } else if !clean_host.is_empty() {
            lines.push(format!("  - DOMAIN,{clean_host},DIRECT"));
        }
    }
    lines.push(format!("  {RULES_END}"));
    lines.join("\n")
}

/// Сделано по образцу apply_rules в routing.rs:
/// идемпотентно, с validate_marker_pair, с сохранением CRLF, блок вставляется сразу после строки `rules:`.
pub fn apply_rules(yaml: &str, cfg: &VkTurnConfig) -> Result<String, String> {
    crate::routing::validate_marker_pair(yaml, RULES_BEGIN, RULES_END)?;
    let cleaned = remove_rules(yaml);
    if !cfg.enabled {
        return Ok(cleaned);
    }
    let eol = if cleaned.contains("\r\n") { "\r\n" } else { "\n" };
    let block = rules_block(cfg).replace('\n', eol);
    let mut out = Vec::new();
    let mut inserted = false;
    for line in cleaned.lines() {
        out.push(line.to_string());
        if !inserted && line.trim_end() == "rules:" {
            out.push(block.clone());
            inserted = true;
        }
    }
    if !inserted {
        return Err("В config.yaml нет секции rules:".into());
    }
    Ok(out.join(eol))
}

pub fn remove_rules(yaml: &str) -> String {
    let eol = if yaml.contains("\r\n") { "\r\n" } else { "\n" };
    let mut out = Vec::new();
    let mut skip = false;
    for line in yaml.lines() {
        let t = line.trim();
        if t == RULES_BEGIN {
            skip = true;
            continue;
        }
        if t == RULES_END {
            skip = false;
            continue;
        }
        if !skip {
            out.push(line);
        }
    }
    out.join(eol)
}

// ---------- Исключения в iptables ----------
//
// XKeen вешает цепочку `xkeen_mask` на OUTPUT (собственный трафик роутера), Zapret — `zapret`
// на OUTPUT для TCP 80/443. Соединения самого клиента к TURN и к VPS не должны попадать ни в прокси
// (петля), ни под десинхронизацию nfqws. Правило RETURN в начале этих цепочек выводит их из обработки.

pub const IPT_COMMENT: &str = "xkeen-route-vkturn";

fn ipv4_in_cidr(ip: Ipv4Addr, cidr: &str) -> bool {
    let Some((net_str, prefix_str)) = cidr.split_once('/') else {
        return false;
    };
    let (Ok(net), Ok(prefix)) = (net_str.parse::<Ipv4Addr>(), prefix_str.parse::<u32>()) else {
        return false;
    };
    if prefix == 0 {
        return true;
    }
    if prefix > 32 {
        return false;
    }
    let mask = !0u32 << (32 - prefix);
    (u32::from(ip) & mask) == (u32::from(net) & mask)
}

/// Адреса, к которым клиент подключается сам. Только константы и разобранные Ipv4Addr — без инъекций.
pub fn exempt_destinations(peer_ipv4: Option<Ipv4Addr>, turn_ips: &BTreeSet<Ipv4Addr>) -> Vec<String> {
    let mut v: Vec<String> = VK_NETWORK_CIDRS.iter().map(|c| c.to_string()).collect();
    let in_vk_subnet = |ip: &Ipv4Addr| VK_NETWORK_CIDRS.iter().any(|cidr| ipv4_in_cidr(*ip, cidr));
    for ip in turn_ips.iter().filter(|ip| !in_vk_subnet(ip)) {
        v.push(format!("{ip}/32"));
    }
    if let Some(ip) = peer_ipv4 {
        let s = format!("{ip}/32");
        if !v.contains(&s) {
            v.push(s);
        }
    }
    v
}

/// Идемпотентно: правило добавляется, только если его ещё нет; отсутствующие цепочки пропускаются.
pub fn iptables_apply_script(dsts: &[String]) -> String {
    let list = dsts.join(" ");
    format!(
        r#"for tbl in mangle nat; do
  for ch in xkeen xkeen_mask XKEEN_BS zapret; do
    iptables -w -t "$tbl" -nL "$ch" >/dev/null 2>&1 || continue
    iptables -w -t "$tbl" -C "$ch" -m mark --mark {ROUTE_TABLE} -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null \
      || iptables -w -t "$tbl" -I "$ch" 1 -m mark --mark {ROUTE_TABLE} -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null || true
    iptables -w -t "$tbl" -C "$ch" -o {TUN_IFACE} -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null \
      || iptables -w -t "$tbl" -I "$ch" 1 -o {TUN_IFACE} -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null || true
    iptables -w -t "$tbl" -C "$ch" -i {TUN_IFACE} -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null \
      || iptables -w -t "$tbl" -I "$ch" 1 -i {TUN_IFACE} -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null || true
    for dst in {list}; do
      iptables -w -t "$tbl" -C "$ch" -d "$dst" -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null \
        || iptables -w -t "$tbl" -I "$ch" 1 -d "$dst" -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null || true
    done
  done
done"#
    )
}

/// Снимает все наши правила (по комментарию), в том числе для старого адреса VPS.
pub fn iptables_remove_script() -> String {
    format!(
        r#"for tbl in mangle nat; do
  for ch in xkeen xkeen_mask XKEEN_BS zapret; do
    iptables -w -t "$tbl" -S "$ch" 2>/dev/null | grep -- "--comment {IPT_COMMENT}" | sed 's/^-A /-D /' | while read -r rule; do
      eval iptables -w -t "$tbl" $rule 2>/dev/null || true
    done
  done
done
for ch in POSTROUTING OUTPUT FORWARD PREROUTING INPUT; do
  iptables -w -t mangle -S "$ch" 2>/dev/null | grep -- "--comment {IPT_COMMENT}" | sed 's/^-A /-D /' | while read -r rule; do
    eval iptables -w -t mangle $rule 2>/dev/null || true
  done
done
for ch in OUTPUT FORWARD; do
  iptables -w -t filter -S "$ch" 2>/dev/null | grep -- "--comment {IPT_COMMENT}" | sed 's/^-A /-D /' | while read -r rule; do
    eval iptables -w -t filter $rule 2>/dev/null || true
  done
done"#
    )
}

async fn run_sh(script: &str) {
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("sh").arg("-c").arg(script).output().await;
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = script;
    }
}

async fn run_sh_checked(script: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let mut command = tokio::process::Command::new("sh");
        command.arg("-c").arg(script).kill_on_drop(true);
        let output = tokio::time::timeout(Duration::from_secs(5), command.output()).await
            .map_err(|_| "Превышено время настройки сетевых правил".to_string())?
            .map_err(|e| format!("Не удалось запустить настройку сетевых правил: {e}"))?;
        if !output.status.success() {
            return Err(format!("Не удалось настроить сетевые правила (код {:?})", output.status.code()));
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = script;
        Ok(())
    }
}

/// IPv4 сервера на VPS: либо сам адрес из peer, либо результат DNS-запроса (таймаут 3 с).
pub async fn resolve_peer_ipv4(peer: &str) -> Option<Ipv4Addr> {
    let (host, port) = peer.trim().rsplit_once(':')?;
    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return Some(ip);
    }
    let port: u16 = port.parse().ok()?;
    let addrs = tokio::time::timeout(Duration::from_secs(3), tokio::net::lookup_host((host, port)))
        .await
        .ok()?
        .ok()?;
    addrs
        .filter_map(|a| match a.ip() {
            std::net::IpAddr::V4(v4) => Some(v4),
            _ => None,
        })
        .next()
}

pub async fn apply_exemptions(peer: &str, turn_ips: &BTreeSet<Ipv4Addr>) {
    let ip = resolve_peer_ipv4(peer).await;
    run_sh(&iptables_apply_script(&exempt_destinations(ip, turn_ips))).await;
}

pub async fn remove_exemptions() {
    run_sh(&iptables_remove_script()).await;
}

// ---------- TUN-интерфейс csq0 ----------

/// Адрес туннеля на csq0 и маршрутизация сокетов, привязанных к интерфейсу (Mihomo `interface-name`).
/// В скрипт попадают только константы и разобранный Ipv4Addr.
/// Совместимо с Busybox на KeeneticOS: таблица < 255 и селектор dev вместо oif.
pub fn tun_configure_script(ip: Ipv4Addr) -> String {
    format!(
        r#"set -e
ip rule show | sed -n 's/^40:.*from \([0-9.]*\) .*lookup {ROUTE_TABLE}.*/\1/p' | while read -r old_ip; do ip rule del pref 40 from "$old_ip" table {ROUTE_TABLE} 2>/dev/null || true; done
ip rule del pref {ROUTE_TABLE} dev {TUN_IFACE} table {ROUTE_TABLE} 2>/dev/null || true
ip rule del pref 45 fwmark {ROUTE_TABLE} table {ROUTE_TABLE} 2>/dev/null || true
ip route del default dev {TUN_IFACE} table {ROUTE_TABLE} 2>/dev/null || true
ip addr flush dev {TUN_IFACE} 2>/dev/null || true
existing_rules="$(ip rule show)"
if printf '%s\n' "$existing_rules" | grep -Eq '^(40|45|{ROUTE_TABLE}):'; then exit 1; fi
existing_routes="$(ip route show table {ROUTE_TABLE} 2>/dev/null || true)"
if [ -n "$existing_routes" ]; then exit 1; fi
addr_added=0; route_added=0; rule_dev=0; rule_src=0; rule_mark=0
cleanup() {{
  [ "$rule_mark" = 0 ] || ip rule del pref 45 fwmark {ROUTE_TABLE} table {ROUTE_TABLE} 2>/dev/null || true
  [ "$rule_src" = 0 ] || ip rule del pref 40 from {ip} table {ROUTE_TABLE} 2>/dev/null || true
  [ "$rule_dev" = 0 ] || ip rule del pref {ROUTE_TABLE} dev {TUN_IFACE} table {ROUTE_TABLE} 2>/dev/null || true
  [ "$route_added" = 0 ] || ip route del default dev {TUN_IFACE} table {ROUTE_TABLE} 2>/dev/null || true
  [ "$addr_added" = 0 ] || ip addr del {ip}/32 dev {TUN_IFACE} 2>/dev/null || true
}}
trap cleanup EXIT
ip addr add {ip}/32 dev {TUN_IFACE}; addr_added=1
ip link set dev {TUN_IFACE} mtu {TUN_MTU} up
ip route add default dev {TUN_IFACE} table {ROUTE_TABLE}; route_added=1
ip rule add dev {TUN_IFACE} table {ROUTE_TABLE} pref {ROUTE_TABLE}; rule_dev=1
ip rule add from {ip} table {ROUTE_TABLE} pref 40; rule_src=1
ip rule add fwmark {ROUTE_TABLE} table {ROUTE_TABLE} pref 45; rule_mark=1
echo 2 > /proc/sys/net/ipv4/conf/{TUN_IFACE}/rp_filter 2>/dev/null || true
for ch in POSTROUTING OUTPUT; do
  iptables -w -t mangle -C "$ch" -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS} 2>/dev/null \
    || iptables -w -t mangle -I "$ch" 1 -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS} 2>/dev/null || true
done
for ch in PREROUTING INPUT FORWARD; do
  iptables -w -t mangle -C "$ch" -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS} 2>/dev/null \
    || iptables -w -t mangle -I "$ch" 1 -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS} 2>/dev/null || true
done
for ch in OUTPUT FORWARD; do
  iptables -w -t filter -C "$ch" -o {TUN_IFACE} -p udp --dport 443 -m comment --comment {IPT_COMMENT} -j REJECT 2>/dev/null \
    || iptables -w -t filter -I "$ch" 1 -o {TUN_IFACE} -p udp --dport 443 -m comment --comment {IPT_COMMENT} -j REJECT 2>/dev/null || true
done
trap - EXIT"#
    )
}

/// Снимает маршрутизацию туннеля (сам интерфейс исчезает, когда панель закрывает дескриптор TUN).
pub fn tun_teardown_script() -> String {
    tun_teardown_script_for_ip(None)
}

fn tun_teardown_script_for_ip(ip: Option<Ipv4Addr>) -> String {
    let source_rule = ip.map(|ip| format!("ip rule del pref 40 from {ip} table {ROUTE_TABLE} 2>/dev/null || true\nip addr del {ip}/32 dev {TUN_IFACE} 2>/dev/null || true\n"))
        .unwrap_or_default();
    format!(
        "for ch in POSTROUTING OUTPUT; do
  while iptables -w -t mangle -D \"$ch\" -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS} 2>/dev/null; do :; done
done
for ch in PREROUTING INPUT FORWARD; do
  while iptables -w -t mangle -D \"$ch\" -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS} 2>/dev/null; do :; done
done
for ch in OUTPUT FORWARD; do
  while iptables -w -t filter -D \"$ch\" -o {TUN_IFACE} -p udp --dport 443 -m comment --comment {IPT_COMMENT} -j REJECT 2>/dev/null; do :; done
done
ip rule del pref {ROUTE_TABLE} dev {TUN_IFACE} table {ROUTE_TABLE} 2>/dev/null || true
{source_rule}ip rule del pref 45 fwmark {ROUTE_TABLE} table {ROUTE_TABLE} 2>/dev/null || true
ip route del default dev {TUN_IFACE} table {ROUTE_TABLE} 2>/dev/null || true"
    )
}

async fn apply_tun_config<F, Fut>(
    rt: &RwLock<VkTurnRuntime>, ip: Ipv4Addr, dns: Option<String>, apply: F,
) -> Result<(), String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let previous_ip = {
        let mut state = rt.write().await;
        if state.tun_configured && state.tunnel_ip == Some(ip) {
            state.tunnel_dns = dns;
            return Ok(());
        }
        let previous = if state.tun_configured { state.tunnel_ip } else { None };
        state.tun_configured = false;
        state.tun_attached = false;
        state.tunnel_ip = None;
        previous
    };
    let script = match previous_ip {
        Some(previous) => format!("{}\n{}", tun_teardown_script_for_ip(Some(previous)), tun_configure_script(ip)),
        None => tun_configure_script(ip),
    };
    apply(script).await?;
    let mut state = rt.write().await;
    state.tunnel_ip = Some(ip);
    state.tunnel_dns = dns;
    state.tun_configured = true;
    Ok(())
}

#[cfg(target_os = "linux")]
mod tun_sys {
    use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

    #[cfg(any(target_arch = "mips", target_arch = "mips64", target_arch = "powerpc", target_arch = "powerpc64"))]
    const TUNSETIFF: u64 = 0x800454ca;
    #[cfg(not(any(target_arch = "mips", target_arch = "mips64", target_arch = "powerpc", target_arch = "powerpc64")))]
    const TUNSETIFF: u64 = 0x400454ca;
    const IFF_TUN: i16 = 0x0001;
    const IFF_NO_PI: i16 = 0x1000;

    #[repr(C)]
    struct IfReq {
        name: [u8; 16],
        flags: i16,
        _pad: [u8; 22],
    }

    /// Создаёт TUN-интерфейс (без заголовка PI — как VpnService на Android).
    pub fn create(name: &str) -> std::io::Result<OwnedFd> {
        let path = c"/dev/net/tun";
        let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_CLOEXEC) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut req = IfReq { name: [0; 16], flags: IFF_TUN | IFF_NO_PI, _pad: [0; 22] };
        let bytes = name.as_bytes();
        if bytes.len() >= req.name.len() {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "слишком длинное имя интерфейса"));
        }
        req.name[..bytes.len()].copy_from_slice(bytes);
        let rc = unsafe { libc::ioctl(fd.as_raw_fd(), TUNSETIFF as _, &mut req as *mut IfReq) };
        if rc < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(fd)
    }

    /// Передаёт дескриптор TUN клиенту csqtt по абстрактному unix-сокету (SCM_RIGHTS) и ждёт подтверждения «1».
    pub fn send_fd(uds_name: &str, tun: RawFd) -> std::io::Result<()> {
        use std::io::Read;
        use std::os::linux::net::SocketAddrExt;
        let addr = std::os::unix::net::SocketAddr::from_abstract_name(uds_name.as_bytes())?;
        let mut stream = std::os::unix::net::UnixStream::connect_addr(&addr)?;
        stream.set_read_timeout(Some(std::time::Duration::from_secs(3)))?;

        let mut payload = [1u8];
        let mut iov = libc::iovec { iov_base: payload.as_mut_ptr().cast(), iov_len: 1 };
        let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<RawFd>() as u32) } as usize;
        let mut control = vec![0u8; space];
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = control.as_mut_ptr().cast();
        msg.msg_controllen = space as _;
        unsafe {
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            (*cmsg).cmsg_len = libc::CMSG_LEN(std::mem::size_of::<RawFd>() as u32) as _;
            std::ptr::copy_nonoverlapping(
                &tun as *const RawFd as *const u8,
                libc::CMSG_DATA(cmsg),
                std::mem::size_of::<RawFd>(),
            );
        }
        let sent = unsafe { libc::sendmsg(stream.as_raw_fd(), &msg, libc::MSG_NOSIGNAL) };
        if sent < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut ack = [0u8; 1];
        stream.read_exact(&mut ack)?;
        if ack[0] != 1 {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "клиент не подтвердил TUN"));
        }
        Ok(())
    }
}

fn retry_fd_send<F>(
    cancel: &std::sync::atomic::AtomicBool,
    attempts: usize,
    delay: Duration,
    mut send: F,
) -> Result<(), String>
where
    F: FnMut() -> Result<(), String>,
{
    let mut last = String::new();
    for _ in 0..attempts {
        if cancel.load(std::sync::atomic::Ordering::Acquire) { return Err("Передача TUN отменена".into()); }
        match send() {
            Ok(()) => return Ok(()),
            Err(e) => last = e,
        }
        let until = Instant::now() + delay;
        while Instant::now() < until {
            if cancel.load(std::sync::atomic::Ordering::Acquire) { return Err("Передача TUN отменена".into()); }
            std::thread::sleep(until.saturating_duration_since(Instant::now()).min(Duration::from_millis(50)));
        }
    }
    Err(format!("Клиент не принял TUN-интерфейс: {last}"))
}

/// Дескриптор TUN, которым владеет надзор. На других ОС (тесты на Windows) — пустышка.
struct TunHandle {
    #[cfg(target_os = "linux")]
    fd: std::os::fd::OwnedFd,
}

impl TunHandle {
    fn create() -> Result<Self, String> {
        #[cfg(target_os = "linux")]
        {
            tun_sys::create(TUN_IFACE)
                .map(|fd| Self { fd })
                .map_err(|e| format!("Не удалось создать TUN-интерфейс {TUN_IFACE}: {e}"))
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(Self {})
        }
    }

    /// Копия дескриптора для фоновой передачи клиенту (надзор может закрыть свой раньше).
    fn duplicate(&self) -> Result<TunHandle, String> {
        #[cfg(target_os = "linux")]
        {
            self.fd.try_clone().map(|fd| TunHandle { fd }).map_err(|e| format!("dup TUN: {e}"))
        }
        #[cfg(not(target_os = "linux"))]
        {
            Ok(TunHandle {})
        }
    }

    /// Блокирующая передача: клиент поднимает сокет не сразу после старта — до 60 попыток раз в 0,5 с.
    fn send_to_client_blocking(&self, uds_name: &str, cancel: &std::sync::atomic::AtomicBool) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            retry_fd_send(cancel, 60, Duration::from_millis(500), || {
                tun_sys::send_fd(uds_name, self.fd.as_raw_fd()).map_err(|e| e.to_string())
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (uds_name, cancel);
            Err("TUN поддерживается только на Linux".into())
        }
    }
}

// ---------- Запись в config.json / config.yaml (одна транзакция) ----------

/// Сохраняет настройки VK-туннеля. Mihomo перечитывает конфигурацию только при изменении
/// правил или файла провайдера. Всё в одной транзакции ConfigTx.
pub async fn commit_vkturn<F>(state: &crate::AppState, mutate: F) -> Result<VkTurnConfig, String>
where
    F: FnOnce(&mut VkTurnConfig) -> Result<(), String>,
{
    let mut tx = crate::transaction::ConfigTx::begin(state)
        .await
        .map_err(|e| format!("Ошибка начала транзакции: {e}"))?;

    let mut new_cfg = tx.config().vkturn.clone();
    mutate(&mut new_cfg)?;
    tx.config_mut().vkturn = new_cfg.clone();
    if new_cfg.enabled {
        if !tx.config().whitelist_providers.iter().any(|p| p == PROVIDER_ID) {
            tx.config_mut().whitelist_providers.push(PROVIDER_ID.to_string());
        }
    } else {
        tx.config_mut().whitelist_providers.retain(|p| p != PROVIDER_ID);
    }

    let yaml = tx.read_yaml().await.map_err(|e| format!("Ошибка чтения config.yaml: {e}"))?;
    let has_prov = crate::routing::parse_provider_names(&yaml).iter().any(|p| p == PROVIDER_ID);
    let mut provider_changed = false;
    if new_cfg.enabled {
        let providers_dir = std::path::Path::new(&tx.config().mihomo.config_path)
            .parent()
            .map(|p| p.join("providers"))
            .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));
        let prov_file = providers_dir.join(format!("{PROVIDER_ID}.yaml"));
        let provider_yaml = render_provider_yaml();
        provider_changed = match tokio::fs::read_to_string(&prov_file).await {
            Ok(current) => current != provider_yaml,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
            Err(e) => return Err(format!("Ошибка чтения файла провайдера VK-туннеля: {e}")),
        };
        if provider_changed {
            tx.set_extra_file(&prov_file, provider_yaml, true)
                .await
                .map_err(|e| format!("Ошибка записи файла провайдера VK-туннеля: {e}"))?;
        }
    }
    let yaml_with_prov = if new_cfg.enabled && !has_prov {
        crate::routing::add_file_provider_to_yaml(
            &yaml,
            PROVIDER_ID,
            &format!("./providers/{PROVIDER_ID}.yaml"),
            Some(tx.config().health_check_url()),
            Some(tx.config().mihomo.health_check_interval),
        )
        .map_err(|e| format!("Ошибка добавления провайдера в YAML: {e}"))?
    } else if !new_cfg.enabled && has_prov {
        crate::routing::delete_provider_from_yaml(&yaml, PROVIDER_ID).unwrap_or_else(|_| yaml.clone())
    } else {
        yaml.clone()
    };
    let yaml_with_prov = if new_cfg.enabled {
        normalize_vkturn_provider_in_yaml(&yaml_with_prov)
    } else {
        yaml_with_prov
    };

    let new_yaml = apply_rules(&yaml_with_prov, &new_cfg)
        .map_err(|e| format!("Ошибка применения правил маршрутизации VK-туннеля: {e}"))?;
    // Преобразование правил может убрать перевод строки в конце файла.
    let yaml_changed = new_yaml.trim_end_matches(['\r', '\n']) != yaml.trim_end_matches(['\r', '\n']);
    if yaml_changed {
        tx.set_yaml(new_yaml).map_err(|e| format!("Ошибка сохранения нового YAML: {e}"))?;
    }
    let result = if yaml_changed || provider_changed {
        tx.commit_and_reload().await
    } else {
        tx.commit_without_reload().await
    };
    result.map_err(|e| format!("Ошибка транзакции сохранения: {e}"))?;
    Ok(new_cfg)
}

/// Убирает узел VK-туннеля из активных селекторов Mihomo (`PROXY`, `🛡️ Whitelist`, `GLOBAL`),
/// если туннель выключен или сброшен, чтобы маршрутизация не зависла на мёртвом прокси.
pub async fn evacuate_vkturn_selectors(state: &crate::AppState) {
    let cfg = state.config.read().await.clone();
    let Ok(proxies) = crate::mihomo::get_proxies(&state.http, &cfg).await else { return };
    for group_name in ["PROXY", "🛡️ Whitelist", "GLOBAL"] {
        let Some(group) = proxies.get(group_name) else { continue };
        let typ = group.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if !typ.eq_ignore_ascii_case("selector") {
            continue;
        }
        if group.get("now").and_then(|n| n.as_str()) != Some(PROXY_NAME) {
            continue;
        }
        let target = group
            .get("all")
            .and_then(|a| a.as_array())
            .and_then(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .find(|id| *id != PROXY_NAME && *id != "REJECT" && *id != "DIRECT")
                    .or_else(|| {
                        arr.iter()
                            .filter_map(|v| v.as_str())
                            .find(|id| *id != PROXY_NAME && *id != "REJECT")
                    })
                    .map(|s| s.to_string())
            });
        if let Some(target_node) = target {
            if let Err(e) = crate::mihomo::switch_selector(&state.http, &cfg, group_name, &target_node).await {
                crate::log_w!("[VK-туннель] Не удалось перевести селектор {group_name} с выключенного туннеля: {e}");
            } else {
                crate::log_i!("[VK-туннель] Селектор {group_name} автоматически переведён на '{target_node}'");
            }
        }
    }
}

/// Синхронизирует файл провайдера/YAML и фоновый супервизор VK-туннеля после восстановления бэкапа.
pub async fn sync_vkturn_after_restore(state: &crate::AppState) {
    let _update_guard = state.vkturn.config_update_lock.lock().await;
    let synced = match commit_vkturn(state, |_| Ok(())).await {
        Ok(c) => c,
        Err(e) => {
            crate::log_w!("[VK-туннель] Не удалось синхронизировать конфигурацию после восстановления бэкапа: {e}");
            state.config.read().await.vkturn.clone()
        }
    };
    if synced.enabled {
        state.vkturn.restart(synced, state.clone()).await;
    } else {
        state.vkturn.stop().await;
        evacuate_vkturn_selectors(state).await;
    }
}

// ---------- Менеджер процесса ----------

/// Запущенный надзор: канал остановки и задача, завершения которой ждём при остановке.
struct Supervisor {
    stop_tx: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

struct ClientLogTasks(Vec<tokio::task::JoinHandle<()>>);

impl ClientLogTasks {
    async fn stop(&mut self) {
        for task in &self.0 { task.abort(); }
        for task in self.0.drain(..) { let _ = task.await; }
    }
}

impl Drop for ClientLogTasks {
    fn drop(&mut self) {
        for task in &self.0 { task.abort(); }
    }
}

struct FdHandoff {
    cancel: Arc<std::sync::atomic::AtomicBool>,
    task: tokio::task::JoinHandle<()>,
}

#[derive(Debug, PartialEq, Eq)]
enum HandoffDecision { Wait, Start, Restart }

fn handoff_decision(configured: bool, attached: bool, task_running: bool, failures: u8, retry_ready: bool) -> HandoffDecision {
    if !configured || attached || task_running { return HandoffDecision::Wait; }
    if failures >= 3 { return HandoffDecision::Restart; }
    if retry_ready { HandoffDecision::Start } else { HandoffDecision::Wait }
}

struct HandoffRecovery {
    configured: bool,
    failures: u8,
    retry_at: Instant,
}

impl HandoffRecovery {
    fn new() -> Self { Self { configured: false, failures: 0, retry_at: Instant::now() } }
    fn failed(&mut self) {
        self.failures = self.failures.saturating_add(1);
        self.retry_at = Instant::now() + Duration::from_secs(1 << self.failures.min(3));
    }
}

impl FdHandoff {
    async fn stop(mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Release);
        let _ = tokio::time::timeout(Duration::from_secs(5), &mut self.task).await;
    }
}

impl Drop for FdHandoff {
    fn drop(&mut self) {
        self.cancel.store(true, std::sync::atomic::Ordering::Release);
    }
}

pub struct VkTurnManager {
    pub rt: RwLock<VkTurnRuntime>,
    /// Сериализует изменение VK-настроек целиком — от чтения актуального конфига до перезапуска.
    /// Без этого два сохранения могли применить конфиг в одном порядке, а запустить процессы в другом.
    pub config_update_lock: Mutex<()>,
    /// Сериализует restart/stop целиком: два одновременных перезапуска не могут оставить
    /// «осиротевший» надзор и второй клиент.
    op_lock: Mutex<Option<Supervisor>>,
    /// Путь к бинарнику клиента (в тестах подменяется).
    core_path: std::path::PathBuf,
    /// Число живых задач надзора — для диагностики и анти-регрессионного теста гонки перезапусков.
    active_supervisors: Arc<std::sync::atomic::AtomicUsize>,
    /// Фоновая установка ядра: скачивание идёт дольше таймаута запроса веб-панели (30 с).
    pub install: Arc<std::sync::Mutex<CoreInstall>>,
}

/// Состояние фоновой установки ядра для статуса VK-туннеля.
#[derive(Debug, Clone, Default, Serialize, PartialEq)]
pub struct CoreInstall {
    pub running: bool,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}

/// Учитывает живую задачу надзора: +1 при создании, −1 при любом выходе (в том числе при abort,
/// даже если задача ни разу не успела выполниться — guard живёт внутри future и падает вместе с ним).
struct SupervisorGuard(Arc<std::sync::atomic::AtomicUsize>);
impl SupervisorGuard {
    fn new(counter: &Arc<std::sync::atomic::AtomicUsize>) -> Self {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Self(counter.clone())
    }
}
impl Drop for SupervisorGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Состояния, которые объясняют паузу пользователю и не должны затираться на «Переподключаюсь».
pub fn keeps_phase_during_backoff(phase: VkTurnPhase) -> bool {
    is_sticky(phase)
}

/// Что делать после завершения клиента.
#[derive(Debug, PartialEq)]
pub struct ExitDecision {
    pub wait: Duration,
    pub notice: Option<&'static str>,
}

const LONG_PAUSE: Duration = Duration::from_secs(600);

/// Чистая функция решения после выхода клиента (покрыта тестами).
/// Все ссылки клиент использует одновременно, поэтому переключать нечего: при окончательной
/// ошибке — длинная пауза, чтобы не долбить ВК и сервер. Уведомление — один раз на серию сбоев.
pub fn decide_after_exit(
    phase: VkTurnPhase,
    ran_for: Duration,
    backoff: &mut Duration,
    last_notice: &mut Option<VkTurnPhase>,
) -> ExitDecision {
    // Долгая успешная работа — новая серия: сбрасываем паузу и «уже предупреждали».
    if ran_for > Duration::from_secs(300) {
        *backoff = Duration::from_secs(5);
        *last_notice = None;
    }
    let mut notice_once = |p: VkTurnPhase, text: &'static str| -> Option<&'static str> {
        if *last_notice == Some(p) {
            None
        } else {
            *last_notice = Some(p);
            Some(text)
        }
    };
    match phase {
        VkTurnPhase::CaptchaRequired => ExitDecision {
            wait: LONG_PAUSE,
            notice: notice_once(phase, "📞 VK-туннель: ВК запросил капчу, автоматически пройти не удалось. Повторяю попытки раз в 10 минут."),
        },
        VkTurnPhase::LinkInvalid => ExitDecision {
            wait: LONG_PAUSE,
            notice: notice_once(phase, "📞 VK-туннель: ссылка на звонок ВК не работает. Добавьте новую ссылку в панели."),
        },
        VkTurnPhase::AuthFailed => ExitDecision {
            wait: LONG_PAUSE,
            notice: notice_once(phase, "📞 VK-туннель: сервер на VPS отклонил пароль подключения. Проверьте пароль в панели."),
        },
        _ => {
            if ran_for <= Duration::from_secs(300) {
                *backoff = (*backoff * 2).min(Duration::from_secs(120));
            }
            ExitDecision { wait: *backoff, notice: None }
        }
    }
}

impl VkTurnManager {
    pub fn new() -> Arc<Self> {
        Self::with_core_path(CORE_PATH)
    }

    pub fn with_core_path(path: impl Into<std::path::PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            rt: RwLock::new(VkTurnRuntime::default()),
            config_update_lock: Mutex::new(()),
            op_lock: Mutex::new(None),
            core_path: path.into(),
            active_supervisors: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            install: Arc::new(std::sync::Mutex::new(CoreInstall::default())),
        })
    }

    pub fn install_state(&self) -> CoreInstall {
        self.install.lock().map(|g| g.clone()).unwrap_or_default()
    }

    /// Помечает начало установки. `false` — установка уже идёт (повторное нажатие не запускает вторую).
    pub fn begin_install(&self) -> bool {
        let Ok(mut g) = self.install.lock() else { return false };
        if g.running {
            return false;
        }
        *g = CoreInstall { running: true, ..Default::default() };
        true
    }

    pub fn finish_install(&self, result: &Result<String, String>) {
        if let Ok(mut g) = self.install.lock() {
            g.running = false;
            g.error = result.as_ref().err().cloned();
        }
    }

    pub fn active_supervisors(&self) -> usize {
        self.active_supervisors.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn core_installed() -> bool {
        std::path::Path::new(CORE_PATH).exists()
    }

    /// Идемпотентно: останавливает текущий процесс и запускает надзор с новым конфигом.
    pub async fn restart(self: &Arc<Self>, cfg: VkTurnConfig, state: crate::AppState) {
        let mut slot = self.op_lock.lock().await;
        self.stop_locked(&mut slot).await;
        // Старые исключения снимаются всегда: адрес VPS мог смениться. Надзор поставит актуальные.
        remove_exemptions().await;
        let has_token = selected_vk_token(&cfg).is_some();
        if !cfg.enabled || (!has_token && call_hashes(&cfg).is_empty()) || cfg.password.is_empty() || cfg.peer.is_empty() {
            self.rt.write().await.phase = VkTurnPhase::Disabled;
            return;
        }
        if !self.core_path.exists() {
            self.rt.write().await.phase = VkTurnPhase::NotInstalled;
            return;
        }
        kill_stale_from_pidfile().await;
        let (stop_tx, rx) = watch::channel(false);
        let me = self.clone();
        let alive = SupervisorGuard::new(&self.active_supervisors);
        let task = tokio::spawn(async move {
            let _alive = alive;
            me.supervise(cfg, rx, state).await
        });
        *slot = Some(Supervisor { stop_tx, task });
    }

    pub async fn stop(&self) {
        let mut slot = self.op_lock.lock().await;
        self.stop_locked(&mut slot).await;
        remove_exemptions().await;
    }

    /// Останавливает надзор и ЖДЁТ его завершения (процесс клиента убит, TUN закрыт до выхода из функции).
    async fn stop_locked(&self, slot: &mut Option<Supervisor>) {
        self.begin_run().await;
        if let Some(sup) = slot.take() {
            let _ = sup.stop_tx.send(true);
            let mut task = sup.task;
            if tokio::time::timeout(Duration::from_secs(15), &mut task).await.is_err() {
                task.abort();
                crate::log_w!("[VK-туннель] Надзор не завершился за 15 с — прерван принудительно");
            }
        }
        let owned_ip = {
            let rt = self.rt.read().await;
            if rt.tun_configured { rt.tunnel_ip } else { None }
        };
        if owned_ip.is_some() { let _ = run_sh_checked(&tun_teardown_script_for_ip(owned_ip)).await; }
        let _ = tokio::fs::remove_file(PID_FILE).await;
        let mut rt = self.rt.write().await;
        rt.pid = None;
        rt.connected_streams = 0;
        rt.tunnel_ip = None;
        rt.tun_attached = false;
        rt.tun_configured = false;
        rt.phase = VkTurnPhase::Disabled;
    }

    async fn supervise(self: Arc<Self>, cfg: VkTurnConfig, mut stop_rx: watch::Receiver<bool>, state: crate::AppState) {
        let mut backoff = Duration::from_secs(5);
        let mut last_notice: Option<VkTurnPhase> = None;
        let _ = tokio::fs::create_dir_all(WORK_DIR).await;

        // TUN живёт, пока жив надзор: перезапуски клиента получают тот же интерфейс.
        let tun = loop {
            match TunHandle::create() {
                Ok(t) => break t,
                Err(e) => {
                    self.set_error(VkTurnPhase::Crashed, e).await;
                    if !sleep_or_stop(&mut stop_rx, backoff).await {
                        return;
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(120));
                }
            }
        };
        let tun = Arc::new(tun);
        let mut auto_call_ids: Vec<String> = Vec::new();

        loop {
            if *stop_rx.borrow() {
                break;
            }
            // Перед каждым запуском: XKeen/Zapret могли пересоздать свои цепочки и стереть исключения.
            let known_turn = self.rt.read().await.turn_ips.clone();
            apply_exemptions(&cfg.peer, &known_turn).await;

            // Завершаем прошлые авто-звонки, если были
            if let Some(tok) = selected_vk_token(&cfg) {
                finish_vk_calls_batch(&state.http, tok, &auto_call_ids).await;
                auto_call_ids.clear();
            }

            // Получаем хеши звонков: автоматически через calls.start (как в focsq) или из конфига
            let hashes = if let Some(tok) = selected_vk_token(&cfg) {
                if !tok.trim().is_empty() {
                    let needed = call_count_for_workers(effective_workers(&cfg));
                    let mut created_ids = Vec::new();
                    let mut created_hashes = Vec::new();
                    let mut last_err = None;

                    for i in 0..needed {
                        if i > 0 {
                            tokio::time::sleep(Duration::from_millis(150)).await;
                        }
                        match start_vk_call(&state.http, tok).await {
                            Ok((cid, h)) => {
                                crate::log_i!("[VK-туннель] Авто-звонок {cid} (хеш: {h}) успешно создан через API calls.start");
                                created_ids.push(cid);
                                created_hashes.push(h);
                            }
                            Err((is_invalid, e)) => {
                                last_err = Some((is_invalid, e));
                                break;
                            }
                        }
                    }

                    if created_hashes.is_empty() {
                        let (is_invalid, err_msg) = last_err.unwrap_or((false, "Не удалось создать звонок".into()));
                        crate::log_w!("[VK-туннель] Ошибка создания авто-звонка через API: {err_msg}");
                        {
                            let mut rt = self.rt.write().await;
                            rt.phase = if is_invalid { VkTurnPhase::AuthFailed } else { VkTurnPhase::Backoff };
                            rt.last_error = Some(err_msg);
                            rt.error_source = Some(if is_invalid { "vk" } else { "network" });
                            rt.error_reason = Some(if is_invalid { "authorization_rejected" } else { "call_creation_temporary" });
                        }
                        if is_invalid {
                            crate::log_w!("[VK-туннель] Токен VK отозван или аккаунт заблокирован. Остановка надзора до повторного входа.");
                            break;
                        }
                        if !sleep_or_stop(&mut stop_rx, backoff).await {
                            break;
                        }
                        backoff = (backoff * 2).min(Duration::from_secs(120));
                        continue;
                    }
                    auto_call_ids = created_ids;
                    created_hashes
                } else {
                    call_hashes(&cfg)
                }
            } else {
                call_hashes(&cfg)
            };

            let run_generation = self.begin_run().await;
            let uds_name = new_tun_uds_name();
            let mut cmd = tokio::process::Command::new(&self.core_path);
            cmd.args(build_client_args_for_uds(&cfg, &hashes, &uds_name))
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
            for (k, v) in client_env(&cfg) {
                cmd.env(k, v);
            }
            if std::path::Path::new(WORK_DIR).is_dir() {
                cmd.current_dir(WORK_DIR);
            }
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    self.set_error(VkTurnPhase::Crashed, format!("Не удалось запустить клиент: {e}")).await;
                    if !sleep_or_stop(&mut stop_rx, backoff).await {
                        break;
                    }
                    backoff = (backoff * 2).min(Duration::from_secs(120));
                    continue;
                }
            };
            let pid = child.id();
            if let Some(p) = pid {
                let _ = tokio::fs::write(PID_FILE, p.to_string()).await;
            }
            {
                let mut rt = self.rt.write().await;
                rt.phase = VkTurnPhase::Starting;
                rt.pid = pid;
                rt.connected_streams = 0;
                rt.tun_attached = false;
                rt.started_at = Some(Instant::now());
                rt.zero_streams_since = Some(Instant::now());
                rt.getconf_silent = 0;
            }
            crate::log_i!(
                "[VK-туннель] Клиент csqtt запущен (pid {:?}, ссылок {}, потоков {})",
                pid,
                hashes.len(),
                effective_workers(&cfg)
            );

            // stdout (события, CAPTCHA_SOLVE) и stderr (лог) → разбор; действия — в канал надзора.
            let (eff_tx, mut eff_rx) = mpsc::unbounded_channel::<LineEffect>();
            let mut log_tasks = ClientLogTasks(Vec::new());
            for stream in [
                child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
                child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            ]
            .into_iter()
            .flatten()
            {
                log_tasks.0.push(self.spawn_log_reader(stream, eff_tx.clone()));
            }
            drop(eff_tx);
            let mut stdin = child.stdin.take();
            let mut fd_handoff: Option<FdHandoff> = None;
            let mut handoff_recovery = HandoffRecovery::new();

            // ждём: выход процесса / команду стоп / «висим без соединений» дольше допустимого
            let started = Instant::now();
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            let mut handoff_tick = tokio::time::interval(Duration::from_secs(1));
            let mut ticks: u32 = 0;
            let mut last_tun_rx = 0u64;
            let mut last_tun_tx = 0u64;
            let mut blackhole_ticks: u32 = 0;
            let mut exempted_turn = known_turn;
            let exit_reason = loop {
                tokio::select! {
                    st = child.wait() => break format!("клиент завершился: {:?}", st.ok()),
                    _ = stop_rx.changed() => {
                        eff_rx.close();
                        log_tasks.stop().await;
                        if let Some(handoff) = fd_handoff.take() { handoff.stop().await; }
                        // Завершаем звонки в VK API параллельно
                        if let Some(tok) = selected_vk_token(&cfg) {
                            finish_vk_calls_batch(&state.http, tok, &auto_call_ids).await;
                        }
                        // Мягкая остановка: клиент закрывает сессии, сервер сразу освобождает слоты.
                        if let Some(mut si) = stdin.take() {
                            let _ = si.write_all(b"STOP\n").await;
                            let _ = si.flush().await;
                        }
                        if tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_err() {
                            let _ = child.kill().await;
                        }
                        return;
                    }
                    Some(eff) = eff_rx.recv() => match eff {
                        LineEffect::CaptchaSolve => {
                            if let Some(si) = stdin.as_mut() {
                                let _ = si.write_all(CAPTCHA_NO_WEBVIEW_REPLY.as_bytes()).await;
                                let _ = si.flush().await;
                            }
                        }
                        LineEffect::TunConfig { ip, dns } => {
                            if let Err(error) = apply_tun_config(&self.rt, ip, dns, |script| async move { run_sh_checked(&script).await }).await {
                                self.set_error(VkTurnPhase::Crashed, error).await;
                                let _ = child.kill().await;
                                break "не удалось настроить интерфейс или маршруты туннеля".to_string();
                            }
                            handoff_recovery.configured = true;
                            if self.drive_handoff(&tun, run_generation, &uds_name, &mut fd_handoff, &mut handoff_recovery).await {
                                let _ = child.kill().await;
                                break "не удалось передать TUN после нескольких попыток".to_string();
                            }
                        }
                        LineEffect::None => {}
                    },
                    _ = handoff_tick.tick() => {
                        if self.drive_handoff(&tun, run_generation, &uds_name, &mut fd_handoff, &mut handoff_recovery).await {
                            let _ = child.kill().await;
                            break "не удалось передать TUN после нескольких попыток".to_string();
                        }
                    }
                    _ = tick.tick() => {
                        ticks = ticks.wrapping_add(1);
                        let rt = self.rt.read().await;
                        let turn_now = rt.turn_ips.clone();
                        let is_connected = rt.phase == VkTurnPhase::Connected && rt.tun_attached;
                        let stalled = rt.connected_streams == 0
                            && rt.zero_streams_since.map(|t| t.elapsed() > stall_limit(rt.phase)).unwrap_or(false);
                        drop(rt);

                        // раз в минуту (или при новых адресах TURN) восстанавливаем исключения
                        if ticks % 6 == 0 || turn_now != exempted_turn {
                            apply_exemptions(&cfg.peer, &turn_now).await;
                            exempted_turn = turn_now;
                        }

                        // Zero-CPU Blackhole Detector:
                        // Каждые 30 секунд (3 тика) пассивно проверяем дельту счетчиков csq0 из /proc/net/dev.
                        // Если клиенты шлют трафик (TX delta > 30 КБ), а ответа нет вообще (RX delta == 0)
                        // в течение 60 секунд подряд (2 замера) — сессия TURN заблокирована/сброшена:
                        // производим мягкий перезапуск для восстановления связи.
                        if is_connected && ticks % 3 == 0 {
                            let (cur_rx, cur_tx) = read_tun_interface_bytes(TUN_IFACE);
                            if last_tun_rx > 0 || last_tun_tx > 0 {
                                let tx_delta = cur_tx.saturating_sub(last_tun_tx);
                                let rx_delta = cur_rx.saturating_sub(last_tun_rx);
                                if tx_delta > 30_000 && rx_delta == 0 {
                                    blackhole_ticks += 1;
                                    if blackhole_ticks >= 2 {
                                        crate::log_w!(
                                            "[VK-туннель] Blackhole Detector: TX delta {} байт, RX delta 0 байт за 60 сек — сессия сброшена, мягкий перезапуск",
                                            tx_delta
                                        );
                                        let _ = child.kill().await;
                                        break "сессия TURN заблокирована (нет входящих пакетов при активном TX)".to_string();
                                    }
                                } else {
                                    blackhole_ticks = 0;
                                }
                            }
                            last_tun_rx = cur_rx;
                            last_tun_tx = cur_tx;
                        }

                        if stalled {
                            let _ = child.kill().await;
                            break "нет ни одного соединения дольше допустимого".to_string();
                        }
                    }
                }
            };

            eff_rx.close();
            log_tasks.stop().await;
            self.begin_run().await;
            if let Some(handoff) = fd_handoff.take() { handoff.stop().await; }

            let owned_ip = {
                let rt = self.rt.read().await;
                if rt.tun_configured { rt.tunnel_ip } else { None }
            };
            if owned_ip.is_some() {
                let _ = run_sh_checked(&tun_teardown_script_for_ip(owned_ip)).await;
            }

            let phase = {
                let mut rt = self.rt.write().await;
                rt.restarts += 1;
                rt.pid = None;
                rt.connected_streams = 0;
                rt.tun_attached = false;
                rt.tun_configured = false;
                rt.tunnel_ip = None;
                rt.phase
            };
            crate::log_w!("[VK-туннель] Перезапуск: {exit_reason}, состояние {phase:?}");

            let d = decide_after_exit(phase, started.elapsed(), &mut backoff, &mut last_notice);
            if let Some(text) = d.notice {
                notify_user(&state, text).await;
            }
            {
                let mut rt = self.rt.write().await;
                if !keeps_phase_during_backoff(rt.phase) {
                    rt.phase = VkTurnPhase::Backoff;
                }
            }
            if !sleep_or_stop(&mut stop_rx, d.wait).await {
                break;
            }
        }
    }

    fn spawn_log_reader(
        self: &Arc<Self>,
        stream: Box<dyn tokio::io::AsyncRead + Unpin + Send>,
        eff_tx: mpsc::UnboundedSender<LineEffect>,
    ) -> tokio::task::JoinHandle<()> {
        let me = self.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stream).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let mut rt = me.rt.write().await;
                if eff_tx.is_closed() { break; }
                let eff = apply_log_line(&mut *rt, &line, Instant::now());
                drop(rt);
                if eff != LineEffect::None { let _ = eff_tx.send(eff); }
            }
        })
    }

    async fn drive_handoff(
        self: &Arc<Self>,
        tun: &Arc<TunHandle>,
        generation: u64,
        uds_name: &str,
        task: &mut Option<FdHandoff>,
        recovery: &mut HandoffRecovery,
    ) -> bool {
        if task.as_ref().is_some_and(|handoff| handoff.task.is_finished()) {
            if let Some(finished) = task.take() { finished.stop().await; }
            if self.rt.read().await.tun_attached {
                recovery.failures = 0;
            } else {
                recovery.failed();
            }
        }
        let attached = self.rt.read().await.tun_attached;
        match handoff_decision(recovery.configured, attached, task.is_some(), recovery.failures, Instant::now() >= recovery.retry_at) {
            HandoffDecision::Wait => false,
            HandoffDecision::Restart => true,
            HandoffDecision::Start => {
                *task = self.spawn_fd_handoff(tun, generation, uds_name.to_string());
                if task.is_none() { recovery.failed(); }
                recovery.failures >= 3 && task.is_none()
            }
        }
    }

    /// Передаёт клиенту копию дескриптора TUN в фоне: надзор продолжает читать вывод клиента.
    async fn begin_run(&self) -> u64 {
        let mut rt = self.rt.write().await;
        rt.run_generation = rt.run_generation.wrapping_add(1);
        rt.run_generation
    }

    fn spawn_fd_handoff(self: &Arc<Self>, tun: &Arc<TunHandle>, generation: u64, uds_name: String) -> Option<FdHandoff> {
        let copy = match tun.duplicate() {
            Ok(c) => c,
            Err(e) => {
                crate::log_w!("[VK-туннель] {e}");
                return None;
            }
        };
        let me = self.clone();
        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let task = tokio::spawn(async move {
            let res = tokio::task::spawn_blocking(move || copy.send_to_client_blocking(&uds_name, &worker_cancel))
                .await
                .unwrap_or_else(|e| Err(format!("задача передачи TUN: {e}")));
            me.publish_handoff_result(generation, res).await;
        });
        Some(FdHandoff { cancel, task })
    }

    async fn publish_handoff_result(&self, generation: u64, res: Result<(), String>) {
            let mut rt = self.rt.write().await;
            if rt.run_generation != generation { return; }
            match res {
                Ok(()) => {
                    rt.tun_attached = true;
                    push_log(&mut rt, format!("[ТУННЕЛЬ] Интерфейс {TUN_IFACE} передан клиенту"));
                }
                Err(e) => {
                    rt.last_error = Some(e.clone());
                    push_log(&mut rt, format!("[ОШИБКА] {e}"));
                }
            }
    }

    async fn set_error(&self, phase: VkTurnPhase, msg: String) {
        let mut rt = self.rt.write().await;
        rt.phase = phase;
        rt.last_error = Some(msg);
        rt.error_source = None;
        rt.error_reason = None;
    }
}

async fn sleep_or_stop(stop_rx: &mut watch::Receiver<bool>, d: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => !*stop_rx.borrow(),
        _ = stop_rx.changed() => false,
    }
}

async fn kill_stale_from_pidfile() {
    #[cfg(target_os = "linux")]
    {
        let _ = tokio::process::Command::new("killall").arg("csqtt-client").status().await;
    }
    let Ok(s) = tokio::fs::read_to_string(PID_FILE).await else { return };
    let Ok(pid) = s.trim().parse::<u32>() else { return };
    if let Ok(cmd) = tokio::fs::read(format!("/proc/{pid}/cmdline")).await {
        if String::from_utf8_lossy(&cmd).contains("csqtt-client") {
            let _ = tokio::process::Command::new("kill").arg("-9").arg(pid.to_string()).status().await;
        }
    }
    let _ = tokio::fs::remove_file(PID_FILE).await;
}

async fn notify_user(state: &crate::AppState, text: &str) {
    let cfg = state.config.read().await;
    if !cfg.notifications.telegram_enabled
        || cfg.notifications.telegram_bot_token.is_empty()
        || cfg.notifications.telegram_chat_id.is_empty()
    {
        return;
    }
    let proxy_url = cfg.mihomo_proxy_url();
    let http = state.http.clone();
    let token = cfg.notifications.telegram_bot_token.clone();
    let chat_id = cfg.notifications.telegram_chat_id.clone();
    let msg = text.to_string();
    drop(cfg);

    tokio::spawn(async move {
        let _ = crate::notifications::send_telegram_with_proxy(
            &http,
            Some(&proxy_url),
            &token,
            &chat_id,
            &msg,
        )
        .await;
    });
}

// ---------- Установка ядра ----------

/// Целевая архитектура для validate_elf_header по строке detect_mihomo_arch().
pub fn elf_target_for_arch(arch: &str) -> &'static str {
    let a = arch.to_ascii_lowercase();
    if a.starts_with("mipsle") || a.starts_with("mipsel") {
        "mipsel"
    } else if a.starts_with("mips") {
        "mips"
    } else if a.contains("arm64") || a.contains("aarch64") {
        "aarch64"
    } else if a.starts_with("arm") {
        "arm"
    } else {
        "x86_64"
    }
}

pub async fn install_core(state: &crate::AppState) -> Result<String, String> {
    let arch = crate::updater::detect_mihomo_arch().await;
    let asset = core_asset_for_arch(arch).ok_or(format!(
        "VK-туннель на ядре csqtt пока работает только на роутерах с процессором ARM64 (у этого роутера: {arch})"
    ))?;
    let expected = CORE_SHA256
        .iter()
        .find(|(a, _)| *a == asset)
        .map(|(_, h)| *h)
        .ok_or("Для этой архитектуры нет проверенного хеша ядра")?;
    let url = format!("{CORE_RELEASES}/{CORE_TAG}/{asset}");
    let tmp = format!("{CORE_PATH}.tmp");

    let cfg = state.config.read().await.clone();
    let proxy_url = cfg.mihomo_proxy_url();
    let install = state.vkturn.install.clone();
    let progress = move |done: u64, total: Option<u64>| {
        if let Ok(mut g) = install.lock() {
            g.downloaded = done;
            g.total = total;
        }
    };
    let bytes = crate::updater::download_file_verified_with_progress(
        &state.http,
        &url,
        Some(expected),
        &proxy_url,
        crate::updater::DownloadLimits::default(),
        &progress,
    )
    .await?;

    if let Some(parent) = std::path::Path::new(CORE_PATH).parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    if let Err(e) = tokio::fs::write(&tmp, &bytes).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Ошибка записи временного файла: {e}"));
    }

    if let Err(e) = crate::updater::validate_elf_header(&bytes, elf_target_for_arch(arch)) {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Некорректный ELF-заголовок ядра: {e}"));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = tokio::fs::metadata(&tmp).await {
            let mut perms = meta.permissions();
            perms.set_mode(0o755);
            let _ = tokio::fs::set_permissions(&tmp, perms).await;
        }

        // Пробный запуск без пароля: клиент сразу выходит с ошибкой «нужен пароль». Важно лишь,
        // что процесс запустился на этом процессоре (не убит сигналом).
        let test_run = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new(&tmp)
                .env_remove("CSQTT_PASSWORD")
                .stdin(Stdio::null())
                .output(),
        )
        .await;

        match test_run {
            Ok(Ok(out)) => {
                if out.status.code().is_none() {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err("Ядро не запускается на этом процессоре (завершено сигналом)".into());
                }
            }
            Ok(Err(e)) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err(format!("Ядро не запускается на этом процессоре ({e})"));
            }
            Err(_) => {
                let _ = tokio::fs::remove_file(&tmp).await;
                return Err("Таймаут при пробном запуске ядра".into());
            }
        }
    }

    if let Err(e) = tokio::fs::rename(&tmp, CORE_PATH).await {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(format!("Ошибка перемещения бинарника в {CORE_PATH}: {e}"));
    }
    // Ядро прошлой версии панели больше не используется.
    let _ = tokio::fs::remove_file(LEGACY_CORE_PATH).await;
    let _ = tokio::fs::remove_dir_all("/opt/var/lib/wdtt").await;

    Ok(format!("Ядро VK-туннеля csqtt ({asset}) успешно установлено"))
}

// ---------- Тесты ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(links: &[&str]) -> VkTurnConfig {
        let mut cfg = VkTurnConfig::default();
        cfg.enabled = true;
        cfg.peer = "1.2.3.4:46000".into();
        cfg.password = "s3cret-pass".into();
        cfg.links = links.iter().map(|s| s.to_string()).collect();
        cfg
    }

    // Регрессия: установка ядра шла внутри запроса и обрывалась таймаутом веб-панели (30 с).
    // Теперь она фоновая: повторное нажатие не запускает вторую, итог виден в статусе.
    #[test]
    fn core_install_runs_once_and_reports_result() {
        let mgr = VkTurnManager::with_core_path("/nonexistent/csqtt-client");
        assert_eq!(mgr.install_state(), CoreInstall::default());
        assert!(mgr.begin_install());
        assert!(!mgr.begin_install(), "вторая установка не должна стартовать, пока идёт первая");
        mgr.install.lock().unwrap().downloaded = 4096;
        assert!(mgr.install_state().running);
        mgr.finish_install(&Err("данные перестали приходить".into()));
        let st = mgr.install_state();
        assert!(!st.running);
        assert_eq!(st.error.as_deref(), Some("данные перестали приходить"));
        // Новая попытка сбрасывает прошлую ошибку и прогресс
        assert!(mgr.begin_install());
        assert_eq!(mgr.install_state(), CoreInstall { running: true, ..Default::default() });
        mgr.finish_install(&Ok("ok".into()));
        assert_eq!(mgr.install_state().error, None);
    }

    #[test]
    fn test_parse_call_link() {
        assert_eq!(parse_call_link("https://vk.com/call/join/abc12345").unwrap(), "abc12345");
        assert_eq!(
            parse_call_link("  https://vk.com/call/join/Very_Long-Valid_Hash12345/  ").unwrap(),
            "Very_Long-Valid_Hash12345"
        );
        assert_eq!(parse_call_link("https://vk.ru/call/join/abc12345?from=app").unwrap(), "abc12345");
        assert_eq!(parse_call_link("abcDEF_12-34").unwrap(), "abcDEF_12-34", "голый хеш тоже принимается");
        // Реальная ссылка пользователя (vk.ru, хеш с _ и -)
        assert_eq!(
            parse_call_link("https://vk.ru/call/join/dVi6izW61Ty9UuE1hEkU_uSdoNuihCgPQnILjgk4AdI").unwrap(),
            "dVi6izW61Ty9UuE1hEkU_uSdoNuihCgPQnILjgk4AdI"
        );

        let e = parse_call_link("https://telemost.yandex.ru/j/12345678").unwrap_err();
        assert!(e.contains("Телемост"));

        assert!(parse_call_link("http://vk.com/call/join/abc12345").is_err());
        assert!(parse_call_link("https://vk.com.evil.ru/call/join/abc12345").is_err());
        assert!(parse_call_link("https://other.domain.com/call/join/abc12345").is_err());
        assert!(parse_call_link("https://vk.com/call/join/abc12;reboot").is_err());
        assert!(parse_call_link("https://vk.com/call/join/abc$(id)").is_err());
        assert!(parse_call_link("https://vk.com/call/join/short").is_err());
        assert!(parse_call_link("https://vk.com/call/join/has space 1234").is_err());
        assert!(parse_call_link("https://vk.com/call/join/has\nnewline123").is_err());
        assert!(parse_call_link("abc,def12345").is_err(), "запятая сломала бы список -vk");
    }

    #[test]
    fn regression_android_encrypted_profile_is_readable() {
        let link = "csqtt://rGMTblReroqI7mbb2bHPcn2Ht7npV8ThxgOFgtHXHPhRHTbmWrXbOg8IO7oBJMO7wbpulj877bUgDF3do4Af6y1VvT9ipPKD4wwp7_lE6mItWw";
        let (peer, password) = parse_csqtt_link(link).expect("encrypted Android profile");
        assert_eq!(peer, "example.com:46000");
        assert_eq!(password, "dummy");
        let mut corrupted = link.to_string();
        corrupted.pop();
        corrupted.push('A');
        assert!(parse_csqtt_link(&corrupted).is_none());
    }

    #[test]
    fn regression_csqtt_backend_preserves_encoded_password_and_rejects_lost_fields() {
        let parsed = parse_csqtt_link("csqtt://connect?v=2&host=example.com&peer=46000&password=abc%2Bxyz").unwrap();
        assert_eq!(parsed.1, "abc+xyz");
        assert!(parse_csqtt_link("csqtt://connect?v=2&host=example.com&peer=46000&password=p&token=profile").is_none());
        assert!(parse_csqtt_link("csqtt://connect?v=3&host=example.com&peer=46000&password=p").is_none());
        assert!(parse_csqtt_link("csqtt://connect?v=2&host=example.com&host=other.example&peer=46000&password=p").is_none());
    }

    #[test]
    fn regression_csqtt_backend_rejects_invalid_profile_ports() {
        assert!(parse_csqtt_link("csqtt://connect?v=2&host=example.com&peer=46000oops&password=p").is_none());
        assert!(parse_csqtt_link("csqtt://connect?v=2&host=example.com&peer=0&password=p").is_none());
        assert!(parse_csqtt_link("csqtt://p@example.com:99999").is_none());
        assert!(parse_csqtt_link("csqtt://p@example.com:46000oops").is_none());
    }

    #[test]
    fn regression_csqtt_backend_glued_query_and_scheme_less_call_links() {
        assert_eq!(
            parse_call_link("vk.com/call/join/abcDEF_1234567890").unwrap(),
            "abcDEF_1234567890"
        );
        assert_eq!(
            parse_call_link("m.vk.ru/call/join/abcDEF_1234567890?from=app").unwrap(),
            "abcDEF_1234567890"
        );
        let glued = "csqtt://connect?v=2host=example.com&peer=46000&password=secret_pw";
        assert_eq!(
            parse_csqtt_link(glued),
            Some(("example.com:46000".into(), "secret_pw".into()))
        );
        let prof = parse_csqtt_profile(glued).expect("glued profile");
        assert_eq!(prof.peer, "example.com:46000");
        assert_eq!(prof.password, "secret_pw");
        assert!(parse_csqtt_profile("csqtt://connect?v=2host=example.com&peer=46000&password=pwtoken=abc").is_none());
    }

    #[test]
    fn test_validate_and_normalize_peer() {
        assert!(validate_peer("1.2.3.4:46000").is_ok());
        assert!(validate_peer("vps.example.com:46000").is_ok());
        assert!(validate_peer(":46000").is_err());
        assert!(validate_peer("1.2.3.4:0").is_err());
        assert!(validate_peer("1.2.3.4:70000").is_err());
        assert!(validate_peer("a b:1").is_err());
        assert!(validate_peer("1.2.3.4").is_err());

        assert_eq!(normalize_peer("203.0.113.7").unwrap(), "203.0.113.7:46000", "PEER-порт csqtt по умолчанию — 46000");
        assert_eq!(normalize_peer(" vps.example.com:443 ").unwrap(), "vps.example.com:443");
        assert_eq!(normalize_peer("").unwrap(), "");
        assert!(normalize_peer("1.2.3.4;reboot").is_err());

        // Проверка парсинга ссылок csqtt://
        let (p1, pw1) = parse_csqtt_link("csqtt://connect?v=2&host=203.0.113.88&peer=46000&password=secret_pw").unwrap();
        assert_eq!(p1, "203.0.113.88:46000");
        assert_eq!(pw1, "secret_pw");

        let (p2, pw2) = parse_csqtt_link("csqtt://myPass@198.51.100.5:1234").unwrap();
        assert_eq!(p2, "198.51.100.5:1234");
        assert_eq!(pw2, "myPass");

        assert_eq!(normalize_peer("csqtt://connect?v=2&host=1.2.3.4&peer=46000&password=pw").unwrap(), "1.2.3.4:46000");
    }

    #[test]
    fn test_validate_password() {
        assert!(validate_password("abc").is_ok());
        assert!(validate_password("a|b").is_ok(), "пароль идёт в переменную окружения — | допустим");
        assert!(validate_password("").is_err());
        assert!(validate_password("a\nb").is_err());
        assert!(validate_password(&"x".repeat(129)).is_err());
    }

    #[test]
    fn test_effective_workers() {
        let mut cfg = cfg_with(&["https://vk.com/call/join/abc12345"]);
        assert_eq!(effective_workers(&cfg), 20);
        cfg.streams = 35;
        assert_eq!(effective_workers(&cfg), 35, "точное число воркеров");
        cfg.streams = 200;
        assert_eq!(effective_workers(&cfg), 100);
        cfg.streams = 1;
        assert_eq!(effective_workers(&cfg), 3);
    }

    /// Пароль не должен попадать в аргументы процесса (их видно в `ps` любому пользователю роутера).
    #[test]
    fn regression_tun_scripts_do_not_delete_foreign_policy_rules_or_flush_table() {
        let configure = tun_configure_script("10.66.67.3".parse().unwrap());
        let teardown = tun_teardown_script();
        for script in [&configure, &teardown] {
            assert!(!script.contains("ip rule del pref 40 2>"));
            assert!(!script.contains("ip rule del pref 45 2>"));
            assert!(!script.contains("ip rule del pref 222 2>"));
            assert!(!script.contains("ip route flush table 222"));
        }
        assert!(configure.contains("ip rule show"));
        assert!(configure.contains("ip route show table 222"));
    }

    #[tokio::test]
    async fn regression_failed_tun_setup_is_not_ready_and_same_config_retries() {
        let rt = RwLock::new(VkTurnRuntime::default());
        let ip = "10.66.67.3".parse().unwrap();
        let first = apply_tun_config(&rt, ip, None, |_| async { Err("route failed".into()) }).await;
        assert!(first.is_err());
        assert!(rt.read().await.tunnel_ip.is_none());
        let called = std::sync::atomic::AtomicBool::new(false);
        apply_tun_config(&rt, ip, None, |_| async { called.store(true, std::sync::atomic::Ordering::SeqCst); Ok(()) }).await.unwrap();
        assert!(called.load(std::sync::atomic::Ordering::SeqCst));
        assert_eq!(rt.read().await.tunnel_ip, Some(ip));
    }

    #[test]
    fn regression_handoff_retries_are_bounded_and_nonparallel() {
        assert_eq!(handoff_decision(true, false, true, 0, true), HandoffDecision::Wait);
        assert_eq!(handoff_decision(true, false, false, 1, false), HandoffDecision::Wait);
        assert_eq!(handoff_decision(true, false, false, 1, true), HandoffDecision::Start);
        assert_eq!(handoff_decision(true, false, false, 3, true), HandoffDecision::Restart);
        assert_eq!(handoff_decision(true, true, false, 0, true), HandoffDecision::Wait);
    }

    #[test]
    fn regression_handoff_retry_stops_after_cancellation() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let cancelled = AtomicBool::new(false);
        let mut calls = 0;
        let result = retry_fd_send(&cancelled, 3, Duration::ZERO, || {
            calls += 1;
            cancelled.store(true, Ordering::SeqCst);
            Err("unavailable".into())
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn regression_client_launches_use_distinct_tun_socket_names() {
        let cfg = cfg_with(&["abcdefghijklmnop"]);
        let first = build_client_args_with_hashes(&cfg, &call_hashes(&cfg));
        let second = build_client_args_with_hashes(&cfg, &call_hashes(&cfg));
        let socket = |args: &[String]| args[args.iter().position(|s| s == "-tun-uds").unwrap() + 1].clone();
        assert_ne!(socket(&first), socket(&second));
    }

    #[test]
    fn test_build_client_args_and_env() {
        let cfg = cfg_with(&["https://vk.com/call/join/abc12345", "https://vk.com/call/join/def67890"]);
        let args = build_client_args(&cfg);
        let uds_name = args[10].clone();
        assert!(uds_name.starts_with(&format!("{TUN_UDS_NAME}_")));
        assert_eq!(uds_name.len(), TUN_UDS_NAME.len() + 17);
        assert_eq!(
            args,
            vec![
                "-peer", "1.2.3.4:46000",
                "-vk", "abc12345,def67890",
                "-n", "20",
                "--exact-workers",
                "-listen", "127.0.0.1:0",
                "-tun-uds", uds_name.as_str(),
                "-captcha-mode", "auto",
                "-obfs", "audio",
            ]
        );
        assert!(!args.iter().any(|a| a.contains("s3cret-pass")), "пароль в аргументах процесса");
        let env = client_env(&cfg);
        assert!(env.contains(&("CSQTT_PASSWORD", "s3cret-pass".to_string())));
        assert!(env.contains(&("CSQTT_EVENTS", "1".to_string())));
        let dup = cfg_with(&["https://vk.com/call/join/abc12345", "abc12345"]);
        assert_eq!(call_hashes(&dup), vec!["abc12345"]);
    }

    #[test]
    fn test_vk_auth_parsing_and_bootstrap_formatting() {
        let url = "https://oauth.vk.ru/blank.html#access_token=vk1.a.TestTokenSecret123&user_id=12345678&expires_in=0";
        let (token, uid) = parse_vk_redirect_hash(url).expect("redirect url should be parsed");
        assert_eq!(token, "vk1.a.TestTokenSecret123");
        assert_eq!(uid, Some(12345678));

        let frag = "#access_token=vk1.a.AnotherToken&user_id=987654";
        let (token2, uid2) = parse_vk_redirect_hash(frag).expect("fragment should be parsed");
        assert_eq!(token2, "vk1.a.AnotherToken");
        assert_eq!(uid2, Some(987654));

        let raw = "vk1.a.RawDirectTokenOnly";
        let (token3, uid3) = parse_vk_redirect_hash(raw).expect("raw token should be accepted");
        assert_eq!(token3, "vk1.a.RawDirectTokenOnly");
        assert_eq!(uid3, None);

        assert!(parse_vk_redirect_hash("https://oauth.vk.ru/blank.html#error=access_denied").is_err());

        // Проверка формирования строки VK_JS_BOOTSTRAP
        let b_line = format_vk_bootstrap_line("vk1.a.SecretToken", 12345);
        assert!(b_line.starts_with("VK_JS_BOOTSTRAP:"));
        assert!(b_line.ends_with('\n'));
        let b64 = b_line.trim_start_matches("VK_JS_BOOTSTRAP:").trim();
        let decoded = crate::subscription_parser::base64_decode(b64).expect("valid base64");
        let json: serde_json::Value = serde_json::from_slice(&decoded).expect("valid json in bootstrap");
        assert_eq!(json["token"], "vk1.a.SecretToken");
        assert_eq!(json["access_token"], "vk1.a.SecretToken");
        assert_eq!(json["user_id"], 12345);

        // Проверка маскирования токена
        assert_eq!(mask_vk_token("vk1.a.SensitiveData"), "vk1.a.***");
        assert_eq!(mask_vk_token("other_token_secret"), "[REDACTED]");
        assert_eq!(mask_vk_token(""), "");

        // Проверка формирования аргументов csqtt с хешами auto_api (как в focsq)
        let oauth_cfg = cfg_with(&[]);
        let oauth_args = build_client_args_with_hashes(&oauth_cfg, &["autoGeneratedHash123".into()]);
        assert!(oauth_args.contains(&"-vk".to_string()));
        assert!(oauth_args.contains(&"autoGeneratedHash123".to_string()));
        assert!(oauth_args.contains(&"--exact-workers".to_string()));
    }

    #[test]
    fn test_with_primary_link() {
        let links = vec![
            "https://vk.com/call/join/oldmain1234".to_string(),
            "https://vk.com/call/join/backup5678".to_string(),
        ];
        let out = with_primary_link(&links, "  https://vk.com/call/join/fresh12345/ ").unwrap();
        assert_eq!(out, vec![
            "https://vk.com/call/join/fresh12345".to_string(),
            "https://vk.com/call/join/oldmain1234".to_string(),
            "https://vk.com/call/join/backup5678".to_string(),
        ]);
        let out2 = with_primary_link(&out, "backup5678").unwrap();
        assert_eq!(out2.len(), 3);
        assert_eq!(out2[0], "https://vk.com/call/join/backup5678");
        let many: Vec<String> = (0..10).map(|i| format!("https://vk.com/call/join/link{i:06}")).collect();
        assert_eq!(with_primary_link(&many, "https://vk.com/call/join/newlink000").unwrap().len(), MAX_LINKS);
        assert!(with_primary_link(&links, "https://telemost.yandex.ru/j/12345678").is_err());
    }

    // Строки ниже — формат событий и сообщений из исходников клиента csqtt (events.rs, worker.rs, protocol.rs).
    const EV_READY: &str = r#"__CSQTT_EVENT__|READY|{"worker":1}"#;
    const EV_STATS_3: &str = r#"__CSQTT_EVENT__|STATS|{"active":3,"bytes_down":2048,"bytes_up":1024}"#;
    const EV_STATS_0: &str = r#"__CSQTT_EVENT__|STATS|{"active":0,"bytes_down":2048,"bytes_up":1024}"#;
    const EV_AUTH: &str = r#"__CSQTT_EVENT__|ERROR|{"code":"authentication_rejected","fatal":true,"message":"FATAL_AUTH: invalid password"}"#;

    #[test]
    fn regression_tun_lost_requests_replacement_while_client_runs() {
        let mut rt = VkTurnRuntime::default();
        let ip = "10.66.67.3".parse().unwrap();
        rt.tunnel_ip = Some(ip);
        rt.tunnel_dns = Some("1.1.1.1".into());
        rt.tun_attached = true;
        let result = apply_log_line(&mut rt, r#"__CSQTT_EVENT__|TUN_LOST|{}"#, Instant::now());
        assert_eq!(result, LineEffect::TunConfig { ip, dns: Some("1.1.1.1".into()) });
        assert!(!rt.tun_attached);
    }

    #[test]
    fn distinguishes_csqtt_vk_and_timeout_error_sources() {
        let mut rt = VkTurnRuntime::default();
        apply_log_line(&mut rt, EV_AUTH, Instant::now());
        assert_eq!((rt.error_source, rt.error_reason), (Some("csqtt"), Some("password_invalid")));
        apply_log_line(&mut rt, "User authorization failed", Instant::now());
        assert_eq!((rt.error_source, rt.error_reason), (Some("vk"), Some("authorization_rejected")));
        rt.phase = VkTurnPhase::Starting;
        rt.connected_streams = 0;
        for _ in 0..GETCONF_SILENT_THRESHOLD { note_getconf_silent(&mut rt); }
        assert_eq!((rt.error_source, rt.error_reason), (Some("network"), Some("server_timeout")));
    }

    #[tokio::test]
    async fn regression_stale_log_reader_cannot_change_new_status() {
        let manager = VkTurnManager::with_core_path("unused");
        let (reader, mut writer) = tokio::io::duplex(1024);
        let (sender, receiver) = mpsc::unbounded_channel();
        let task = manager.spawn_log_reader(Box::new(reader), sender);
        let mut rt = manager.rt.write().await;
        rt.phase = VkTurnPhase::Starting;
        writer.write_all(format!("{EV_AUTH}\n").as_bytes()).await.unwrap();
        tokio::task::yield_now().await; // читатель может ждать блокировку состояния
        drop(receiver); // завершённый запуск больше не имеет получателя событий
        drop(rt);
        drop(writer);
        tokio::time::timeout(Duration::from_secs(2), task).await.unwrap().unwrap();
        assert_eq!(manager.rt.read().await.phase, VkTurnPhase::Starting);
    }

    #[tokio::test]
    async fn regression_stale_handoff_completion_cannot_mark_new_run_ready() {
        let manager = VkTurnManager::with_core_path("unused");
        let old_generation = manager.begin_run().await;
        let (release, delayed) = tokio::sync::oneshot::channel();
        let old_manager = manager.clone();
        let old_task = tokio::spawn(async move {
            let _ = delayed.await;
            old_manager.publish_handoff_result(old_generation, Ok(())).await;
        });
        manager.stop().await;
        let new_generation = manager.begin_run().await;
        assert_ne!(old_generation, new_generation);
        release.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(2), old_task).await.unwrap().unwrap();
        assert!(!manager.rt.read().await.tun_attached);
        manager.publish_handoff_result(new_generation, Ok(())).await;
        manager.publish_handoff_result(old_generation, Err("old failure".into())).await;
        let rt = manager.rt.read().await;
        assert!(rt.tun_attached);
        assert!(rt.last_error.is_none());
    }
    const L_CREDS: &str = "[ГРУППА #1] Запрос кредов (хеш: dVi6izW6...)";
    const L_VKCALLS: &str = "[STREAM 100] [VKCalls] step1 OK, anonymous_token (290 chars)";
    const L_CAPTCHA_START: &str = "[STREAM 100] [КАПЧА] AUTO: старт цепочки (captcha attempt 1)";
    const L_CAPTCHA_FAIL: &str = "[ГРУППА #1] Ошибка кредов: all VK credentials failed: CAPTCHA_WAIT_REQUIRED";
    const L_AUTH_WRONG: &str = "[ВОРКЕР #1] Фатальная ошибка: FATAL_AUTH: неверный пароль подключения";
    const L_AUTH_EXPIRED: &str = "[ВОРКЕР #1] Фатальная ошибка: FATAL_AUTH: срок действия пароля истёк";
    const L_WRAP_TIMEOUT: &str = "[ВОРКЕР #2] [HANDSHAKE][RETRY] WRAP_AUTH_TIMEOUT, 3 попыток восстановления";
    const L_CALL_DEAD: &str = "[ГРУППА #1] Ошибка кредов: VK returns error: Call not found (error_code=9000)";
    const L_TURN_URL: &str = "[VKCalls] turn_server.urls[0]=95.163.34.143:19302";

    #[test]
    fn events_drive_streams_and_traffic() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        assert_eq!(apply_log_line(&mut rt, L_CREDS, now), LineEffect::None);
        assert_eq!(rt.phase, VkTurnPhase::Authorizing);
        apply_log_line(&mut rt, EV_READY, now);
        assert_eq!(rt.phase, VkTurnPhase::Connected);
        assert_eq!(rt.connected_streams, 1);
        apply_log_line(&mut rt, EV_STATS_3, now);
        assert_eq!(rt.connected_streams, 3);
        assert_eq!((rt.bytes_up, rt.bytes_down), (1024, 2048));
        apply_log_line(&mut rt, EV_STATS_0, now);
        assert_eq!(rt.connected_streams, 0);
        assert_eq!(rt.phase, VkTurnPhase::Degraded);
        assert!(rt.zero_streams_since.is_some());
        assert!(rt.log_tail.iter().all(|l| !l.contains("__CSQTT_EVENT__")), "машинные события не засоряют лог");
    }

    /// Адрес туннеля от сервера (protocol.rs: `TUNCONF:{ip}:{dns}:0:stream-v1`, бывает с хвостом pad=…).
    #[test]
    fn tunconf_event_returns_tunnel_address() {
        let mut rt = VkTurnRuntime::default();
        let ev = r#"__CSQTT_EVENT__|CONFIG|{"config":"TUNCONF:10.66.67.3:1.1.1.1:0:stream-v1"}"#;
        assert_eq!(
            apply_log_line(&mut rt, ev, Instant::now()),
            LineEffect::TunConfig { ip: "10.66.67.3".parse().unwrap(), dns: Some("1.1.1.1".into()) }
        );
        assert_eq!(
            parse_tunconf("TUNCONF:10.66.67.9:8.8.8.8,1.1.1.1:0:stream-v1:pad=xxxx"),
            Some(("10.66.67.9".parse().unwrap(), Some("8.8.8.8,1.1.1.1".into())))
        );
        assert_eq!(parse_tunconf("TUNCONF:10.66.67.9:;reboot:0"), Some(("10.66.67.9".parse().unwrap(), None)), "мусор в DNS отбрасывается");
        assert_eq!(parse_tunconf("TUNCONF:0.0.0.0:1.1.1.1"), None);
        assert_eq!(parse_tunconf("TUNCONF:10.66.67.3;id:1.1.1.1"), None, "инъекция в адрес невозможна");
        assert_eq!(parse_tunconf("GARBAGE"), None);
        // Туннель «готов» только когда и адрес получен, и TUN передан клиенту
        rt.tunnel_ip = Some("10.66.67.3".parse().unwrap());
        assert!(!rt.tunnel_ready());
        rt.tun_attached = true;
        assert!(!rt.tunnel_ready());
        rt.tun_configured = true;
        assert!(rt.tunnel_ready());
    }

    #[test]
    fn tun_scripts_only_use_constants_and_parsed_ip() {
        let s = tun_configure_script("10.66.67.3".parse().unwrap());
        assert!(s.contains("ip addr add 10.66.67.3/32 dev csq0"));
        assert!(s.contains("ip link set dev csq0 mtu 1280 up"));
        assert!(s.contains("TCPMSS --set-mss 1220"));
        assert!(s.contains("-o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT"));
        assert!(s.contains("ip route add default dev csq0 table 222"));
        assert!(s.contains("ip rule add dev csq0 table 222 pref 222"));
        assert!(s.contains("ip rule del pref 222"), "правило не дублируется при повторной настройке");
        assert!(s.contains("ip rule add from 10.66.67.3 table 222 pref 40"));
        assert!(s.contains("ip rule add fwmark 222 table 222 pref 45"));
        let t = tun_teardown_script_for_ip(Some("10.66.67.3".parse().unwrap()));
        assert!(t.contains("TCPMSS --set-mss 1220"));
        assert!(t.contains("-o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT"));
        assert!(t.contains("ip rule del pref 222 dev csq0 table 222"));
        assert!(t.contains("ip route del default dev csq0 table 222"));
        assert!(t.contains("ip rule del pref 40 from 10.66.67.3 table 222"));
        assert!(t.contains("ip rule del pref 45 fwmark 222 table 222"));
    }

    #[test]
    fn captcha_solve_request_is_answered_and_token_hidden() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        let req = "CAPTCHA_SOLVE|auto|https://id.vk.ru/not_robot_captcha?session_token=FAKE_SESSION_TOKEN_1|FAKE_SESSION_TOKEN_1";
        assert_eq!(apply_log_line(&mut rt, req, now), LineEffect::CaptchaSolve);
        assert_eq!(rt.phase, VkTurnPhase::SolvingCaptcha);
        assert!(rt.log_tail.iter().all(|l| !l.contains("FAKE_SESSION_TOKEN_1")));
        assert!(CAPTCHA_NO_WEBVIEW_REPLY.starts_with("CAPTCHA_RESULT|error:"), "клиент ждёт error:… чтобы сразу идти дальше");
        assert!(CAPTCHA_NO_WEBVIEW_REPLY.ends_with('\n'));
    }

    #[test]
    fn captcha_failure_is_sticky_until_connected() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        apply_log_line(&mut rt, L_VKCALLS, now);
        apply_log_line(&mut rt, L_CAPTCHA_START, now);
        assert_eq!(rt.phase, VkTurnPhase::SolvingCaptcha);
        apply_log_line(&mut rt, L_CAPTCHA_FAIL, now);
        assert_eq!(rt.phase, VkTurnPhase::CaptchaRequired);
        assert!(rt.last_error.as_deref().unwrap().contains("не робот"));
        apply_log_line(&mut rt, L_CREDS, now);
        apply_log_line(&mut rt, L_CAPTCHA_START, now);
        assert_eq!(rt.phase, VkTurnPhase::CaptchaRequired);
        apply_log_line(&mut rt, EV_READY, now);
        assert_eq!(rt.phase, VkTurnPhase::Connected);
        assert!(rt.last_error.is_none());
    }

    // Строки клиента csqtt с роутера: DTLS с сервером проходит, а GETCONF с неверным паролем остаётся без ответа.
    const S_DTLS_OK: &str = r#"__CSQTT_STREAM__|{"attempt":177,"elapsed_ms":164,"reason":"DTLS-SRTP established","sequence":1939,"stage":"dtls","status":"ok","time_ms":1793949028092,"transport":"UDP","turn":"95.163.34.139:19302","worker":16}"#;
    const S_GETCONF_START: &str = r#"__CSQTT_STREAM__|{"attempt":177,"elapsed_ms":0,"reason":"","sequence":1940,"stage":"getconf","status":"start","time_ms":1793949028092,"transport":"UDP","turn":"95.163.34.139:19302","worker":16}"#;
    const S_GETCONF_TIMEOUT: &str = r#"__CSQTT_STREAM__|{"attempt":160,"elapsed_ms":5804,"reason":"GETCONF чтение ответа конфига: timeout после 4 попыток","sequence":1950,"stage":"getconf","status":"error","time_ms":1793949030728,"transport":"UDP","turn":"95.163.34.176:19302","worker":5}"#;
    const S_GETCONF_OK: &str = r#"__CSQTT_STREAM__|{"attempt":1,"elapsed_ms":90,"reason":"","sequence":20,"stage":"getconf","status":"ok","time_ms":1793949030728,"transport":"UDP","turn":"95.163.34.176:19302","worker":2}"#;

    #[test]
    fn regression_vk_getconf_timeouts_do_not_prove_wrong_password() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        rt.phase = VkTurnPhase::Authorizing;
        for _ in 0..3 {
            apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        }
        assert_eq!(rt.phase, VkTurnPhase::ServerUnreachable);
        assert!(rt.last_error.as_deref().unwrap().contains("пароль"));
        apply_log_line(&mut rt, "[DTLS] retry", now);
        assert_eq!(rt.phase, VkTurnPhase::ServerUnreachable);
        apply_log_line(&mut rt, EV_READY, now);
        assert_eq!(rt.phase, VkTurnPhase::Connected);
    }

    // Регрессия: при неверном пароле сервер csqtt молчит на GETCONF, а панель ждала строку
    // WRAP_AUTH_TIMEOUT прежнего ядра — статус навсегда оставался «Подключаюсь…» без поля для пароля.
    #[test]
    fn silent_getconf_after_dtls_reports_unreachable_server() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        rt.phase = VkTurnPhase::Authorizing;
        apply_log_line(&mut rt, S_DTLS_OK, now);
        apply_log_line(&mut rt, S_GETCONF_START, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        assert_eq!(rt.phase, VkTurnPhase::Authorizing, "одна-две потери пакетов — ещё не повод");
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        assert_eq!(rt.phase, VkTurnPhase::ServerUnreachable);
        assert!(rt.last_error.as_deref().unwrap().contains("пароль"));
        assert!(is_sticky(rt.phase), "фаза держится и показывает поле для нового пароля");

        // Журнал: успешные этапы не засоряют его JSON-ом, ошибки — коротко
        assert!(rt.log_tail.iter().all(|l| !l.contains("__CSQTT_STREAM__")));
        assert!(rt.log_tail.iter().any(|l| l == "[ПОТОК #5] getconf: GETCONF чтение ответа конфига: timeout после 4 попыток"));
    }

    #[test]
    fn getconf_answer_resets_silence_counter() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        rt.phase = VkTurnPhase::Authorizing;
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        apply_log_line(&mut rt, S_GETCONF_OK, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        apply_log_line(&mut rt, S_GETCONF_TIMEOUT, now);
        assert_eq!(rt.phase, VkTurnPhase::Authorizing, "сервер ответил — пароль верный, редкие потери не считаются");

        // При живых потоках потерянный GETCONF не объявляет пароль неверным
        let mut rt2 = VkTurnRuntime::default();
        rt2.connected_streams = 3;
        rt2.phase = VkTurnPhase::Connected;
        for _ in 0..5 {
            apply_log_line(&mut rt2, S_GETCONF_TIMEOUT, now);
        }
        assert_eq!(rt2.phase, VkTurnPhase::Connected);
    }

    #[test]
    fn server_auth_errors_are_recognized() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        apply_log_line(&mut rt, L_AUTH_WRONG, now);
        assert_eq!(rt.phase, VkTurnPhase::AuthFailed);
        assert!(rt.last_error.as_deref().unwrap().contains("неверный"));
        apply_log_line(&mut rt, L_WRAP_TIMEOUT, now);
        assert_eq!(rt.phase, VkTurnPhase::AuthFailed, "таймаут после отказа не меняет понятную причину");

        let mut rt2 = VkTurnRuntime::default();
        apply_log_line(&mut rt2, L_AUTH_EXPIRED, now);
        assert!(rt2.last_error.as_deref().unwrap().contains("истёк"));

        let mut rt3 = VkTurnRuntime::default();
        apply_log_line(&mut rt3, L_WRAP_TIMEOUT, now);
        assert_eq!(rt3.phase, VkTurnPhase::ServerUnreachable);
        assert!(rt3.last_error.as_deref().unwrap().contains("пароль"), "при неверном пароле сервер молчит — подсказываем");

        let mut rt4 = VkTurnRuntime::default();
        apply_log_line(&mut rt4, EV_AUTH, now);
        assert_eq!(rt4.phase, VkTurnPhase::AuthFailed, "машинное событие ERROR authentication_rejected");
        assert!(rt4.last_error.as_deref().unwrap().contains("неверный"));
    }

    #[test]
    fn dead_call_link_is_recognized_but_does_not_hide_working_streams() {
        let now = Instant::now();
        let mut rt = VkTurnRuntime::default();
        apply_log_line(&mut rt, L_CALL_DEAD, now);
        assert_eq!(rt.phase, VkTurnPhase::LinkInvalid);

        let mut rt2 = VkTurnRuntime::default();
        apply_log_line(&mut rt2, EV_STATS_3, now);
        apply_log_line(&mut rt2, L_CALL_DEAD, now);
        assert_eq!(rt2.phase, VkTurnPhase::Connected);
        assert!(rt2.last_error.is_some());
    }

    /// Регрессия WDTT: адреса TURN ВК в реальном логе идут без префикса `turn:` (95.163.34.143:19302) —
    /// старый разбор их не видел, и исключения в iptables ставились только на подсеть 155.212.192.0/20.
    #[test]
    fn turn_addresses_are_learned_for_exemptions() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        apply_log_line(&mut rt, L_TURN_URL, now);
        apply_log_line(&mut rt, "[ГРУППА #1] Креды OK, TURN: [95.163.34.143:19302 95.163.34.131:19302], 10 воркеров", now);
        apply_log_line(&mut rt, "[СЕССИЯ #1] Relay: 95.163.34.131:62836", now);
        apply_log_line(&mut rt, "[TURN] Подключение к 127.0.0.1:3478", now);
        apply_log_line(&mut rt, "[TURN] Подключение к 10.0.0.1:3478", now);
        assert_eq!(
            rt.turn_ips.iter().map(|i| i.to_string()).collect::<Vec<_>>(),
            vec!["95.163.34.131", "95.163.34.143"]
        );
        for i in 0..100u8 {
            apply_log_line(&mut rt, &format!("[TURN] Подключение к 198.51.{i}.1:3478"), now);
        }
        assert!(rt.turn_ips.len() <= MAX_TURN_IPS, "список адресов TURN ограничен");
    }

    #[test]
    fn log_tail_is_bounded_and_redacted() {
        let mut rt = VkTurnRuntime::default();
        for i in 0..400 {
            apply_log_line(&mut rt, &format!("log line {i}"), Instant::now());
        }
        assert_eq!(rt.log_tail.len(), LOG_TAIL);

        let r = redact_line("join https://vk.com/call/join/SecretCallToken123456789 ok");
        assert!(!r.contains("SecretCallToken123456789"));
        assert!(r.contains("call/join/Secr…"));
        let r = redact_line("x?session_token=abc123def456&variant=popup token=zzz999yyy888");
        assert!(!r.contains("abc123def456") && !r.contains("zzz999yyy888"));
        assert!(r.contains("&variant=popup"));
        let r = redact_line("[КЛИЕНТ] Запрос кредов (хеш: dVi6izW61Ty9UuE1hEkU)");
        assert!(!r.contains("dVi6izW61Ty9UuE1hEkU"), "хеш звонка скрыт: {r}");
    }

    #[test]
    fn stall_limits() {
        assert_eq!(stall_limit(VkTurnPhase::AuthFailed), Duration::from_secs(15));
        assert_eq!(stall_limit(VkTurnPhase::LinkInvalid), Duration::from_secs(20));
        assert_eq!(stall_limit(VkTurnPhase::CaptchaRequired), Duration::from_secs(45));
        assert_eq!(stall_limit(VkTurnPhase::SolvingCaptcha), Duration::from_secs(300));
        assert_eq!(stall_limit(VkTurnPhase::Authorizing), Duration::from_secs(120));
    }

    #[test]
    fn test_render_provider_yaml() {
        let yaml = render_provider_yaml();
        assert!(yaml.contains(PROXY_NAME));
        assert!(yaml.contains("[БС]"));
        assert!(yaml.contains("type: direct"));
        assert!(yaml.contains("interface-name: csq0"), "трафик узла уходит в TUN клиента csqtt");
        assert!(yaml.contains("routing-mark: 222"));
        assert!(yaml.contains("ip-version: ipv4"));
        assert!(yaml.contains("udp: true"));
        assert!(yaml.contains("tfo: false"));
    }

    #[test]
    fn test_apply_and_remove_rules() {
        let yaml = "port: 7890\nrules:\n  - MATCH,DIRECT\n";
        let mut cfg = cfg_with(&["https://vk.com/call/join/abc12345"]);

        let applied = apply_rules(yaml, &cfg).expect("apply rules");
        assert!(applied.contains(RULES_BEGIN));
        assert!(applied.contains("IP-CIDR,155.212.192.0/20,DIRECT,no-resolve"));
        assert!(applied.contains("IP-CIDR,1.2.3.4/32,DIRECT,no-resolve"));
        assert_eq!(applied, apply_rules(&applied, &cfg).unwrap(), "идемпотентно");

        cfg.enabled = false;
        let disabled = apply_rules(&applied, &cfg).expect("disable rules");
        assert!(!disabled.contains(RULES_BEGIN));
        assert_eq!(disabled.trim(), yaml.trim());

        cfg.enabled = true;
        assert!(apply_rules("port: 7890\r\nrules:\r\n  - MATCH,DIRECT\r\n", &cfg).unwrap().contains("\r\n"));
        assert!(apply_rules("port: 7890\nproxies:\n", &cfg).is_err());
    }

    #[test]
    fn test_core_asset_for_arch() {
        assert_eq!(core_asset_for_arch("arm64"), Some("csqtt-client-linux-arm64"));
        assert_eq!(core_asset_for_arch("aarch64"), Some("csqtt-client-linux-arm64"));
        assert_eq!(core_asset_for_arch("mipsle-softfloat"), None, "для MIPS сборки csqtt нет — понятная ошибка");
        assert_eq!(core_asset_for_arch("unknown_arch"), None);
        assert_eq!(elf_target_for_arch("mipsle-softfloat"), "mipsel");
        assert_eq!(elf_target_for_arch("arm64"), "aarch64");
        assert_eq!(elf_target_for_arch("armv7"), "arm");
    }

    /// Каждой архитектуре из сборки соответствует проверенный SHA-256 (64 hex), без заглушек.
    #[test]
    fn every_core_asset_has_a_real_hash() {
        for arch in ["arm64"] {
            let asset = core_asset_for_arch(arch).unwrap();
            let hash = CORE_SHA256.iter().find(|(a, _)| *a == asset).map(|(_, h)| *h);
            let hash = hash.unwrap_or_else(|| panic!("нет хеша для {asset}"));
            assert_eq!(hash.len(), 64);
            assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert!(CORE_TAG.ends_with(&CORE_UPSTREAM_COMMIT[..7]), "тег релиза ядра соответствует коммиту сборки");
    }

    /// Пароль сервера csqtt и токен VK — секреты: в API настроек они скрыты, а «******» при сохранении не затирает настоящий.
    #[test]
    fn settings_api_masks_and_preserves_tunnel_password() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.vkturn.password = "top-secret-pw".into();
        cfg.vkturn.vk_token = Some("vk1.a.VerySecretTokenLongString".into());
        let masked = crate::api::mask_settings_for_api(&cfg);
        assert_eq!(masked["vkturn"]["password"], "******");
        assert_eq!(masked["vkturn"]["vk_token"], "vk1.a.***");
        assert!(!masked.to_string().contains("top-secret-pw"));
        assert!(!masked.to_string().contains("VerySecretTokenLongString"));
        let merged = crate::api::merge_settings_update(&cfg, &masked).expect("merge masked settings");
        assert_eq!(merged.vkturn.password, "top-secret-pw");
    }

    #[tokio::test]
    async fn regression_vk_unavailable_does_not_replace_existing_account() {
        let (root, mut state) = setup_saved_state("auth-offline").await;
        state.http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .timeout(Duration::from_secs(2)).build().unwrap();
        commit_vkturn(&state, |v| { v.vk_token = Some("vk1.a.PreviousSyntheticToken".into()); Ok(()) }).await.unwrap();
        let req = crate::api::VkTurnAuthSaveReq { token: Some("vk1.a.NewSyntheticToken".into()), ..Default::default() };
        let result = response_json(crate::api::save_vkturn_auth(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], false);
        assert_eq!(state.config.read().await.vkturn.vk_token.as_deref(), Some("vk1.a.PreviousSyntheticToken"));
        commit_vkturn(&state, |v| { v.vk_token = None; Ok(()) }).await.unwrap();
        let req = crate::api::VkTurnAuthSaveReq { token: Some("vk1.a.NewSyntheticToken".into()), ..Default::default() };
        let result = response_json(crate::api::save_vkturn_auth(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], true);
        assert_eq!(result["data"]["verified"], false);
        assert!(!state.config.read().await.vkturn.vk_token_verified);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_vk_rejected_permission_code_is_never_saved() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            loop {
                let Ok((mut stream, _)) = listener.accept().await else { break; };
                tokio::spawn(async move {
                    let mut buf = [0u8; 2048];
                    let n = stream.read(&mut buf).await.unwrap_or(0);
                    let req_str = String::from_utf8_lossy(&buf[..n]);
                    if req_str.contains("account.getAppPermissions") {
                        let body = r#"{"error":{"error_code":27,"error_msg":"Group authorization failed: method is unavailable with group auth."}}"#;
                        let resp = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        let _ = stream.write_all(resp.as_bytes()).await;
                    } else {
                        let resp = "HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                        let _ = stream.write_all(resp.as_bytes()).await;
                    }
                });
            }
        });

        let (root, mut state) = setup_saved_state("auth-rejected-code27").await;
        state.http = reqwest::Client::builder()
            .resolve("api.vk.ru", addr)
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();

        let err = fetch_vk_user_profile_at(
            &state.http,
            &format!("http://{addr}"),
            "vk1.a.InvalidGroupTokenSynthetic",
            Some(12345),
        )
        .await;
        assert_eq!(err, Err(VkProfileError::Rejected));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn test_vk_auth_endpoints_save_and_logout() {
        let (root, mut state) = setup_saved_state("auth").await;
        state.http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all("http://127.0.0.1:1").unwrap())
            .timeout(Duration::from_secs(2)).build().unwrap();

        // 1. Сохранение авторизации через фрагмент URL
        let req = crate::api::VkTurnAuthSaveReq {
            url: Some("https://oauth.vk.ru/blank.html#access_token=vk1.a.UnitTestSecretToken&user_id=88889999".into()),
            auth_mode: Some("oauth".into()),
            ..Default::default()
        };
        let resp = crate::api::save_vkturn_auth(axum::extract::State(state.clone()), axum::Json(req)).await;
        let v = response_json(resp).await;
        assert_eq!(v["success"], true);
        assert_eq!(v["data"]["user_id"], 88889999);
        assert_eq!(v["data"]["verified"], false);

        let cur_cfg = state.config.read().await.vkturn.clone();
        assert_eq!(cur_cfg.vk_token.as_deref(), Some("vk1.a.UnitTestSecretToken"));
        assert_eq!(cur_cfg.vk_user_id, Some(88889999));
        assert_eq!(cur_cfg.auth_mode, "oauth");

        // 2. Проверка статуса API на отдачу маскированного токена и профиля
        let st_resp = crate::api::get_vkturn_status(axum::extract::State(state.clone())).await;
        let st = response_json(st_resp).await;
        assert_eq!(st["data"]["has_token"], true);
        assert_eq!(st["data"]["vk_token"], "vk1.a.***");
        assert_eq!(st["data"]["vk_user_id"], 88889999);

        // 3. Выход из аккаунта
        let logout_resp = crate::api::logout_vkturn_auth(axum::extract::State(state.clone())).await;
        let lv = response_json(logout_resp).await;
        assert_eq!(lv["success"], true);

        let logged_out_cfg = state.config.read().await.vkturn.clone();
        assert!(logged_out_cfg.vk_token.is_none());
        assert!(logged_out_cfg.vk_user_id.is_none());
        assert!(logged_out_cfg.vk_user_name.is_none());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_is_whitelist_server_full_vkturn() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist_providers.push(PROVIDER_ID.to_string());
        assert!(crate::failover::is_whitelist_server_full(PROXY_NAME, "any_id", Some(PROVIDER_ID), &cfg));
    }

    // ---------- Анти-регрессионные тесты ----------

    #[tokio::test]
    async fn regression_vk_switch_rejects_unready_tunnel_before_contacting_mihomo() {
        let root = std::env::temp_dir();
        let mgr = VkTurnManager::with_core_path(root.join("nonexistent-csqtt"));
        let state = test_state(&root, mgr, 1);
        let error = crate::api::execute_server_switch_workflow(&state, PROXY_NAME).await.unwrap_err();
        assert!(error.contains("Туннель ещё не подключился"), "unexpected error: {error}");
    }

    #[tokio::test]
    async fn regression_vk_switch_rejects_tun_without_connected_streams() {
        let root = std::env::temp_dir();
        let mgr = VkTurnManager::with_core_path(root.join("nonexistent-csqtt"));
        {
            let mut rt = mgr.rt.write().await;
            rt.tunnel_ip = Some("10.0.0.2".parse().unwrap());
            rt.tun_attached = true;
            rt.tun_configured = true;
            rt.connected_streams = 0;
        }
        let state = test_state(&root, mgr, 1);
        let error = crate::api::execute_server_switch_workflow(&state, PROXY_NAME).await.unwrap_err();
        assert!(error.contains("Туннель ещё не подключился"), "unexpected error: {error}");
    }

    fn test_state(root: &std::path::Path, mgr: Arc<VkTurnManager>, mihomo_port: u16) -> crate::AppState {
        let yaml_path = root.join("config.yaml");
        let json_path = root.join("config.json");
        let mut cfg = crate::config::AppConfig::default();
        cfg.mihomo.config_path = yaml_path.display().to_string();
        cfg.mihomo.port = mihomo_port;
        let config_arc = Arc::new(tokio::sync::RwLock::new(Arc::new(cfg)));
        let ag = Arc::new(crate::antigravity::AntigravityManager::new(config_arc.clone()));
        crate::AppState {
            config: config_arc,
            config_path: Arc::new(json_path),
            http: reqwest::Client::new(),
            failover_log: Arc::new(crate::failover::FailoverLog::default()),
            routing_lock: Arc::new(tokio::sync::Mutex::new(())),
            config_lock: Arc::new(tokio::sync::Mutex::new(())),
            speedtest_lock: Arc::new(tokio::sync::Mutex::new(())),
            zapret_lock: Arc::new(tokio::sync::Mutex::new(())),
            antigravity: ag,
            vkturn: mgr,
        }
    }

    /// Мок Mihomo: на любой запрос отвечает 204 (reload «принят»).
    async fn spawn_mock_mihomo_204() -> u16 {
        use tokio::io::AsyncReadExt;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else { return };
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let _ = sock.read(&mut buf).await;
                    let _ = sock.write_all(b"HTTP/1.1 204 No Content\r\nConnection: close\r\n\r\n").await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        port
    }

    async fn response_json(resp: axum::response::Response) -> serde_json::Value {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&bytes).unwrap()
    }

    const BASE_YAML: &str = "mode: rule\nproxies: []\nproxy-providers:\nproxy-groups:\n  - name: PROXY\n    type: select\n    proxies: [DIRECT]\nrules:\n  - MATCH,PROXY\n";

    async fn setup_saved_state(tag: &str) -> (std::path::PathBuf, crate::AppState) {
        let root = std::env::temp_dir().join(format!("xkeen-vkturn-{tag}-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&root).unwrap();
        let port = spawn_mock_mihomo_204().await;
        let state = test_state(&root, VkTurnManager::with_core_path(root.join("no-core")), port);
        std::fs::write(root.join("config.yaml"), BASE_YAML).unwrap();
        let cfg = (**state.config.read().await).clone();
        crate::config::save(&root.join("config.json"), &cfg).await.unwrap();
        (root, state)
    }

    fn enable_req() -> crate::api::SaveVkTurnConfigReq {
        crate::api::SaveVkTurnConfigReq {
            enabled: Some(true),
            peer: Some("1.2.3.4".into()),
            password: Some("s3cret-pass".into()),
            links: Some(vec!["https://vk.com/call/join/abcdEFGH1234".into()]),
            ..Default::default()
        }
    }

    /// Ревью: обработчик сам захватывал routing_lock/config_lock, а затем ConfigTx::begin
    /// захватывал их повторно — вечная взаимная блокировка.
    /// csqtt: провайдер (direct через csq0) добавляется сразу при включении и попадает в серверы БС.
    #[tokio::test]
    async fn regression_save_config_does_not_deadlock_and_adds_provider() {
        let (root, state) = setup_saved_state("save").await;
        let resp = tokio::time::timeout(
            Duration::from_secs(15),
            crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(enable_req())),
        )
        .await
        .expect("сохранение VK-туннеля зависло (взаимная блокировка)");
        let v = response_json(resp).await;
        assert_eq!(v["success"], true, "ответ: {v}");
        assert!(state.routing_lock.try_lock().is_ok(), "routing_lock остался занятым");
        assert!(state.config_lock.try_lock().is_ok(), "config_lock остался занятым");

        let saved = state.config.read().await.vkturn.clone();
        assert_eq!(saved.peer, "1.2.3.4:46000", "PEER-порт по умолчанию подставлен");
        assert_eq!(saved.links, vec!["https://vk.com/call/join/abcdEFGH1234"]);

        let yaml = std::fs::read_to_string(root.join("config.yaml")).unwrap();
        assert!(yaml.contains(RULES_BEGIN), "правила DIRECT для TURN/VPS ставятся сразу");
        assert!(yaml.contains("  vkturn:"), "провайдер VK-туннеля добавлен: {yaml}");
        let prov = std::fs::read_to_string(root.join("providers").join("vkturn.yaml")).unwrap();
        assert!(prov.contains("interface-name: csq0"));
        assert!(prov.contains("routing-mark: 222"));
        assert!(state.config.read().await.whitelist_providers.iter().any(|p| p == PROVIDER_ID));

        // Повторное сохранение не теряет пароль
        let v = response_json(
            crate::api::save_vkturn_config(
                axum::extract::State(state.clone()),
                axum::Json(crate::api::SaveVkTurnConfigReq { streams: Some(30), ..Default::default() }),
            )
            .await,
        )
        .await;
        assert_eq!(v["success"], true, "ответ: {v}");
        assert_eq!(state.config.read().await.vkturn.password, "s3cret-pass", "пароль сохраняется");
        assert_eq!(state.config.read().await.vkturn.streams, 30);

        // Выключение через тот же обработчик тоже не зависает и снимает правила
        let resp = tokio::time::timeout(
            Duration::from_secs(15),
            crate::api::vkturn_action(
                axum::extract::State(state.clone()),
                axum::Json(crate::api::VkTurnActionReq { action: "disable".into() }),
            ),
        )
        .await
        .expect("выключение VK-туннеля зависло");
        assert_eq!(response_json(resp).await["success"], true);
        assert!(!std::fs::read_to_string(root.join("config.yaml")).unwrap().contains(RULES_BEGIN));

        // Сброс через reset очищает peer, password и links
        let reset_resp = tokio::time::timeout(
            Duration::from_secs(15),
            crate::api::vkturn_action(
                axum::extract::State(state.clone()),
                axum::Json(crate::api::VkTurnActionReq { action: "reset".into() }),
            ),
        )
        .await
        .expect("сброс VK-туннеля завис");
        let reset_json = response_json(reset_resp).await;
        assert_eq!(reset_json["success"], true);
        let cur = state.config.read().await.vkturn.clone();
        assert!(!cur.enabled);
        assert!(cur.peer.is_empty());
        assert!(cur.password.is_empty());
        assert!(cur.links.is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[tokio::test]
    async fn regression_reset_clears_profile_token_and_runtime_state() {
        let (root, state) = setup_saved_state("reset-profile-token").await;
        let req = crate::api::SaveVkTurnConfigReq {
            enabled: Some(true),
            peer: Some("example.com:46000".into()),
            password: Some("dummy".into()),
            auth_mode: Some("profile_token".into()),
            profile_token: Some("profile-secret".into()),
            vk_token: Some("vk1.a.PersonalSavedToken".into()),
            streams: Some(35),
            ..Default::default()
        };
        let saved = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(saved["success"], true);
        {
            let mut rt = state.vkturn.rt.write().await;
            rt.phase = VkTurnPhase::AuthFailed;
            rt.last_error = Some("Сервер csqtt отклонил пароль".into());
            rt.error_source = Some("csqtt");
            rt.error_reason = Some("password_invalid");
            rt.restarts = 4;
            rt.bytes_up = 1024;
            rt.bytes_down = 2048;
            rt.log_tail.push_back("old log".into());
        }

        let reset_resp = crate::api::vkturn_action(
            axum::extract::State(state.clone()),
            axum::Json(crate::api::VkTurnActionReq { action: "reset".into() }),
        )
        .await;
        let reset_json = response_json(reset_resp).await;
        assert_eq!(reset_json["success"], true);
        assert!(!reset_json.to_string().contains("PersonalSavedToken"));

        let cur = state.config.read().await.vkturn.clone();
        assert!(!cur.enabled);
        assert!(cur.peer.is_empty());
        assert!(cur.password.is_empty());
        assert!(cur.links.is_empty());
        assert!(cur.profile_token.is_none());
        assert_eq!(cur.auth_mode, "oauth");
        assert_eq!(cur.vk_token.as_deref(), Some("vk1.a.PersonalSavedToken"));

        let rt = state.vkturn.rt.read().await;
        assert_eq!(rt.phase, VkTurnPhase::Disabled);
        assert!(rt.last_error.is_none());
        assert!(rt.error_source.is_none());
        assert!(rt.error_reason.is_none());
        assert_eq!(rt.restarts, 0);
        assert_eq!(rt.bytes_up, 0);
        assert_eq!(rt.bytes_down, 0);
        assert!(rt.log_tail.is_empty());
        drop(rt);

        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_logout_keeps_profile_source_and_running_status() {
        let (root, state) = setup_saved_state("profile-logout").await;
        let req = crate::api::SaveVkTurnConfigReq {
            enabled: Some(true), peer: Some("example.com:46000".into()), password: Some("dummy".into()),
            auth_mode: Some("profile_token".into()), profile_token: Some("profile-synthetic".into()),
            vk_token: Some("vk1.a.PersonalSyntheticToken".into()), ..Default::default()
        };
        let saved = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(saved["success"], true);
        state.vkturn.rt.write().await.phase = VkTurnPhase::Connected;
        let result = response_json(crate::api::logout_vkturn_auth(axum::extract::State(state.clone())).await).await;
        assert_eq!(result["success"], true);
        assert_eq!(state.vkturn.rt.read().await.phase, VkTurnPhase::Connected);
        let persisted = crate::config::load(&root.join("config.json"));
        assert_eq!(selected_vk_token(&persisted.vkturn), Some("profile-synthetic"));
        assert!(persisted.vkturn.vk_token.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_direct_profile_import_preserves_source_and_workers() {
        let (root, state) = setup_saved_state("direct-profile").await;
        let req = crate::api::SaveVkTurnConfigReq {
            enabled: Some(true),
            peer: Some("csqtt://connect?v=2&host=example.com&peer=46000&password=abc%2Bxyz&hashes=abcdefghijklmnop+qrstuvwxyzABCDEF&power=27".into()),
            vk_token: Some("vk1.a.PersonalSyntheticToken".into()),
            ..Default::default()
        };
        let result = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], true);
        let saved = state.config.read().await.vkturn.clone();
        assert_eq!(saved.password, "abc+xyz");
        assert_eq!(saved.auth_mode, "manual");
        assert_eq!(call_hashes(&saved), vec!["abcdefghijklmnop", "qrstuvwxyzABCDEF"]);
        assert_eq!(saved.streams, 27);
        assert_eq!(selected_vk_token(&saved), None);
        let req = crate::api::SaveVkTurnConfigReq {
            peer: Some("csqtt://connect?v=2&host=second.example&peer=443&password=p&token=profile-synthetic&workers=35".into()),
            ..Default::default()
        };
        let result = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], true);
        let saved = state.config.read().await.vkturn.clone();
        assert!(saved.links.is_empty());
        assert_eq!(selected_vk_token(&saved), Some("profile-synthetic"));
        assert_eq!(saved.vk_token.as_deref(), Some("vk1.a.PersonalSyntheticToken"));
        assert_eq!(saved.streams, 35);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_profile_token_is_separate_and_masked() {
        let (root, state) = setup_saved_state("profile-token").await;
        let req: crate::api::SaveVkTurnConfigReq = serde_json::from_value(serde_json::json!({
            "enabled": true, "peer": "example.com:46000", "password": "dummy",
            "auth_mode": "profile_token", "profile_token": "profile-secret", "vk_token": "personal-secret", "streams": 27
        })).unwrap();
        let result = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], true);
        let saved = state.config.read().await.vkturn.clone();
        assert_eq!(selected_vk_token(&saved), Some("profile-secret"));
        assert_eq!(saved.vk_token.as_deref(), Some("personal-secret"));
        assert_eq!(effective_workers(&saved), 27);
        let masked = crate::api::mask_settings_for_api(&**state.config.read().await);
        assert!(!masked.to_string().contains("profile-secret"));
        assert!(!masked.to_string().contains("personal-secret"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_manual_source_requires_hashes_even_with_saved_account() {
        let mut source = cfg_with(&["abcdefghijklmnop"]);
        source.vk_token = Some("synthetic-token".into());
        source.auth_mode = "manual".into();
        assert_eq!(selected_vk_token(&source), None);
        assert_eq!(call_hashes(&source), vec!["abcdefghijklmnop"]);
        source.auth_mode = "oauth".into();
        assert_eq!(selected_vk_token(&source), Some("synthetic-token"));
        let (root, state) = setup_saved_state("manual-source").await;
        let req = crate::api::SaveVkTurnConfigReq {
            auth_mode: Some("manual".into()),
            vk_token: Some("synthetic-token".into()),
            links: Some(vec![]),
            ..enable_req()
        };
        let result = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
        assert_eq!(result["success"], false);
        assert!(!state.config.read().await.vkturn.enabled);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn regression_enable_requires_peer_password_and_vk_link() {
        let (root, state) = setup_saved_state("validate").await;
        for (req, what) in [
            (crate::api::SaveVkTurnConfigReq { peer: Some(String::new()), ..enable_req() }, "без адреса сервера"),
            (crate::api::SaveVkTurnConfigReq { password: Some(String::new()), ..enable_req() }, "без пароля"),
            (crate::api::SaveVkTurnConfigReq { links: Some(vec![]), ..enable_req() }, "без ссылки"),
            (
                crate::api::SaveVkTurnConfigReq {
                    links: Some(vec!["https://telemost.yandex.ru/j/12345678".into()]),
                    ..enable_req()
                },
                "со ссылкой Телемоста",
            ),
            (crate::api::SaveVkTurnConfigReq { password: Some("a\nb".into()), ..enable_req() }, "с переводом строки в пароле"),
        ] {
            let v = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
            assert_eq!(v["success"], false, "включение {what} должно отклоняться: {v}");
        }
        assert!(!state.config.read().await.vkturn.enabled);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ревью: два одновременных restart() перезаписывали канал остановки — первый надзор «осиротевал»,
    /// и оказывались два клиента.
    #[tokio::test]
    async fn regression_concurrent_restarts_leave_single_supervisor() {
        let root = std::env::temp_dir().join(format!("xkeen-vkturn-race-{}", crate::auth::random_hex(8)));
        std::fs::create_dir_all(&root).unwrap();
        // Файл существует, но не исполняется: надзор жив и ждёт паузу после неудачного запуска.
        let fake_core = root.join("fake-core.txt");
        std::fs::write(&fake_core, "not a binary").unwrap();
        let mgr = VkTurnManager::with_core_path(&fake_core);
        let state = test_state(&root, mgr.clone(), 1);
        let cfg = cfg_with(&["https://vk.com/call/join/abcdEFGH1234"]);

        tokio::join!(
            mgr.restart(cfg.clone(), state.clone()),
            mgr.restart(cfg.clone(), state.clone()),
            mgr.restart(cfg.clone(), state.clone()),
        );
        assert_eq!(mgr.active_supervisors(), 1, "после одновременных перезапусков должен остаться один надзор");
        mgr.restart(cfg.clone(), state.clone()).await;
        assert_eq!(mgr.active_supervisors(), 1);
        mgr.stop().await;
        assert_eq!(mgr.active_supervisors(), 0, "stop() обязан дождаться завершения надзора");
        assert_eq!(mgr.rt.read().await.phase, VkTurnPhase::Disabled);

        // Без пароля клиент не запускается вовсе (csqtt сразу вышел бы с ошибкой)
        let mut no_pw = cfg.clone();
        no_pw.password.clear();
        mgr.restart(no_pw, state.clone()).await;
        assert_eq!(mgr.active_supervisors(), 0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn regression_explanatory_phases_survive_backoff() {
        assert!(keeps_phase_during_backoff(VkTurnPhase::CaptchaRequired));
        assert!(keeps_phase_during_backoff(VkTurnPhase::LinkInvalid));
        assert!(keeps_phase_during_backoff(VkTurnPhase::AuthFailed));
        assert!(!keeps_phase_during_backoff(VkTurnPhase::Crashed));
        assert!(!keeps_phase_during_backoff(VkTurnPhase::Degraded));
        assert!(!keeps_phase_during_backoff(VkTurnPhase::Connected));
    }

    #[test]
    fn decide_after_exit_definitive_failures_pause_and_notify_once() {
        for phase in [VkTurnPhase::CaptchaRequired, VkTurnPhase::LinkInvalid, VkTurnPhase::AuthFailed] {
            let mut backoff = Duration::from_secs(5);
            let mut last = None;
            let d1 = decide_after_exit(phase, Duration::from_secs(10), &mut backoff, &mut last);
            assert_eq!(d1.wait, Duration::from_secs(600), "{phase:?}: не долбим ВК и сервер");
            assert!(d1.notice.is_some());
            let d2 = decide_after_exit(phase, Duration::from_secs(10), &mut backoff, &mut last);
            assert!(d2.notice.is_none(), "{phase:?}: повторное уведомление — спам");
            let d3 = decide_after_exit(phase, Duration::from_secs(400), &mut backoff, &mut last);
            assert!(d3.notice.is_some(), "{phase:?}: после долгой работы — новая серия");
        }
    }

    #[test]
    fn decide_after_exit_backoff_grows_and_resets() {
        let mut backoff = Duration::from_secs(5);
        let mut last = None;
        let d = decide_after_exit(VkTurnPhase::Crashed, Duration::from_secs(1), &mut backoff, &mut last);
        assert_eq!(d.wait, Duration::from_secs(10));
        for _ in 0..10 {
            decide_after_exit(VkTurnPhase::Crashed, Duration::from_secs(1), &mut backoff, &mut last);
        }
        assert_eq!(backoff, Duration::from_secs(120), "пауза ограничена 120 с");
        let d = decide_after_exit(VkTurnPhase::Crashed, Duration::from_secs(301), &mut backoff, &mut last);
        assert_eq!(d.wait, Duration::from_secs(5), "после долгой работы пауза сбрасывается");
    }

    /// Ревью: без исключений в iptables трафик клиента мог уйти в xkeen_mask (петля) или под nfqws.
    #[test]
    fn iptables_exemptions_cover_turn_and_peer() {
        let mut turn = BTreeSet::new();
        turn.insert("203.0.113.50".parse().unwrap());
        turn.insert("155.212.200.1".parse().unwrap()); // уже покрыт подсетью ВК
        let dsts = exempt_destinations(Some("198.51.100.7".parse().unwrap()), &turn);
        let mut expected = VK_NETWORK_CIDRS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        expected.push("203.0.113.50/32".into());
        expected.push("198.51.100.7/32".into());
        assert_eq!(dsts, expected);
        assert_eq!(exempt_destinations(None, &BTreeSet::new()), VK_NETWORK_CIDRS);

        let apply = iptables_apply_script(&dsts);
        assert!(apply.contains("for ch in xkeen xkeen_mask XKEEN_BS zapret"));
        assert!(apply.contains("iptables -w -t"));
        assert!(!apply.contains("-w 2"));
        assert!(apply.contains("-m mark --mark 222"));
        assert!(apply.contains("-o csq0"));
        assert!(apply.contains("-i csq0"));
        assert!(apply.contains("-C \"$ch\""), "проверка -C делает правило идемпотентным");
        assert!(apply.contains("-I \"$ch\" 1"), "RETURN обязан стоять первым в цепочке");
        assert!(apply.contains(&format!("--comment {IPT_COMMENT} -j RETURN")));
        assert!(apply.contains("|| continue"), "отсутствующая цепочка пропускается");
        let remove = iptables_remove_script();
        assert!(remove.contains("for ch in xkeen xkeen_mask XKEEN_BS zapret"));
        assert!(remove.contains("iptables -w -t"));
        assert!(!remove.contains("-w 2"));
        assert!(remove.contains(&format!("--comment {IPT_COMMENT}")));
        assert!(remove.contains("sed 's/^-A /-D /'"));
    }

    #[tokio::test]
    async fn resolve_peer_ipv4_accepts_literal_and_rejects_garbage() {
        assert_eq!(resolve_peer_ipv4("1.2.3.4:46000").await, Some("1.2.3.4".parse().unwrap()));
        assert_eq!(resolve_peer_ipv4("no-port").await, None);
    }

    #[test]
    fn regression_narrow_direct_rules_and_router_exemptions() {
        let cfg = cfg_with(&["https://vk.com/call/join/abcdEFGH1234"]);
        let block = rules_block(&cfg);
        assert!(block.contains("IP-CIDR,155.212.192.0/20,DIRECT,no-resolve"));
        assert!(block.contains("IP-CIDR,1.2.3.4/32,DIRECT,no-resolve"));
        assert!(block.contains("DOMAIN,api.vk.com,DIRECT"));
        assert!(block.contains("DOMAIN,oauth.vk.ru,DIRECT"));
        assert!(!block.contains("DOMAIN,vk.com,DIRECT"));
        assert!(!block.contains("DOMAIN,vk.ru,DIRECT"));
        assert!(!block.contains("87.240.128.0/18"));
        assert!(!block.contains("95.213.0.0/17"));

        let mut turn = BTreeSet::new();
        turn.insert("87.240.130.10".parse().unwrap());
        turn.insert("95.213.10.20".parse().unwrap());
        turn.insert("203.0.113.50".parse().unwrap());
        let dsts = exempt_destinations(Some("198.51.100.7".parse().unwrap()), &turn);
        assert!(!dsts.contains(&"87.240.130.10/32".to_string()));
        assert!(!dsts.contains(&"95.213.10.20/32".to_string()));
        assert!(dsts.contains(&"203.0.113.50/32".to_string()));
        assert!(dsts.contains(&"198.51.100.7/32".to_string()));

        let apply = iptables_apply_script(&dsts);
        assert!(apply.contains("for ch in xkeen xkeen_mask XKEEN_BS zapret"));
        let remove = iptables_remove_script();
        assert!(remove.contains("for ch in xkeen xkeen_mask XKEEN_BS zapret"));
    }

    #[test]
    fn regression_vkturn_25_tun_configure_precleans_stale_csq0_and_supervise_tears_down() {
        let script = tun_configure_script("10.66.67.3".parse().unwrap());
        let check_pos = script
            .find("if printf '%s\\n' \"$existing_rules\"")
            .expect("foreign rule guard must stay");
        for stale_cleanup in [
            "ip rule del pref 222 dev csq0 table 222",
            "ip rule del pref 45 fwmark 222 table 222",
            "ip route del default dev csq0 table 222",
            "ip addr flush dev csq0",
        ] {
            let pos = script
                .find(stale_cleanup)
                .unwrap_or_else(|| panic!("missing pre-cleanup command: {stale_cleanup}"));
            assert!(
                pos < check_pos,
                "stale csq0 cleanup `{stale_cleanup}` must run BEFORE foreign rule check"
            );
        }
        assert!(
            script.contains("ip rule del pref 40 from") && script.find("ip rule del pref 40 from").unwrap() < check_pos,
            "stale pref 40 from old csq0 address must be pre-cleaned before foreign rule check"
        );

        let src = include_str!("vkturn.rs");
        let sup_body = src
            .split("async fn supervise(")
            .nth(1)
            .and_then(|s| s.split("fn spawn_log_reader(").next())
            .expect("supervise function must exist");
        assert!(
            sup_body.contains("tun_teardown_script_for_ip") && sup_body.contains("rt.tun_configured = false"),
            "supervise post-exit must tear down csq0 routes/rules and reset tun_configured before backoff"
        );

        let commit_body = src
            .split("pub async fn commit_vkturn")
            .nth(1)
            .and_then(|s| s.split("struct Supervisor").next())
            .expect("commit_vkturn function must exist");
        assert!(
            commit_body.contains("delete_provider_from_yaml"),
            "commit_vkturn must remove phantom vkturn provider when tunnel is disabled"
        );
    }

    #[test]
    fn regression_vkturn_speed_fix_mtu_mss_quic_and_mark222_exemptions() {
        // 1. MTU TUN строго равен MAX_PACKET_PAYLOAD ядра csqtt (1280 байт), чтобы исключить фрагментацию 0xCF
        assert_eq!(TUN_MTU, 1280);
        assert_eq!(TUN_TCP_MSS, 1220);

        // 2. TCP MSS clamping и мгновенный fallback с QUIC (UDP/443 REJECT) на csq0 + очистка
        let configure = tun_configure_script("10.66.67.3".parse().unwrap());
        let teardown = tun_teardown_script_for_ip(Some("10.66.67.3".parse().unwrap()));
        for ch in ["POSTROUTING", "OUTPUT"] {
            assert!(configure.contains(ch), "отсутствует цепочка исходящего зажима MSS: {ch}");
        }
        for ch in ["PREROUTING", "INPUT", "FORWARD"] {
            assert!(configure.contains(ch), "отсутствует цепочка входящего зажима MSS: {ch}");
        }
        assert!(configure.contains("-o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1220"));
        assert!(configure.contains("-i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1220"));
        assert!(configure.contains("-o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT"));

        assert!(teardown.contains("-o csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1220"));
        assert!(teardown.contains("-i csq0 -p tcp --tcp-flags SYN,RST SYN -m comment --comment xkeen-route-vkturn -j TCPMSS --set-mss 1220"));
        assert!(teardown.contains("-o csq0 -p udp --dport 443 -m comment --comment xkeen-route-vkturn -j REJECT"));

        // 3. Исключения в xkeen/xkeen_mask/XKEEN_BS/zapret для mark 222 и интерфейса csq0 + полная очистка
        let apply = iptables_apply_script(&[]);
        assert!(apply.contains("-I \"$ch\" 1 -m mark --mark 222 -m comment --comment xkeen-route-vkturn -j RETURN"));
        assert!(apply.contains("-I \"$ch\" 1 -o csq0 -m comment --comment xkeen-route-vkturn -j RETURN"));
        assert!(apply.contains("-I \"$ch\" 1 -i csq0 -m comment --comment xkeen-route-vkturn -j RETURN"));

        let remove = iptables_remove_script();
        assert!(remove.contains("for ch in POSTROUTING OUTPUT FORWARD PREROUTING INPUT; do"));
        assert!(remove.contains("iptables -w -t mangle -S \"$ch\""));
        assert!(remove.contains("for ch in OUTPUT FORWARD; do"));
        assert!(remove.contains("iptables -w -t filter -S \"$ch\""));

        // 4. Провайдер vkturn использует IPv4-only и tfo: false (включая добавление и миграцию config.yaml)
        let prov_yaml = render_provider_yaml();
        assert!(prov_yaml.contains("ip-version: ipv4"));
        assert!(prov_yaml.contains("tfo: false"));
        assert!(!prov_yaml.contains("tfo: true"));

        let base = "proxy-providers:\n  other:\n    type: file\n    override:\n      tfo: true\nrules:\n  - MATCH,DIRECT\n";
        let with_vk = crate::routing::add_file_provider_to_yaml(base, PROVIDER_ID, "./providers/vkturn.yaml", None, None).unwrap();
        let vk_section = with_vk.split("  vkturn:").nth(1).unwrap().split("  other:").next().unwrap();
        assert!(vk_section.contains("tfo: false"), "add_file_provider_to_yaml обязан ставить tfo: false для vkturn: {vk_section}");
        assert!(!vk_section.contains("tfo: true"));
        let other_section = with_vk.split("  other:").nth(1).unwrap();
        assert!(other_section.contains("tfo: true"), "другие провайдеры не должны терять tfo: true");

        let legacy_yaml = "proxy-providers:\r\n  sub1:\r\n    type: http\r\n    override:\r\n      tfo: true\r\n  vkturn:\r\n    type: file\r\n    override:\r\n      udp: true\r\n      tfo: true\r\n  sub2:\r\n    type: file\r\n    override:\r\n      tfo: true\r\nrules:\r\n  - MATCH,PROXY\r\n";
        let normalized = normalize_vkturn_provider_in_yaml(legacy_yaml);
        assert!(normalized.contains("\r\n"), "CRLF должен сохраняться");
        let norm_vk = normalized.split("  vkturn:").nth(1).unwrap().split("  sub2:").next().unwrap();
        assert!(norm_vk.contains("tfo: false") && !norm_vk.contains("tfo: true"));
        assert!(normalized.split("  sub1:").nth(1).unwrap().split("  vkturn:").next().unwrap().contains("tfo: true"));
        assert!(normalized.split("  sub2:").nth(1).unwrap().contains("tfo: true"));
        assert_eq!(normalize_vkturn_provider_in_yaml(&normalized), normalized, "идемпотентность нормализации");
        let unchanged = "proxy-providers:\n  vkturn:\n    type: file\n    path: ./providers/vkturn.yaml\nrules:\n  - MATCH,PROXY\n";
        assert_eq!(normalize_vkturn_provider_in_yaml(unchanged), unchanged);
    }
}
