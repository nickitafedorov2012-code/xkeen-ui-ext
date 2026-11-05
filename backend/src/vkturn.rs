//! VK-туннель: обход белых списков через TURN-серверы ВК-звонков на ядре WDTT
//! (клиент из amurcanov/proxy-turn-vk-android, GPL-3.0; сборка — `scripts/build-wdtt-core.sh`).
//! Панель сама скачивает клиент, запускает его, следит за ним и подключает к Mihomo
//! как WireGuard-сервер второго уровня (Tier 2, «БС»).
//!
//! Что проверено по исходникам WDTT (коммит CORE_UPSTREAM_COMMIT, 20.10.2026):
//! - флаги клиента: `-peer host:port -vk <хеши через запятую> -n <воркеры, кратно 9> -listen 127.0.0.1:<порт>
//!   -device-id <id> -password <пароль> -captcha-mode auto`; `-password` обязателен (из него выводится ключ WRAP);
//! - при `WDTT_EVENTS=1` клиент печатает в stdout машинные события `__WDTT_EVENT__|<ТИП>|<json>`:
//!   READY (воркер поднял DTLS), STATS (каждые 3 с: active, bytes_up, bytes_down), CONFIG (wg-конфиг от сервера);
//! - WireGuard-конфиг выдаёт сам сервер WDTT на VPS (ответ на GETCONF), пароль привязывается к device-id —
//!   поэтому device-id роутера генерируется один раз и хранится в config.json;
//! - ручная капча: клиент печатает `CAPTCHA_SOLVE|<mode>|<redirect_uri>|<session_token>` и ждёт
//!   `CAPTCHA_RESULT|<token>` в stdin. На роутере WebView нет — панель сразу отвечает ошибкой,
//!   и клиент продолжает встроенное автоматическое решение (PoW + слайдер) без ожидания таймаутов;
//! - если у всех групп воркеров не получилось получить TURN-креды (капча, мёртвая ссылка, отказ сервера),
//!   клиент завершается сам — надзор перезапускает его с паузой.

use std::collections::{BTreeSet, VecDeque};
use std::net::Ipv4Addr;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, watch, Mutex, RwLock};

use crate::config::{VkTurnConfig, VkTurnWireGuard};

/// Тег релиза xkeen-ui-ext, в котором опубликованы бинарники ядра (см. scripts/build-wdtt-core.sh).
pub const CORE_TAG: &str = "wdtt-core-7355598";
/// Коммит amurcanov/proxy-turn-vk-android, из которого собрано ядро.
pub const CORE_UPSTREAM_COMMIT: &str = "7355598eae4a359d6874409f3437768f3548565b";
pub const CORE_RELEASES: &str = "https://github.com/nickitafedorov2012-code/xkeen-ui-ext/releases/download";
pub const CORE_PATH: &str = "/opt/sbin/wdtt-client";
/// Рабочий каталог клиента: WDTT сохраняет туда полученный wg-конфиг (wg-turn.conf, права 0600).
pub const WORK_DIR: &str = "/opt/var/lib/wdtt";
pub const PID_FILE: &str = "/opt/var/run/wdtt-client.pid";
pub const PROVIDER_ID: &str = "vkturn";
pub const PROXY_NAME: &str = "📞 VK-туннель [БС]";
/// Порт DTLS сервера WDTT на VPS по умолчанию (`linux-server -listen 0.0.0.0:56000`).
pub const DEFAULT_SERVER_PORT: u16 = 56000;
/// Подсеть TURN-серверов ВК-звонков. Трафик клиента туда идёт строго напрямую.
/// Конкретные адреса TURN, которые ВК выдал клиенту, панель дополнительно берёт из его лога.
pub const VK_TURN_CIDRS: &[&str] = &["155.212.192.0/20"];
pub const RULES_BEGIN: &str = "# BEGIN xkeen-route vkturn";
pub const RULES_END: &str = "# END xkeen-route vkturn";
const LOG_TAIL: usize = 300;
/// WDTT делит воркеров на группы по 9 (`workersPerGroup` в worker_group.go), каждая группа берёт свой хеш звонка.
pub const WORKERS_PER_GROUP: u32 = 9;
pub const MAX_WORKERS: u32 = 108;
pub const DEFAULT_WORKERS: u32 = 18;
/// Максимум ссылок: все используются одновременно (по группе воркеров на ссылку), как в приложении WDTT.
pub const MAX_LINKS: usize = 3;
/// Сколько адресов TURN запоминаем для исключений (защита от разрастания правил).
const MAX_TURN_IPS: usize = 32;
const EVENT_PREFIX: &str = "__WDTT_EVENT__|";
const CAPTCHA_SOLVE_PREFIX: &str = "CAPTCHA_SOLVE|";
/// Ответ клиенту на запрос WebView-капчи: на роутере WebView нет.
pub const CAPTCHA_NO_WEBVIEW_REPLY: &str = "CAPTCHA_RESULT|error:webview unavailable on router\n";

/// SHA-256 бинарников CORE_TAG (вывод scripts/build-wdtt-core.sh, go1.27.3).
pub const CORE_SHA256: &[(&str, &str)] = &[
    ("wdtt-client-linux-arm64", "9c0593b9efb8ac70adae8ac8eb4b1ef55211662d5629ab5a3970a2a6e7af5089"),
    ("wdtt-client-linux-mipsle", "da75eb994042c00ff203414ccbaea52dee0fa24e4c91ccd0d7b61dc89c1c55b1"),
    ("wdtt-client-linux-mips", "eba0b64d8134a5f1ec535c6be776d23ac24c8c736709d9646a1af98103f1e1a4"),
    ("wdtt-client-linux-armv7", "f49f51510d89a4a060aedc30375bc065890499d5c135be1b2325fbf17e22d119"),
    ("wdtt-client-linux-amd64", "18fa03369230f2d4e8dc7d6c430de6f9454f26511eead1803c11b2bfc0a35676"),
];

