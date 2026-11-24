use std::collections::BTreeSet;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use crate::config::VkTurnConfig;
use super::*;

/// Аргументы передаются МАССИВОМ, без shell — никакой подстановки команд. Пароля здесь нет:
/// он уходит в `CSQTT_PASSWORD` (см. client_env).
pub fn build_client_args_with_hashes(cfg: &VkTurnConfig, hashes: &[String]) -> Vec<String> {
    build_client_args_for_uds(cfg, hashes, &new_tun_uds_name())
}

pub(super) fn new_tun_uds_name() -> String {
    format!("{TUN_UDS_NAME}_{}", crate::auth::random_hex(8))
}

pub(super) fn build_client_args_for_uds(cfg: &VkTurnConfig, hashes: &[String], uds_name: &str) -> Vec<String> {
    let initial = if cfg.turn_transport.eq_ignore_ascii_case("tcp") { "tcp" } else { "udp" };
    build_client_args_for_uds_with_transport(cfg, hashes, uds_name, initial)
}

pub(super) fn build_client_args_for_uds_with_transport(
    cfg: &VkTurnConfig,
    hashes: &[String],
    uds_name: &str,
    transport: &str,
) -> Vec<String> {
    let tr = if transport.eq_ignore_ascii_case("tcp") { "tcp" } else { "udp" };
    vec![
        "-peer".into(),
        cfg.peer.trim().to_string(),
        "-vk".into(),
        hashes.join(","),
        "-n".into(),
        effective_workers_for_transport(cfg, tr).to_string(),
        "--exact-workers".into(),
        "-listen".into(),
        "127.0.0.1:0".into(),
        "-tun-uds".into(),
        uds_name.into(),
        "-captcha-mode".into(),
        "auto".into(),
        "-obfs".into(),
        "audio".into(),
        "-turn-transport".into(),
        tr.into(),
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
    /// Фактически используемый транспорт до TURN-серверов ("udp" | "tcp").
    pub active_transport: &'static str,
    /// Число потоков, успешно завершивших DTLS-SRTP рукопожатие в текущем запуске.
    pub dtls_ok_streams: u32,
    /// Число потоков, упавших в WRAP-SRTP (входящих 0) / ошибку DTLS в текущем запуске.
    pub dtls_fallback_streams: u32,
    pub wan_streams: std::collections::BTreeMap<String, i32>,
    #[serde(skip)]
    pub(super) dtls_ok_workers: BTreeSet<u32>,
    #[serde(skip)]
    pub(super) dtls_fallback_workers: BTreeSet<u32>,
    #[serde(skip)]
    pub(super) run_generation: u64,
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
            active_transport: "udp",
            dtls_ok_streams: 0,
            dtls_fallback_streams: 0,
            wan_streams: std::collections::BTreeMap::new(),
            dtls_ok_workers: BTreeSet::new(),
            dtls_fallback_workers: BTreeSet::new(),
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
pub(super) fn is_sticky(phase: VkTurnPhase) -> bool {
    matches!(
        phase,
        VkTurnPhase::CaptchaRequired | VkTurnPhase::LinkInvalid | VkTurnPhase::AuthFailed | VkTurnPhase::ServerUnreachable
    )
}

pub(super) fn push_log(rt: &mut VkTurnRuntime, line: String) {
    if rt.log_tail.len() >= LOG_TAIL {
        rt.log_tail.pop_front();
    }
    rt.log_tail.push_back(line);
}

/// Сервер промолчал на GETCONF: тайм-аут не доказывает отказ в авторизации.
pub(super) fn note_getconf_silent(rt: &mut VkTurnRuntime) {
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
    if stage == "dtls" {
        let worker = json.get("worker").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        match status {
            "ok" => {
                rt.dtls_fallback_workers.remove(&worker);
                rt.dtls_ok_workers.insert(worker);
                rt.dtls_ok_streams = rt.dtls_ok_workers.len() as u32;
                rt.dtls_fallback_streams = rt.dtls_fallback_workers.len() as u32;
            }
            "error" => {
                if !rt.dtls_ok_workers.contains(&worker) {
                    rt.dtls_fallback_workers.insert(worker);
                    rt.dtls_fallback_streams = rt.dtls_fallback_workers.len() as u32;
                }
            }
            _ => {}
        }
    }
    if status == "error" {
        let worker = json.get("worker").and_then(|v| v.as_i64()).unwrap_or(0);
        push_log(rt, redact_line(&format!("[ПОТОК #{worker}] {stage}: {reason}")));
    }
}

/// Чистая функция: решает, пора ли в режиме `auto` переключить транспорт с UDP на TCP
/// из-за массового провала DTLS-рукопожатий (резка/потери крупных UDP-пакетов на LTE/ТСПУ)
/// или при срабатывании детектора blackhole.
pub fn should_fallback_to_tcp(
    turn_transport: &str,
    current_transport: &str,
    elapsed: Duration,
    dtls_ok: u32,
    dtls_fallback: u32,
    blackhole_hit: bool,
) -> bool {
    if !turn_transport.trim().eq_ignore_ascii_case("auto") || current_transport != "udp" {
        return false;
    }
    if blackhole_hit {
        return true;
    }
    if elapsed < Duration::from_secs(25) {
        return false;
    }
    (dtls_fallback >= 2 && dtls_ok == 0) || (dtls_fallback >= 3 && dtls_ok * 2 < dtls_fallback)
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

    if line.contains("WRAP-SRTP") && line.contains("входящих 0") {
        let synthetic_id = 10_000u32.saturating_add(rt.dtls_fallback_workers.len() as u32);
        rt.dtls_fallback_workers.insert(synthetic_id);
        rt.dtls_fallback_streams = rt.dtls_fallback_workers.len() as u32;
    }

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
            if let Some(ws) = json.get("wan_streams").and_then(|v| v.as_object()) {
                rt.wan_streams = ws
                    .iter()
                    .filter_map(|(k, v)| v.as_i64().map(|n| (k.clone(), n as i32)))
                    .collect();
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
    format!("proxies:\n  - name: \"{PROXY_NAME}\"\n    type: direct\n    interface-name: {TUN_IFACE}\n    routing-mark: {ROUTE_TABLE}\n    ip-version: ipv4\n    udp: false\n    tfo: false\n")
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
done
for ch in OUTPUT FORWARD; do
  iptables -w -t filter -C "$ch" -o {TUN_IFACE} -p udp --dport 443 -m comment --comment {IPT_COMMENT} -j REJECT --reject-with icmp-port-unreachable 2>/dev/null \
    || iptables -w -t filter -I "$ch" 1 -o {TUN_IFACE} -p udp --dport 443 -m comment --comment {IPT_COMMENT} -j REJECT --reject-with icmp-port-unreachable 2>/dev/null || true
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

pub(super) async fn run_sh_checked(script: &str) -> Result<(), String> {
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
    tun_configure_script_for_transport(ip, "udp")
}

pub fn tun_configure_script_for_transport(ip: Ipv4Addr, transport: &str) -> String {
    let (mtu, mss, other_mss) = if transport.eq_ignore_ascii_case("tcp") {
        (TUN_MTU_TCP, TUN_TCP_MSS_TCP, TUN_TCP_MSS)
    } else {
        (TUN_MTU, TUN_TCP_MSS, TUN_TCP_MSS_TCP)
    };
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
ip link set dev {TUN_IFACE} mtu {mtu} up
ip route add default dev {TUN_IFACE} table {ROUTE_TABLE}; route_added=1
ip rule add dev {TUN_IFACE} table {ROUTE_TABLE} pref {ROUTE_TABLE}; rule_dev=1
ip rule add from {ip} table {ROUTE_TABLE} pref 40; rule_src=1
ip rule add fwmark {ROUTE_TABLE} table {ROUTE_TABLE} pref 45; rule_mark=1
echo 2 > /proc/sys/net/ipv4/conf/{TUN_IFACE}/rp_filter 2>/dev/null || true
echo 1 > /proc/sys/net/ipv4/tcp_mtu_probing 2>/dev/null || true
for ch in POSTROUTING OUTPUT; do
  while iptables -w -t mangle -D "$ch" -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {other_mss} 2>/dev/null; do :; done
  while iptables -w -t mangle -D "$ch" -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {LEGACY_TUN_TCP_MSS_TCP} 2>/dev/null; do :; done
  iptables -w -t mangle -C "$ch" -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {mss} 2>/dev/null \
    || iptables -w -t mangle -I "$ch" 1 -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {mss} 2>/dev/null || true
done
for ch in PREROUTING INPUT FORWARD; do
  while iptables -w -t mangle -D "$ch" -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {other_mss} 2>/dev/null; do :; done
  while iptables -w -t mangle -D "$ch" -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {LEGACY_TUN_TCP_MSS_TCP} 2>/dev/null; do :; done
  iptables -w -t mangle -C "$ch" -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {mss} 2>/dev/null \
    || iptables -w -t mangle -I "$ch" 1 -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {mss} 2>/dev/null || true
done
for ch in OUTPUT FORWARD; do
  iptables -w -t filter -C "$ch" -o {TUN_IFACE} -p udp --dport 443 -m comment --comment {IPT_COMMENT} -j REJECT --reject-with icmp-port-unreachable 2>/dev/null \
    || iptables -w -t filter -I "$ch" 1 -o {TUN_IFACE} -p udp --dport 443 -m comment --comment {IPT_COMMENT} -j REJECT --reject-with icmp-port-unreachable 2>/dev/null || true
done
trap - EXIT"#
    )
}

/// Снимает маршрутизацию туннеля (сам интерфейс исчезает, когда панель закрывает дескриптор TUN).
pub fn tun_teardown_script() -> String {
    tun_teardown_script_for_ip(None)
}

pub(super) fn tun_teardown_script_for_ip(ip: Option<Ipv4Addr>) -> String {
    let source_rule = ip.map(|ip| format!("ip rule del pref 40 from {ip} table {ROUTE_TABLE} 2>/dev/null || true\nip addr del {ip}/32 dev {TUN_IFACE} 2>/dev/null || true\n"))
        .unwrap_or_default();
    format!(
        "for ch in POSTROUTING OUTPUT; do
  while iptables -w -t mangle -D \"$ch\" -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS} 2>/dev/null; do :; done
  while iptables -w -t mangle -D \"$ch\" -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS_TCP} 2>/dev/null; do :; done
  while iptables -w -t mangle -D \"$ch\" -o {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {LEGACY_TUN_TCP_MSS_TCP} 2>/dev/null; do :; done
done
for ch in PREROUTING INPUT FORWARD; do
  while iptables -w -t mangle -D \"$ch\" -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS} 2>/dev/null; do :; done
  while iptables -w -t mangle -D \"$ch\" -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {TUN_TCP_MSS_TCP} 2>/dev/null; do :; done
  while iptables -w -t mangle -D \"$ch\" -i {TUN_IFACE} -p tcp --tcp-flags SYN,RST SYN -m comment --comment {IPT_COMMENT} -j TCPMSS --set-mss {LEGACY_TUN_TCP_MSS_TCP} 2>/dev/null; do :; done
done
for ch in OUTPUT FORWARD; do
  while iptables -w -t filter -D \"$ch\" -o {TUN_IFACE} -p udp --dport 443 -m comment --comment {IPT_COMMENT} -j REJECT --reject-with icmp-port-unreachable 2>/dev/null; do :; done
  while iptables -w -t filter -D \"$ch\" -o {TUN_IFACE} -p udp --dport 443 -m comment --comment {IPT_COMMENT} -j REJECT 2>/dev/null; do :; done
done
ip rule del pref {ROUTE_TABLE} dev {TUN_IFACE} table {ROUTE_TABLE} 2>/dev/null || true
{source_rule}ip rule del pref 45 fwmark {ROUTE_TABLE} table {ROUTE_TABLE} 2>/dev/null || true
ip route del default dev {TUN_IFACE} table {ROUTE_TABLE} 2>/dev/null || true"
    )
}

pub(super) async fn apply_tun_config<F, Fut>(
    rt: &RwLock<VkTurnRuntime>, ip: Ipv4Addr, dns: Option<String>, apply: F,
) -> Result<(), String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    apply_tun_config_for_transport(rt, ip, dns, "udp", apply).await
}

pub(super) async fn apply_tun_config_for_transport<F, Fut>(
    rt: &RwLock<VkTurnRuntime>, ip: Ipv4Addr, dns: Option<String>, transport: &str, apply: F,
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
        Some(previous) => format!("{}\n{}", tun_teardown_script_for_ip(Some(previous)), tun_configure_script_for_transport(ip, transport)),
        None => tun_configure_script_for_transport(ip, transport),
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

#[cfg(any(target_os = "linux", test))]
pub(super) fn retry_fd_send<F>(
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
pub(super) struct TunHandle {
    #[cfg(target_os = "linux")]
    fd: std::os::fd::OwnedFd,
}

impl TunHandle {
    pub(super) fn create() -> Result<Self, String> {
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
    pub(super) fn duplicate(&self) -> Result<TunHandle, String> {
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
    pub(super) fn send_to_client_blocking(&self, uds_name: &str, cancel: &std::sync::atomic::AtomicBool) -> Result<(), String> {
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
        let with_groups = crate::routing::add_provider_to_proxy_groups(&yaml_with_prov, PROVIDER_ID);
        normalize_vkturn_provider_in_yaml(&with_groups)
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

