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
/// MTU как у Android-клиента csqtt (CsqttConstants.Vpn.DEFAULT_MTU).
pub const TUN_MTU: u16 = 1500;
/// Отдельная таблица маршрутизации для сокетов, привязанных к csq0 (правило `oif csq0`).
pub const ROUTE_TABLE: u32 = 7300;
/// Подсеть TURN-серверов ВК-звонков. Трафик клиента туда идёт строго напрямую.
/// Конкретные адреса TURN, которые ВК выдал клиенту, панель дополнительно берёт из его лога.
pub const VK_TURN_CIDRS: &[&str] = &["155.212.192.0/20"];
pub const RULES_BEGIN: &str = "# BEGIN xkeen-route vkturn";
pub const RULES_END: &str = "# END xkeen-route vkturn";
const LOG_TAIL: usize = 300;
/// csqtt делит потоки на группы по 10 (`WORKERS_PER_GROUP` в worker.rs), каждая группа берёт свой хеш звонка.
pub const WORKERS_PER_GROUP: u32 = 10;
/// `MAX_WORKERS` в main.rs клиента.
pub const MAX_WORKERS: u32 = 100;
pub const DEFAULT_WORKERS: u32 = 20;
/// Максимум ссылок: все используются одновременно (`MAX_VK_HASHES` в main.rs клиента).
pub const MAX_LINKS: usize = 6;
/// Сколько адресов TURN запоминаем для исключений (защита от разрастания правил).
const MAX_TURN_IPS: usize = 32;
const EVENT_PREFIX: &str = "__CSQTT_EVENT__|";
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
    let link_re = regex_lite::Regex::new(r"^https://(?:m\.)?vk\.(?:com|ru)/call/join/([^?#/\s]+)/?(?:[?#][^\s]*)?$").unwrap();
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

/// Пользователь может ввести только IP/домен — PEER-порт сервера csqtt по умолчанию подставляется сам.
pub fn normalize_peer(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(String::new());
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

/// Число потоков для `-n`: кратно 10, в пределах 10–100, и не меньше одной группы на каждую ссылку —
/// иначе клиент не возьмёт запасные хеши (группа g берёт хеш g % len).
pub fn effective_workers(cfg: &VkTurnConfig) -> u32 {
    let wanted = if cfg.streams == 0 { DEFAULT_WORKERS } else { cfg.streams as u32 };
    let per_links = WORKERS_PER_GROUP * cfg.links.len().clamp(1, MAX_LINKS) as u32;
    let n = wanted.max(per_links).clamp(WORKERS_PER_GROUP, MAX_WORKERS);
    (n / WORKERS_PER_GROUP) * WORKERS_PER_GROUP
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

/// Аргументы передаются МАССИВОМ, без shell — никакой подстановки команд. Пароля здесь нет:
/// он уходит в `CSQTT_PASSWORD` (см. client_env).
pub fn build_client_args(cfg: &VkTurnConfig) -> Vec<String> {
    vec![
        "-peer".into(),
        cfg.peer.trim().to_string(),
        "-vk".into(),
        call_hashes(cfg).join(","),
        "-n".into(),
        effective_workers(cfg).to_string(),
        "-listen".into(),
        "127.0.0.1:0".into(),
        "-tun-uds".into(),
        TUN_UDS_NAME.into(),
        "-captcha-mode".into(),
        "auto".into(),
    ]
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
    /// Адрес туннеля, выданный сервером (TUNCONF).
    pub tunnel_ip: Option<Ipv4Addr>,
    /// DNS, который сервер предлагает внутри туннеля.
    pub tunnel_dns: Option<String>,
    /// Дескриптор TUN передан текущему процессу клиента.
    pub tun_attached: bool,
    /// Адреса TURN, которые ВК выдал клиенту — для исключений в iptables.
    #[serde(skip)]
    pub turn_ips: BTreeSet<Ipv4Addr>,
    #[serde(skip)]
    pub started_at: Option<Instant>,
    #[serde(skip)]
    pub zero_streams_since: Option<Instant>,
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
            tunnel_ip: None,
            tunnel_dns: None,
            tun_attached: false,
            turn_ips: BTreeSet::new(),
            started_at: None,
            zero_streams_since: None,
            log_tail: VecDeque::with_capacity(LOG_TAIL),
        }
    }
}

impl VkTurnRuntime {
    /// Туннель готов принимать трафик Mihomo: сервер выдал адрес и TUN передан клиенту.
    pub fn tunnel_ready(&self) -> bool {
        self.tunnel_ip.is_some() && self.tun_attached
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
        VkTurnPhase::CaptchaRequired | VkTurnPhase::LinkInvalid | VkTurnPhase::AuthFailed
    )
}

