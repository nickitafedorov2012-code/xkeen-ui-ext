use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::RwLock;

static SHUTDOWN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub fn shutdown() {
    SHUTDOWN.store(true, std::sync::atomic::Ordering::Release);
}

/// Статус отдельного DNS-провайдера
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DnsProviderStatus {
    pub name: String,
    pub provider_type: String, // "udp" | "doh"
    pub is_substituting: bool, // Подменяет ли заблокированные IP
    pub last_latency_ms: Option<u64>,
    pub resolved_ips: Vec<String>,
    pub error: Option<String>,
    pub last_check: Option<String>,
}

/// Событие подсистемы Antigravity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityEvent {
    pub time: String,
    pub message: String,
    pub level: String, // "info" | "warn" | "error" | "success"
}

/// Полный статус для REST API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityStatus {
    pub enabled: bool,
    pub state: String, // "working" | "healing" | "disabled" | "error"
    pub current_route: String, // "direct" | "vpn" | "proxy"
    pub active_ip: Option<String>,
    pub latency_ms: Option<u64>,
    pub proxy_port: u16,
    pub proxy_running: bool,
    pub mode: String,
    pub providers: Vec<DnsProviderStatus>,
    pub events: Vec<AntigravityEvent>,
    pub targets: Vec<String>,
    pub own_proxy: String,
}

/// Проверка, является ли IP адрес внутренним, loopback, link-local, unspecified или SSRF-уязвимым
pub fn is_ssrf_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(ipv4) => {
            ipv4.is_loopback()
                || ipv4.is_private()
                || ipv4.is_link_local()
                || ipv4.is_unspecified()
                || ipv4.is_broadcast()
        }
        std::net::IpAddr::V6(ipv6) => {
            ipv6.is_loopback()
                || ipv6.is_unspecified()
                || ((ipv6.segments()[0] & 0xfe00) == 0xfc00) // ULA fc00::/7
                || ((ipv6.segments()[0] & 0xffc0) == 0xfe80) // Link-local fe80::/10
        }
    }
}

pub struct AntigravityManager {
    pub config: Arc<RwLock<Arc<crate::config::AppConfig>>>,
    pub state: Arc<RwLock<AntigravityInternalState>>,
    pub proxy_cancel_tx: Arc<tokio::sync::Mutex<Option<tokio::sync::watch::Sender<bool>>>>,
    http: reqwest::Client,
}

pub struct AntigravityInternalState {
    pub active_ip: Option<Ipv4Addr>,
    pub latency_ms: Option<u64>,
    pub consecutive_failures: u32,
    pub state_status: String,
    pub providers: Vec<DnsProviderStatus>,
    pub events: VecDeque<AntigravityEvent>,
    pub proxy_running: bool,
}

impl AntigravityInternalState {
    pub fn new() -> Self {
        Self {
            active_ip: None,
            latency_ms: None,
            consecutive_failures: 0,
            state_status: "disabled".into(),
            providers: Vec::new(),
            events: VecDeque::with_capacity(50),
            proxy_running: false,
        }
    }

    pub fn log(&mut self, msg: impl Into<String>, level: impl Into<String>) {
        let now = chrono::Local::now().format("%H:%M:%S").to_string();
        if self.events.len() >= 50 {
            self.events.pop_back();
        }
        self.events.push_front(AntigravityEvent {
            time: now,
            message: msg.into(),
            level: level.into(),
        });
    }
}

// Построение стандартного бинарного DNS-запроса (A-запись, RD=1)
pub fn build_dns_query(domain: &str, tx_id: u16) -> Vec<u8> {
    let mut buf = Vec::with_capacity(64);
    // Header
    buf.extend_from_slice(&tx_id.to_be_bytes()); // ID
    buf.extend_from_slice(&[0x01, 0x00]); // Flags: RD=1
    buf.extend_from_slice(&[0x00, 0x01]); // QDCOUNT: 1
    buf.extend_from_slice(&[0x00, 0x00]); // ANCOUNT: 0
    buf.extend_from_slice(&[0x00, 0x00]); // NSCOUNT: 0
    buf.extend_from_slice(&[0x00, 0x00]); // ARCOUNT: 0

    // Question
    for label in domain.split('.') {
        if label.is_empty() {
            continue;
        }
        buf.push(label.len() as u8);
        buf.extend_from_slice(label.as_bytes());
    }
    buf.push(0); // Terminator
    buf.extend_from_slice(&[0x00, 0x01]); // TYPE: A (1)
    buf.extend_from_slice(&[0x00, 0x01]); // CLASS: IN (1)
    buf
}

