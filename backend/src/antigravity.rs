use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::net::Ipv4Addr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream, UdpSocket};
use tokio::sync::RwLock;

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

pub struct AntigravityManager {
    pub config: Arc<RwLock<Arc<crate::config::AppConfig>>>,
    pub state: Arc<RwLock<AntigravityInternalState>>,
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

// Разбор A-записей из ответа DNS
pub fn parse_dns_a_records(buf: &[u8]) -> Vec<Ipv4Addr> {
    let mut ips = Vec::new();
    if buf.len() < 12 {
        return ips;
    }
    let ancount = u16::from_be_bytes([buf[6], buf[7]]) as usize;
    if ancount == 0 {
        return ips;
    }

    // Пропуск секции Question
    let mut pos = 12;
    while pos < buf.len() {
        let len = buf[pos] as usize;
        if len == 0 {
            pos += 5; // 0 + QTYPE(2) + QCLASS(2)
            break;
        }
        if len >= 192 {
            pos += 6; // pointer(2) + QTYPE(2) + QCLASS(2)
            break;
        }
        pos += 1 + len;
    }

    // Разбор Answers
    for _ in 0..ancount {
        if pos >= buf.len() {
            break;
        }
        if buf[pos] >= 192 {
            pos += 2;
        } else {
            while pos < buf.len() && buf[pos] != 0 {
                let l = buf[pos] as usize;
                pos += 1 + l;
            }
            pos += 1;
        }
        if pos + 10 > buf.len() {
            break;
        }
        let atype = u16::from_be_bytes([buf[pos], buf[pos + 1]]);
        let rdlength = u16::from_be_bytes([buf[pos + 8], buf[pos + 9]]) as usize;
        pos += 10;
        if atype == 1 && rdlength == 4 && pos + 4 <= buf.len() {
            ips.push(Ipv4Addr::new(buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]));
        }
        pos += rdlength;
    }
    ips
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
            http,
        }
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
            let mut st = self.state.write().await;
            if st.state_status != "disabled" {
                st.state_status = "disabled".into();
                st.log("Служба Antigravity отключена пользователем", "info");
                self.clean_keenetic_rules(&st.active_ip).await;
                st.active_ip = None;
                st.latency_ms = None;
            }
            return;
        }

        let main_target = targets.get(0).cloned().unwrap_or_else(|| "cloudcode-pa.googleapis.com".into());

        // 1. Получаем эталонные адреса Google через 8.8.8.8
        let ref_ips: Vec<Ipv4Addr> = match self.query_udp("8.8.8.8:53", &main_target).await {
            Ok((ips, _)) => ips,
            Err(_) => Vec::new(),
        };

        // 2. Список кандидатов для проверки
        let resolver_candidates = vec![
            ("comss.one", "udp", "83.220.169.155"),
            ("comss.one (alt)", "udp", "212.109.195.93"),
            ("geohide.ru", "udp", "45.155.204.190"),
            ("dns-ai.ru", "doh", "https://dns.dns-ai.ru/dns-query"),
        ];

        let mut provider_statuses = Vec::new();
        let mut valid_substituted_ips: Vec<(Ipv4Addr, u64, String)> = Vec::new();

        for (name, kind, endpoint) in resolver_candidates {
            let now_str = chrono::Local::now().format("%H:%M:%S").to_string();
            let res = if kind == "doh" {
                self.query_doh(endpoint, &main_target).await
            } else {
                self.query_udp(endpoint, &main_target).await
            };

            match res {
                Ok((ips, lat)) => {
                    // Проверка: подменяет ли IP
                    let is_sub = !ips.is_empty()
                        && !ips.iter().any(|ip| {
                            ref_ips.contains(ip)
                                || ip.octets()[0] == 172 && ip.octets()[1] == 217
                                || ip.octets()[0] == 142 && ip.octets()[1] == 250
                        });

                    if is_sub {
                        for ip in &ips {
                            valid_substituted_ips.push((*ip, lat, name.to_string()));
                        }
                    }

                    provider_statuses.push(DnsProviderStatus {
                        name: name.to_string(),
                        provider_type: kind.to_string(),
                        is_substituting: is_sub,
                        last_latency_ms: Some(lat),
                        resolved_ips: ips.into_iter().map(|ip| ip.to_string()).collect(),
                        error: None,
                        last_check: Some(now_str),
                    });
                }
                Err(err) => {
                    provider_statuses.push(DnsProviderStatus {
                        name: name.to_string(),
                        provider_type: kind.to_string(),
                        is_substituting: false,
                        last_latency_ms: None,
                        resolved_ips: Vec::new(),
                        error: Some(err),
                        last_check: Some(now_str),
                    });
                }
            }
        }

        // 3. Выбираем лучший подменный IP (проверяем реальную доступность по порту 443)
        let mut chosen_ip = None;
        let mut chosen_latency = None;

        for (ip, _, provider_name) in valid_substituted_ips {
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

            if prev_ip != Some(ip) {
                st.log(
                    format!("Выбран рабочий подменный IP {ip} от {prov_name} (задержка {lat} мс)"),
                    "success",
                );
                // Применяем правила в Keenetic
                self.apply_keenetic_rules(ip, &targets, prev_ip).await;
            }
        } else {
            st.consecutive_failures += 1;
            st.state_status = if st.consecutive_failures >= 3 {
                "error".into()
            } else {
                "healing".into()
            };
            st.log(
                format!(
                    "Внимание: не найдено доступных подменных IP (ошибок подряд: {})",
                    st.consecutive_failures
                ),
                "warn",
            );
        }
    }

    /// Применение DNS-записей в KeeneticOS и добавление маршрута мимо VPN
    async fn apply_keenetic_rules(&self, ip: Ipv4Addr, targets: &[String], prev_ip: Option<Ipv4Addr>) {
        // 1. Очистка прошлого IP из маршрутизации
        if let Some(old) = prev_ip {
            let _ = tokio::process::Command::new("ip")
                .args(["rule", "del", "to", &format!("{old}/32"), "table", "4096", "priority", "90"])
                .output()
                .await;
        }

        // 2. Добавление правила маршрутизации через основной WAN (таблица 4096 на Keenetic)
        let _ = tokio::process::Command::new("ip")
            .args(["rule", "add", "to", &format!("{ip}/32"), "table", "4096", "priority", "90"])
            .output()
            .await;

        // 3. Установка статических DNS-записей в Keenetic ndnproxy
        for target in targets {
            let _ = tokio::process::Command::new("ndmc")
                .args(["-c", &format!("ip host {target} {ip}")])
                .output()
                .await;
        }
    }

    /// Очистка всех правил из Keenetic
    pub async fn clean_keenetic_rules(&self, active_ip: &Option<Ipv4Addr>) {
        if let Some(ip) = active_ip {
            let _ = tokio::process::Command::new("ip")
                .args(["rule", "del", "to", &format!("{ip}/32"), "table", "4096", "priority", "90"])
                .output()
                .await;
        }

        let targets = {
            let c = self.config.read().await;
            c.antigravity.targets.clone()
        };

        for target in targets {
            let _ = tokio::process::Command::new("ndmc")
                .args(["-c", &format!("no ip host {target}")])
                .output()
                .await;
        }
    }

    /// Запуск легковесного HTTP CONNECT прокси
    pub fn start_proxy(self: Arc<Self>, port: u16) {
        tokio::spawn(async move {
            let addr = format!("0.0.0.0:{port}");
            let listener = match TcpListener::bind(&addr).await {
                Ok(l) => {
                    self.state.write().await.proxy_running = true;
                    self.state.write().await.log(format!("HTTP CONNECT прокси запущен на порту {port}"), "info");
                    l
                }
                Err(e) => {
                    self.state.write().await.log(format!("Не удалось запустить прокси на {port}: {e}"), "error");
                    return;
                }
            };

            loop {
                let (stream, _) = match listener.accept().await {
                    Ok(res) => res,
                    Err(_) => continue,
                };

                let mgr = self.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_proxy_conn(stream, mgr).await {
                        let _ = e;
                    }
                });
            }
        });
    }

    /// Запуск периодического warm-loop
    pub fn start_warm_loop(self: Arc<Self>) {
        tokio::spawn(async move {
            // Начальная проверка через 2 секунды после старта
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            self.check_and_update().await;

            loop {
                let interval = {
                    let c = self.config.read().await;
                    c.antigravity.health_check_interval.max(30)
                };
                tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
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
    let mut buf = [0u8; 1024];
    let n = client.read(&mut buf).await?;
    if n == 0 {
        return Ok(());
    }

    let req_str = String::from_utf8_lossy(&buf[..n]);
    let first_line = req_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();

    if parts.len() < 2 || parts[0] != "CONNECT" {
        client.write_all(b"HTTP/1.1 405 Method Not Allowed\r\n\r\n").await?;
        return Ok(());
    }

    let target_host_port = parts[1];
    let host_only = target_host_port.split(':').next().unwrap_or(target_host_port);

    // Определяем, куда отправлять трафик:
    let targets = {
        let c = mgr.config.read().await;
        c.antigravity.targets.clone()
    };

    let destination = if targets.iter().any(|t| t.eq_ignore_ascii_case(host_only)) {
        let st = mgr.state.read().await;
        if let Some(sub_ip) = st.active_ip {
            format!("{sub_ip}:443")
        } else {
            target_host_port.to_string()
        }
    } else {
        target_host_port.to_string()
    };

    let mut server = match TcpStream::connect(&destination).await {
        Ok(s) => s,
        Err(e) => {
            let resp = format!("HTTP/1.1 502 Bad Gateway\r\nX-Error: {}\r\n\r\n", e);
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