/// Сопоставление с архитектурами от updater::detect_mihomo_arch()
pub fn core_asset_for_arch(arch: &str) -> Option<&'static str> {
    let a = arch.to_ascii_lowercase();
    if a.contains("arm64") || a.contains("aarch64") {
        Some("wdtt-client-linux-arm64")
    } else if a.starts_with("mipsle") || a.starts_with("mipsel") {
        Some("wdtt-client-linux-mipsle")
    } else if a.starts_with("mips") {
        Some("wdtt-client-linux-mips")
    } else if a.starts_with("arm") {
        Some("wdtt-client-linux-armv7")
    } else if a.contains("amd64") || a.contains("x86_64") {
        Some("wdtt-client-linux-amd64")
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
        return Err("Ядро WDTT работает только со звонками ВКонтакте — ссылка Телемоста не подойдёт".into());
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

/// Пользователь может ввести только IP/домен — порт сервера WDTT по умолчанию подставляется сам.
pub fn normalize_peer(raw: &str) -> Result<String, String> {
    let s = raw.trim();
    if s.is_empty() {
        return Ok(String::new());
    }
    let with_port = if s.contains(':') { s.to_string() } else { format!("{s}:{DEFAULT_SERVER_PORT}") };
    validate_peer(&with_port)?;
    Ok(with_port)
}

/// Пароль подключения уходит на сервер строкой `GETCONF:<порт>|<device-id>|<пароль>`,
/// поэтому `|` и управляющие символы недопустимы.
pub fn validate_password(pw: &str) -> Result<(), String> {
    if pw.is_empty() {
        return Err("Укажите пароль подключения к серверу WDTT".into());
    }
    if pw.chars().count() > 128 {
        return Err("Пароль подключения слишком длинный (больше 128 символов)".into());
    }
    if pw.contains('|') || pw.chars().any(|c| c.is_control()) {
        return Err("Пароль подключения не должен содержать символ «|» и переводы строк".into());
    }
    Ok(())
}

/// Постоянный идентификатор роутера для сервера WDTT (пароль привязывается к нему при первом входе).
pub fn generate_device_id() -> String {
    format!("xkeen-route-{}", crate::auth::random_hex(8))
}

pub fn is_valid_device_id(id: &str) -> bool {
    let re = regex_lite::Regex::new(r"^[A-Za-z0-9_\-]{6,64}$").unwrap();
    re.is_match(id)
}

/// Локальный порт клиента: только непривилегированные порты (клиент слушает 127.0.0.1).
pub fn validate_listen_port(port: u16) -> Result<(), String> {
    if port < 1024 {
        return Err(format!("Локальный порт VK-туннеля {port} недопустим: нужен порт 1024–65535"));
    }
    Ok(())
}

/// Порт клиента не должен совпадать с портами Mihomo (входы config.yaml, DNS, API панели Mihomo).
pub fn listen_port_conflict(port: u16, yaml: &str, mihomo_api_port: u16) -> Option<String> {
    let mut busy: Vec<(u16, &str)> = vec![(mihomo_api_port, "API Mihomo")];
    for line in yaml.lines() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        for key in ["port", "socks-port", "mixed-port", "redir-port", "tproxy-port"] {
            if let Some(rest) = line.strip_prefix(key).and_then(|r| r.strip_prefix(':')) {
                if let Ok(p) = rest.trim().trim_matches(['"', '\'']).parse::<u16>() {
                    busy.push((p, key));
                }
            }
        }
    }
    if let Some(p) = crate::routing::parse_mihomo_dns_listen_port(yaml) {
        busy.push((p, "DNS Mihomo"));
    }
    busy.iter()
        .find(|(p, _)| *p == port)
        .map(|(_, what)| format!("Локальный порт VK-туннеля {port} уже занят ({what}). Выберите другой."))
}

/// Число воркеров для `-n`: кратно 9, в пределах 9–108, и не меньше одной группы на каждую ссылку —
/// иначе клиент просто не возьмёт запасные хеши (группа g берёт хеш g % len).
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

/// Аргументы передаются МАССИВОМ, без shell — никакой подстановки команд.
pub fn build_client_args(cfg: &VkTurnConfig) -> Vec<String> {
    vec![
        "-peer".into(),
        cfg.peer.trim().to_string(),
        "-vk".into(),
        call_hashes(cfg).join(","),
        "-n".into(),
        effective_workers(cfg).to_string(),
        "-listen".into(),
        format!("127.0.0.1:{}", cfg.listen_port),
        "-device-id".into(),
        cfg.device_id.clone(),
        "-password".into(),
        cfg.password.clone(),
        "-captcha-mode".into(),
        "auto".into(),
    ]
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
    /// сервер WDTT отклонил пароль (неверный, истёк, привязан к другому устройству)
    AuthFailed,
    /// DTLS до сервера на VPS не устанавливается
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
            turn_ips: BTreeSet::new(),
            started_at: None,
            zero_streams_since: None,
            log_tail: VecDeque::with_capacity(LOG_TAIL),
        }
    }
}

/// Что надзору нужно сделать в ответ на строку вывода клиента.
#[derive(Debug, Clone, PartialEq)]
pub enum LineEffect {
    None,
    /// Клиент ждёт ответ на запрос WebView-капчи в stdin.
    CaptchaSolve,
    /// Сервер выдал WireGuard-конфиг (текст .conf, содержит приватный ключ — не логировать).
    WgConfig(String),
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

/// Чистая функция — покрыта тестами на строках из исходников WDTT.
pub fn apply_log_line(rt: &mut VkTurnRuntime, line: &str, now: Instant) -> LineEffect {
    let line = line.trim_end_matches('\r');
    let t = line.trim();
    if t.is_empty() {
        return LineEffect::None;
    }
    // Рамка с wg-конфигом в stdout (`╔══ WireGuard Конфиг ══╗`, `║ PrivateKey = … ║`) — в лог не пускаем.
    if t.starts_with('║') || t.starts_with('╚') {
        return LineEffect::None;
    }
    if t.starts_with('╔') {
        push_log(rt, "[КОНФИГ] Сервер выдал WireGuard-конфиг (ключи скрыты)".into());
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
            rt.last_error = Some("Сервер WDTT на VPS не отвечает: проверьте адрес, порт и пароль подключения".into());
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
    } else if line.contains("[КАПЧА]") || line.contains("[Captcha]") || line.contains("captcha gate appeared") {
        if no_streams && !is_sticky(rt.phase) {
            rt.phase = VkTurnPhase::SolvingCaptcha;
        }
    } else if line.contains("Запрос кредов")
        || line.contains("[VK Auth] Trying credentials")
        || line.contains("[VKCalls] Identity")
        || line.contains("[DTLS] Рукопожатие")
    {
        if no_streams && !is_sticky(rt.phase) && rt.phase != VkTurnPhase::SolvingCaptcha {
            rt.phase = VkTurnPhase::Authorizing;
        }
    } else if line.contains("Rate limit detected") {
        rt.last_error = Some("ВК ограничил частоту запросов, клиент пробует другие учётные данные".into());
    } else if line.contains("Сервер ещё не выдал WireGuard-конфиг") {
        rt.last_error = Some("Сервер на VPS пока не выдал WireGuard-конфиг, клиент повторит запрос".into());
    } else if line.contains("[ФАТАЛ]") || line.contains("[КЛИЕНТ] Нужн") || line.contains("Ошибка разбора пира") || line.contains("Нет хешей VK") {
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
            let active = json.get("active").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
            rt.bytes_up = json.get("bytes_up").and_then(|v| v.as_u64()).unwrap_or(rt.bytes_up);
            rt.bytes_down = json.get("bytes_down").and_then(|v| v.as_u64()).unwrap_or(rt.bytes_down);
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
                return LineEffect::WgConfig(conf.to_string());
            }
        }
        "ERROR" => {
            let msg = json.get("message").and_then(|v| v.as_str()).unwrap_or("ошибка клиента");
            let fatal = json.get("fatal").and_then(|v| v.as_bool()).unwrap_or(false);
            push_log(rt, redact_line(&format!("[ОШИБКА] {msg}")));
            rt.last_error = Some(redact_line(msg));
            if fatal {
                rt.phase = VkTurnPhase::Crashed;
            }
        }
        _ => {}
    }
    LineEffect::None
}