/// Пропуск доменного имени в DNS-пакете с поддержкой меток и сжатых указателей 0xC0 (RFC 1035).
/// Возвращает смещение в буфере сразу после конца имени.
pub fn skip_dns_name(buf: &[u8], mut pos: usize) -> Option<usize> {
    while pos < buf.len() {
        let b = buf[pos];
        if b == 0 {
            return Some(pos + 1);
        }
        if (b & 0xC0) == 0xC0 {
            // Сжатый указатель (2 байта). Указатель завершает имя в данном месте пакета.
            if pos + 2 <= buf.len() {
                return Some(pos + 2);
            } else {
                return None;
            }
        }
        let len = b as usize;
        if len > 63 || pos + 1 + len > buf.len() {
            return None;
        }
        pos += 1 + len;
    }
    None
}

// Разбор A-записей из ответа DNS с корректной поддержкой сжатых указателей 0xC0 (BUG-01)
pub fn parse_dns_a_records(buf: &[u8]) -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    if buf.len() < 12 {
        return ips;
    }
    let qdcount = u16::from_be_bytes([buf[4], buf[5]]) as usize;
    let ancount = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    if ancount == 0 {
        return ips;
    }

    // Пропуск секции Question
    let mut pos = 12;
    let q_count = if qdcount == 0 { 1 } else { qdcount };
    for _ in 0..q_count {
        pos = match skip_dns_name(buf, pos) {
            Some(p) => p,
            None => return ips,
        };
        if pos + 4 > buf.len() {
            return ips;
        }
        pos += 4; // QTYPE(2) + QCLASS(2)
    }

    // Разбор Answers
    for _ in 0..ancount {
        pos = match skip_dns_name(buf, pos) {
            Some(p) => p,
            None => break,
        };
        if pos + 10 > buf.len() {
            break;
        }
        let atype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let rdlength = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;
        if atype == 1 && rdlength == 4 && pos + 4 <= buf.len() {
            ips.push(Ipv4Addr::new(buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]));
        }
        if pos + rdlength > buf.len() {
            break;
        }
        pos += rdlength;
    }
    ips
}

/// Проверка, принадлежит ли IPv4-адрес официальным подсетям Google (Google APIs / Cloud Code)
pub fn is_google_ip(ip: &Ipv4Addr) -> bool {
    let o = ip.octets();
    match o[0] {
        172 => o[1] == 217,
        142 => o[1] == 250 || o[1] == 251,
        216 => o[1] == 58 || o[1] == 239,
        74 => o[1] == 125,
        173 => o[1] == 194,
        209 => o[1] == 85,
        108 => o[1] == 177,
        64 => o[1] == 233,
        66 => o[1] == 102 || o[1] == 249,
        _ => false,
    }
}

async fn run_cmd(program: &str, args: &[&str]) -> Result<(), String> {
    match tokio::process::Command::new(program).args(args).output().await {
        Ok(out) => {
            if !out.status.success() {
                let err = String::from_utf8_lossy(&out.stderr);
                crate::log_d!("[ANTIGRAVITY] Команда {} {:?} завершилась с кодом {:?}: {}", program, args, out.status.code(), err.trim());
                Err(format!("Команда {program} {:?} статус {:?}: {}", args, out.status.code(), err.trim()))
            } else {
                Ok(())
            }
        }
        Err(e) => {
            crate::log_w!("[ANTIGRAVITY] Ошибка запуска команды {} {:?}: {}", program, args, e);
            Err(format!("Ошибка запуска {program} {:?}: {e}", args))
        }
    }
}

impl AntigravityManager {
    pub fn new(config: Arc<RwLock<Arc<crate::config::AppConfig>>>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(5))
            .build()
            .unwrap_or_default();