fn push_log(rt: &mut VkTurnRuntime, line: String) {
    if rt.log_tail.len() >= LOG_TAIL {
        rt.log_tail.pop_front();
    }
    rt.log_tail.push_back(line);
}

fn set_connected(rt: &mut VkTurnRuntime) {
    rt.phase = VkTurnPhase::Connected;
    rt.zero_streams_since = None;
    rt.last_error = None;
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
    } else if line.contains("WRAP_AUTH_TIMEOUT") {
        if no_streams && rt.phase != VkTurnPhase::AuthFailed {
            rt.phase = VkTurnPhase::ServerUnreachable;
            rt.last_error = Some(
                "Сервер csqtt на VPS не отвечает: проверьте адрес, порт и пароль подключения (при неверном пароле сервер молчит)".into(),
            );
        }
    } else if is_call_unavailable_line(line) {
        if no_streams {
            rt.phase = VkTurnPhase::LinkInvalid;
        }
        rt.last_error = Some("Ссылка на звонок ВК не работает: звонок завершён или удалён".into());
    } else if is_captcha_failed_line(line) {
        if no_streams {
            rt.phase = VkTurnPhase::CaptchaRequired;
        }
        rt.last_error = Some("ВК требует подтвердить «я не робот», автоматически пройти проверку не удалось".into());
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
            push_log(rt, "[ТУННЕЛЬ] Клиент потерял TUN-интерфейс — передам заново после перезапуска".into());
        }
        "ERROR" => {
            let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("ошибка клиента");
            let code = json.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let fatal = json.get("fatal").and_then(|v| v.as_bool()).unwrap_or(false);
            push_log(rt, redact_line(&format!("[ОШИБКА] {msg}")));
            if code.contains("auth") || msg.contains("FATAL_AUTH") {
                rt.phase = VkTurnPhase::AuthFailed;
                rt.last_error = Some(auth_error_text(msg).into());
            } else {
                rt.last_error = Some(redact_line(msg));
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
    format!("proxies:\n  - name: \"{PROXY_NAME}\"\n    type: direct\n    interface-name: {TUN_IFACE}\n    udp: true\n")
}

/// Правила DIRECT в начало `rules:`, чтобы трафик к TURN и к VPS никогда не ушёл в туннель или петлю.
pub fn rules_block(cfg: &VkTurnConfig) -> String {
    let mut lines = vec![format!("  {RULES_BEGIN}")];
    for c in VK_TURN_CIDRS {
        lines.push(format!("  - IP-CIDR,{c},DIRECT,no-resolve"));
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

/// Адреса, к которым клиент подключается сам. Только константы и разобранные Ipv4Addr — без инъекций.
pub fn exempt_destinations(peer_ipv4: Option<Ipv4Addr>, turn_ips: &BTreeSet<Ipv4Addr>) -> Vec<String> {
    let mut v: Vec<String> = VK_TURN_CIDRS.iter().map(|c| c.to_string()).collect();
    let in_vk_subnet = |ip: &Ipv4Addr| (u32::from(*ip) & 0xFFFF_F000) == u32::from(Ipv4Addr::new(155, 212, 192, 0));
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
  for ch in xkeen_mask zapret; do
    iptables -t "$tbl" -nL "$ch" >/dev/null 2>&1 || continue
    for dst in {list}; do
      iptables -t "$tbl" -C "$ch" -d "$dst" -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null \
        || iptables -t "$tbl" -I "$ch" 1 -d "$dst" -m comment --comment {IPT_COMMENT} -j RETURN 2>/dev/null || true
    done
  done
done"#
    )
}

/// Снимает все наши правила (по комментарию), в том числе для старого адреса VPS.
pub fn iptables_remove_script() -> String {
    format!(
        r#"for tbl in mangle nat; do
  for ch in xkeen_mask zapret; do
    iptables -t "$tbl" -S "$ch" 2>/dev/null | grep -- "--comment {IPT_COMMENT}" | sed 's/^-A /-D /' | while read -r rule; do
      eval iptables -t "$tbl" $rule 2>/dev/null || true
    done
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
pub fn tun_configure_script(ip: Ipv4Addr) -> String {
    format!(
        "ip addr flush dev {TUN_IFACE} 2>/dev/null
ip addr add {ip}/32 dev {TUN_IFACE}
ip link set dev {TUN_IFACE} mtu {TUN_MTU} up
ip route replace default dev {TUN_IFACE} table {ROUTE_TABLE}
ip rule del pref {ROUTE_TABLE} 2>/dev/null
ip rule add oif {TUN_IFACE} lookup {ROUTE_TABLE} pref {ROUTE_TABLE}
echo 2 > /proc/sys/net/ipv4/conf/{TUN_IFACE}/rp_filter 2>/dev/null
true"
    )
}

/// Снимает маршрутизацию туннеля (сам интерфейс исчезает, когда панель закрывает дескриптор TUN).
pub fn tun_teardown_script() -> String {
    format!(
        "ip rule del pref {ROUTE_TABLE} 2>/dev/null
ip route flush table {ROUTE_TABLE} 2>/dev/null
true"
    )
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
    fn send_to_client_blocking(&self) -> Result<(), String> {
        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let mut last = String::new();
            for _ in 0..60 {
                match tun_sys::send_fd(TUN_UDS_NAME, self.fd.as_raw_fd()) {
                    Ok(()) => return Ok(()),
                    Err(e) => last = e.to_string(),
                }
                std::thread::sleep(Duration::from_millis(500));
            }
            Err(format!("Клиент не принял TUN-интерфейс: {last}"))
        }
        #[cfg(not(target_os = "linux"))]
        {
            Err("TUN поддерживается только на Linux".into())
        }
    }
}

// ---------- Запись в config.json / config.yaml (одна транзакция) ----------

/// Применяет изменение настроек VK-туннеля: config.json, файл провайдера, провайдер и правила в config.yaml,
/// перезагрузка Mihomo. Всё в одной транзакции ConfigTx (блокировки берёт только она).
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
    if !tx.config().whitelist_providers.iter().any(|p| p == PROVIDER_ID) {
        tx.config_mut().whitelist_providers.push(PROVIDER_ID.to_string());
    }

    let providers_dir = std::path::Path::new(&tx.config().mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));
    let prov_file = providers_dir.join(format!("{PROVIDER_ID}.yaml"));
    tx.set_extra_file(&prov_file, render_provider_yaml(), true)
        .await
        .map_err(|e| format!("Ошибка записи файла провайдера VK-туннеля: {e}"))?;

    let yaml = tx.read_yaml().await.map_err(|e| format!("Ошибка чтения config.yaml: {e}"))?;
    let has_prov = crate::routing::parse_provider_names(&yaml).iter().any(|p| p == PROVIDER_ID);
    let yaml_with_prov = if new_cfg.enabled && !has_prov {
        crate::routing::add_file_provider_to_yaml(
            &yaml,
            PROVIDER_ID,
            &format!("./providers/{PROVIDER_ID}.yaml"),
            Some(tx.config().health_check_url()),
            Some(tx.config().mihomo.health_check_interval),
        )
        .map_err(|e| format!("Ошибка добавления провайдера в YAML: {e}"))?
    } else {
        yaml
    };

    let new_yaml = apply_rules(&yaml_with_prov, &new_cfg)
        .map_err(|e| format!("Ошибка применения правил маршрутизации VK-туннеля: {e}"))?;
    tx.set_yaml(new_yaml).map_err(|e| format!("Ошибка сохранения нового YAML: {e}"))?;
    tx.commit_and_reload().await.map_err(|e| format!("Ошибка транзакции сохранения: {e}"))?;
    drop(tx);
    Ok(new_cfg)
}

// ---------- Менеджер процесса ----------

/// Запущенный надзор: канал остановки и задача, завершения которой ждём при остановке.
struct Supervisor {
    stop_tx: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

pub struct VkTurnManager {
    pub rt: RwLock<VkTurnRuntime>,
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
        if !cfg.enabled || call_hashes(&cfg).is_empty() || cfg.password.is_empty() || cfg.peer.is_empty() {
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
        if let Some(sup) = slot.take() {
            let _ = sup.stop_tx.send(true);
            let mut task = sup.task;
            if tokio::time::timeout(Duration::from_secs(5), &mut task).await.is_err() {
                task.abort();
                crate::log_w!("[VK-туннель] Надзор не завершился за 5 с — прерван принудительно");
            }
            run_sh(&tun_teardown_script()).await;
        }
        let _ = tokio::fs::remove_file(PID_FILE).await;
        let mut rt = self.rt.write().await;
        rt.pid = None;
        rt.connected_streams = 0;
        rt.tunnel_ip = None;
        rt.tun_attached = false;
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

        loop {
            if *stop_rx.borrow() {
                break;
            }
            // Перед каждым запуском: XKeen/Zapret могли пересоздать свои цепочки и стереть исключения.
            let known_turn = self.rt.read().await.turn_ips.clone();
            apply_exemptions(&cfg.peer, &known_turn).await;

            let mut cmd = tokio::process::Command::new(&self.core_path);
            cmd.args(build_client_args(&cfg))
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
            }
            crate::log_i!(
                "[VK-туннель] Клиент csqtt запущен (pid {:?}, ссылок {}, потоков {})",
                pid,
                call_hashes(&cfg).len(),
                effective_workers(&cfg)
            );

            // stdout (события, CAPTCHA_SOLVE) и stderr (лог) → разбор; действия — в канал надзора.
            let (eff_tx, mut eff_rx) = mpsc::unbounded_channel::<LineEffect>();
            for stream in [
                child.stderr.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
                child.stdout.take().map(|s| Box::new(s) as Box<dyn tokio::io::AsyncRead + Unpin + Send>),
            ]
            .into_iter()
            .flatten()
            {
                let me = self.clone();
                let eff_tx = eff_tx.clone();
                tokio::spawn(async move {
                    let mut lines = BufReader::new(stream).lines();
                    while let Ok(Some(l)) = lines.next_line().await {
                        let eff = apply_log_line(&mut *me.rt.write().await, &l, Instant::now());
                        if eff != LineEffect::None {
                            let _ = eff_tx.send(eff);
                        }
                    }
                });
            }
            drop(eff_tx);
            let mut stdin = child.stdin.take();
            let mut fd_sending = false;

            // ждём: выход процесса / команду стоп / «висим без соединений» дольше допустимого
            let started = Instant::now();
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            let mut ticks: u32 = 0;
            let mut exempted_turn = known_turn;
            let exit_reason = loop {
                tokio::select! {
                    st = child.wait() => break format!("клиент завершился: {:?}", st.ok()),
                    _ = stop_rx.changed() => {
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
                            let changed = {
                                let mut rt = self.rt.write().await;
                                let changed = rt.tunnel_ip != Some(ip);
                                rt.tunnel_ip = Some(ip);
                                rt.tunnel_dns = dns;
                                changed
                            };
                            if changed {
                                run_sh(&tun_configure_script(ip)).await;
                            }
                            if !fd_sending {
                                fd_sending = true;
                                self.spawn_fd_handoff(&tun);
                            }
                        }
                        LineEffect::None => {}
                    },
                    _ = tick.tick() => {
                        ticks = ticks.wrapping_add(1);
                        let rt = self.rt.read().await;
                        let turn_now = rt.turn_ips.clone();
                        let stalled = rt.connected_streams == 0
                            && rt.zero_streams_since.map(|t| t.elapsed() > stall_limit(rt.phase)).unwrap_or(false);
                        drop(rt);
                        // раз в минуту (или при новых адресах TURN) восстанавливаем исключения
                        if ticks % 6 == 0 || turn_now != exempted_turn {
                            apply_exemptions(&cfg.peer, &turn_now).await;
                            exempted_turn = turn_now;
                        }
                        if stalled {
                            let _ = child.kill().await;
                            break "нет ни одного соединения дольше допустимого".to_string();
                        }
                    }
                }
            };

            let phase = {
                let mut rt = self.rt.write().await;
                rt.restarts += 1;
                rt.pid = None;
                rt.connected_streams = 0;
                rt.tun_attached = false;
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

    /// Передаёт клиенту копию дескриптора TUN в фоне: надзор продолжает читать вывод клиента.
    fn spawn_fd_handoff(self: &Arc<Self>, tun: &Arc<TunHandle>) {
        let copy = match tun.duplicate() {
            Ok(c) => c,
            Err(e) => {
                crate::log_w!("[VK-туннель] {e}");
                return;
            }
        };
        let me = self.clone();
        tokio::spawn(async move {
            let res = tokio::task::spawn_blocking(move || copy.send_to_client_blocking())
                .await
                .unwrap_or_else(|e| Err(format!("задача передачи TUN: {e}")));
            let mut rt = me.rt.write().await;
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
        });
    }

    async fn set_error(&self, phase: VkTurnPhase, msg: String) {
        let mut rt = self.rt.write().await;
        rt.phase = phase;
        rt.last_error = Some(msg);
    }
}

async fn sleep_or_stop(stop_rx: &mut watch::Receiver<bool>, d: Duration) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => !*stop_rx.borrow(),
        _ = stop_rx.changed() => false,
    }
}

async fn kill_stale_from_pidfile() {
    let Ok(s) = tokio::fs::read_to_string(PID_FILE).await else { return };
    let Ok(pid) = s.trim().parse::<u32>() else { return };
    if let Ok(cmd) = tokio::fs::read(format!("/proc/{pid}/cmdline")).await {
        if String::from_utf8_lossy(&cmd).contains("csqtt-client") {
            let _ = tokio::process::Command::new("kill").arg(pid.to_string()).status().await;
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
        assert_eq!(effective_workers(&cfg), 30, "кратно 10");
        cfg.streams = 200;
        assert_eq!(effective_workers(&cfg), 100);
        cfg.streams = 1;
        assert_eq!(effective_workers(&cfg), 10);
        // Три ссылки при 10 потоках — клиент взял бы только первую: поднимаем до 30
        cfg.links = vec![
            "https://vk.com/call/join/aaaa1111".into(),
            "https://vk.com/call/join/bbbb2222".into(),
            "https://vk.com/call/join/cccc3333".into(),
        ];
        assert_eq!(effective_workers(&cfg), 30);
    }

    /// Пароль не должен попадать в аргументы процесса (их видно в `ps` любому пользователю роутера).
    #[test]
    fn test_build_client_args_and_env() {
        let cfg = cfg_with(&["https://vk.com/call/join/abc12345", "https://vk.com/call/join/def67890"]);
        let args = build_client_args(&cfg);
        assert_eq!(
            args,
            vec![
                "-peer", "1.2.3.4:46000",
                "-vk", "abc12345,def67890",
                "-n", "20",
                "-listen", "127.0.0.1:0",
                "-tun-uds", TUN_UDS_NAME,
                "-captcha-mode", "auto",
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
        assert!(rt.tunnel_ready());
    }

    #[test]
    fn tun_scripts_only_use_constants_and_parsed_ip() {
        let s = tun_configure_script("10.66.67.3".parse().unwrap());
        assert!(s.contains("ip addr add 10.66.67.3/32 dev csq0"));
        assert!(s.contains("ip link set dev csq0 mtu 1500 up"));
        assert!(s.contains("ip route replace default dev csq0 table 7300"));
        assert!(s.contains("ip rule add oif csq0 lookup 7300 pref 7300"));
        assert!(s.contains("ip rule del pref 7300"), "правило не дублируется при повторной настройке");
        let t = tun_teardown_script();
        assert!(t.contains("ip rule del pref 7300") && t.contains("ip route flush table 7300"));
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
        assert!(yaml.contains("udp: true"));
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

    /// Пароль сервера csqtt — секрет: в API настроек он скрыт, а «******» при сохранении не затирает настоящий.
    #[test]
    fn settings_api_masks_and_preserves_tunnel_password() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.vkturn.password = "top-secret-pw".into();
        let masked = crate::api::mask_settings_for_api(&cfg);
        assert_eq!(masked["vkturn"]["password"], "******");
        assert!(!masked.to_string().contains("top-secret-pw"));
        let merged = crate::api::merge_settings_update(&cfg, &masked).expect("merge masked settings");
        assert_eq!(merged.vkturn.password, "top-secret-pw");
    }

    #[test]
    fn test_is_whitelist_server_full_vkturn() {
        let mut cfg = crate::config::AppConfig::default();
        cfg.whitelist_providers.push(PROVIDER_ID.to_string());
        assert!(crate::failover::is_whitelist_server_full(PROXY_NAME, "any_id", Some(PROVIDER_ID), &cfg));
    }

    // ---------- Анти-регрессионные тесты ----------

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
        let _ = std::fs::remove_dir_all(&root);
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
        assert_eq!(dsts, vec!["155.212.192.0/20", "203.0.113.50/32", "198.51.100.7/32"]);
        assert_eq!(exempt_destinations(None, &BTreeSet::new()), vec!["155.212.192.0/20"]);

        let apply = iptables_apply_script(&dsts);
        assert!(apply.contains("for ch in xkeen_mask zapret"));
        assert!(apply.contains("-C \"$ch\""), "проверка -C делает правило идемпотентным");
        assert!(apply.contains("-I \"$ch\" 1"), "RETURN обязан стоять первым в цепочке");
        assert!(apply.contains(&format!("--comment {IPT_COMMENT} -j RETURN")));
        assert!(apply.contains("|| continue"), "отсутствующая цепочка пропускается");
        let remove = iptables_remove_script();
        assert!(remove.contains(&format!("--comment {IPT_COMMENT}")));
        assert!(remove.contains("sed 's/^-A /-D /'"));
    }

    #[tokio::test]
    async fn resolve_peer_ipv4_accepts_literal_and_rejects_garbage() {
        assert_eq!(resolve_peer_ipv4("1.2.3.4:46000").await, Some("1.2.3.4".parse().unwrap()));
        assert_eq!(resolve_peer_ipv4("no-port").await, None);
    }
}