fn auth_error_text(line: &str) -> &'static str {
    if line.contains("неверный пароль") {
        "Сервер WDTT отклонил пароль подключения: пароль неверный"
    } else if line.contains("истёк") {
        "Срок действия пароля подключения истёк — получите новый у владельца сервера"
    } else if line.contains("другому устройству") {
        "Пароль уже привязан к другому устройству — нужен отдельный пароль для роутера"
    } else {
        "Сервер WDTT отказал в доступе"
    }
}

/// Ошибки ВК «звонок недоступен» (error_code 951/954/9xxx — `CallUnavailableError` в vk_auth.go).
fn is_call_unavailable_line(line: &str) -> bool {
    line.contains("VK returns error:")
        || line.contains("VK call is unavailable")
        || line.contains("non-retryable call error")
        || line.contains("хеш мёртв")
}

/// Автоматическое решение капчи окончательно не удалось (vk_auth.go: lockout + CAPTCHA_WAIT_REQUIRED).
fn is_captcha_failed_line(line: &str) -> bool {
    line.contains("CAPTCHA_WAIT_REQUIRED")
        || line.contains("FATAL_CAPTCHA")
        || line.contains("[Captcha] Solve failed")
        || line.contains("[Captcha] Max attempts reached")
}

/// Адреса TURN из строк `turn_server.urls[0]=turn:155.212.200.1:3478?transport=udp` и списка `TURN urls`.
fn learn_turn_ips(rt: &mut VkTurnRuntime, line: &str) {
    if !line.contains("turn:") && !line.contains("turns:") {
        return;
    }
    let re = regex_lite::Regex::new(r"turns?:(\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})").unwrap();
    for c in re.captures_iter(line) {
        if let Ok(ip) = c[1].parse::<Ipv4Addr>() {
            if !ip.is_loopback() && !ip.is_private() && !ip.is_unspecified() && rt.turn_ips.len() < MAX_TURN_IPS {
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
    let keys = regex_lite::Regex::new(r"(?i)((?:PrivateKey|PresharedKey)\s*=\s*)\S+").unwrap();
    let out = links.replace_all(line, "$1$2…");
    let out = tokens.replace_all(&out, "${1}…");
    keys.replace_all(&out, "${1}…").to_string()
}

/// Сколько ждать без единого активного соединения, прежде чем перезапустить клиент.
pub fn stall_limit(phase: VkTurnPhase) -> Duration {
    match phase {
        VkTurnPhase::AuthFailed => Duration::from_secs(15),
        VkTurnPhase::LinkInvalid => Duration::from_secs(20),
        VkTurnPhase::CaptchaRequired => Duration::from_secs(45),
        // Солвер WDTT: PoW + до нескольких попыток слайдера — даём время, но не бесконечно.
        VkTurnPhase::SolvingCaptcha => Duration::from_secs(300),
        _ => Duration::from_secs(120),
    }
}

// ---------- YAML для Mihomo ----------

fn is_wg_key(k: &str) -> bool {
    let re = regex_lite::Regex::new(r"^[A-Za-z0-9+/]{42}[AEIMQUYcgkosw048]=$").unwrap();
    re.is_match(k.trim())
}

fn is_ipv4(s: &str) -> bool {
    s.parse::<Ipv4Addr>().is_ok()
}

/// WireGuard-конфиг от сервера уже получен и годится для Mihomo.
pub fn wg_ready(cfg: &VkTurnConfig) -> bool {
    render_provider_yaml(cfg).is_ok()
}

/// Содержимое file-провайдера `vkturn`. Все значения проверяются заранее — защита от YAML-инъекций.
pub fn render_provider_yaml(cfg: &VkTurnConfig) -> Result<String, String> {
    let wg = &cfg.wg;
    if !is_wg_key(&wg.private_key) {
        return Err("Неверный PrivateKey WireGuard".into());
    }
    if !is_wg_key(&wg.public_key) {
        return Err("Неверный PublicKey сервера WireGuard".into());
    }
    if let Some(psk) = &wg.preshared_key {
        if !is_wg_key(psk) {
            return Err("Неверный PresharedKey".into());
        }
    }
    if !is_ipv4(&wg.address) {
        return Err("Address клиента должен быть IPv4, например 10.66.66.2".into());
    }
    if !(1200..=1420).contains(&wg.mtu) {
        return Err("MTU должен быть в диапазоне 1200–1420".into());
    }
    let dns: Vec<&String> = wg.dns.iter().filter(|d| is_ipv4(d)).collect();
    let dns_line = if dns.is_empty() {
        String::new()
    } else {
        format!(
            "    dns: [{}]\n",
            dns.iter().map(|d| d.as_str()).collect::<Vec<_>>().join(", ")
        )
    };
    let psk_line = wg
        .preshared_key
        .as_ref()
        .map(|p| format!("    pre-shared-key: \"{p}\"\n"))
        .unwrap_or_default();

    Ok(format!(
        "proxies:\n  - name: \"{PROXY_NAME}\"\n    type: wireguard\n    server: 127.0.0.1\n    port: {port}\n    ip: {ip}\n    private-key: \"{pk}\"\n    public-key: \"{pubk}\"\n{psk_line}    allowed-ips: ['0.0.0.0/0']\n    mtu: {mtu}\n    udp: true\n    remote-dns-resolve: true\n{dns_line}",
        port = cfg.listen_port,
        ip = wg.address,
        pk = wg.private_key.trim(),
        pubk = wg.public_key.trim(),
        mtu = wg.mtu,
    ))
}

/// Правила DIRECT в начало `rules:`, чтобы трафик самого клиента никогда не ушёл в туннель или петлю.
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

// ---------- Разбор wg-конфига от сервера ----------

pub struct ParsedWg {
    pub wg: VkTurnWireGuard,
    pub endpoint_host: Option<String>,
}

pub fn parse_wg_conf(text: &str) -> Result<ParsedWg, String> {
    let mut wg = VkTurnWireGuard::default();
    let mut endpoint_host = None;
    let mut section = "";
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.eq_ignore_ascii_case("[Interface]") {
            section = "i";
            continue;
        }
        if line.eq_ignore_ascii_case("[Peer]") {
            section = "p";
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim().to_ascii_lowercase(), v.trim());
        match (section, k.as_str()) {
            ("i", "privatekey") => wg.private_key = v.to_string(),
            ("i", "address") => {
                wg.address = v
                    .split(',')
                    .map(str::trim)
                    .find(|a| a.contains('.'))
                    .unwrap_or("")
                    .split('/')
                    .next()
                    .unwrap_or("")
                    .to_string()
            }
            ("i", "dns") => wg.dns = v.split(',').map(|d| d.trim().to_string()).filter(|d| is_ipv4(d)).collect(),
            ("i", "mtu") => wg.mtu = v.parse().unwrap_or(1280).min(1280),
            ("p", "publickey") => wg.public_key = v.to_string(),
            ("p", "presharedkey") => wg.preshared_key = Some(v.to_string()),
            ("p", "endpoint") => endpoint_host = v.rsplit_once(':').map(|(h, _)| h.trim_matches(['[', ']']).to_string()),
            _ => {}
        }
    }
    if wg.private_key.is_empty() || wg.public_key.is_empty() || wg.address.is_empty() {
        return Err("В конфиге не найдены PrivateKey, PublicKey или Address".into());
    }
    Ok(ParsedWg { wg, endpoint_host })
}

// ---------- Запись в config.json / config.yaml (одна транзакция) ----------

/// Применяет изменение настроек VK-туннеля: config.json, файл провайдера, провайдер и правила в config.yaml,
/// перезагрузка Mihomo. Всё в одной транзакции ConfigTx (блокировки берёт только она).
/// Провайдер добавляется в YAML, только когда wg-конфиг уже получен — file-провайдер на несуществующий
/// файл Mihomo не загрузит.
pub async fn commit_vkturn<F>(state: &crate::AppState, mutate: F) -> Result<VkTurnConfig, String>
where
    F: FnOnce(&mut VkTurnConfig) -> Result<(), String>,
{
    let mut tx = crate::transaction::ConfigTx::begin(state)
        .await
        .map_err(|e| format!("Ошибка начала транзакции: {e}"))?;

    let mut new_cfg = tx.config().vkturn.clone();
    mutate(&mut new_cfg)?;
    if new_cfg.device_id.is_empty() {
        new_cfg.device_id = generate_device_id();
    }
    tx.config_mut().vkturn = new_cfg.clone();
    if !tx.config().whitelist_providers.iter().any(|p| p == PROVIDER_ID) {
        tx.config_mut().whitelist_providers.push(PROVIDER_ID.to_string());
    }

    let providers_dir = std::path::Path::new(&tx.config().mihomo.config_path)
        .parent()
        .map(|p| p.join("providers"))
        .unwrap_or_else(|| std::path::PathBuf::from("/opt/etc/mihomo/providers"));
    let prov_file = providers_dir.join(format!("{PROVIDER_ID}.yaml"));
    let prov_yaml = render_provider_yaml(&new_cfg).ok();
    if let Some(ref y) = prov_yaml {
        tx.set_extra_file(&prov_file, y.clone(), true)
            .await
            .map_err(|e| format!("Ошибка записи файла провайдера VK-туннеля: {e}"))?;
    }

    let yaml = tx.read_yaml().await.map_err(|e| format!("Ошибка чтения config.yaml: {e}"))?;
    if let Some(conflict) = listen_port_conflict(new_cfg.listen_port, &yaml, tx.config().mihomo.port) {
        return Err(conflict);
    }

    let has_prov = crate::routing::parse_provider_names(&yaml).iter().any(|p| p == PROVIDER_ID);
    let yaml_with_prov = if prov_yaml.is_some() && !has_prov {
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

/// WireGuard-конфиг, который выдал сервер: если он новый — сохраняем и подключаем к Mihomo.
/// Возвращает true, если конфиг изменился.
pub async fn apply_server_wg(state: &crate::AppState, conf_text: &str) -> Result<bool, String> {
    let parsed = parse_wg_conf(conf_text)?;
    let mut candidate = state.config.read().await.vkturn.clone();
    if candidate.wg == parsed.wg {
        return Ok(false);
    }
    candidate.wg = parsed.wg.clone();
    render_provider_yaml(&candidate).map_err(|e| format!("Сервер выдал непригодный WireGuard-конфиг: {e}"))?;
    let wg = parsed.wg;
    commit_vkturn(state, move |v| {
        v.wg = wg;
        Ok(())
    })
    .await?;
    Ok(true)
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
    /// «осиротевший» надзор и второй клиент на том же порту.
    op_lock: Mutex<Option<Supervisor>>,
    /// Путь к бинарнику клиента (в тестах подменяется).
    core_path: std::path::PathBuf,
    /// Число живых задач надзора — для диагностики и анти-регрессионного теста гонки перезапусков.
    active_supervisors: Arc<std::sync::atomic::AtomicUsize>,
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
        })
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

    /// Останавливает надзор и ЖДЁТ его завершения (процесс клиента убит до выхода из функции).
    async fn stop_locked(&self, slot: &mut Option<Supervisor>) {
        if let Some(sup) = slot.take() {
            let _ = sup.stop_tx.send(true);
            let mut task = sup.task;
            if tokio::time::timeout(Duration::from_secs(5), &mut task).await.is_err() {
                task.abort();
                crate::log_w!("[VK-туннель] Надзор не завершился за 5 с — прерван принудительно");
            }
        }
        let _ = tokio::fs::remove_file(PID_FILE).await;
        let mut rt = self.rt.write().await;
        rt.pid = None;
        rt.connected_streams = 0;
        rt.phase = VkTurnPhase::Disabled;
    }

    async fn supervise(self: Arc<Self>, cfg: VkTurnConfig, mut stop_rx: watch::Receiver<bool>, state: crate::AppState) {
        let mut backoff = Duration::from_secs(5);
        let mut last_notice: Option<VkTurnPhase> = None;
        let _ = tokio::fs::create_dir_all(WORK_DIR).await;
        loop {
            if *stop_rx.borrow() {
                break;
            }
            // Перед каждым запуском: XKeen/Zapret могли пересоздать свои цепочки и стереть исключения.
            let known_turn = self.rt.read().await.turn_ips.clone();
            apply_exemptions(&cfg.peer, &known_turn).await;

            let mut cmd = tokio::process::Command::new(&self.core_path);
            cmd.args(build_client_args(&cfg))
                .env("WDTT_EVENTS", "1")
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);
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
                rt.started_at = Some(Instant::now());
                rt.zero_streams_since = Some(Instant::now());
            }
            crate::log_i!(
                "[VK-туннель] Клиент WDTT запущен (pid {:?}, ссылок {}, воркеров {})",
                pid,
                call_hashes(&cfg).len(),
                effective_workers(&cfg)
            );

            // stdout (события, CAPTCHA_SOLVE) и stderr (лог Go) → разбор; действия — в канал надзора.
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

            // ждём: выход процесса / команду стоп / «висим без соединений» дольше допустимого
            let started = Instant::now();
            let mut tick = tokio::time::interval(Duration::from_secs(10));
            let mut ticks: u32 = 0;
            let mut exempted_turn = known_turn;
            let exit_reason = loop {
                tokio::select! {
                    st = child.wait() => break format!("клиент завершился: {:?}", st.ok()),
                    _ = stop_rx.changed() => {
                        // Мягкая остановка: клиент закрывает DTLS-сессии, сервер сразу освобождает слоты.
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
                        LineEffect::WgConfig(conf) => {
                            // Отдельной задачей: надзор не должен ждать блокировки конфига
                            // (иначе restart() из обработчика сохранения ждал бы его до таймаута).
                            let st = state.clone();
                            let me = self.clone();
                            tokio::spawn(async move {
                                match apply_server_wg(&st, &conf).await {
                                    Ok(true) => crate::log_i!("[VK-туннель] Получен WireGuard-конфиг от сервера — узел подключён к Mihomo"),
                                    Ok(false) => {}
                                    Err(e) => {
                                        crate::log_w!("[VK-туннель] WireGuard-конфиг от сервера не применён: {e}");
                                        me.rt.write().await.last_error = Some(e);
                                    }
                                }
                            });
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
        if String::from_utf8_lossy(&cmd).contains("wdtt-client") {
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
    let asset = core_asset_for_arch(arch).ok_or(format!("Архитектура {arch} не поддерживается VK-туннелем"))?;
    let expected = CORE_SHA256
        .iter()
        .find(|(a, _)| *a == asset)
        .map(|(_, h)| *h)
        .ok_or("Для этой архитектуры нет проверенного хеша ядра")?;
    let url = format!("{CORE_RELEASES}/{CORE_TAG}/{asset}");
    let tmp = format!("{CORE_PATH}.tmp");

    let cfg = state.config.read().await.clone();
    let proxy_url = cfg.mihomo_proxy_url();
    let bytes = crate::updater::download_file_verified_with_client(
        &state.http,
        &url,
        Some(expected),
        &proxy_url,
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

        // Пробный запуск `tmp -h` с таймаутом 5 с (Go-флаги: -h печатает справку и выходит).
        let test_run = tokio::time::timeout(
            Duration::from_secs(5),
            tokio::process::Command::new(&tmp).arg("-h").stdin(Stdio::null()).output(),
        )
        .await;

        match test_run {
            Ok(Ok(out)) => {
                if !out.status.success() && out.status.code().is_none() {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    return Err("Ядро не запускается на этом процессоре (завершено сигналом, возможно нужна softfloat-сборка)".into());
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

    Ok(format!("Ядро VK-туннеля WDTT ({asset}) успешно установлено"))
}

// ---------- Тесты ----------

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(links: &[&str]) -> VkTurnConfig {
        let mut cfg = VkTurnConfig::default();
        cfg.enabled = true;
        cfg.peer = "1.2.3.4:56000".into();
        cfg.password = "s3cret-pass".into();
        cfg.device_id = "xkeen-route-0011223344556677".into();
        cfg.links = links.iter().map(|s| s.to_string()).collect();
        cfg
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

        // Телемост ядро WDTT не поддерживает — понятная ошибка
        let e = parse_call_link("https://telemost.yandex.ru/j/12345678").unwrap_err();
        assert!(e.contains("Телемост"));

        // Отказ для http, чужих доменов, инъекций, пробелов
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
        assert!(validate_peer("1.2.3.4:56000").is_ok());
        assert!(validate_peer("vps.example.com:56000").is_ok());
        assert!(validate_peer(":56000").is_err());
        assert!(validate_peer("1.2.3.4:0").is_err());
        assert!(validate_peer("1.2.3.4:70000").is_err());
        assert!(validate_peer("a b:1").is_err());
        assert!(validate_peer("1.2.3.4").is_err());

        assert_eq!(normalize_peer("203.0.113.7").unwrap(), "203.0.113.7:56000", "порт по умолчанию — 56000");
        assert_eq!(normalize_peer(" vps.example.com:443 ").unwrap(), "vps.example.com:443");
        assert_eq!(normalize_peer("").unwrap(), "");
        assert!(normalize_peer("1.2.3.4;reboot").is_err());
    }

    #[test]
    fn test_validate_password() {
        assert!(validate_password("abc").is_ok());
        assert!(validate_password("").is_err());
        assert!(validate_password("a|b").is_err(), "| — разделитель в GETCONF");
        assert!(validate_password("a\nb").is_err());
        assert!(validate_password(&"x".repeat(129)).is_err());
    }

    #[test]
    fn test_device_id() {
        let id = generate_device_id();
        assert!(id.starts_with("xkeen-route-"));
        assert!(is_valid_device_id(&id));
        assert_ne!(id, generate_device_id(), "каждый роутер получает свой id");
        assert!(!is_valid_device_id("bad id"));
        assert!(!is_valid_device_id("a|b|c|d"));
    }

    #[test]
    fn test_effective_workers() {
        let mut cfg = cfg_with(&["https://vk.com/call/join/abc12345"]);
        assert_eq!(effective_workers(&cfg), 18);
        cfg.streams = 30;
        assert_eq!(effective_workers(&cfg), 27, "кратно 9");
        cfg.streams = 200;
        assert_eq!(effective_workers(&cfg), 108);
        cfg.streams = 1;
        assert_eq!(effective_workers(&cfg), 9);
        // Три ссылки при 9 воркерах — клиент взял бы только первую: поднимаем до 27
        cfg.links = vec![
            "https://vk.com/call/join/aaaa1111".into(),
            "https://vk.com/call/join/bbbb2222".into(),
            "https://vk.com/call/join/cccc3333".into(),
        ];
        assert_eq!(effective_workers(&cfg), 27);
    }

    #[test]
    fn test_build_client_args() {
        let cfg = cfg_with(&["https://vk.com/call/join/abc12345", "https://vk.com/call/join/def67890"]);
        let args = build_client_args(&cfg);
        assert_eq!(
            args,
            vec![
                "-peer", "1.2.3.4:56000",
                "-vk", "abc12345,def67890",
                "-n", "18",
                "-listen", "127.0.0.1:9000",
                "-device-id", "xkeen-route-0011223344556677",
                "-password", "s3cret-pass",
                "-captcha-mode", "auto",
            ]
        );
        // Дубли ссылок не дублируют хеши
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
        // Повтор существующей ссылки — без дублей, она поднимается наверх
        let out2 = with_primary_link(&out, "backup5678").unwrap();
        assert_eq!(out2.len(), 3);
        assert_eq!(out2[0], "https://vk.com/call/join/backup5678");
        // Не больше MAX_LINKS
        let many: Vec<String> = (0..10).map(|i| format!("https://vk.com/call/join/link{i:06}")).collect();
        assert_eq!(with_primary_link(&many, "https://vk.com/call/join/newlink000").unwrap().len(), MAX_LINKS);
        assert!(with_primary_link(&links, "https://telemost.yandex.ru/j/12345678").is_err());
    }

    // Строки ниже взяты из исходников клиента WDTT (main.go, session.go, protocol.go, vk_auth.go, worker_group.go).
    const EV_READY: &str = "__WDTT_EVENT__|READY|";
    const EV_STATS_3: &str = r#"__WDTT_EVENT__|STATS|{"active":3,"bytes_down":2048,"bytes_up":1024}"#;
    const EV_STATS_0: &str = r#"__WDTT_EVENT__|STATS|{"active":0,"bytes_down":2048,"bytes_up":1024}"#;
    const L_CREDS: &str = "2026/11/06 10:00:00.000001 [ГРУППА #1] Запрос кредов (хеш: abcd1234...)";
    const L_TRYING: &str = "2026/11/06 10:00:01.000001 [STREAM 100] [VK Auth] Trying credentials: client_id=6287487 (attempt 1/2)";
    const L_CAPTCHA_START: &str = "2026/11/06 10:00:02.000001 [STREAM 100] [КАПЧА] AUTO: старт цепочки (captcha attempt 1)";
    const L_CAPTCHA_FAIL: &str = "2026/11/06 10:00:30.000001 [ГРУППА #1] Ошибка кредов: all VK credentials failed: CAPTCHA_WAIT_REQUIRED";
    const L_AUTH_WRONG: &str = "2026/11/06 10:00:05.000001 [ВОРКЕР #1] Фатальная ошибка: FATAL_AUTH: неверный пароль подключения";
    const L_AUTH_DEVICE: &str = "2026/11/06 10:00:05.000001 [ВОРКЕР #1] Фатальная ошибка: FATAL_AUTH: пароль привязан к другому устройству";
    const L_WRAP_TIMEOUT: &str = "2026/11/06 10:00:25.000001 [ВОРКЕР #2] Ошибка (попытка 1): WRAP_AUTH_TIMEOUT: DTLS timeout, пароль/WRAP не подтверждён";
    const L_CALL_DEAD: &str = "2026/11/06 10:00:03.000001 [ГРУППА #1] Ошибка кредов: VK returns error: Call not found (error_code=9000)";
    const L_TURN_URL: &str = "2026/11/06 10:00:04.000001 [VKCalls] turn_server.urls[0]=turn:203.0.113.50:3478?transport=udp";

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
        // Все соединения упали — «Переподключаюсь», таймер простоя запущен
        apply_log_line(&mut rt, EV_STATS_0, now);
        assert_eq!(rt.connected_streams, 0);
        assert_eq!(rt.phase, VkTurnPhase::Degraded);
        assert!(rt.zero_streams_since.is_some());
        // Машинные события не засоряют лог для пользователя
        assert!(rt.log_tail.iter().all(|l| !l.contains("__WDTT_EVENT__")));
    }

    #[test]
    fn config_event_is_returned_and_never_logged() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        let conf = "[Interface]\nPrivateKey = SECRETPRIVATEKEYaaaaaaaaaaaaaaaaaaaaaaaaaaaa=\nAddress = 10.66.66.2/32\n";
        let ev = format!("__WDTT_EVENT__|CONFIG|{}", serde_json::json!({ "config": conf }));
        assert_eq!(apply_log_line(&mut rt, &ev, now), LineEffect::WgConfig(conf.to_string()));
        // Рамка с конфигом в stdout (main.go) тоже не должна попасть в лог панели
        apply_log_line(&mut rt, "╔══════════════ WireGuard Конфиг ══════════════╗", now);
        apply_log_line(&mut rt, "║ PrivateKey = SECRETPRIVATEKEYaaaaaaaaaaaaaaaaaaaaaaaaaaaa= ║", now);
        apply_log_line(&mut rt, "╚══════════════════════════════════════════════╝", now);
        let all = rt.log_tail.iter().cloned().collect::<Vec<_>>().join("\n");
        assert!(!all.contains("SECRETPRIVATEKEY"), "приватный ключ попал в лог: {all}");
        assert!(all.contains("ключи скрыты"));
        // И на всякий случай redact_line глушит ключи в любой строке
        assert!(!redact_line("PrivateKey = abc123").contains("abc123"));
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
        apply_log_line(&mut rt, L_TRYING, now);
        apply_log_line(&mut rt, L_CAPTCHA_START, now);
        assert_eq!(rt.phase, VkTurnPhase::SolvingCaptcha);
        apply_log_line(&mut rt, L_CAPTCHA_FAIL, now);
        assert_eq!(rt.phase, VkTurnPhase::CaptchaRequired);
        assert!(rt.last_error.as_deref().unwrap().contains("не робот"));
        // Следующий перебор не прячет капчу за «Подключаюсь…»
        apply_log_line(&mut rt, L_CREDS, now);
        apply_log_line(&mut rt, L_CAPTCHA_START, now);
        assert_eq!(rt.phase, VkTurnPhase::CaptchaRequired);
        // Как только поднялось соединение — капча пройдена
        apply_log_line(&mut rt, EV_READY, now);
        assert_eq!(rt.phase, VkTurnPhase::Connected);
        assert!(rt.last_error.is_none());
    }

    #[test]
    fn server_auth_errors_are_recognized() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        apply_log_line(&mut rt, L_AUTH_WRONG, now);
        assert_eq!(rt.phase, VkTurnPhase::AuthFailed);
        assert!(rt.last_error.as_deref().unwrap().contains("неверный"));
        // Таймаут DTLS после отказа в пароле не меняет понятную причину
        apply_log_line(&mut rt, L_WRAP_TIMEOUT, now);
        assert_eq!(rt.phase, VkTurnPhase::AuthFailed);

        let mut rt2 = VkTurnRuntime::default();
        apply_log_line(&mut rt2, L_AUTH_DEVICE, now);
        assert!(rt2.last_error.as_deref().unwrap().contains("другому устройству"));

        let mut rt3 = VkTurnRuntime::default();
        apply_log_line(&mut rt3, L_WRAP_TIMEOUT, now);
        assert_eq!(rt3.phase, VkTurnPhase::ServerUnreachable);
    }

    #[test]
    fn dead_call_link_is_recognized_but_does_not_hide_working_streams() {
        let mut rt = VkTurnRuntime::default();
        let now = Instant::now();
        apply_log_line(&mut rt, L_CALL_DEAD, now);
        assert_eq!(rt.phase, VkTurnPhase::LinkInvalid);

        // Одна из нескольких ссылок умерла, остальные работают — статус «Работает», но с предупреждением
        let mut rt2 = VkTurnRuntime::default();
        apply_log_line(&mut rt2, EV_STATS_3, now);
        apply_log_line(&mut rt2, L_CALL_DEAD, now);
        assert_eq!(rt2.phase, VkTurnPhase::Connected);
        assert!(rt2.last_error.is_some());
    }

    #[test]
    fn turn_addresses_are_learned_for_exemptions() {
        let mut rt = VkTurnRuntime::default();
        apply_log_line(&mut rt, L_TURN_URL, Instant::now());
        apply_log_line(&mut rt, "[VKCalls] turn_server.urls[1]=turn:127.0.0.1:3478", Instant::now());
        apply_log_line(&mut rt, "[VKCalls] turn_server.urls[2]=turn:10.0.0.1:3478", Instant::now());
        assert_eq!(rt.turn_ips.iter().cloned().collect::<Vec<_>>(), vec!["203.0.113.50".parse::<Ipv4Addr>().unwrap()]);
        for i in 0..100u8 {
            apply_log_line(&mut rt, &format!("turn:198.51.{i}.1:3478"), Instant::now());
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
    }

    #[test]
    fn stall_limits() {
        assert_eq!(stall_limit(VkTurnPhase::AuthFailed), Duration::from_secs(15));
        assert_eq!(stall_limit(VkTurnPhase::LinkInvalid), Duration::from_secs(20));
        assert_eq!(stall_limit(VkTurnPhase::CaptchaRequired), Duration::from_secs(45));
        assert_eq!(stall_limit(VkTurnPhase::SolvingCaptcha), Duration::from_secs(300), "решение капчи не бесконечно");
        assert_eq!(stall_limit(VkTurnPhase::Authorizing), Duration::from_secs(120));
    }

    #[test]
    fn test_render_provider_yaml() {
        let mut cfg = VkTurnConfig::default();
        cfg.listen_port = 9000;
        cfg.wg.private_key = "a".repeat(42) + "A=";
        cfg.wg.public_key = "b".repeat(42) + "A=";
        cfg.wg.address = "10.66.66.2".into();
        cfg.wg.mtu = 1280;

        let yaml = render_provider_yaml(&cfg).expect("render valid yaml");
        assert!(yaml.contains(PROXY_NAME));
        assert!(yaml.contains("[БС]"));
        assert!(yaml.contains("type: wireguard"));
        assert!(yaml.contains("port: 9000"));
        assert!(yaml.contains("ip: 10.66.66.2"));
        assert!(wg_ready(&cfg));
        assert!(!wg_ready(&VkTurnConfig::default()), "без конфига от сервера узла в Mihomo нет");

        let mut bad_key = cfg.clone();
        bad_key.wg.private_key = "bad\nkey: 123".into();
        assert!(render_provider_yaml(&bad_key).is_err());
        let mut bad_ip = cfg.clone();
        bad_ip.wg.address = "2001:db8::1".into();
        assert!(render_provider_yaml(&bad_ip).is_err());
        let mut bad_mtu = cfg.clone();
        bad_mtu.wg.mtu = 1500;
        assert!(render_provider_yaml(&bad_mtu).is_err());
    }

    /// Формат конфига, который выдаёт сервер WDTT (linux-server/net.go, buildClientConfig).
    #[test]
    fn test_parse_server_wg_conf() {
        let sample = "[Interface]\nMTU = 1280\nPrivateKey = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa=\nAddress = 10.66.66.5/32\nDNS = 1.1.1.1, 8.8.8.8\n\n[Peer]\nPublicKey = bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb=\nAllowedIPs = 0.0.0.0/0\nEndpoint = 127.0.0.1:9000\nPersistentKeepalive = 25\n";
        let parsed = parse_wg_conf(sample).expect("parse server conf");
        assert_eq!(parsed.wg.address, "10.66.66.5");
        assert_eq!(parsed.wg.mtu, 1280);
        assert_eq!(parsed.wg.dns, vec!["1.1.1.1", "8.8.8.8"]);
        assert_eq!(parsed.endpoint_host.as_deref(), Some("127.0.0.1"));
        assert!(parse_wg_conf("[Interface]\nAddress = 10.0.0.2\n").is_err());
    }

    #[test]
    fn test_apply_and_remove_rules() {
        let yaml = "port: 7890\nrules:\n  - MATCH,DIRECT\n";
        let mut cfg = cfg_with(&["https://vk.com/call/join/abc12345"]);

        let applied = apply_rules(yaml, &cfg).expect("apply rules");
        assert!(applied.contains(RULES_BEGIN));
        assert!(applied.contains("IP-CIDR,155.212.192.0/20,DIRECT,no-resolve"));
        assert!(applied.contains("IP-CIDR,1.2.3.4/32,DIRECT,no-resolve"));
        assert!(!applied.contains("5.255.211.241"), "TURN Телемоста больше не нужен");
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
        assert_eq!(core_asset_for_arch("arm64"), Some("wdtt-client-linux-arm64"));
        assert_eq!(core_asset_for_arch("aarch64"), Some("wdtt-client-linux-arm64"));
        assert_eq!(core_asset_for_arch("mipsle-softfloat"), Some("wdtt-client-linux-mipsle"));
        assert_eq!(core_asset_for_arch("mipsle"), Some("wdtt-client-linux-mipsle"));
        assert_eq!(core_asset_for_arch("mips-softfloat"), Some("wdtt-client-linux-mips"));
        assert_eq!(core_asset_for_arch("armv7"), Some("wdtt-client-linux-armv7"));
        assert_eq!(core_asset_for_arch("amd64"), Some("wdtt-client-linux-amd64"));
        assert_eq!(core_asset_for_arch("unknown_arch"), None);
        assert_eq!(elf_target_for_arch("mipsle-softfloat"), "mipsel");
        assert_eq!(elf_target_for_arch("mips-softfloat"), "mips");
        assert_eq!(elf_target_for_arch("arm64"), "aarch64");
        assert_eq!(elf_target_for_arch("armv7"), "arm");
    }

    /// Каждой архитектуре из сборки соответствует проверенный SHA-256 (64 hex), без заглушек.
    #[test]
    fn every_core_asset_has_a_real_hash() {
        for arch in ["arm64", "mipsle-softfloat", "mips-softfloat", "armv7", "amd64"] {
            let asset = core_asset_for_arch(arch).unwrap();
            let hash = CORE_SHA256.iter().find(|(a, _)| *a == asset).map(|(_, h)| *h);
            let hash = hash.unwrap_or_else(|| panic!("нет хеша для {asset}"));
            assert_eq!(hash.len(), 64);
            assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        }
        assert!(CORE_TAG.ends_with(&CORE_UPSTREAM_COMMIT[..7]), "тег релиза ядра соответствует коммиту сборки");
    }

    /// Пароль сервера WDTT — секрет: в API настроек он скрыт, а «******» при сохранении не затирает настоящий.
    #[test]
    fn settings_api_masks_and_preserves_wdtt_password() {
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
    const SERVER_CONF: &str = "[Interface]\nMTU = 1280\nPrivateKey = aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaA=\nAddress = 10.66.66.5/32\nDNS = 1.1.1.1\n\n[Peer]\nPublicKey = bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbA=\nAllowedIPs = 0.0.0.0/0\nEndpoint = 127.0.0.1:9000\n";

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
    /// WDTT: до получения wg-конфига от сервера провайдер в YAML не добавляется (Mihomo не загрузил бы
    /// file-провайдер на несуществующий файл), а device-id генерируется один раз.
    #[tokio::test]
    async fn regression_save_config_without_wg_yet_does_not_deadlock() {
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
        assert_eq!(saved.peer, "1.2.3.4:56000", "порт сервера по умолчанию подставлен");
        assert_eq!(saved.links, vec!["https://vk.com/call/join/abcdEFGH1234"]);
        assert!(is_valid_device_id(&saved.device_id));
        let first_id = saved.device_id.clone();

        let yaml = std::fs::read_to_string(root.join("config.yaml")).unwrap();
        assert!(yaml.contains(RULES_BEGIN), "правила DIRECT для TURN/VPS ставятся сразу");
        assert!(!yaml.contains("  vkturn:"), "провайдер без wg-конфига ломал бы загрузку Mihomo: {yaml}");

        // Повторное сохранение не меняет device-id (иначе сервер ответил бы device_mismatch)
        let v = response_json(
            crate::api::save_vkturn_config(
                axum::extract::State(state.clone()),
                axum::Json(crate::api::SaveVkTurnConfigReq { streams: Some(27), ..Default::default() }),
            )
            .await,
        )
        .await;
        assert_eq!(v["success"], true, "ответ: {v}");
        assert_eq!(state.config.read().await.vkturn.device_id, first_id);
        assert_eq!(state.config.read().await.vkturn.password, "s3cret-pass", "пароль сохраняется");

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

    /// Конфиг от сервера WDTT сохраняется, подключает узел к Mihomo, повтор того же конфига — без перезаписи.
    #[tokio::test]
    async fn server_wg_config_is_applied_once() {
        let (root, state) = setup_saved_state("wg").await;
        let v = response_json(
            crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(enable_req())).await,
        )
        .await;
        assert_eq!(v["success"], true, "ответ: {v}");

        assert!(apply_server_wg(&state, SERVER_CONF).await.unwrap(), "новый конфиг применён");
        let saved = state.config.read().await.vkturn.clone();
        assert_eq!(saved.wg.address, "10.66.66.5");
        assert!(saved.enabled && saved.password == "s3cret-pass", "остальные настройки не затёрты");
        let yaml = std::fs::read_to_string(root.join("config.yaml")).unwrap();
        assert!(yaml.contains("  vkturn:"), "провайдер добавлен после получения конфига: {yaml}");
        let prov = std::fs::read_to_string(root.join("providers").join("vkturn.yaml")).unwrap();
        assert!(prov.contains(PROXY_NAME) && prov.contains("ip: 10.66.66.5"));
        assert!(state.config.read().await.whitelist_providers.iter().any(|p| p == PROVIDER_ID));

        assert!(!apply_server_wg(&state, SERVER_CONF).await.unwrap(), "тот же конфиг повторно не пишется");
        assert!(apply_server_wg(&state, "мусор").await.is_err());
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
            (crate::api::SaveVkTurnConfigReq { password: Some("a|b".into()), ..enable_req() }, "с | в пароле"),
        ] {
            let v = response_json(crate::api::save_vkturn_config(axum::extract::State(state.clone()), axum::Json(req)).await).await;
            assert_eq!(v["success"], false, "включение {what} должно отклоняться: {v}");
        }
        assert!(!state.config.read().await.vkturn.enabled);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Ревью: два одновременных restart() перезаписывали канал остановки — первый надзор «осиротевал»,
    /// и на одном порту оказывались два клиента.
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

        // Без пароля клиент не запускается вовсе (WDTT упал бы с log.Fatal)
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
        assert_eq!(resolve_peer_ipv4("1.2.3.4:56000").await, Some("1.2.3.4".parse().unwrap()));
        assert_eq!(resolve_peer_ipv4("no-port").await, None);
    }

    #[test]
    fn listen_port_validation_and_conflicts() {
        assert!(validate_listen_port(9000).is_ok());
        assert!(validate_listen_port(0).is_err());
        assert!(validate_listen_port(80).is_err());

        let yaml = "mixed-port: 7890\nredir-port: 7892\ntproxy-port: 7893\nexternal-controller: 0.0.0.0:9090\nproxies: []\n";
        assert!(listen_port_conflict(9000, yaml, 9090).is_none());
        assert!(listen_port_conflict(7890, yaml, 9090).unwrap().contains("mixed-port"));
        assert!(listen_port_conflict(7893, yaml, 9090).is_some());
        assert!(listen_port_conflict(9090, yaml, 9090).unwrap().contains("API Mihomo"));
        assert!(listen_port_conflict(443, "proxies:\n  - name: a\n    port: 443\n", 9090).is_none());
    }
}