        Self {
            config,
            state: Arc::new(RwLock::new(AntigravityInternalState::new())),
            proxy_cancel_tx: Arc::new(tokio::sync::Mutex::new(None)),
            http,
        }
    }

    /// Остановка HTTP CONNECT прокси
    pub async fn stop_proxy(&self) {
        let mut cancel_guard = self.proxy_cancel_tx.lock().await;
        if let Some(old_tx) = cancel_guard.take() {
            let _ = old_tx.send(true);
        }
        let mut st = self.state.write().await;
        st.proxy_running = false;
    }

    /// Перезапуск HTTP CONNECT прокси на новом порту
    pub async fn restart_proxy(self: Arc<Self>, port: u16) {
        self.stop_proxy().await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        self.start_proxy(port);
    }

    /// Получение эталонного DNS-сервера (из настроек AdGuard /etc/resolv.conf или 1.1.1.1:53)
    pub async fn get_reference_dns_server(&self) -> String {
        let cfg_dns = {
            let c = self.config.read().await;
            if !c.adguard.upstream_dns.is_empty() {
                c.adguard.upstream_dns.first().cloned()
            } else {
                None
            }
        };
        if let Some(dns) = cfg_dns {
            let trimmed = dns.trim();
            if let Ok(ip) = trimmed.parse::<Ipv4Addr>() {
                return format!("{ip}:53");
            }
        }

        #[cfg(target_os = "linux")]
        {
            if let Ok(resolv) = tokio::fs::read_to_string("/etc/resolv.conf").await {
                for line in resolv.lines() {
                    let line = line.trim();
                    if line.starts_with("nameserver") {
                        if let Some(ns) = line.strip_prefix("nameserver").map(str::trim) {
                            if let Ok(ip) = ns.parse::<Ipv4Addr>() {
                                if !ip.is_loopback() && !ip.is_unspecified() {
                                    return format!("{ip}:53");
                                }
                            }
                        }
                    }
                }
            }
        }

        "1.1.1.1:53".to_string()
    }

    /// Запрос по обычному UDP DNS
    async fn query_udp(&self, server: &str, domain: &str) -> Result<(Vec<Ipv4Addr>, u64), String> {
        let sock = UdpSocket::bind("0.0.0.0:0").await.map_err(|e| e.to_string())?;
        let target = if server.contains(':') {
            server.to_string()
        } else {
            format!("{server}:53")
        };
        let query = build_dns_query(domain, 0x4a11);
        let start = std::time::Instant::now();
        sock.send_to(&query, &target).await.map_err(|e| e.to_string())?;

        let mut buf = [0u8; 1024];
        let (len, _) = tokio::time::timeout(std::time::Duration::from_millis(2500), sock.recv_from(&mut buf))
            .await
            .map_err(|_| "Таймаут ожидания ответа".to_string())?
            .map_err(|e| e.to_string())?;

        let elapsed = start.elapsed().as_millis() as u64;
        let ips = parse_dns_a_records(&buf[..len]);
        Ok((ips, elapsed))
    }

    /// Запрос по DoH (RFC 8484)
    async fn query_doh(&self, url: &str, domain: &str) -> Result<(Vec<Ipv4Addr>, u64), String> {
        let query = build_dns_query(domain, 0x4a22);
        let start = std::time::Instant::now();
        let resp = self
            .http
            .post(url)
            .header("Content-Type", "application/dns-message")
            .body(query)
            .timeout(std::time::Duration::from_millis(3000))
            .send()
            .await
            .map_err(|e| e.to_string())?;

        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }

        let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
        let elapsed = start.elapsed().as_millis() as u64;
        let ips = parse_dns_a_records(&bytes);
        Ok((ips, elapsed))
    }

    /// Замер TCP-задержки до порта 443
    async fn probe_latency(&self, ip: Ipv4Addr) -> Result<u64, String> {
        let start = std::time::Instant::now();
        let target = format!("{ip}:443");
        let stream = tokio::time::timeout(
            std::time::Duration::from_millis(3000),
            TcpStream::connect(&target),
        )
        .await
        .map_err(|_| "Таймаут подключения к 443 порту".to_string())?
        .map_err(|e| e.to_string())?;
        drop(stream);
        Ok(start.elapsed().as_millis() as u64)
    }

    /// Проверка и обновление всех резолверов и маршрутов
    pub async fn check_and_update(&self) {
        let (enabled, targets) = {
            let c = self.config.read().await;
            (c.antigravity.enabled, c.antigravity.targets.clone())
        };

        if !enabled {
            let old_ip = {
                let mut st = self.state.write().await;
                if st.state_status != "disabled" {
                    st.state_status = "disabled".into();
                    st.log("Служба Antigravity отключена пользователем", "info");
                    let ip = st.active_ip.take();
                    st.latency_ms = None;
                    ip
                } else {
                    None
                }
            };
            if let Some(ip) = old_ip {
                self.clean_keenetic_rules(&Some(ip)).await;
            }
            return;
        }

        let main_target = targets.first().cloned().unwrap_or_else(|| "cloudcode-pa.googleapis.com".into());

        // 1. Получаем эталонные адреса Google через системный / настроенный DNS
        let ref_server = self.get_reference_dns_server().await;
        let ref_ips: Vec<Ipv4Addr> = match self.query_udp(&ref_server, &main_target).await {
            Ok((ips, _)) => ips,
            Err(_) => Vec::new(),
        };

        // 2. Список кандидатов для проверки
        let resolver_candidates = vec![
            ("comss.one", "udp", "83.220.169.155"),
            ("comss.one (alt)", "udp", "212.109.195.93"),
            ("geohide.ru", "udp", "45.155.204.190"),
            ("dns-ai.ru", "udp", "192.144.59.14"),
            ("dns-ai.ru (alt)", "udp", "94.232.43.149"),
            ("xbox-dns.ru", "udp", "111.88.96.54"),
            ("xbox-dns.ru (alt)", "udp", "111.88.96.50"),
        ];

        let mut provider_statuses = Vec::new();
        let mut valid_substituted_ips: Vec<(Ipv4Addr, u64, String)> = Vec::new();

        // Проверяем целевой домен и при необходимости aistudio.google.com
        let probe_domains = if main_target == "aistudio.google.com" || main_target == "gemini.google.com" {
            vec![main_target.clone()]
        } else {
            vec![main_target.clone(), "aistudio.google.com".into()]
        };

        for (name, kind, endpoint) in resolver_candidates {
            let now_str = chrono::Local::now().format("%H:%M:%S").to_string();
            let mut prov_ips = Vec::new();
            let mut prov_lat = None;
            let mut prov_err = None;

            for target_domain in &probe_domains {
                let res = if kind == "doh" {
                    self.query_doh(endpoint, target_domain).await
                } else {
                    self.query_udp(endpoint, target_domain).await
                };

                match res {
                    Ok((ips, lat)) => {
                        prov_lat = Some(lat);
                        let is_sub = !ips.is_empty()
                            && !ips.iter().any(|ip| ref_ips.contains(ip) || is_google_ip(ip));

                        if is_sub {
                            for ip in &ips {
                                valid_substituted_ips.push((*ip, lat, name.to_string()));
                            }
                        }
                        for ip in ips {
                            if !prov_ips.contains(&ip) {
                                prov_ips.push(ip);
                            }
                        }
                        if !prov_ips.is_empty() {
                            break;
                        }
                    }
                    Err(err) => {
                        prov_err = Some(err);
                    }
                }
            }

            let has_ips = !prov_ips.is_empty();
            let is_sub = has_ips && !prov_ips.iter().any(|ip| ref_ips.contains(ip) || is_google_ip(ip));

            provider_statuses.push(DnsProviderStatus {
                name: name.to_string(),
                provider_type: kind.to_string(),
                is_substituting: is_sub,
                last_latency_ms: prov_lat,
                resolved_ips: prov_ips.into_iter().map(|ip| ip.to_string()).collect(),
                error: if has_ips { None } else { prov_err },
                last_check: Some(now_str),
            });
        }

        // 3. Выбираем лучший подменный IP (проверяем реальную доступность по порту 443)
        let mut chosen_ip = None;
        let mut chosen_latency = None;

        for (ip, _, provider_name) in valid_substituted_ips {
            // Пропускаем локальные loopback адреса (127.x.x.x), выбираем только реальные внешние прокси-узлы
            if ip.is_loopback() || ip.is_unspecified() {
                continue;
            }
            match self.probe_latency(ip).await {
                Ok(lat) => {
                    chosen_ip = Some((ip, provider_name));
                    chosen_latency = Some(lat);
                    break;
                }
                Err(_) => {
                    // Этот IP не отвечает по 443, пробуем следующий
                }
            }
        }

        let mut st = self.state.write().await;
        st.providers = provider_statuses;

        if let Some(((ip, prov_name), lat)) = chosen_ip.zip(chosen_latency) {
            let prev_ip = st.active_ip;
            st.active_ip = Some(ip);
            st.latency_ms = Some(lat);
            st.state_status = "working".into();
            st.consecutive_failures = 0;

            let ip_changed = prev_ip != Some(ip);
            if ip_changed {
                st.log(
                    format!("Выбран рабочий подменный IP {ip} от {prov_name} (задержка {lat} мс)"),
                    "success",
                );
            }
            drop(st);

            if ip_changed {
                // Применяем правила в Keenetic
                self.apply_keenetic_rules(ip, &targets, prev_ip).await;
            }
        } else {
            st.consecutive_failures += 1;
            let fails = st.consecutive_failures;
            st.state_status = if fails >= 3 {
                "error".into()
            } else {
                "healing".into()
            };
            st.log(
                format!("Внимание: не найдено доступных подменных IP (ошибок подряд: {fails})"),
                "warn",
            );
        }
    }

    /// Применение DNS-записей в KeeneticOS и добавление маршрута мимо VPN
    async fn apply_keenetic_rules(&self, ip: Ipv4Addr, targets: &[String], prev_ip: Option<Ipv4Addr>) {
        // 1. Очистка прошлого IP из маршрутизации и ipset с дедупликацией (BUG-01)
        if let Some(old) = prev_ip {
            let old_str = old.to_string();
            let old_rule = format!("{old}/32");
            while run_cmd("ip", &["rule", "del", "to", &old_rule, "table", "main", "priority", "90"]).await.is_ok() {}
            let _ = run_cmd("ipset", &["del", "user_exclude", &old_str]).await;
        }

        // 2. Дедуплицированное удаление старых правил ip rule для нового IP перед добавлением (BUG-01)
        let ip_rule = format!("{ip}/32");
        let ip_str = ip.to_string();
        while run_cmd("ip", &["rule", "del", "to", &ip_rule, "table", "main", "priority", "90"]).await.is_ok() {}

        // 3. Исключение подменного IP из перехвата Mihomo (ipset user_exclude)
        if let Err(e) = run_cmd("ipset", &["add", "user_exclude", &ip_str, "-exist"]).await {
            crate::log_w!("[ANTIGRAVITY] Не удалось добавить {ip_str} в ipset user_exclude: {e}");
        }

        // 4. Добавление правила маршрутизации напрямую через основной WAN (таблица main)
        if let Err(e) = run_cmd("ip", &["rule", "add", "to", &ip_rule, "table", "main", "priority", "90"]).await {
            crate::log_w!("[ANTIGRAVITY] Не удалось добавить ip rule для {ip_rule}: {e}");
        }

        // 5. Установка статических DNS-записей в Keenetic ndnproxy с валидацией доменов (SEC-02)
        for target in targets {
            let clean_target = target.trim();
            if !crate::config::is_valid_domain(clean_target) {
                crate::log_w!("[ANTIGRAVITY] Пропуск невалидного домена: {target}");
                continue;
            }
            let clean_cmd = format!("no ip host {clean_target}");
            let _ = run_cmd("ndmc", &["-c", &clean_cmd]).await;
            let cmd = format!("ip host {clean_target} {ip}");
            if let Err(e) = run_cmd("ndmc", &["-c", &cmd]).await {
                crate::log_w!("[ANTIGRAVITY] Не удалось установить DNS запись {clean_target} -> {ip}: {e}");
            }
        }
    }

    /// Очистка всех правил из Keenetic
    pub async fn clean_keenetic_rules(&self, active_ip: &Option<Ipv4Addr>) {
        if let Some(ip) = active_ip {
            let ip_rule = format!("{ip}/32");
            let ip_str = ip.to_string();
            while run_cmd("ip", &["rule", "del", "to", &ip_rule, "table", "main", "priority", "90"]).await.is_ok() {}
            let _ = run_cmd("ipset", &["del", "user_exclude", &ip_str]).await;
        }

        let targets = {
            let c = self.config.read().await;
            c.antigravity.targets.clone()
        };

        for target in targets {
            let clean_target = target.trim();
            if !crate::config::is_valid_domain(clean_target) {
                continue;
            }
            let cmd = format!("no ip host {clean_target}");
            let _ = run_cmd("ndmc", &["-c", &cmd]).await;
        }
    }

    /// Запуск легковесного HTTP CONNECT прокси (привязка к 127.0.0.1 для безопасности от WAN)
    pub fn start_proxy(self: Arc<Self>, port: u16) {
        tokio::spawn(async move {
            let addr = format!("127.0.0.1:{port}");
            let listener = match TcpListener::bind(&addr).await {
                Ok(l) => {
                    let mut st = self.state.write().await;
                    st.proxy_running = true;
                    st.log(format!("HTTP CONNECT прокси запущен на {addr}"), "info");
                    l
                }
                Err(e) => {
                    self.state.write().await.log(format!("Не удалось запустить прокси на {port}: {e}"), "error");
                    return;
                }
            };

            let (cancel_tx, mut cancel_rx) = tokio::sync::watch::channel(false);
            {
                let mut guard = self.proxy_cancel_tx.lock().await;
                if let Some(old) = guard.take() {
                    let _ = old.send(true);
                }
                *guard = Some(cancel_tx);
            }

            let sem = Arc::new(tokio::sync::Semaphore::new(64));

            loop {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) || *cancel_rx.borrow() {
                    break;
                }

                tokio::select! {
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            break;
                        }
                    }
                    res = listener.accept() => {
                        let (stream, _) = match res {
                            Ok(r) => r,
                            Err(_) => continue,
                        };

                        let permit = match sem.clone().try_acquire_owned() {
                            Ok(p) => p,
                            Err(_) => {
                                crate::log_w!("[ANTIGRAVITY] Превышен лимит параллельных подключений к прокси (64)");
                                continue;
                            }
                        };

                        let mgr = self.clone();
                        tokio::spawn(async move {
                            let _permit = permit;
                            let _ = tokio::time::timeout(
                                std::time::Duration::from_secs(60),
                                handle_proxy_conn(stream, mgr)
                            ).await;
                        });
                    }
                }
            }

            let mut st = self.state.write().await;
            st.proxy_running = false;
        });
    }

    /// Запуск периодического warm-loop с поддержкой graceful shutdown
    pub fn start_warm_loop(self: Arc<Self>) {
        tokio::spawn(async move {
            // Начальная проверка через 2 секунды после старта
            for _ in 0..2 {
                if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
            self.check_and_update().await;

            loop {
                let interval = {
                    let c = self.config.read().await;
                    c.antigravity.health_check_interval.max(30)
                };
                for _ in 0..interval {
                    if SHUTDOWN.load(std::sync::atomic::Ordering::Acquire) {
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                }
                self.check_and_update().await;
            }
        });
    }

    /// Получение текущего снимка статуса
    pub async fn get_status(&self) -> AntigravityStatus {
        let (enabled, proxy_port, targets, own_proxy, mode) = {
            let c = self.config.read().await;
            (
                c.antigravity.enabled,
                c.antigravity.proxy_port,
                c.antigravity.targets.clone(),
                c.antigravity.own_proxy.clone(),
                c.antigravity.mode.clone(),
            )
        };

        let st = self.state.read().await;
        AntigravityStatus {
            enabled,
            state: st.state_status.clone(),
            current_route: if st.active_ip.is_some() {
                "direct".into()
            } else {
                "none".into()
            },
            active_ip: st.active_ip.map(|ip| ip.to_string()),
            latency_ms: st.latency_ms,
            proxy_port,
            proxy_running: st.proxy_running,
            mode,
            providers: st.providers.clone(),
            events: st.events.iter().cloned().collect(),
            targets,
            own_proxy,
        }
    }
}

/// Обработка клиентского HTTP CONNECT запроса (TCP splice без MITM)
async fn handle_proxy_conn(mut client: TcpStream, mgr: Arc<AntigravityManager>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Безопасность: доступ разрешён только с loopback и локальной сети (RFC 1918 / ULA)
    if let Ok(peer_addr) = client.peer_addr() {
        let ip = peer_addr.ip();
        let is_private = match ip {
            std::net::IpAddr::V4(ipv4) => ipv4.is_loopback() || ipv4.is_private() || ipv4.is_link_local(),
            std::net::IpAddr::V6(ipv6) => {
                ipv6.is_loopback()
                    || ((ipv6.segments()[0] & 0xfe00) == 0xfc00)
                    || ((ipv6.segments()[0] & 0xffc0) == 0xfe80)
            }
        };
        if !is_private {
            crate::log_w!("[ANTIGRAVITY] Отклонён внешний запрос к прокси с WAN IP: {ip}");
            let _ = client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await;
            return Ok(());
        }
    }

    let mut buf = [0u8; 1024];
    let n = match tokio::time::timeout(std::time::Duration::from_secs(10), client.read(&mut buf)).await {
        Ok(Ok(n)) if n > 0 => n,
        _ => return Ok(()),
    };

    let req_str = String::from_utf8_lossy(&buf[..n]);
    let first_line = req_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() < 2 || parts[0] != "CONNECT" {
        client.write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n").await?;
        return Ok(());
    }

    let target_host_port = parts[1];
    let (host, port) = match target_host_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse::<u16>().unwrap_or(0)),
        None => (target_host_port, 443),
    };

    // SEC-04: Разрешаем CONNECT-прокси только на порт 443
    if port != 443 {
        crate::log_w!("[ANTIGRAVITY] Отклонён CONNECT-запрос на неразрешённый порт: {port}");
        client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
        return Ok(());
    }

    // SEC-04: Проверка, что домен входит в список разрешённых targets
    let targets = {
        let c = mgr.config.read().await;
        c.antigravity.targets.clone()
    };
    let is_target_allowed = targets.iter().any(|t| t.eq_ignore_ascii_case(host));
    if !is_target_allowed {
        crate::log_w!("[ANTIGRAVITY] Отклонён CONNECT-запрос к недопустимому домену: {host}");
        client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
        return Ok(());
    }

    // SEC-04: Если передан литеральный IP, проверяем на SSRF
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        if is_ssrf_ip(ip) {
            crate::log_w!("[ANTIGRAVITY] Отклонён CONNECT-запрос к SSRF IP: {ip}");
            client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
            return Ok(());
        }
    }

    // Определяем, куда отправлять трафик:
    let destination = {
        let st = mgr.state.read().await;
        if let Some(sub_ip) = st.active_ip {
            if is_ssrf_ip(std::net::IpAddr::V4(sub_ip)) {
                crate::log_w!("[ANTIGRAVITY] Заблокирована попытка соединения с SSRF active_ip: {sub_ip}");
                client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
                return Ok(());
            }
            format!("{sub_ip}:443")
        } else {
            format!("{host}:443")
        }
    };

    // SEC-04: Разрешаем и проверяем IP адреса назначения перед подключением
    let addrs = match tokio::net::lookup_host(&destination).await {
        Ok(iter) => iter.collect::<Vec<_>>(),
        Err(e) => {
            let resp = format!("HTTP/1.1 502 Bad Gateway\r\nX-Error: {}\r\n\r\n", e);
            client.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };

    if addrs.is_empty() || addrs.iter().any(|a| is_ssrf_ip(a.ip())) {
        crate::log_w!("[ANTIGRAVITY] Отклонён запрос к адресу из черного списка SSRF: {destination}");
        client.write_all(b"HTTP/1.1 403 Forbidden\r\n\r\n").await?;
        return Ok(());
    }

    let mut server = match tokio::time::timeout(std::time::Duration::from_secs(10), TcpStream::connect(&addrs[..])).await {
        Ok(Ok(s)) => s,
        Ok(Err(e)) => {
            let resp = format!("HTTP/1.1 502 Bad Gateway\r\nX-Error: {}\r\n\r\n", e);
            client.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
        Err(_) => {
            let resp = "HTTP/1.1 504 Gateway Timeout\r\n\r\n";
            client.write_all(resp.as_bytes()).await?;
            return Ok(());
        }
    };

    // Отвечаем клиенту: 200 Connection Established
    client.write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n").await?;

    // Прозрачный бидирекционный байтовый обмен (TCP splice)
    let _ = tokio::io::copy_bidirectional(&mut client, &mut server).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_google_ip() {
        assert!(is_google_ip(&Ipv4Addr::new(172, 217, 16, 206)));
        assert!(is_google_ip(&Ipv4Addr::new(142, 250, 185, 206)));
        assert!(is_google_ip(&Ipv4Addr::new(142, 251, 36, 14)));
        assert!(is_google_ip(&Ipv4Addr::new(216, 239, 38, 120)));
        assert!(is_google_ip(&Ipv4Addr::new(216, 58, 214, 206)));
        assert!(is_google_ip(&Ipv4Addr::new(74, 125, 200, 100)));
        assert!(is_google_ip(&Ipv4Addr::new(173, 194, 76, 138)));
        assert!(is_google_ip(&Ipv4Addr::new(209, 85, 233, 101)));
        assert!(is_google_ip(&Ipv4Addr::new(108, 177, 14, 100)));
        assert!(is_google_ip(&Ipv4Addr::new(64, 233, 165, 100)));
        assert!(is_google_ip(&Ipv4Addr::new(66, 102, 1, 1)));
        assert!(is_google_ip(&Ipv4Addr::new(66, 249, 80, 1)));

        // Non-Google IPs (e.g. SmartDNS / GeoHide proxy IP)
        assert!(!is_google_ip(&Ipv4Addr::new(83, 220, 169, 155)));
        assert!(!is_google_ip(&Ipv4Addr::new(45, 155, 204, 190)));
        assert!(!is_google_ip(&Ipv4Addr::new(192, 168, 1, 1)));
        assert!(!is_google_ip(&Ipv4Addr::new(1, 1, 1, 1)));

        // Xbox-DNS resolvers & Selectel SNI reverse-proxies
        assert!(!is_google_ip(&Ipv4Addr::new(111, 88, 96, 54)));
        assert!(!is_google_ip(&Ipv4Addr::new(111, 88, 96, 50)));
        assert!(!is_google_ip(&Ipv4Addr::new(188, 68, 214, 130)));
        assert!(!is_google_ip(&Ipv4Addr::new(188, 68, 214, 143)));
    }

    #[test]
    fn test_xbox_dns_ip_validity() {
        let primary_dns = Ipv4Addr::new(111, 88, 96, 54);
        let alt_dns = Ipv4Addr::new(111, 88, 96, 50);
        let proxy_node = Ipv4Addr::new(188, 68, 214, 130);

        assert!(!primary_dns.is_loopback());
        assert!(!primary_dns.is_unspecified());
        assert!(!alt_dns.is_loopback());
        assert!(!proxy_node.is_loopback());
        assert!(!is_google_ip(&proxy_node));
    }

    #[test]
    fn test_skip_dns_name_and_compressed_pointers() {
        // Standard uncompressed name: \x03www\x06google\x03com\x00
        let uncompressed = b"\x03www\x06google\x03com\x00extra";
        assert_eq!(skip_dns_name(uncompressed, 0), Some(16));

        // RFC 1035 compressed pointer 0xC0 0x0C (2 bytes)
        let ptr_only = b"\xC0\x0C";
        assert_eq!(skip_dns_name(ptr_only, 0), Some(2));

        // Label prefix with compressed pointer: \x04mail\xC0\x0C
        let label_and_ptr = b"\x04mail\xC0\x0Crest";
        assert_eq!(skip_dns_name(label_and_ptr, 0), Some(7));

        // Malformed / truncated cases
        assert_eq!(skip_dns_name(b"\xC0", 0), None); // pointer truncated
        assert_eq!(skip_dns_name(b"\x05abc", 0), None); // label exceeds buffer
        assert_eq!(skip_dns_name(b"", 0), None);
    }

    #[test]
    fn test_parse_dns_a_records_with_compressed_pointers_0xc0() {
        let mut packet = Vec::new();
        // DNS Header (12 bytes)
        packet.extend_from_slice(&[0x12, 0x34]); // ID
        packet.extend_from_slice(&[0x81, 0x80]); // Flags (QR, RD, RA)
        packet.extend_from_slice(&[0x00, 0x01]); // QDCOUNT: 1
        packet.extend_from_slice(&[0x00, 0x02]); // ANCOUNT: 2
        packet.extend_from_slice(&[0x00, 0x00]); // NSCOUNT: 0
        packet.extend_from_slice(&[0x00, 0x00]); // ARCOUNT: 0

        // Question: example.com, QTYPE=A, QCLASS=IN
        // Offset 12:
        packet.extend_from_slice(b"\x07example\x03com\x00");
        packet.extend_from_slice(&[0x00, 0x01]); // QTYPE: A
        packet.extend_from_slice(&[0x00, 0x01]); // QCLASS: IN

        // Answer 1: name points to question name via 0xC00C
        packet.extend_from_slice(&[0xC0, 0x0C]); // Name pointer
        packet.extend_from_slice(&[0x00, 0x01]); // TYPE: A (1)
        packet.extend_from_slice(&[0x00, 0x01]); // CLASS: IN (1)
        packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x3C]); // TTL: 60s
        packet.extend_from_slice(&[0x00, 0x04]); // RDLENGTH: 4
        packet.extend_from_slice(&[93, 184, 216, 34]); // RDATA IP: 93.184.216.34

        // Answer 2: name with label prefix + pointer (0x03sub 0xC00C)
        packet.extend_from_slice(b"\x03sub");
        packet.extend_from_slice(&[0xC0, 0x0C]);
        packet.extend_from_slice(&[0x00, 0x01]); // TYPE: A (1)
        packet.extend_from_slice(&[0x00, 0x01]); // CLASS: IN (1)
        packet.extend_from_slice(&[0x00, 0x00, 0x00, 0x3C]); // TTL: 60s
        packet.extend_from_slice(&[0x00, 0x04]); // RDLENGTH: 4
        packet.extend_from_slice(&[8, 8, 8, 8]); // RDATA IP: 8.8.8.8

        let ips = parse_dns_a_records(&packet);
        assert_eq!(ips.len(), 2);
        assert_eq!(ips[0], Ipv4Addr::new(93, 184, 216, 34));
        assert_eq!(ips[1], Ipv4Addr::new(8, 8, 8, 8));
    }

    #[test]
    fn test_antigravity_ssrf_and_port_restriction() {
        // SEC-04: Test SSRF IP detection
        use std::net::IpAddr;
        assert!(is_ssrf_ip("127.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_ssrf_ip("10.0.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_ssrf_ip("172.16.0.1".parse::<IpAddr>().unwrap()));
        assert!(is_ssrf_ip("192.168.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_ssrf_ip("169.254.1.1".parse::<IpAddr>().unwrap()));
        assert!(is_ssrf_ip("0.0.0.0".parse::<IpAddr>().unwrap()));
        assert!(is_ssrf_ip("255.255.255.255".parse::<IpAddr>().unwrap()));
        assert!(is_ssrf_ip("::1".parse::<IpAddr>().unwrap()));
        assert!(is_ssrf_ip("fc00::1".parse::<IpAddr>().unwrap()));
        assert!(is_ssrf_ip("fe80::1".parse::<IpAddr>().unwrap()));

        // Public IPs are not SSRF
        assert!(!is_ssrf_ip("142.250.185.206".parse::<IpAddr>().unwrap()));
        assert!(!is_ssrf_ip("1.1.1.1".parse::<IpAddr>().unwrap()));
    }

    #[tokio::test]
    async fn test_antigravity_proxy_restart_channel() {
        // BUG-02: Test proxy cancellation and restart
        let cfg = Arc::new(tokio::sync::RwLock::new(Arc::new(crate::config::AppConfig::default())));
        let mgr = Arc::new(AntigravityManager::new(cfg));
        mgr.stop_proxy().await;
        let st = mgr.get_status().await;
        assert!(!st.proxy_running);

        mgr.clone().restart_proxy(18888).await;
        let mut running = false;
        for _ in 0..15 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if mgr.get_status().await.proxy_running {
                running = true;
                break;
            }
        }
        assert!(running, "Proxy should be running after restart");
        mgr.stop_proxy().await;
        let st3 = mgr.get_status().await;
        assert!(!st3.proxy_running);
    }

    #[tokio::test]
    async fn test_antigravity_dns_fallback_no_hardcoded_8888() {
        // HC-01: Test reference DNS does not default to 8.8.8.8
        let cfg = Arc::new(tokio::sync::RwLock::new(Arc::new(crate::config::AppConfig::default())));
        let mgr = AntigravityManager::new(cfg);
        let dns = mgr.get_reference_dns_server().await;
        assert!(!dns.is_empty());
        // Fallback should be 1.1.1.1:53 if no AdGuard DNS configured, never 8.8.8.8:53
        assert_ne!(dns, "8.8.8.8:53");
    }
}
